use anyhow::Result;

#[derive(Debug, Clone)]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone)]
pub struct PortEntry {
    pub port: u16,
    pub protocol: Protocol,
    pub address: String,
    pub pid: u32,
    pub process_name: String,
    pub command: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

pub trait PortScanner: Send + Sync {
    fn scan(&self) -> Result<Vec<PortEntry>>;
}

pub struct SystemPortScanner;

impl PortScanner for SystemPortScanner {
    fn scan(&self) -> Result<Vec<PortEntry>> {
        Ok(vec![]) // OS-specific impl in Task 11
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockPortScanner { entries: Vec<PortEntry> }

    impl PortScanner for MockPortScanner {
        fn scan(&self) -> Result<Vec<PortEntry>> { Ok(self.entries.clone()) }
    }

    #[test]
    fn test_port_entry_fields() {
        let entry = PortEntry {
            port: 3000, protocol: Protocol::Tcp, address: "127.0.0.1".to_string(),
            pid: 1234, process_name: "next-dev".to_string(), command: "node .next/server.js".to_string(),
            cpu_percent: 12.5, memory_bytes: 340_000_000,
        };
        assert_eq!(entry.port, 3000);
        assert_eq!(entry.process_name, "next-dev");
        assert!(matches!(entry.protocol, Protocol::Tcp));
    }

    #[test]
    fn test_mock_scanner_returns_entries() {
        let scanner = MockPortScanner {
            entries: vec![
                PortEntry { port: 3000, protocol: Protocol::Tcp, address: "0.0.0.0".into(),
                    pid: 100, process_name: "node".into(), command: "node server.js".into(),
                    cpu_percent: 5.0, memory_bytes: 100_000_000 },
                PortEntry { port: 5432, protocol: Protocol::Tcp, address: "127.0.0.1".into(),
                    pid: 200, process_name: "postgres".into(), command: "postgres -D /var/lib/postgresql".into(),
                    cpu_percent: 2.0, memory_bytes: 50_000_000 },
            ],
        };
        let results = scanner.scan().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].port, 3000);
        assert_eq!(results[1].port, 5432);
    }

    #[test]
    fn test_empty_scan() {
        let scanner = MockPortScanner { entries: vec![] };
        assert!(scanner.scan().unwrap().is_empty());
    }
}
