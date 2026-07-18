//! エージェントセッション収集の公開インターフェースと収集の入口（詳細設計1節・3節・8.4節）。

pub mod claude;
pub mod cmux;
pub mod gitinfo;
pub mod kimi;
pub mod merge;
pub mod model;
pub mod process;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;

use model::AgentSessionRow;

/// 既定の外部コマンド上限時間（詳細設計3節、agents.command_timeout_ms既定値）。
pub const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 1000;
/// 既定のquiet閾値秒（詳細設計6節、agents.quiet_threshold_s既定値）。
pub const DEFAULT_QUIET_THRESHOLD_S: u64 = 480;

/// 外部コマンド実行の結果。タイムアウト時もそれまでの標準出力を保持する。
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub timed_out: bool,
    pub success: bool,
}

/// 外部コマンド実行の注入点（既存のProcessSource/PortScanner慣例と同型のtrait方式）。
#[async_trait]
pub trait CommandRunner: Send + Sync {
    /// コマンドを実行し、上限時間内の標準出力を返す。上限超過時はプロセスを止め、
    /// それまでの出力をtimed_out=trueで返す。起動失敗はErr。
    async fn run(&self, program: &str, args: &[&str], timeout_ms: u64) -> Result<CommandOutput>;

    /// コマンドがPATH上に存在するかの軽量判定。
    fn exists(&self, program: &str) -> bool;
}

/// 実コマンドを実行するCommandRunner。
pub struct SystemCommandRunner;

#[async_trait]
impl CommandRunner for SystemCommandRunner {
    async fn run(&self, program: &str, args: &[&str], timeout_ms: u64) -> Result<CommandOutput> {
        use tokio::io::AsyncReadExt;

        let mut child = tokio::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn {program}: {e}"))?;

        let mut stdout_pipe = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("no stdout pipe for {program}"))?;
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut timed_out = false;
        loop {
            match tokio::time::timeout_at(deadline, stdout_pipe.read(&mut chunk)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => buf.extend_from_slice(&chunk[..n]),
                Ok(Err(e)) => return Err(anyhow::anyhow!("read from {program} failed: {e}")),
                Err(_) => {
                    timed_out = true;
                    let _ = child.start_kill();
                    break;
                }
            }
        }
        let success = if timed_out {
            false
        } else {
            match tokio::time::timeout(Duration::from_millis(200), child.wait()).await {
                Ok(Ok(status)) => status.success(),
                _ => {
                    let _ = child.start_kill();
                    false
                }
            }
        };
        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&buf).into_owned(),
            timed_out,
            success,
        })
    }

    fn exists(&self, program: &str) -> bool {
        let Some(paths) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
    }
}

/// cmux eventsから抽出する活動イベント（詳細設計4.1節）。
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityEvent {
    pub session_id: String,
    pub cwd: PathBuf,
    pub occurred_at: DateTime<Utc>,
}

/// 取得元アダプタの共通契約（詳細設計3節）。
#[async_trait]
pub trait AgentSource: Send + Sync {
    /// アダプタの表示名（縮退表示に使う）。
    fn name(&self) -> &'static str;
    /// アダプタが現在使えるかの軽量判定。
    async fn is_available(&self) -> bool;
    /// 統一行モデルの配列を返す。失敗はエラーで返し、呼び出し側が欠落として扱う。
    async fn collect(&self) -> Result<Vec<AgentSessionRow>>;
    /// 活動イベントの単発取得（cmuxのみ実装。既定は空）。
    async fn activity(&self) -> Vec<ActivityEvent> {
        Vec::new()
    }
}

/// 取得元別の失敗情報（collector出力のsource_errorsに対応する）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceError {
    pub source: String,
    pub error: String,
}

/// 収集オプション。config.toml agentsセクション（詳細設計9節）の値を受け取る。
#[derive(Debug, Clone)]
pub struct CollectOptions {
    pub command_timeout_ms: u64,
    pub quiet_threshold_s: u64,
}

impl Default for CollectOptions {
    fn default() -> Self {
        Self {
            command_timeout_ms: DEFAULT_COMMAND_TIMEOUT_MS,
            quiet_threshold_s: DEFAULT_QUIET_THRESHOLD_S,
        }
    }
}

/// 1収集サイクルの結果スナップショット（詳細設計8.4節）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentsSnapshot {
    pub collected_at: DateTime<Utc>,
    pub sessions: Vec<AgentSessionRow>,
    pub source_errors: Vec<SourceError>,
}

/// 全アダプタ実行→統合→git補完→状態決定を行う収集の入口（詳細設計8.4節の1周期分）。
pub async fn collect_snapshot(
    cli_sources: &[Box<dyn AgentSource>],
    process_source: &dyn crate::data::processes::ProcessSource,
    tty_provider: &dyn process::TtyProvider,
    git: &mut gitinfo::GitEnricher,
    opts: &CollectOptions,
    now: DateTime<Utc>,
) -> AgentsSnapshot {
    let started = std::time::Instant::now();
    tracing::info!("agent collect cycle started");
    let mut source_errors = Vec::new();
    // 完了ログ用: 取得元ごとの行数（"claude=2,cmux=1" 形式で集計する）
    let mut rows_by_source: Vec<(String, usize)> = Vec::new();

    // プロセスフォールバック（詳細設計4.4節）: ps併用のtty突き合わせ
    let tty_by_pid = match tty_provider.tty_by_pid().await {
        Ok(map) => Some(map),
        Err(e) => {
            source_errors.push(SourceError {
                source: "ps".into(),
                error: e.to_string(),
            });
            None
        }
    };
    let process_rows = match process_source.list_processes() {
        Ok(procs) => {
            process::build_rows(&procs, tty_by_pid.as_ref().unwrap_or(&Default::default()))
        }
        Err(e) => {
            source_errors.push(SourceError {
                source: "process".into(),
                error: e.to_string(),
            });
            Vec::new()
        }
    };

    // 各取得元アダプタの実行
    let mut cmux_rows = Vec::new();
    let mut cli_rows = Vec::new();
    let mut activity = Vec::new();
    for source in cli_sources {
        if !source.is_available().await {
            tracing::debug!(source = source.name(), "agent source unavailable");
            source_errors.push(SourceError {
                source: source.name().into(),
                error: "unavailable".into(),
            });
            continue;
        }
        match source.collect().await {
            Ok(rows) => {
                rows_by_source.push((source.name().into(), rows.len()));
                for row in rows {
                    if row.orchestrator == model::Orchestrator::Cmux {
                        cmux_rows.push(row);
                    } else {
                        cli_rows.push(row);
                    }
                }
            }
            Err(e) => {
                tracing::warn!(source = source.name(), error = %e, "agent source failed");
                source_errors.push(SourceError {
                    source: source.name().into(),
                    error: e.to_string(),
                });
            }
        }
        activity.extend(source.activity().await);
    }
    rows_by_source.push(("process".into(), process_rows.len()));

    // 統合（詳細設計5節）
    let tty_universe = tty_by_pid.as_ref().map(|m| {
        m.values()
            .cloned()
            .collect::<std::collections::HashSet<_>>()
    });
    let mut rows = merge::merge_rows(cmux_rows, cli_rows, process_rows, tty_universe.as_ref());

    // 活動イベントの適用（詳細設計4.1節: 一致した行のlast_activity_atを最新イベント時刻で更新）
    merge::apply_activity(&mut rows, &activity);

    // git補完（詳細設計4.5節）
    git.enrich(&mut rows).await;

    // 状態決定（詳細設計6節）
    merge::apply_state_decision(&mut rows, now, opts.quiet_threshold_s);

    // 完了ログ（会話内容は含めない: 件数・取得元名・エラー文字列のみ）
    let rows_summary = rows_by_source
        .iter()
        .map(|(name, n)| format!("{name}={n}"))
        .collect::<Vec<_>>()
        .join(",");
    let errors_summary = source_errors
        .iter()
        .map(|e| format!("{}:{}", e.source, e.error))
        .collect::<Vec<_>>()
        .join("; ");
    tracing::info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        rows = %rows_summary,
        source_errors = %errors_summary,
        total_sessions = rows.len(),
        "agent collect cycle completed"
    );

    AgentsSnapshot {
        collected_at: now,
        sessions: rows,
        source_errors,
    }
}

/// 実環境向けの既定アダプタ一式を組み立てる。
pub fn default_sources(opts: &CollectOptions) -> Vec<Box<dyn AgentSource>> {
    let timeout = opts.command_timeout_ms;
    vec![
        Box::new(cmux::CmuxAdapter::new(
            std::sync::Arc::new(SystemCommandRunner),
            timeout,
            cmux::default_cursor_path(),
        )),
        Box::new(claude::ClaudeAdapter::new(
            std::sync::Arc::new(SystemCommandRunner),
            timeout,
        )),
        Box::new(kimi::KimiAdapter::new(kimi::default_sessions_dir())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn system_runner_captures_stdout_of_short_command() {
        let out = SystemCommandRunner
            .run("echo", &["hello"], 2000)
            .await
            .unwrap();
        assert_eq!(out.stdout.trim(), "hello");
        assert!(!out.timed_out);
        assert!(out.success);
    }

    #[tokio::test]
    async fn system_runner_times_out_and_returns_partial_output() {
        let out = SystemCommandRunner
            .run("sh", &["-c", "echo partial; sleep 5"], 300)
            .await
            .unwrap();
        assert!(out.timed_out);
        assert!(!out.success);
        assert_eq!(out.stdout.trim(), "partial");
    }

    #[tokio::test]
    async fn system_runner_errors_on_missing_command() {
        let err = SystemCommandRunner
            .run("devpulse-no-such-command-xyz", &[], 500)
            .await;
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("failed to spawn"));
    }

    #[test]
    fn exists_finds_sh_but_not_bogus_command() {
        let runner = SystemCommandRunner;
        assert!(runner.exists("sh"));
        assert!(!runner.exists("devpulse-no-such-command-xyz"));
    }

    #[test]
    fn collect_options_defaults_match_design_section9() {
        let opts = CollectOptions::default();
        assert_eq!(opts.command_timeout_ms, 1000);
        assert_eq!(opts.quiet_threshold_s, 480);
    }

    // ---- 収集の入口の統合テスト（モック注入） ----

    use crate::data::processes::{ProcessInfo, ProcessSource};
    use chrono::TimeZone;
    use model::{AgentKind, Confidence, Orchestrator, SessionState, StateSource};
    use std::collections::HashMap;

    struct MockAgentSource {
        source_name: &'static str,
        available: bool,
        rows: Result<Vec<AgentSessionRow>, String>,
        events: Vec<ActivityEvent>,
    }

    #[async_trait]
    impl AgentSource for MockAgentSource {
        fn name(&self) -> &'static str {
            self.source_name
        }
        async fn is_available(&self) -> bool {
            self.available
        }
        async fn collect(&self) -> Result<Vec<AgentSessionRow>> {
            self.rows.clone().map_err(|e| anyhow::anyhow!("{e}"))
        }
        async fn activity(&self) -> Vec<ActivityEvent> {
            self.events.clone()
        }
    }

    struct MockProcessSource {
        processes: Vec<ProcessInfo>,
    }

    impl ProcessSource for MockProcessSource {
        fn list_processes(&self) -> Result<Vec<ProcessInfo>> {
            Ok(self.processes.clone())
        }
        fn kill_process(&self, _pid: u32, _force: bool) -> Result<()> {
            Ok(())
        }
    }

    struct MockTtyProvider {
        result: Result<HashMap<u32, String>, String>,
    }

    #[async_trait]
    impl process::TtyProvider for MockTtyProvider {
        async fn tty_by_pid(&self) -> Result<HashMap<u32, String>> {
            self.result.clone().map_err(|e| anyhow::anyhow!("{e}"))
        }
    }

    fn agent_proc(pid: u32, name: &str, cwd: &str) -> ProcessInfo {
        ProcessInfo {
            pid,
            name: name.into(),
            command: name.into(),
            user: "dev".into(),
            cpu_percent: 1.0,
            memory_bytes: 10_000_000,
            threads: 1,
            parent_pid: Some(1),
            listening_ports: vec![],
            start_time: 0,
            cwd: Some(PathBuf::from(cwd)),
        }
    }

    fn claude_reported_row(id: &str, cwd: &str, state: SessionState) -> AgentSessionRow {
        let mut row = AgentSessionRow::new(id, AgentKind::Claude, StateSource::ClaudeCli);
        row.cwd = Some(PathBuf::from(cwd));
        row.state = state;
        row.confidence = Confidence::Reported;
        row
    }

    fn test_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap()
    }

    async fn run_pipeline(
        sources: Vec<Box<dyn AgentSource>>,
        procs: Vec<ProcessInfo>,
        ttys: Result<HashMap<u32, String>, String>,
    ) -> AgentsSnapshot {
        let process_source = MockProcessSource { processes: procs };
        let tty_provider = MockTtyProvider { result: ttys };
        let mut git = gitinfo::GitEnricher::new(std::sync::Arc::new(SystemCommandRunner), 1000);
        collect_snapshot(
            &sources,
            &process_source,
            &tty_provider,
            &mut git,
            &CollectOptions::default(),
            test_now(),
        )
        .await
    }

    #[tokio::test]
    async fn collect_cycle_completion_emits_info_event() {
        let cap = crate::logging::capture::Capture::new();
        let _guard = tracing::subscriber::set_default(cap.subscriber());
        let sources: Vec<Box<dyn AgentSource>> = vec![Box::new(MockAgentSource {
            source_name: "claude",
            available: true,
            rows: Ok(vec![claude_reported_row(
                "aaa",
                "/nonexistent/devpulse-log-test",
                SessionState::Waiting,
            )]),
            events: vec![],
        })];
        run_pipeline(sources, vec![], Ok(HashMap::new())).await;
        assert_eq!(
            cap.count(tracing::Level::INFO, "agent collect cycle completed"),
            1
        );
        let ev = cap.find("agent collect cycle completed").unwrap();
        assert!(ev.fields.contains("elapsed_ms="), "{}", ev.fields);
        assert!(ev.fields.contains("claude=1"), "{}", ev.fields);
    }

    #[tokio::test]
    async fn failed_source_emits_warn_event_with_reason() {
        let cap = crate::logging::capture::Capture::new();
        let _guard = tracing::subscriber::set_default(cap.subscriber());
        let sources: Vec<Box<dyn AgentSource>> = vec![Box::new(MockAgentSource {
            source_name: "cmux",
            available: true,
            rows: Err("cmux tree timed out".into()),
            events: vec![],
        })];
        run_pipeline(sources, vec![], Ok(HashMap::new())).await;
        assert_eq!(cap.count(tracing::Level::WARN, "agent source failed"), 1);
        let ev = cap.find("agent source failed").unwrap();
        assert!(ev.fields.contains("source=cmux"), "{}", ev.fields);
        assert!(ev.fields.contains("timed out"), "{}", ev.fields);
    }

    #[tokio::test]
    async fn snapshot_merges_cli_row_with_process_and_applies_state() {
        let sources: Vec<Box<dyn AgentSource>> = vec![Box::new(MockAgentSource {
            source_name: "claude",
            available: true,
            rows: Ok(vec![claude_reported_row(
                "aaa",
                "/nonexistent/devpulse-test-dir",
                SessionState::Waiting,
            )]),
            events: vec![],
        })];
        let snapshot = run_pipeline(
            sources,
            vec![agent_proc(600, "claude", "/nonexistent/devpulse-test-dir")],
            Ok(HashMap::new()),
        )
        .await;
        assert!(snapshot.source_errors.is_empty());
        assert_eq!(snapshot.sessions.len(), 1);
        let row = &snapshot.sessions[0];
        assert_eq!(row.id, "aaa");
        assert_eq!(row.pid, Some(600));
        assert_eq!(row.state, SessionState::Waiting);
        assert_eq!(row.confidence, Confidence::Reported);
        assert_eq!(snapshot.collected_at, test_now());
    }

    #[tokio::test]
    async fn snapshot_records_partial_failures_and_keeps_other_sources() {
        // T1: 部分失敗（4取得元の一部だけ失敗）
        let sources: Vec<Box<dyn AgentSource>> = vec![
            Box::new(MockAgentSource {
                source_name: "cmux",
                available: true,
                rows: Err("cmux tree timed out after 1000ms".into()),
                events: vec![],
            }),
            Box::new(MockAgentSource {
                source_name: "kimi",
                available: false,
                rows: Ok(vec![]),
                events: vec![],
            }),
            Box::new(MockAgentSource {
                source_name: "claude",
                available: true,
                rows: Ok(vec![claude_reported_row(
                    "bbb",
                    "/tmp/x",
                    SessionState::Failed,
                )]),
                events: vec![],
            }),
        ];
        let snapshot = run_pipeline(sources, vec![], Ok(HashMap::new())).await;
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].id, "bbb");
        assert_eq!(snapshot.source_errors.len(), 2);
        let cmux_err = snapshot
            .source_errors
            .iter()
            .find(|e| e.source == "cmux")
            .unwrap();
        assert!(cmux_err.error.contains("timed out"));
        let kimi_err = snapshot
            .source_errors
            .iter()
            .find(|e| e.source == "kimi")
            .unwrap();
        assert_eq!(kimi_err.error, "unavailable");
    }

    #[tokio::test]
    async fn snapshot_with_all_sources_failed_is_empty_but_reported() {
        let sources: Vec<Box<dyn AgentSource>> = vec![Box::new(MockAgentSource {
            source_name: "cmux",
            available: true,
            rows: Err("boom".into()),
            events: vec![],
        })];
        let snapshot = run_pipeline(sources, vec![], Err("ps: exec failed".into())).await;
        assert!(snapshot.sessions.is_empty());
        let sources_with_errors: Vec<_> = snapshot
            .source_errors
            .iter()
            .map(|e| e.source.as_str())
            .collect();
        assert!(sources_with_errors.contains(&"cmux"));
        assert!(sources_with_errors.contains(&"ps"));
    }

    #[tokio::test]
    async fn snapshot_applies_cmux_activity_by_session_id_and_derives_quiet() {
        // 順6のquietへの到達経路: session_idが一致したcmux eventsのlast_activity_at由来。
        // cwd一致だけの行（session_id照合の相手がいないcmux surface行）には付けず、
        // unknownのままにする（誤ったquietより正直な表示。誤帰属の実測に基づく仕様変更）。
        let cwd = "/nonexistent/devpulse-quiet-dir";
        let mut cmux_surface = AgentSessionRow::new(
            "surface:1",
            AgentKind::Other("unknown".into()),
            StateSource::CmuxCli,
        );
        cmux_surface.orchestrator = Orchestrator::Cmux;
        cmux_surface.location = "workspace:1/surface:1".into();
        cmux_surface.tty = Some("ttys000".into());
        let mut claude_cli_row =
            AgentSessionRow::new("9e0112cf", AgentKind::Claude, StateSource::ClaudeCli);
        claude_cli_row.cwd = Some(PathBuf::from("/nonexistent/devpulse-other-dir"));
        claude_cli_row.session_id = Some("9e0112cf-9e45-4f76-b22c-64a0712ac9db".into());
        let sources: Vec<Box<dyn AgentSource>> = vec![
            Box::new(MockAgentSource {
                source_name: "cmux",
                available: true,
                rows: Ok(vec![cmux_surface]),
                events: vec![ActivityEvent {
                    session_id: "claude-9e0112cf-9e45-4f76-b22c-64a0712ac9db".into(),
                    // フック時の作業ディレクトリはsurface側プロセスのcwdと同一（誤帰属の再現）
                    cwd: PathBuf::from(cwd),
                    occurred_at: test_now() - chrono::Duration::seconds(1000),
                }],
            }),
            Box::new(MockAgentSource {
                source_name: "claude",
                available: true,
                rows: Ok(vec![claude_cli_row]),
                events: vec![],
            }),
        ];
        let snapshot = run_pipeline(
            sources,
            vec![agent_proc(91658, "claude", cwd)],
            Ok(HashMap::from([(91658, "ttys000".to_string())])),
        )
        .await;
        assert_eq!(snapshot.sessions.len(), 2);
        // cwdが一致するsurface行にはイベントを付けない（session_id照合の相手がいない）
        let surface = snapshot
            .sessions
            .iter()
            .find(|r| r.orchestrator == Orchestrator::Cmux)
            .unwrap();
        assert_eq!(surface.last_activity_at, None);
        assert_eq!(surface.state, SessionState::Unknown);
        // session_idが一致したclaude行にはイベントが付きquietへ至る
        let claude = snapshot
            .sessions
            .iter()
            .find(|r| r.id == "9e0112cf")
            .unwrap();
        assert_eq!(
            claude.last_activity_at,
            Some(test_now() - chrono::Duration::seconds(1000))
        );
        assert_eq!(claude.state, SessionState::Quiet { elapsed_s: 1000 });
        assert_eq!(claude.state_source, StateSource::CmuxCli);
        assert_eq!(claude.confidence, Confidence::Derived);
    }

    #[tokio::test]
    async fn snapshot_collection_is_idempotent_for_same_inputs() {
        // T3冪等性: 同一入力で収集を2回実行して結果が同一で重複統合が発生しない
        let build_sources = || -> Vec<Box<dyn AgentSource>> {
            vec![Box::new(MockAgentSource {
                source_name: "claude",
                available: true,
                rows: Ok(vec![claude_reported_row(
                    "aaa",
                    "/tmp/x",
                    SessionState::Idle,
                )]),
                events: vec![],
            })]
        };
        let procs = vec![agent_proc(600, "claude", "/tmp/x")];
        let s1 = run_pipeline(build_sources(), procs.clone(), Ok(HashMap::new())).await;
        let s2 = run_pipeline(build_sources(), procs, Ok(HashMap::new())).await;
        assert_eq!(s1.sessions, s2.sessions);
        assert_eq!(s1.sessions.len(), 1);
    }
}
