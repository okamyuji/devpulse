use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: u64,
    pub source: String,
    pub level: LogLevel,
    pub message: String,
}

pub struct LogBuffer {
    entries: VecDeque<LogEntry>,
    capacity: usize,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity.min(1024)),
            capacity,
        }
    }
    pub fn push(&mut self, entry: LogEntry) {
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }
    pub fn entries(&self) -> &VecDeque<LogEntry> {
        &self.entries
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_buffer_append_and_capacity() {
        let mut buffer = LogBuffer::new(3);
        buffer.push(LogEntry {
            timestamp: 1,
            source: "app".into(),
            level: LogLevel::Info,
            message: "msg1".into(),
        });
        buffer.push(LogEntry {
            timestamp: 2,
            source: "app".into(),
            level: LogLevel::Info,
            message: "msg2".into(),
        });
        buffer.push(LogEntry {
            timestamp: 3,
            source: "app".into(),
            level: LogLevel::Error,
            message: "msg3".into(),
        });
        assert_eq!(buffer.len(), 3);
        buffer.push(LogEntry {
            timestamp: 4,
            source: "db".into(),
            level: LogLevel::Warn,
            message: "msg4".into(),
        });
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.entries()[0].message, "msg2");
    }

    #[test]
    fn test_log_buffer_filter() {
        let mut buffer = LogBuffer::new(100);
        buffer.push(LogEntry {
            timestamp: 1,
            source: "app-web".into(),
            level: LogLevel::Info,
            message: "request".into(),
        });
        buffer.push(LogEntry {
            timestamp: 2,
            source: "app-db".into(),
            level: LogLevel::Error,
            message: "timeout".into(),
        });
        buffer.push(LogEntry {
            timestamp: 3,
            source: "app-web".into(),
            level: LogLevel::Warn,
            message: "slow".into(),
        });
        let filtered: Vec<_> = buffer
            .entries()
            .iter()
            .filter(|e| e.source == "app-web")
            .collect();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(LogLevel::Info.as_str(), "INFO");
        assert_eq!(LogLevel::Warn.as_str(), "WARN");
        assert_eq!(LogLevel::Error.as_str(), "ERROR");
    }

    #[test]
    fn test_empty_buffer() {
        let buffer = LogBuffer::new(100);
        assert_eq!(buffer.len(), 0);
        assert!(buffer.entries().is_empty());
    }
}
