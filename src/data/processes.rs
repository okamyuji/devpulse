use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub command: String,
    pub user: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub threads: u32,
    pub parent_pid: Option<u32>,
    pub listening_ports: Vec<u16>,
    pub start_time: u64,
    /// プロセスのカレントディレクトリ（sysinfoから取得。取得不能時はNone）
    pub cwd: Option<PathBuf>,
}

pub trait ProcessSource: Send + Sync {
    fn list_processes(&self) -> Result<Vec<ProcessInfo>>;
    fn kill_process(&self, pid: u32, force: bool) -> Result<()>;
}

/// sysinfoによるProcessSource実装。エージェント収集（collectorと背景タスク）が使う。
/// System状態を保持して再走査するためCPU差分が計算される。読み取り専用でkillは非対応。
/// user/threads/portsは収集対象外のため空値（app.rs tick()と同じ規則）。
pub struct SysinfoProcessSource {
    sys: std::sync::Mutex<sysinfo::System>,
}

impl SysinfoProcessSource {
    pub fn new() -> Self {
        Self {
            sys: std::sync::Mutex::new(sysinfo::System::new()),
        }
    }
}

impl Default for SysinfoProcessSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessSource for SysinfoProcessSource {
    fn list_processes(&self) -> Result<Vec<ProcessInfo>> {
        let mut sys = self
            .sys
            .lock()
            .map_err(|_| anyhow::anyhow!("sysinfo lock poisoned"))?;
        // refresh_processesの既定ではcwdが更新されないため明示的に要求する
        sys.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::everything().with_cwd(sysinfo::UpdateKind::Always),
        );
        Ok(sys
            .processes()
            .values()
            .map(|p| ProcessInfo {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().to_string(),
                command: p
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
                user: String::new(),
                cpu_percent: p.cpu_usage(),
                memory_bytes: p.memory(),
                threads: 0,
                parent_pid: p.parent().map(|pp| pp.as_u32()),
                listening_ports: Vec::new(),
                start_time: p.start_time(),
                cwd: p.cwd().map(|c| c.to_path_buf()),
            })
            .collect())
    }

    fn kill_process(&self, _pid: u32, _force: bool) -> Result<()> {
        anyhow::bail!("kill is not supported by the agents collector")
    }
}

#[cfg(test)]
mod sysinfo_source_tests {
    use super::*;

    // 回帰: プロセス行のcwdが全件Noneでworktree競合検出が機能しなかった
    // (2026-07-18 手動検証で発見)。自プロセスのcwdは必ず取得できること。
    #[test]
    fn regression_sysinfo_source_fills_cwd_for_own_process() {
        let src = SysinfoProcessSource::new();
        let procs = src.list_processes().expect("list_processes");
        let me = std::process::id();
        let row = procs
            .into_iter()
            .find(|p| p.pid == me)
            .expect("own process should be listed");
        assert!(
            row.cwd.is_some(),
            "cwd must be populated for own process, got None"
        );
        assert_eq!(
            row.cwd.unwrap(),
            std::env::current_dir().expect("current_dir"),
            "cwd must equal the actual current directory"
        );
    }
}

const DEV_PROCESSES: &[&str] = &[
    "node", "python", "python3", "java", "go", "cargo", "rustc", "ruby", "php", "docker", "npm",
    "yarn", "pnpm", "bun", "deno", "gradle", "mvn", "dotnet", "mix", "elixir",
];

pub fn is_dev_process(name: &str) -> bool {
    let lower = name.to_lowercase();
    DEV_PROCESSES.iter().any(|&dev| lower.starts_with(dev))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_process_info_fields() {
        let proc = ProcessInfo {
            pid: 1234,
            name: "node".into(),
            command: "node server.js".into(),
            user: "yuji".into(),
            cpu_percent: 12.5,
            memory_bytes: 340_000_000,
            threads: 8,
            parent_pid: Some(1),
            listening_ports: vec![3000, 3001],
            start_time: 1700000000,
            cwd: Some(PathBuf::from("/Users/dev/app")),
        };
        assert_eq!(proc.pid, 1234);
        assert_eq!(proc.listening_ports.len(), 2);
        assert_eq!(proc.cwd, Some(PathBuf::from("/Users/dev/app")));
    }

    #[test]
    fn test_is_dev_process() {
        assert!(is_dev_process("node"));
        assert!(is_dev_process("python3"));
        assert!(is_dev_process("java"));
        assert!(is_dev_process("cargo"));
        assert!(is_dev_process("go"));
        assert!(is_dev_process("ruby"));
        assert!(is_dev_process("docker"));
        assert!(!is_dev_process("systemd"));
        assert!(!is_dev_process("launchd"));
    }

    #[test]
    fn test_mock_kill_process() {
        let source = MockProcessSource { processes: vec![] };
        assert!(source.kill_process(1234, false).is_ok());
        assert!(source.kill_process(1234, true).is_ok());
    }
}
