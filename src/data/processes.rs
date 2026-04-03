use anyhow::Result;

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
}

pub trait ProcessSource: Send + Sync {
    fn list_processes(&self) -> Result<Vec<ProcessInfo>>;
    fn kill_process(&self, pid: u32, force: bool) -> Result<()>;
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
        };
        assert_eq!(proc.pid, 1234);
        assert_eq!(proc.listening_ports.len(), 2);
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
