use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub ports: PortsConfig,
    pub docker: DockerConfig,
    pub processes: ProcessesConfig,
    pub logs: LogsConfig,
    pub theme: ThemeConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub refresh_rate_ms: u64,
    pub default_layout: String,
    pub confirm_destructive: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PortsConfig {
    pub sort_by: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DockerConfig {
    /// Docker daemon endpoint. Accepted values (leading/trailing whitespace
    /// is trimmed; the string `"auto"` is matched case-insensitively):
    ///
    /// - `"auto"` (default): auto-detect via `DOCKER_HOST`, the Docker CLI
    ///   context, and well-known socket probes
    /// - scheme URL: `unix://...`, `http(s)://...`, `tcp://...`, `npipe://...`
    /// - bare absolute Unix socket path, e.g. `/var/run/docker.sock`
    /// - bare Windows named-pipe path, e.g. `\\.\pipe\docker_engine`
    pub socket_path: String,
    pub show_stopped: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProcessesConfig {
    pub default_view: String,
    pub dev_process_priority: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum LogSourceConfig {
    #[serde(rename = "docker")]
    Docker {
        #[serde(default = "default_containers")]
        containers: String,
    },
    #[serde(rename = "file")]
    File { path: String },
}

fn default_containers() -> String {
    "all".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LogsConfig {
    pub sources: Vec<LogSourceConfig>,
    pub buffer_lines: usize,
    pub tail_follow: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: String,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            refresh_rate_ms: 2000,
            default_layout: "quad".to_string(),
            confirm_destructive: true,
        }
    }
}

impl Default for PortsConfig {
    fn default() -> Self {
        Self {
            sort_by: "port".to_string(),
        }
    }
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            socket_path: "auto".to_string(),
            show_stopped: true,
        }
    }
}

impl Default for ProcessesConfig {
    fn default() -> Self {
        Self {
            default_view: "flat".to_string(),
            dev_process_priority: true,
        }
    }
}

impl Default for LogsConfig {
    fn default() -> Self {
        Self {
            sources: vec![LogSourceConfig::Docker {
                containers: "all".to_string(),
            }],
            buffer_lines: 10000,
            tail_follow: true,
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: "dark".to_string(),
        }
    }
}

impl LogSourceConfig {
    pub fn is_docker(&self) -> bool {
        matches!(self, Self::Docker { .. })
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Self::File { .. })
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;
        config.general.refresh_rate_ms = config.general.refresh_rate_ms.clamp(1000, 30000);
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_default_logs_config_has_docker_source() {
        let config = LogsConfig::default();
        assert_eq!(config.sources.len(), 1);
        assert!(config.sources[0].is_docker());
        assert_eq!(config.buffer_lines, 10000);
        assert!(config.tail_follow);
    }

    #[test]
    fn test_log_source_config_docker_default_containers() {
        let src = LogSourceConfig::Docker {
            containers: "all".to_string(),
        };
        assert!(src.is_docker());
        assert!(!src.is_file());
    }

    #[test]
    fn test_log_source_config_file() {
        let src = LogSourceConfig::File {
            path: "/var/log/*.log".to_string(),
        };
        assert!(src.is_file());
        assert!(!src.is_docker());
    }

    #[test]
    fn test_parse_toml_with_log_sources() {
        let toml_str = r#"
[logs]
buffer_lines = 5000
tail_follow = false

[[logs.sources]]
type = "docker"
containers = "all"

[[logs.sources]]
type = "file"
path = "/tmp/app.log"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.logs.sources.len(), 2);
        assert!(config.logs.sources[0].is_docker());
        assert!(config.logs.sources[1].is_file());
        assert_eq!(config.logs.buffer_lines, 5000);
        assert!(!config.logs.tail_follow);
    }

    #[test]
    fn test_parse_toml_without_log_sources_uses_default() {
        let toml_str = r#"
[general]
refresh_rate_ms = 3000
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.logs.sources.len(), 1);
        assert!(config.logs.sources[0].is_docker());
    }

    #[test]
    fn test_load_missing_file_uses_defaults() {
        let config = Config::load(std::path::Path::new("/nonexistent/config.toml")).unwrap();
        assert_eq!(config.logs.sources.len(), 1);
        assert!(config.logs.sources[0].is_docker());
    }

    #[test]
    fn test_load_toml_file_with_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"
[[logs.sources]]
type = "file"
path = "/tmp/test.log"
"#
        )
        .unwrap();
        let config = Config::load(&path).unwrap();
        assert_eq!(config.logs.sources.len(), 1);
        assert!(config.logs.sources[0].is_file());
    }
}
