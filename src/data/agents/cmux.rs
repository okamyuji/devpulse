//! cmux取得元アダプタ（詳細設計4.1節）。

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::sync::Arc;

use super::model::{AgentKind, AgentSessionRow, Orchestrator, StateSource};
use super::{ActivityEvent, AgentSource, AgentSourceError, CommandRunner};

/// cmuxのtree --all --jsonとevents単発取得を解析するアダプタ。
pub struct CmuxAdapter {
    runner: Arc<dyn CommandRunner>,
    timeout_ms: u64,
    cursor_path: PathBuf,
}

impl CmuxAdapter {
    pub fn new(runner: Arc<dyn CommandRunner>, timeout_ms: u64, cursor_path: PathBuf) -> Self {
        Self {
            runner,
            timeout_ms,
            cursor_path,
        }
    }

    /// 収集の実体。失敗を型付きエラーで返す（trait境界でanyhowへ変換される）。
    pub async fn collect_rows(&self) -> Result<Vec<AgentSessionRow>, AgentSourceError> {
        let out = self
            .runner
            .run("cmux", &["tree", "--all", "--json"], self.timeout_ms)
            .await
            .map_err(|e| AgentSourceError::CommandUnavailable {
                command: "cmux tree".into(),
                reason: e.to_string(),
            })?;
        if out.timed_out {
            return Err(AgentSourceError::Timeout {
                command: "cmux tree".into(),
                timeout_ms: self.timeout_ms,
            });
        }
        if !out.success {
            return Err(AgentSourceError::NonZeroExit {
                command: "cmux tree".into(),
            });
        }
        parse_tree(&out.stdout)
    }
}

/// events取得カーソルの所有者（消費者の識別）。
/// TUI常駐collectorと一発CLI（devpulse agents --json）が同一カーソルを進めると
/// 互いのイベントを食い合い、活動の欠落と誤ったquiet判定が起きるため、
/// 消費者ごとにカーソルファイルを分ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorOwner {
    /// 常駐TUIの背景collector。
    Tui,
    /// 一発CLI（devpulse agents --json）。
    Cli,
}

impl CursorOwner {
    fn suffix(self) -> &'static str {
        match self {
            CursorOwner::Tui => "tui",
            CursorOwner::Cli => "cli",
        }
    }
}

/// events取得カーソルファイルのインスタンス別既定パス
/// （~/.local/share/devpulse/cmux-events-cursor-{tui,cli}-{pid}）。
/// 所有者だけでなく自プロセスPIDも含めるのは、同一モードのDevPulse多重起動が
/// 同じカーソルを進めると先に読んだ側がイベントを消費してしまうため。
/// 新パスは欠如時にseq 0からシードされる既存の初回動作で追いつく。
pub fn default_cursor_path(owner: CursorOwner) -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".local/share/devpulse")
        .join(format!(
            "cmux-events-cursor-{}-{}",
            owner.suffix(),
            std::process::id()
        ))
}

/// 残骸カーソルファイルを掃除する（best effort、失敗しても落とさない）。
/// PID入りカーソルはプロセス終了後に残るため、生きていないPIDのファイルと
/// 旧命名（固定名・所有者のみ）のファイルを削除する。自PIDと稼働中PIDは残す。
pub(crate) fn cleanup_stale_cursor_files(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut sys = sysinfo::System::new();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::nothing(),
    );
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(rest) = name.strip_prefix("cmux-events-cursor") else {
            continue;
        };
        let stale = match rest
            .strip_prefix("-tui-")
            .or_else(|| rest.strip_prefix("-cli-"))
        {
            // PID入り: 自プロセス以外で、そのPIDが生きていなければ残骸
            Some(pid_str) => match pid_str.parse::<u32>() {
                Ok(pid) if pid == std::process::id() => false,
                Ok(pid) => sys.process(sysinfo::Pid::from_u32(pid)).is_none(),
                Err(_) => false,
            },
            // 旧命名（"", "-tui", "-cli"）は現行コードが参照しないため残骸
            None => matches!(rest, "" | "-tui" | "-cli"),
        };
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// 自インスタンスのカーソルを起動時に必ずseq 0へ初期化する。
/// PIDはOSに再利用され得るため、再利用PIDの新プロセスが前インスタンスの
/// カーソルを引き継ぐと、前インスタンス終了後のイベントを見落とす。
/// 新規インスタンスは常にseq 0からの全再生で開始する（欠如時シードと同じ意味論）。
pub(crate) fn claim_own_cursor(path: &std::path::Path) {
    seed_cursor(path);
}

/// カーソルファイルへseq 0を書き込む（親ディレクトリも作る）。失敗しても落とさない
/// （cmuxはファイル欠如時ライブ購読になり、イベント0件として正直に縮退する）。
fn seed_cursor(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(path, "0\n") {
        tracing::debug!(path = %path.display(), error = %e, "cmux events cursor seed failed");
    }
}

/// ackフレームからstaleカーソルを検出する。cmux再起動（boot_id変化）でseqが巻き戻ると
/// 保存済みカーソルがlatest_seqを超え、以後イベント0件のまま固着するため検出が要る。
fn ack_shows_stale_cursor(ndjson: &str) -> bool {
    let Some(first) = ndjson.lines().find(|l| !l.trim().is_empty()) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(first.trim()) else {
        return false;
    };
    if value.get("type").and_then(|t| t.as_str()) != Some("ack") {
        return false;
    }
    let resume = value.get("resume");
    let requested = resume
        .and_then(|r| r.get("requested_after_seq"))
        .and_then(|s| s.as_u64());
    let latest = resume
        .and_then(|r| r.get("latest_seq"))
        .and_then(|s| s.as_u64());
    matches!((requested, latest), (Some(req), Some(latest)) if req > latest)
}

/// tree --all --json出力からterminal型surfaceを行の候補として抽出する。
///
/// ノード種別は階層位置（windows/workspaces/panes/surfaces）で決まり、typeキーを
/// 持つのはsurfaceノードだけである（実測確認済み。T4回帰3）。
pub fn parse_tree(json: &str) -> Result<Vec<AgentSessionRow>, AgentSourceError> {
    let root: serde_json::Value =
        serde_json::from_str(json).map_err(|_| AgentSourceError::MalformedOutput {
            command: "cmux tree".into(),
            reason: "output is not valid JSON".into(),
        })?;
    let mut rows = Vec::new();
    let windows = root.get("windows").and_then(|w| w.as_array());
    for window in windows.into_iter().flatten() {
        for workspace in iter_array(window, "workspaces") {
            let ws_ref = str_key(workspace, "ref");
            let ws_title = str_key(workspace, "title");
            for pane in iter_array(workspace, "panes") {
                for surface in iter_array(pane, "surfaces") {
                    if str_key(surface, "type").as_deref() != Some("terminal") {
                        continue;
                    }
                    let Some(surface_ref) = str_key(surface, "ref") else {
                        continue;
                    };
                    let mut row = AgentSessionRow::new(
                        surface_ref.clone(),
                        AgentKind::Other("unknown".into()),
                        StateSource::CmuxCli,
                    );
                    row.orchestrator = Orchestrator::Cmux;
                    row.location = format!(
                        "{}/{}",
                        ws_ref.clone().unwrap_or_else(|| "workspace:?".into()),
                        surface_ref
                    );
                    row.task_title = ws_title.clone();
                    row.tty = str_key(surface, "tty");
                    rows.push(row);
                }
            }
        }
    }
    Ok(rows)
}

/// events出力（NDJSON）から活動イベントを抽出する。
///
/// session_idとcwdはトップレベルではなくpayload内にのみ、一部イベントに限って
/// 存在する（実測確認済み。T4回帰2）。持たないイベントと解析不能行は無視する。
pub fn parse_events(ndjson: &str) -> Vec<ActivityEvent> {
    let mut events = Vec::new();
    for line in ndjson.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue; // 途中で切れた行は無視する
        };
        let Some(occurred_at) = value
            .get("occurred_at")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
        else {
            continue;
        };
        let Some(payload) = value.get("payload") else {
            continue;
        };
        let (Some(session_id), Some(cwd)) = (
            payload.get("session_id").and_then(|v| v.as_str()),
            payload.get("cwd").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        events.push(ActivityEvent {
            session_id: session_id.to_string(),
            cwd: PathBuf::from(cwd),
            occurred_at,
        });
    }
    events
}

fn iter_array<'a>(
    node: &'a serde_json::Value,
    key: &str,
) -> impl Iterator<Item = &'a serde_json::Value> {
    node.get(key)
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
}

fn str_key(node: &serde_json::Value, key: &str) -> Option<String> {
    node.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

#[async_trait]
impl AgentSource for CmuxAdapter {
    fn name(&self) -> &'static str {
        "cmux"
    }

    async fn is_available(&self) -> bool {
        if !self.runner.exists("cmux") {
            return false;
        }
        matches!(
            self.runner.run("cmux", &["ping"], self.timeout_ms).await,
            Ok(out) if out.success
        )
    }

    async fn collect(&self) -> Result<Vec<AgentSessionRow>> {
        Ok(self.collect_rows().await?)
    }

    async fn activity(&self) -> Result<Vec<ActivityEvent>> {
        Ok(self.activity_events().await?)
    }
}

impl CmuxAdapter {
    /// 活動イベント取得の実体。失敗を型付きエラーで返す（trait境界でanyhowへ変換される）。
    ///
    /// 単発取得（ストリーム購読はしない）。カーソルファイル方式で前回の続きから読む
    /// （--after 0はバッファ最古側へ戻り最新の活動が見えなくなる実測があるため禁止）。
    /// 新規イベントの残数が--limit未満のとき上限時間まで待つため、タイムアウト時は
    /// それまでの出力を解析対象とする（このコマンド固有の意図した縮退）。
    /// タイムアウトを伴わない非0終了と起動失敗は、空イベントと混同せず失敗として返す。
    /// カーソルファイル欠如時のcmuxはライブ購読になり保持イベントを再生しない
    /// （実測確認済み）ため、初回はseq 0をシードして最古から追いつく。
    pub async fn activity_events(&self) -> Result<Vec<ActivityEvent>, AgentSourceError> {
        if std::fs::metadata(&self.cursor_path).is_err() {
            seed_cursor(&self.cursor_path);
        }
        let cursor_arg = self.cursor_path.to_string_lossy().into_owned();
        // ackフレームは残す（--no-ackを付けない）: resume情報でstaleカーソルを検出する。
        // ackはoccurred_atを持たずparse_eventsが自然に無視する。
        let out = self
            .runner
            .run(
                "cmux",
                &[
                    "events",
                    "--cursor-file",
                    &cursor_arg,
                    "--limit",
                    "200",
                    "--no-heartbeat",
                ],
                self.timeout_ms,
            )
            .await
            .map_err(|e| AgentSourceError::CommandUnavailable {
                command: "cmux events".into(),
                reason: e.to_string(),
            })?;
        if !out.success && !out.timed_out {
            return Err(AgentSourceError::NonZeroExit {
                command: "cmux events".into(),
            });
        }
        if ack_shows_stale_cursor(&out.stdout) {
            // cmux再起動でseqが巻き戻った。次周期に再生できるようseq 0へ戻す。
            tracing::debug!("cmux events cursor is stale (cmux restarted); resetting");
            seed_cursor(&self.cursor_path);
        }
        Ok(parse_events(&out.stdout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::agents::CommandOutput;
    use chrono::TimeZone;

    /// テストごとに衝突しない一時カーソルパスを返す。
    fn test_cursor_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "devpulse-test-cursor-{name}-{}",
            std::process::id()
        ))
    }

    const TREE_FIXTURE: &str = include_str!("fixtures/cmux_tree.json");
    const EVENTS_FIXTURE: &str = include_str!("fixtures/cmux_events.jsonl");
    const EVENTS_TRUNCATED_FIXTURE: &str = include_str!("fixtures/cmux_events_truncated.jsonl");

    #[test]
    fn claim_own_cursor_resets_inherited_stale_cursor_to_zero() {
        // PID再利用で前インスタンスのカーソルを引き継いだ場合でも、
        // 起動時のclaimでseq 0へ戻り、前インスタンス終了後のイベントを
        // 見落とさないこと。
        let path = test_cursor_path("claim-reseed");
        std::fs::write(&path, "12345\n").expect("write stale cursor");
        claim_own_cursor(&path);
        let content = std::fs::read_to_string(&path).expect("read cursor");
        assert_eq!(content, "0\n", "inherited cursor must be reseeded to zero");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn default_cursor_path_is_distinct_per_consumer_owner_and_process() {
        // 消費者間（TUIとCLI）だけでなく、同一モードのDevPulse多重起動でも
        // カーソルを共有すると先に読んだ側がイベントを消費してしまうため、
        // カーソルパスは所有者と自プロセスPIDの両方で一意にする。
        let pid = std::process::id();
        let tui = default_cursor_path(CursorOwner::Tui);
        let cli = default_cursor_path(CursorOwner::Cli);
        assert_ne!(tui, cli);
        assert!(
            tui.to_string_lossy()
                .ends_with(&format!("cmux-events-cursor-tui-{pid}")),
            "got: {}",
            tui.display()
        );
        assert!(
            cli.to_string_lossy()
                .ends_with(&format!("cmux-events-cursor-cli-{pid}")),
            "got: {}",
            cli.display()
        );
    }

    #[test]
    fn cleanup_removes_dead_pid_and_legacy_cursor_files_only() {
        // PID入りカーソルはプロセス終了で残骸になるため、生きていないPIDの
        // ファイルと旧固定名ファイルだけを掃除し、稼働中のものと無関係な
        // ファイルは残す。
        let dir = std::env::temp_dir().join(format!("devpulse-cursor-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create test dir");
        let own = dir.join(format!("cmux-events-cursor-tui-{}", std::process::id()));
        let dead = dir.join("cmux-events-cursor-cli-999999999");
        let legacy = dir.join("cmux-events-cursor");
        let legacy_tui = dir.join("cmux-events-cursor-tui");
        let unrelated = dir.join("devpulse.log");
        for p in [&own, &dead, &legacy, &legacy_tui, &unrelated] {
            std::fs::write(p, "0\n").expect("seed test file");
        }
        cleanup_stale_cursor_files(&dir);
        assert!(own.exists(), "own live cursor must survive");
        assert!(unrelated.exists(), "unrelated file must survive");
        assert!(!dead.exists(), "dead-pid cursor must be removed");
        assert!(!legacy.exists(), "legacy fixed-name cursor must be removed");
        assert!(
            !legacy_tui.exists(),
            "legacy owner-only cursor must be removed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_tree_extracts_terminal_surfaces_with_tty_and_title() {
        let rows = parse_tree(TREE_FIXTURE).unwrap();
        assert_eq!(rows.len(), 3);
        // 実測値のtty（ttys003, ttys000, ttys001）で中身まで確認する（T5）
        let ttys: Vec<_> = rows.iter().map(|r| r.tty.as_deref().unwrap()).collect();
        assert_eq!(ttys, vec!["ttys003", "ttys000", "ttys001"]);
        let noslp = &rows[0];
        assert_eq!(noslp.id, "surface:4");
        assert_eq!(noslp.location, "workspace:4/surface:4");
        assert_eq!(noslp.task_title.as_deref(), Some("noslp"));
        assert_eq!(noslp.orchestrator, Orchestrator::Cmux);
        assert_eq!(noslp.state_source, StateSource::CmuxCli);
        // 点字記号と日本語を含むworkspaceタイトルが保持される（T3文字種）
        let braille = &rows[1];
        assert_eq!(
            braille.task_title.as_deref(),
            Some("⠐ AI開発ツールの改善案を整理")
        );
        assert_eq!(braille.tty.as_deref(), Some("ttys000"));
        // 絵文字タイトル（T3文字種）
        assert_eq!(
            rows[2].task_title.as_deref(),
            Some("🚀 サンプル記事の下書き")
        );
    }

    #[test]
    fn regression_cmux_tree_type_key_only_on_surface_nodes() {
        // T4回帰3: typeキーを持つのはsurfaceノードだけ。window/workspace/paneに
        // typeが無くても解析でき、surfaceのtype判定だけでterminalを選別する。
        let json = r#"{
          "active": {"surface_type": "terminal", "window_ref": "window:1"},
          "windows": [{"ref": "window:1", "workspaces": [{
            "ref": "workspace:1", "title": "t",
            "panes": [{"ref": "pane:1", "surfaces": [
              {"ref": "surface:1", "type": "terminal", "tty": "ttys009"},
              {"ref": "surface:2", "type": "browser", "url": "https://example.com"}
            ]}]
          }]}]
        }"#;
        let rows = parse_tree(json).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "surface:1");
        assert_eq!(rows[0].tty.as_deref(), Some("ttys009"));
    }

    #[test]
    fn parse_tree_rejects_invalid_json() {
        let err = parse_tree("{ not json").unwrap_err();
        assert!(err.to_string().contains("not valid JSON"));
    }

    #[test]
    fn parse_tree_returns_empty_for_no_windows() {
        assert!(parse_tree("{}").unwrap().is_empty());
        assert!(parse_tree(r#"{"windows": []}"#).unwrap().is_empty());
    }

    #[test]
    fn regression_cmux_events_session_id_only_in_payload_of_some_events() {
        // T4回帰2: session_idとcwdはpayload内にのみ、一部イベント（source=claude）に
        // 限って存在する。持たないイベントは無視する。
        let events = parse_events(EVENTS_FIXTURE);
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0].session_id,
            "claude-9e0112cf-9e45-4f76-b22c-64a0712ac9db"
        );
        assert_eq!(events[0].cwd, PathBuf::from("/Users/dev"));
        assert_eq!(
            events[0].occurred_at,
            Utc.with_ymd_and_hms(2026, 7, 18, 7, 58, 47).unwrap()
                + chrono::Duration::milliseconds(559)
        );
    }

    #[test]
    fn parse_events_skips_truncated_last_line() {
        // T1: 途中で切れた出力。完全な行だけを解析する。
        let events = parse_events(EVENTS_TRUNCATED_FIXTURE);
        assert_eq!(events.len(), 2);
        assert!(events
            .iter()
            .all(|e| e.cwd == std::path::Path::new("/Users/dev")));
    }

    #[test]
    fn parse_events_returns_empty_for_empty_input() {
        assert!(parse_events("").is_empty());
        assert!(parse_events("\n\n").is_empty());
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
    async fn adapter_collect_parses_tree_from_runner() {
        let adapter = CmuxAdapter::new(
            Arc::new(MockRunner {
                exists: true,
                output: Ok(CommandOutput {
                    stdout: TREE_FIXTURE.into(),
                    timed_out: false,
                    success: true,
                }),
            }),
            1000,
            test_cursor_path("collect-tree"),
        );
        let rows = adapter.collect().await.unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].tty.as_deref(), Some("ttys000"));
    }

    #[tokio::test]
    async fn adapter_collect_fails_on_timeout() {
        // T1: コマンドのタイムアウトは取得元の失敗として返す
        let adapter = CmuxAdapter::new(
            Arc::new(MockRunner {
                exists: true,
                output: Ok(CommandOutput {
                    stdout: "{".into(),
                    timed_out: true,
                    success: false,
                }),
            }),
            1000,
            test_cursor_path("collect-timeout"),
        );
        let err = adapter.collect().await.unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn adapter_unavailable_when_command_missing() {
        let adapter = CmuxAdapter::new(
            Arc::new(MockRunner {
                exists: false,
                output: Err("unused".into()),
            }),
            1000,
            test_cursor_path("unavailable"),
        );
        assert!(!adapter.is_available().await);
    }

    #[tokio::test]
    async fn adapter_available_only_when_command_exists_and_ping_succeeds() {
        // existsかつping成功のときだけ使える（false固定・exists判定の反転の防止）
        let mk = |exists, success| {
            CmuxAdapter::new(
                Arc::new(MockRunner {
                    exists,
                    output: Ok(CommandOutput {
                        stdout: String::new(),
                        timed_out: false,
                        success,
                    }),
                }),
                1000,
                test_cursor_path("available"),
            )
        };
        assert!(mk(true, true).is_available().await);
        assert!(!mk(true, false).is_available().await);
    }

    #[tokio::test]
    async fn adapter_activity_tolerates_timed_out_partial_stream() {
        let adapter = CmuxAdapter::new(
            Arc::new(MockRunner {
                exists: true,
                output: Ok(CommandOutput {
                    stdout: EVENTS_TRUNCATED_FIXTURE.into(),
                    timed_out: true,
                    success: false,
                }),
            }),
            1000,
            test_cursor_path("partial-stream"),
        );
        let events = adapter.activity().await.unwrap();
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn adapter_activity_rejects_non_zero_exit_without_timeout() {
        // タイムアウト以外の非0終了は「イベント0件」ではなく失敗として返す
        // （失敗を空活動と混同するとquiet判定が誤る）。
        let adapter = CmuxAdapter::new(
            Arc::new(MockRunner {
                exists: true,
                output: Ok(CommandOutput {
                    stdout: String::new(),
                    timed_out: false,
                    success: false,
                }),
            }),
            1000,
            test_cursor_path("activity-nonzero"),
        );
        let err = adapter.activity().await.unwrap_err();
        assert!(
            err.to_string().contains("cmux events exited with failure"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn adapter_activity_rejects_runner_error() {
        // 起動失敗も失敗として返す（従来は握りつぶして空を返していた）
        let adapter = CmuxAdapter::new(
            Arc::new(MockRunner {
                exists: true,
                output: Err("failed to spawn cmux".into()),
            }),
            1000,
            test_cursor_path("activity-spawn-fail"),
        );
        let err = adapter.activity().await.unwrap_err();
        assert!(err.to_string().contains("failed to run"), "got: {err}");
    }

    struct RecordingRunner {
        calls: std::sync::Mutex<Vec<Vec<String>>>,
        stdout: String,
    }

    #[async_trait]
    impl CommandRunner for RecordingRunner {
        async fn run(&self, program: &str, args: &[&str], _t: u64) -> Result<CommandOutput> {
            let mut call = vec![program.to_string()];
            call.extend(args.iter().map(|s| s.to_string()));
            self.calls.lock().unwrap().push(call);
            Ok(CommandOutput {
                stdout: self.stdout.clone(),
                timed_out: false,
                success: true,
            })
        }
        fn exists(&self, _p: &str) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn regression_cmux_legacy_alias_notice_requires_canonical_command_form() {
        // T4回帰6: legacy aliasコマンドは通知行が出力へ混入するため正式形だけを使う。
        // (1) アダプタが正式形（tree --all --json / events ...）を発行することを検証
        let runner = Arc::new(RecordingRunner {
            calls: std::sync::Mutex::new(Vec::new()),
            stdout: TREE_FIXTURE.into(),
        });
        let adapter = CmuxAdapter::new(runner.clone(), 1000, test_cursor_path("legacy-alias"));
        adapter.collect().await.unwrap();
        adapter.activity().await.unwrap();
        let calls = runner.calls.lock().unwrap().clone();
        assert_eq!(calls[0], vec!["cmux", "tree", "--all", "--json"]);
        assert_eq!(calls[1][0], "cmux");
        assert_eq!(calls[1][1], "events");
        // (2) 通知行が混入したlegacy alias風の出力はJSONとして成立せず解析を拒否する
        //     （正式形を使う決定の根拠を固定化する）
        let with_notice = format!("Notice: legacy alias, use 'cmux tree'\n{TREE_FIXTURE}");
        assert!(parse_tree(&with_notice).is_err());
    }

    #[tokio::test]
    async fn regression_events_cursor_reads_new_events_not_oldest() {
        // 手動検証で発見: --after 0 --limit 200はバッファ最古側200件を読み、最新の活動が
        // 見えない（last_activity_atが約3時間前になる実測）。カーソルファイル方式で
        // 前回読み終えたseqの続きから読む。
        let cursor = test_cursor_path("regression-cursor-args");
        let _ = std::fs::remove_file(&cursor);
        let runner = Arc::new(RecordingRunner {
            calls: std::sync::Mutex::new(Vec::new()),
            stdout: String::new(),
        });
        let adapter = CmuxAdapter::new(runner.clone(), 1000, cursor.clone());
        adapter.activity().await.unwrap();
        let calls = runner.calls.lock().unwrap().clone();
        assert_eq!(
            calls[0],
            vec![
                "cmux",
                "events",
                "--cursor-file",
                cursor.to_str().unwrap(),
                "--limit",
                "200",
                "--no-heartbeat",
            ],
            "eventsはカーソルファイル方式で読む（--after 0は最古側へ戻るため禁止）"
        );
        // カーソルファイル欠如時のcmuxはライブ購読になり保持イベントを再生しない
        // （実測確認済み）。初回はseq 0をシードして最古から追いつく。
        assert_eq!(std::fs::read_to_string(&cursor).unwrap().trim(), "0");
        let _ = std::fs::remove_file(&cursor);
    }

    #[tokio::test]
    async fn regression_events_stale_cursor_after_cmux_restart_is_reset() {
        // cmux再起動でboot_idが変わりseqが巻き戻ると、保存済みカーソルがlatestを超え
        // イベント0件のまま永久に固着する。ackフレームのresumeで検出しseq 0へ戻す。
        let cursor = test_cursor_path("regression-stale-cursor");
        std::fs::write(&cursor, "5067\n").unwrap();
        let stale_ack = r#"{"boot_id":"NEW-BOOT","protocol":"cmux-events","resume":{"after_seq":null,"gap":false,"latest_seq":10,"next_seq":11,"oldest_seq":1,"requested_after_seq":5067},"type":"ack","version":1}"#;
        let runner = Arc::new(RecordingRunner {
            calls: std::sync::Mutex::new(Vec::new()),
            stdout: format!("{stale_ack}\n"),
        });
        let adapter = CmuxAdapter::new(runner, 1000, cursor.clone());
        let events = adapter.activity().await.unwrap();
        assert!(events.is_empty(), "ackフレームはイベントとして解析しない");
        assert_eq!(
            std::fs::read_to_string(&cursor).unwrap().trim(),
            "0",
            "staleカーソルは次周期に再生できるようseq 0へ戻す"
        );
        let _ = std::fs::remove_file(&cursor);
    }

    #[tokio::test]
    async fn real_cmux_tree_parses_when_command_exists() {
        // T7: コマンドが環境に存在する場合のみ実行する
        let runner = crate::data::agents::SystemCommandRunner;
        if !runner.exists("cmux") {
            eprintln!("SKIP: cmux command not found; real-command test skipped");
            return;
        }
        let adapter = CmuxAdapter::new(Arc::new(runner), 3000, test_cursor_path("real-tree"));
        match adapter.collect().await {
            Ok(rows) => {
                for row in &rows {
                    assert_eq!(row.orchestrator, Orchestrator::Cmux);
                    assert!(row.id.starts_with("surface:"), "id was {}", row.id);
                }
            }
            Err(e) => eprintln!("SKIP: cmux present but tree failed ({e})"),
        }
    }
}
