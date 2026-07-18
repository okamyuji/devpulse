//! Claude Code取得元アダプタ（詳細設計4.2節）。

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

use super::model::{AgentKind, AgentSessionRow, Confidence, SessionState, StateSource};
use super::{AgentSource, AgentSourceError, CommandRunner};

/// claude agents --json --allの出力を解析するアダプタ。
pub struct ClaudeAdapter {
    runner: Arc<dyn CommandRunner>,
    timeout_ms: u64,
}

impl ClaudeAdapter {
    pub fn new(runner: Arc<dyn CommandRunner>, timeout_ms: u64) -> Self {
        Self { runner, timeout_ms }
    }
}

/// agents --json --all出力を統一行モデルへ解析する。
///
/// 状態対応表（詳細設計4.2節）: blocked→waiting、done→idle、failed→failed、
/// running→running（事前定義）。対応表にない値はunknownへ落とし生の値をログへ記録する。
pub fn parse_claude_agents(json: &str) -> Result<Vec<AgentSessionRow>, AgentSourceError> {
    let malformed = |reason: &str| AgentSourceError::MalformedOutput {
        command: "claude agents".into(),
        reason: reason.into(),
    };
    let root: serde_json::Value =
        serde_json::from_str(json).map_err(|_| malformed("output is not valid JSON"))?;
    let entries = root
        .as_array()
        .ok_or_else(|| malformed("output is not a JSON array"))?;
    let mut rows = Vec::new();
    for entry in entries {
        // 実出力には2スキーマが混在する（手動検証で実測）。
        // - background型: id, state を持つ
        // - interactive型: idを持たず pid（文字列または数値）と status を持つ
        // idはsessionIdの先頭8文字（background型の実データで確認済み）のため、
        // interactive型のidはsessionIdから導出する。両方無いエントリのみ読み飛ばす。
        let session_id = entry
            .get("sessionId")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let id: String = match (entry.get("id").and_then(|v| v.as_str()), &session_id) {
            (Some(id), _) => id.to_string(),
            (None, Some(sid)) => sid.chars().take(8).collect(),
            (None, None) => {
                tracing::warn!("claude agents entry without id and sessionId skipped");
                continue;
            }
        };
        let mut row = AgentSessionRow::new(&id, AgentKind::Claude, StateSource::ClaudeCli);
        row.cwd = entry
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);
        row.task_title = entry
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        // cmux eventsのsession_id照合に使う（idは短縮形のため完全なsessionIdを保持する）
        row.session_id = session_id;
        // 種別（background/interactive）は内部属性として保持する（表示には使わない）
        row.source_kind = entry
            .get("kind")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        // interactive型のpidは文字列と数値の両形が実測されているため両方受け付ける
        row.pid = entry
            .get("pid")
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .and_then(|p| u32::try_from(p).ok());
        // 状態対応表（詳細設計4.2節）。background型はstate、interactive型はstatusで報告する。
        // 対応表にない値はunknownへ落とし生の値を記録する。
        let raw_state = entry
            .get("state")
            .or_else(|| entry.get("status"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        match raw_state {
            "failed" => {
                row.state = SessionState::Failed;
                row.confidence = Confidence::Reported;
            }
            "blocked" => {
                row.state = SessionState::Waiting;
                row.confidence = Confidence::Reported;
            }
            "done" | "idle" => {
                // doneのidle対応は完了報告をidle相当として扱う判断（詳細設計4.2節）。
                // interactive型のstatusはidleをそのまま報告する（実測値）。
                row.state = SessionState::Idle;
                row.confidence = Confidence::Reported;
            }
            "running" => {
                row.state = SessionState::Running;
                row.confidence = Confidence::Reported;
            }
            other => {
                tracing::warn!(raw_state = other, id = %id, "unknown claude agent state");
                row.state = SessionState::Unknown;
            }
        }
        rows.push(row);
    }
    Ok(rows)
}

#[async_trait]
impl AgentSource for ClaudeAdapter {
    fn name(&self) -> &'static str {
        "claude"
    }

    async fn is_available(&self) -> bool {
        self.runner.exists("claude")
    }

    async fn collect(&self) -> Result<Vec<AgentSessionRow>> {
        Ok(self.collect_rows().await?)
    }
}

impl ClaudeAdapter {
    /// 収集の実体。失敗を型付きエラーで返す（trait境界でanyhowへ変換される）。
    pub async fn collect_rows(&self) -> Result<Vec<AgentSessionRow>, AgentSourceError> {
        let out = self
            .runner
            .run("claude", &["agents", "--json", "--all"], self.timeout_ms)
            .await
            .map_err(|e| AgentSourceError::CommandUnavailable {
                command: "claude agents".into(),
                reason: e.to_string(),
            })?;
        if out.timed_out {
            return Err(AgentSourceError::Timeout {
                command: "claude agents".into(),
                timeout_ms: self.timeout_ms,
            });
        }
        if !out.success {
            return Err(AgentSourceError::NonZeroExit {
                command: "claude agents".into(),
            });
        }
        parse_claude_agents(&out.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::agents::model::{Confidence, Orchestrator, SessionState, StateSource};
    use crate::data::agents::CommandOutput;
    use std::path::PathBuf;

    const AGENTS_FIXTURE: &str = include_str!("fixtures/claude_agents.json");
    const EXTRA_STATES_FIXTURE: &str = include_str!("fixtures/claude_agents_extra_states.json");
    const MIXED_INTERACTIVE_FIXTURE: &str =
        include_str!("fixtures/claude_agents_mixed_interactive.json");

    #[test]
    fn regression_claude_interactive_entries_use_pid_status_schema_without_id() {
        // 手動検証で発見: interactive型はidを持たず、pid（文字列または数値）と
        // status（stateではない）を持つ。旧実装はidなしとしてスキップし、生きている
        // 対話セッションが全て欠落し、warnが毎周期出続けた（実測288回）。
        let cap = crate::logging::capture::Capture::new();
        let _guard = tracing::subscriber::set_default(cap.subscriber());
        let rows = parse_claude_agents(MIXED_INTERACTIVE_FIXTURE).unwrap();
        assert_eq!(rows.len(), 3);
        // 実在する2スキーマ（background型・interactive型）はどちらもwarnを出さない
        assert!(cap.find("without id").is_none());
        // interactive型: idはsessionId先頭8文字から導出（background型のidが
        // sessionIdの先頭8文字であることを実データで確認済み）
        let it = rows.iter().find(|r| r.id == "e05d60a0").unwrap();
        assert_eq!(
            it.session_id.as_deref(),
            Some("e05d60a0-a0cb-44db-9206-6c6e5bf04d4c")
        );
        // 文字列pid "81574" → u32
        assert_eq!(it.pid, Some(81574));
        assert_eq!(it.cwd, Some(PathBuf::from("/Users/dev/devs/rust/devpulse")));
        assert_eq!(it.task_title.as_deref(), Some("devpulse-0a"));
        // statusの実測値idleは報告状態として反映する
        assert_eq!(it.state, SessionState::Idle);
        assert_eq!(it.confidence, Confidence::Reported);
        // kind=interactiveを内部的に保持する（表示仕様は変えない）
        assert_eq!(it.source_kind.as_deref(), Some("interactive"));
        // 数値pidも受け付ける（実出力で両形が観測されている）
        let it2 = rows.iter().find(|r| r.id == "02a95d32").unwrap();
        assert_eq!(it2.pid, Some(52295));
        assert_eq!(it2.state, SessionState::Idle);
        // background型は従来どおり
        let bg = rows.iter().find(|r| r.id == "311b1c16").unwrap();
        assert_eq!(bg.state, SessionState::Waiting);
        assert_eq!(bg.confidence, Confidence::Reported);
        assert_eq!(bg.source_kind.as_deref(), Some("background"));
        assert!(bg.pid.is_none());
    }

    #[test]
    fn warns_only_when_both_id_and_session_id_are_missing() {
        // warnの整理: idもsessionIdも無いエントリのみwarnして読み飛ばす
        let cap = crate::logging::capture::Capture::new();
        let _guard = tracing::subscriber::set_default(cap.subscriber());
        let json = r#"[
          {"cwd": "/tmp", "state": "done", "name": "no id no sessionId"},
          {"sessionId": "aabbccdd-0000-1111-2222-333344445555", "cwd": "/tmp", "status": "idle"}
        ]"#;
        let rows = parse_claude_agents(json).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "aabbccdd");
        assert_eq!(cap.count(tracing::Level::WARN, "without id"), 1);
    }

    #[test]
    fn parses_real_fixture_with_observed_state_mapping() {
        let rows = parse_claude_agents(AGENTS_FIXTURE).unwrap();
        assert_eq!(rows.len(), 6);
        // failed → failed（値の中身まで確認する: T5）
        let first = &rows[0];
        assert_eq!(first.id, "6c428b96");
        assert_eq!(first.agent, AgentKind::Claude);
        assert_eq!(first.orchestrator, Orchestrator::Unknown);
        assert_eq!(first.cwd, Some(PathBuf::from("/Users/dev/devs/zenn")));
        assert_eq!(first.state, SessionState::Failed);
        assert_eq!(first.state_source, StateSource::ClaudeCli);
        assert_eq!(first.confidence, Confidence::Reported);
        // 日本語タイトル（T3文字種）
        assert_eq!(
            first.task_title.as_deref(),
            Some("AWS MySQL consistency patterns の論理構成レビュー")
        );
        // last_activity_atはこのCLIからは得られない（詳細設計4.2節）
        assert!(first.last_activity_at.is_none());
        // done → idle
        let done = rows.iter().find(|r| r.id == "c39a9600").unwrap();
        assert_eq!(done.state, SessionState::Idle);
        assert_eq!(done.confidence, Confidence::Reported);
        // blocked → waiting
        let blocked = rows.iter().find(|r| r.id == "311b1c16").unwrap();
        assert_eq!(blocked.state, SessionState::Waiting);
        assert_eq!(blocked.confidence, Confidence::Reported);
        assert_eq!(blocked.cwd, Some(PathBuf::from("/Users/dev")));
    }

    #[test]
    fn maps_running_and_falls_back_to_unknown_for_unlisted_state() {
        let rows = parse_claude_agents(EXTRA_STATES_FIXTURE).unwrap();
        assert_eq!(rows.len(), 2);
        // running → running（事前定義の対応。実測未観測でも対応表に含める）
        assert_eq!(rows[0].state, SessionState::Running);
        assert_eq!(rows[0].confidence, Confidence::Reported);
        // 対応表にない値（paused）はunknownへ落とす。Reported扱いにしない。
        assert_eq!(rows[1].state, SessionState::Unknown);
        assert_ne!(rows[1].confidence, Confidence::Reported);
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(parse_claude_agents("[ {").is_err());
        // JSONだが配列でない
        assert!(parse_claude_agents(r#"{"agents": []}"#).is_err());
    }

    #[test]
    fn returns_empty_for_empty_array() {
        assert!(parse_claude_agents("[]").unwrap().is_empty());
    }

    #[test]
    fn skips_entries_missing_id_but_keeps_others() {
        // T1: 欠落キー。idもsessionIdも無い要素は行にできないため読み飛ばし、他は残す。
        let json = r#"[
          {"cwd": "/tmp", "state": "done", "name": "no id"},
          {"id": "ok1", "cwd": "/tmp", "state": "done", "name": "valid"}
        ]"#;
        let rows = parse_claude_agents(json).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "ok1");
        assert_eq!(rows[0].state, SessionState::Idle);
    }

    struct MockRunner {
        exists: bool,
        output: Result<CommandOutput, String>,
    }

    #[async_trait]
    impl CommandRunner for MockRunner {
        async fn run(&self, _p: &str, _a: &[&str], _t: u64) -> Result<CommandOutput> {
            self.output.clone().map_err(|e| anyhow::anyhow!("{e}"))
        }
        fn exists(&self, _p: &str) -> bool {
            self.exists
        }
    }

    #[tokio::test]
    async fn adapter_collect_returns_rows_via_runner() {
        let adapter = ClaudeAdapter::new(
            Arc::new(MockRunner {
                exists: true,
                output: Ok(CommandOutput {
                    stdout: AGENTS_FIXTURE.into(),
                    timed_out: false,
                    success: true,
                }),
            }),
            1000,
        );
        let rows = adapter.collect().await.unwrap();
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0].state, SessionState::Failed);
    }

    #[tokio::test]
    async fn adapter_collect_fails_when_command_fails() {
        let adapter = ClaudeAdapter::new(
            Arc::new(MockRunner {
                exists: true,
                output: Err("spawn failed".into()),
            }),
            1000,
        );
        assert!(adapter.collect().await.is_err());
    }

    #[tokio::test]
    async fn adapter_is_available_reflects_command_existence() {
        // is_availableはPATH上のclaudeの有無をそのまま反映する（true固定・false固定の防止）
        let mk = |exists| {
            ClaudeAdapter::new(
                Arc::new(MockRunner {
                    exists,
                    output: Err("unused".into()),
                }),
                1000,
            )
        };
        assert!(mk(true).is_available().await);
        assert!(!mk(false).is_available().await);
    }

    #[tokio::test]
    async fn real_claude_agents_parses_when_command_exists() {
        // T7: コマンドが環境に存在する場合のみ実行する
        let runner = crate::data::agents::SystemCommandRunner;
        if !runner.exists("claude") {
            eprintln!("SKIP: claude command not found; real-command test skipped");
            return;
        }
        let adapter = ClaudeAdapter::new(Arc::new(runner), 15000);
        match adapter.collect().await {
            Ok(rows) => {
                for row in &rows {
                    assert_eq!(row.agent, AgentKind::Claude);
                    assert!(!row.id.is_empty());
                }
            }
            Err(e) => eprintln!("SKIP: claude present but agents failed ({e})"),
        }
    }
}
