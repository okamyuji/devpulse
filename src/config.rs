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
    pub socket_path: String,
    pub show_stopped: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProcessesConfig {
    pub default_view: String,
    pub dev_process_priority: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LogsConfig {
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
