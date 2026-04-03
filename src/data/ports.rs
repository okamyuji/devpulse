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
        platform::scan_ports()
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::process::Command;

    pub fn scan_ports() -> Result<Vec<PortEntry>> {
        let output = Command::new("lsof")
            .args(["-iTCP", "-sTCP:LISTEN", "-P", "-n"])
            .output()?;
        if !output.status.success() {
            return Ok(vec![]);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut entries = Vec::new();
        for line in stdout.lines().skip(1) {
            if let Some(entry) = parse_lsof_line(line) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    fn parse_lsof_line(line: &str) -> Option<PortEntry> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            return None;
        }
        let process_name = parts[0].to_string();
        let pid: u32 = parts[1].parse().ok()?;
        let name_field = parts[8];
        let port = name_field.rsplit(':').next()?.parse::<u16>().ok()?;
        Some(PortEntry {
            port,
            protocol: Protocol::Tcp,
            address: name_field
                .rsplit_once(':')
                .map(|x| x.0)
                .unwrap_or("*")
                .to_string(),
            pid,
            process_name,
            command: String::new(),
            cpu_percent: 0.0,
            memory_bytes: 0,
        })
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::fs;

    pub fn scan_ports() -> Result<Vec<PortEntry>> {
        let content = fs::read_to_string("/proc/net/tcp").unwrap_or_default();
        let mut entries = Vec::new();
        for line in content.lines().skip(1) {
            if let Some(entry) = parse_proc_net_tcp_line(line) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    fn parse_proc_net_tcp_line(line: &str) -> Option<PortEntry> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            return None;
        }
        if parts[3] != "0A" {
            return None;
        }
        let local_addr = parts[1];
        let addr_parts: Vec<&str> = local_addr.split(':').collect();
        if addr_parts.len() != 2 {
            return None;
        }
        let port = u16::from_str_radix(addr_parts[1], 16).ok()?;
        Some(PortEntry {
            port,
            protocol: Protocol::Tcp,
            address: format_hex_addr(addr_parts[0]),
            pid: 0,
            process_name: String::new(),
            command: String::new(),
            cpu_percent: 0.0,
            memory_bytes: 0,
        })
    }

    fn format_hex_addr(hex: &str) -> String {
        if hex.len() != 8 {
            return hex.to_string();
        }
        let bytes: Vec<u8> = (0..8)
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect();
        if bytes.len() == 4 {
            format!("{}.{}.{}.{}", bytes[3], bytes[2], bytes[1], bytes[0])
        } else {
            hex.to_string()
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;
    pub fn scan_ports() -> Result<Vec<PortEntry>> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockPortScanner {
        entries: Vec<PortEntry>,
    }

    impl PortScanner for MockPortScanner {
        fn scan(&self) -> Result<Vec<PortEntry>> {
            Ok(self.entries.clone())
        }
    }

    #[test]
    fn test_port_entry_fields() {
        let entry = PortEntry {
            port: 3000,
            protocol: Protocol::Tcp,
            address: "127.0.0.1".to_string(),
            pid: 1234,
            process_name: "next-dev".to_string(),
            command: "node .next/server.js".to_string(),
            cpu_percent: 12.5,
            memory_bytes: 340_000_000,
        };
        assert_eq!(entry.port, 3000);
        assert_eq!(entry.process_name, "next-dev");
        assert!(matches!(entry.protocol, Protocol::Tcp));
    }

    #[test]
    fn test_mock_scanner_returns_entries() {
        let scanner = MockPortScanner {
            entries: vec![
                PortEntry {
                    port: 3000,
                    protocol: Protocol::Tcp,
                    address: "0.0.0.0".into(),
                    pid: 100,
                    process_name: "node".into(),
                    command: "node server.js".into(),
                    cpu_percent: 5.0,
                    memory_bytes: 100_000_000,
                },
                PortEntry {
                    port: 5432,
                    protocol: Protocol::Tcp,
                    address: "127.0.0.1".into(),
                    pid: 200,
                    process_name: "postgres".into(),
                    command: "postgres -D /var/lib/postgresql".into(),
                    cpu_percent: 2.0,
                    memory_bytes: 50_000_000,
                },
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
