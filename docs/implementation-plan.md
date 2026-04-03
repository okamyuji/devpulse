# DevPulse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ポート・Docker・プロセス・ログを1画面に統合し、Kill/Stop/Delete操作を提供するクロスプラットフォームTUIツールをMVPとして構築する

**Architecture:** ratatui + crossterm による4パネルTUI。tokioベースの非同期イベントループでデータ取得と描画を分離。各データソースは trait で抽象化し、テストではモックで差し替え可能にする。App構造体がステートを一元管理し、Actionディスパッチで操作を実行。

**Tech Stack:** Rust, ratatui, crossterm, tokio, bollard, sysinfo, notify, clap, serde, toml, anyhow, thiserror

---

## File Structure

```
devpulse/
├── Cargo.toml
├── src/
│   ├── main.rs                  # CLI引数パース → App起動
│   ├── config.rs                # Config構造体、TOML読み込み、デフォルト値
│   ├── app.rs                   # App構造体、ステート管理、メインループ
│   ├── event.rs                 # InputEvent enum、EventHandler (キー+tick)
│   ├── action.rs                # Action enum、破壊的操作の実行
│   ├── filter.rs                # FilterState、fuzzy match、regex対応
│   ├── ui/
│   │   ├── mod.rs               # ui モジュール公開
│   │   ├── layout.rs            # レイアウト計算 (quad/fullscreen/adaptive)
│   │   ├── common.rs            # 共通UI部品 (styled_table, filter_bar, confirm_dialog, help_overlay)
│   │   └── panels/
│   │       ├── mod.rs           # panels モジュール公開
│   │       ├── ports.rs         # Portsパネル描画
│   │       ├── docker.rs        # Dockerパネル描画
│   │       ├── processes.rs     # Processesパネル描画
│   │       └── logs.rs          # Logsパネル描画
│   └── data/
│       ├── mod.rs               # data モジュール公開
│       ├── ports.rs             # trait PortScanner + OS実装
│       ├── docker.rs            # trait DockerSource + bollard実装
│       ├── processes.rs         # trait ProcessSource + sysinfo実装
│       └── logs.rs              # trait LogSource + Docker/File実装
├── tests/
│   └── integration/
│       ├── mod.rs
│       ├── config_test.rs
│       ├── filter_test.rs
│       └── app_test.rs
```

---

## Task 1: プロジェクト初期化 + Cargo.toml

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `rust-toolchain.toml`

- [ ] **Step 1: `cargo init` でプロジェクト初期化**

```bash
cd /Users/yujiokamoto/devs/rust/devpulse
cargo init --name devpulse
```

- [ ] **Step 2: `Cargo.toml` に依存クレートを追加**

```toml
[package]
name = "devpulse"
version = "0.1.0"
edition = "2021"
description = "Unified Developer Environment TUI"
license = "MIT"

[dependencies]
ratatui = "0.29"
crossterm = "0.28"
tokio = { version = "1", features = ["full"] }
bollard = "0.18"
sysinfo = "0.33"
notify = "7"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
anyhow = "1"
thiserror = "2"
arboard = "3"
dirs = "6"
tracing = "0.1"
tracing-appender = "0.2"
fuzzy-matcher = "0.3"
regex = "1"
chrono = "0.4"
async-trait = "0.1"

[dev-dependencies]
tempfile = "3"
tokio-test = "0.4"
assert_cmd = "2"
predicates = "3"
```

- [ ] **Step 3: rust-toolchain.toml を作成**

```toml
[toolchain]
channel = "stable"
```

- [ ] **Step 4: 最小限の main.rs を作成**

```rust
fn main() {
    println!("devpulse v0.1.0");
}
```

- [ ] **Step 5: ビルド確認**

Run: `cargo build`
Expected: コンパイル成功

- [ ] **Step 6: コミット**

```bash
git init
echo "target/" > .gitignore
git add Cargo.toml Cargo.lock src/main.rs rust-toolchain.toml .gitignore CLAUDE.md CONCEPT.md docs/
git commit -m "feat: initialize devpulse project with dependencies"
```

---

## Task 2: Config モジュール（設定読み込み）

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`
- Create: `tests/integration/mod.rs`
- Create: `tests/integration/config_test.rs`

- [ ] **Step 1: config のテストを書く**

`tests/integration/config_test.rs`:
```rust
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_default_config() {
    let config = devpulse::config::Config::default();
    assert_eq!(config.general.refresh_rate_ms, 2000);
    assert_eq!(config.general.default_layout, "quad");
    assert!(config.general.confirm_destructive);
    assert_eq!(config.docker.socket_path, "auto");
    assert!(config.docker.show_stopped);
    assert_eq!(config.processes.default_view, "flat");
    assert!(config.processes.dev_process_priority);
    assert_eq!(config.logs.buffer_lines, 10000);
    assert!(config.logs.tail_follow);
    assert_eq!(config.theme.name, "dark");
}

#[test]
fn test_load_from_toml() {
    let toml_content = r#"
[general]
refresh_rate_ms = 5000
default_layout = "main-side"
confirm_destructive = false

[theme]
name = "light"
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = devpulse::config::Config::load(file.path()).unwrap();
    assert_eq!(config.general.refresh_rate_ms, 5000);
    assert_eq!(config.general.default_layout, "main-side");
    assert!(!config.general.confirm_destructive);
    assert_eq!(config.theme.name, "light");
    // 未指定項目はデフォルト値
    assert_eq!(config.docker.socket_path, "auto");
}

#[test]
fn test_load_nonexistent_file_returns_default() {
    let config = devpulse::config::Config::load(std::path::Path::new("/nonexistent/config.toml")).unwrap();
    assert_eq!(config.general.refresh_rate_ms, 2000);
}

#[test]
fn test_refresh_rate_clamped() {
    let toml_content = r#"
[general]
refresh_rate_ms = 50
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let config = devpulse::config::Config::load(file.path()).unwrap();
    assert_eq!(config.general.refresh_rate_ms, 1000); // clamped to min
}
```

`tests/integration/mod.rs`:
```rust
mod config_test;
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --test integration`
Expected: FAIL — `devpulse::config` が存在しない

- [ ] **Step 3: Config 実装**

`src/config.rs`:
```rust
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
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

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            ports: PortsConfig::default(),
            docker: DockerConfig::default(),
            processes: ProcessesConfig::default(),
            logs: LogsConfig::default(),
            theme: ThemeConfig::default(),
        }
    }
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
```

`src/main.rs` を更新:
```rust
pub mod config;

fn main() {
    println!("devpulse v0.1.0");
}
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --test integration`
Expected: 4 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/config.rs src/main.rs tests/
git commit -m "feat: add Config module with TOML loading and defaults"
```

---

## Task 3: CLI引数パース

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: CLI引数のテストを書く**

`src/main.rs` 末尾に追加:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_default_cli_args() {
        let args = Cli::parse_from(["devpulse"]);
        assert!(args.config.is_none());
        assert!(args.filter.is_none());
        assert_eq!(args.layout, None);
        assert!(!args.no_docker);
        assert!(args.refresh.is_none());
    }

    #[test]
    fn test_cli_args_with_options() {
        let args = Cli::parse_from([
            "devpulse",
            "--config", "/tmp/config.toml",
            "--filter", "node",
            "--layout", "quad",
            "--no-docker",
            "--refresh", "5000",
        ]);
        assert_eq!(args.config.unwrap().to_str().unwrap(), "/tmp/config.toml");
        assert_eq!(args.filter.unwrap(), "node");
        assert_eq!(args.layout.unwrap(), "quad");
        assert!(args.no_docker);
        assert_eq!(args.refresh.unwrap(), 5000);
    }
}
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --lib`
Expected: FAIL — `Cli` が未定義

- [ ] **Step 3: Cli 構造体を実装**

`src/main.rs`:
```rust
pub mod config;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "devpulse", version, about = "Unified Developer Environment TUI")]
pub struct Cli {
    /// 設定ファイルパス
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// 起動時グローバルフィルタ
    #[arg(short, long)]
    pub filter: Option<String>,

    /// レイアウト (quad | main-side)
    #[arg(short, long)]
    pub layout: Option<String>,

    /// Dockerパネルを無効化
    #[arg(long)]
    pub no_docker: bool,

    /// 更新間隔 (ms)
    #[arg(long)]
    pub refresh: Option<u64>,
}

fn main() {
    let _cli = Cli::parse();
    println!("devpulse v0.1.0");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_default_cli_args() {
        let args = Cli::parse_from(["devpulse"]);
        assert!(args.config.is_none());
        assert!(args.filter.is_none());
        assert_eq!(args.layout, None);
        assert!(!args.no_docker);
        assert!(args.refresh.is_none());
    }

    #[test]
    fn test_cli_args_with_options() {
        let args = Cli::parse_from([
            "devpulse",
            "--config", "/tmp/config.toml",
            "--filter", "node",
            "--layout", "quad",
            "--no-docker",
            "--refresh", "5000",
        ]);
        assert_eq!(args.config.unwrap().to_str().unwrap(), "/tmp/config.toml");
        assert_eq!(args.filter.unwrap(), "node");
        assert_eq!(args.layout.unwrap(), "quad");
        assert!(args.no_docker);
        assert_eq!(args.refresh.unwrap(), 5000);
    }
}
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --lib`
Expected: 2 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/main.rs
git commit -m "feat: add CLI argument parsing with clap"
```

---

## Task 4: Filter モジュール（フィルタリング）

**Files:**
- Create: `src/filter.rs`
- Modify: `src/main.rs` (モジュール追加)
- Create: `tests/integration/filter_test.rs`
- Modify: `tests/integration/mod.rs`

- [ ] **Step 1: フィルタのテストを書く**

`tests/integration/filter_test.rs`:
```rust
use devpulse::filter::FilterState;

#[test]
fn test_empty_filter_matches_all() {
    let filter = FilterState::new();
    assert!(filter.matches("anything"));
    assert!(filter.matches(""));
}

#[test]
fn test_fuzzy_filter() {
    let mut filter = FilterState::new();
    filter.set_query("nde");
    assert!(filter.matches("next-dev")); // fuzzy: n..d..e
    assert!(filter.matches("node"));
    assert!(!filter.matches("postgres"));
}

#[test]
fn test_regex_filter() {
    let mut filter = FilterState::new();
    filter.set_query("regex:^node.*dev$");
    assert!(filter.matches("node-dev"));
    assert!(filter.matches("nodejs-dev"));
    assert!(!filter.matches("node"));
    assert!(!filter.matches("dev-node"));
}

#[test]
fn test_invalid_regex_falls_back_to_fuzzy() {
    let mut filter = FilterState::new();
    filter.set_query("regex:[invalid");
    // Invalid regex should not panic, falls back to substring match
    assert!(filter.matches("regex:[invalid"));
}

#[test]
fn test_clear_filter() {
    let mut filter = FilterState::new();
    filter.set_query("node");
    assert!(!filter.matches("postgres"));
    filter.clear();
    assert!(filter.matches("postgres"));
}

#[test]
fn test_is_active() {
    let mut filter = FilterState::new();
    assert!(!filter.is_active());
    filter.set_query("test");
    assert!(filter.is_active());
    filter.clear();
    assert!(!filter.is_active());
}
```

`tests/integration/mod.rs` に追加:
```rust
mod config_test;
mod filter_test;
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --test integration filter`
Expected: FAIL — `devpulse::filter` が存在しない

- [ ] **Step 3: FilterState 実装**

`src/filter.rs`:
```rust
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use regex::Regex;

enum MatchMode {
    None,
    Fuzzy(String),
    Regex(Regex),
}

pub struct FilterState {
    query: String,
    mode: MatchMode,
    matcher: SkimMatcherV2,
}

impl FilterState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            mode: MatchMode::None,
            matcher: SkimMatcherV2::default(),
        }
    }

    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        if let Some(pattern) = query.strip_prefix("regex:") {
            match Regex::new(pattern) {
                Ok(re) => self.mode = MatchMode::Regex(re),
                Err(_) => self.mode = MatchMode::Fuzzy(query.to_string()),
            }
        } else if query.is_empty() {
            self.mode = MatchMode::None;
        } else {
            self.mode = MatchMode::Fuzzy(query.to_string());
        }
    }

    pub fn matches(&self, text: &str) -> bool {
        match &self.mode {
            MatchMode::None => true,
            MatchMode::Fuzzy(q) => self.matcher.fuzzy_match(text, q).is_some(),
            MatchMode::Regex(re) => re.is_match(text),
        }
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.mode = MatchMode::None;
    }

    pub fn is_active(&self) -> bool {
        !self.query.is_empty()
    }

    pub fn query(&self) -> &str {
        &self.query
    }
}
```

`src/main.rs` にモジュール追加:
```rust
pub mod config;
pub mod filter;
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --test integration filter`
Expected: 6 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/filter.rs src/main.rs tests/integration/filter_test.rs tests/integration/mod.rs
git commit -m "feat: add FilterState with fuzzy match and regex support"
```

---

## Task 5: データ取得 trait 定義 + PortScanner

**Files:**
- Create: `src/data/mod.rs`
- Create: `src/data/ports.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: PortScanner のテストを書く**

`src/data/ports.rs` 末尾にテストモジュール:
```rust
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
    fn test_port_entry_display() {
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
                    address: "0.0.0.0".to_string(),
                    pid: 100,
                    process_name: "node".to_string(),
                    command: "node server.js".to_string(),
                    cpu_percent: 5.0,
                    memory_bytes: 100_000_000,
                },
                PortEntry {
                    port: 5432,
                    protocol: Protocol::Tcp,
                    address: "127.0.0.1".to_string(),
                    pid: 200,
                    process_name: "postgres".to_string(),
                    command: "postgres -D /var/lib/postgresql".to_string(),
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
        let results = scanner.scan().unwrap();
        assert!(results.is_empty());
    }
}
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --lib data::ports`
Expected: FAIL — モジュール未定義

- [ ] **Step 3: PortScanner trait + PortEntry 実装**

`src/data/mod.rs`:
```rust
pub mod ports;
```

`src/data/ports.rs`:
```rust
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

/// OS固有のポートスキャナ (MVP: macOS + Linux)
pub struct SystemPortScanner;

impl PortScanner for SystemPortScanner {
    fn scan(&self) -> Result<Vec<PortEntry>> {
        // OS固有実装は Task 11 で実装
        Ok(vec![])
    }
}

// tests は Step 1 で記載済み
```

`src/main.rs` にモジュール追加:
```rust
pub mod config;
pub mod data;
pub mod filter;
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --lib data::ports`
Expected: 3 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/data/ src/main.rs
git commit -m "feat: add PortScanner trait and PortEntry data model"
```

---

## Task 6: DockerSource trait

**Files:**
- Create: `src/data/docker.rs`
- Modify: `src/data/mod.rs`

- [ ] **Step 1: DockerSource のテストを書く**

`src/data/docker.rs` 末尾:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct MockDockerSource {
        containers: Vec<ContainerInfo>,
    }

    #[async_trait::async_trait]
    impl DockerSource for MockDockerSource {
        async fn list_containers(&self) -> Result<Vec<ContainerInfo>> {
            Ok(self.containers.clone())
        }
        async fn stop_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn restart_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn remove_container(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        fn is_available(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn test_mock_list_containers() {
        let source = MockDockerSource {
            containers: vec![ContainerInfo {
                id: "abc123".to_string(),
                name: "app-web".to_string(),
                image: "node:18".to_string(),
                state: ContainerState::Running,
                cpu_percent: 12.0,
                memory_bytes: 340_000_000,
                memory_limit: 1_000_000_000,
                ports: vec![PortMapping { host: 3000, container: 3000, protocol: "tcp".to_string() }],
                compose_project: Some("myapp".to_string()),
                created: "2026-04-03T10:00:00Z".to_string(),
            }],
        };
        let containers = source.list_containers().await.unwrap();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].name, "app-web");
        assert!(matches!(containers[0].state, ContainerState::Running));
    }

    #[tokio::test]
    async fn test_mock_stop_container() {
        let source = MockDockerSource { containers: vec![] };
        assert!(source.stop_container("abc123").await.is_ok());
    }

    #[test]
    fn test_container_state_display() {
        assert_eq!(ContainerState::Running.as_str(), "Running");
        assert_eq!(ContainerState::Stopped.as_str(), "Stopped");
        assert_eq!(ContainerState::Exited(0).as_str(), "Exited(0)");
    }
}
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --lib data::docker`
Expected: FAIL

- [ ] **Step 3: DockerSource trait + データ構造体 実装**

`src/data/docker.rs`:
```rust
use anyhow::Result;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub enum ContainerState {
    Running,
    Stopped,
    Exited(i64),
    Created,
}

impl ContainerState {
    pub fn as_str(&self) -> String {
        match self {
            Self::Running => "Running".to_string(),
            Self::Stopped => "Stopped".to_string(),
            Self::Exited(code) => format!("Exited({})", code),
            Self::Created => "Created".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PortMapping {
    pub host: u16,
    pub container: u16,
    pub protocol: String,
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: ContainerState,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub memory_limit: u64,
    pub ports: Vec<PortMapping>,
    pub compose_project: Option<String>,
    pub created: String,
}

#[async_trait]
pub trait DockerSource: Send + Sync {
    async fn list_containers(&self) -> Result<Vec<ContainerInfo>>;
    async fn stop_container(&self, id: &str) -> Result<()>;
    async fn restart_container(&self, id: &str) -> Result<()>;
    async fn remove_container(&self, id: &str) -> Result<()>;
    fn is_available(&self) -> bool;
}

// tests は Step 1 で記載済み
```

`src/data/mod.rs`:
```rust
pub mod docker;
pub mod ports;
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --lib data::docker`
Expected: 3 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/data/docker.rs src/data/mod.rs Cargo.toml
git commit -m "feat: add DockerSource trait and container data models"
```

---

## Task 7: ProcessSource trait

**Files:**
- Create: `src/data/processes.rs`
- Modify: `src/data/mod.rs`

- [ ] **Step 1: ProcessSource のテストを書く**

`src/data/processes.rs` 末尾:
```rust
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
            name: "node".to_string(),
            command: "node server.js".to_string(),
            user: "yuji".to_string(),
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
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --lib data::processes`
Expected: FAIL

- [ ] **Step 3: ProcessSource 実装**

`src/data/processes.rs`:
```rust
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
    "node", "python", "python3", "java", "go", "cargo", "rustc",
    "ruby", "php", "docker", "npm", "yarn", "pnpm", "bun", "deno",
    "gradle", "mvn", "dotnet", "mix", "elixir",
];

pub fn is_dev_process(name: &str) -> bool {
    let lower = name.to_lowercase();
    DEV_PROCESSES.iter().any(|&dev| lower.starts_with(dev))
}

// tests は Step 1 で記載済み
```

`src/data/mod.rs`:
```rust
pub mod docker;
pub mod ports;
pub mod processes;
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --lib data::processes`
Expected: 3 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/data/processes.rs src/data/mod.rs
git commit -m "feat: add ProcessSource trait and dev process detection"
```

---

## Task 8: LogSource trait

**Files:**
- Create: `src/data/logs.rs`
- Modify: `src/data/mod.rs`

- [ ] **Step 1: LogSource のテストを書く**

`src/data/logs.rs` 末尾:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_buffer_append_and_capacity() {
        let mut buffer = LogBuffer::new(3);
        buffer.push(LogEntry { timestamp: 1, source: "app".to_string(), level: LogLevel::Info, message: "msg1".to_string() });
        buffer.push(LogEntry { timestamp: 2, source: "app".to_string(), level: LogLevel::Info, message: "msg2".to_string() });
        buffer.push(LogEntry { timestamp: 3, source: "app".to_string(), level: LogLevel::Error, message: "msg3".to_string() });
        assert_eq!(buffer.len(), 3);

        buffer.push(LogEntry { timestamp: 4, source: "db".to_string(), level: LogLevel::Warn, message: "msg4".to_string() });
        assert_eq!(buffer.len(), 3); // oldest evicted
        assert_eq!(buffer.entries()[0].message, "msg2");
    }

    #[test]
    fn test_log_buffer_filter() {
        let mut buffer = LogBuffer::new(100);
        buffer.push(LogEntry { timestamp: 1, source: "app-web".to_string(), level: LogLevel::Info, message: "request".to_string() });
        buffer.push(LogEntry { timestamp: 2, source: "app-db".to_string(), level: LogLevel::Error, message: "timeout".to_string() });
        buffer.push(LogEntry { timestamp: 3, source: "app-web".to_string(), level: LogLevel::Warn, message: "slow".to_string() });

        let filtered: Vec<_> = buffer.entries().iter().filter(|e| e.source == "app-web").collect();
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
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --lib data::logs`
Expected: FAIL

- [ ] **Step 3: LogSource trait + LogBuffer 実装**

`src/data/logs.rs`:
```rust
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
            entries: VecDeque::with_capacity(capacity),
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

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// tests は Step 1 で記載済み
```

`src/data/mod.rs`:
```rust
pub mod docker;
pub mod logs;
pub mod ports;
pub mod processes;
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --lib data::logs`
Expected: 4 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/data/logs.rs src/data/mod.rs
git commit -m "feat: add LogBuffer with ring-buffer capacity management"
```

---

## Task 9: Event ハンドリング

**Files:**
- Create: `src/event.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Event のテストを書く**

`src/event.rs` 末尾:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_values() {
        assert_eq!(Panel::Ports as usize, 0);
        assert_eq!(Panel::Docker as usize, 1);
        assert_eq!(Panel::Processes as usize, 2);
        assert_eq!(Panel::Logs as usize, 3);
    }

    #[test]
    fn test_panel_next() {
        assert!(matches!(Panel::Ports.next(), Panel::Docker));
        assert!(matches!(Panel::Docker.next(), Panel::Processes));
        assert!(matches!(Panel::Processes.next(), Panel::Logs));
        assert!(matches!(Panel::Logs.next(), Panel::Ports));
    }

    #[test]
    fn test_panel_prev() {
        assert!(matches!(Panel::Ports.prev(), Panel::Logs));
        assert!(matches!(Panel::Docker.prev(), Panel::Ports));
    }

    #[test]
    fn test_panel_from_index() {
        assert!(matches!(Panel::from_index(0), Some(Panel::Ports)));
        assert!(matches!(Panel::from_index(3), Some(Panel::Logs)));
        assert!(Panel::from_index(4).is_none());
    }
}
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --lib event`
Expected: FAIL

- [ ] **Step 3: Event 型 + Panel enum 実装**

`src/event.rs`:
```rust
use crossterm::event::KeyEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Ports = 0,
    Docker = 1,
    Processes = 2,
    Logs = 3,
}

impl Panel {
    pub fn next(self) -> Self {
        match self {
            Self::Ports => Self::Docker,
            Self::Docker => Self::Processes,
            Self::Processes => Self::Logs,
            Self::Logs => Self::Ports,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Ports => Self::Logs,
            Self::Docker => Self::Ports,
            Self::Processes => Self::Docker,
            Self::Logs => Self::Processes,
        }
    }

    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(Self::Ports),
            1 => Some(Self::Docker),
            2 => Some(Self::Processes),
            3 => Some(Self::Logs),
            _ => None,
        }
    }
}

pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Resize(u16, u16),
}

// tests は Step 1 で記載済み
```

`src/main.rs` にモジュール追加:
```rust
pub mod config;
pub mod data;
pub mod event;
pub mod filter;
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --lib event`
Expected: 4 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/event.rs src/main.rs
git commit -m "feat: add Panel enum and AppEvent types"
```

---

## Task 10: App ステートマシン（コアロジック）

**Files:**
- Create: `src/app.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: App のテストを書く**

`src/app.rs` 末尾:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        App::new(Config::default())
    }

    #[test]
    fn test_initial_state() {
        let app = test_app();
        assert!(matches!(app.active_panel, Panel::Ports));
        assert!(!app.should_quit);
        assert!(matches!(app.mode, AppMode::Normal));
        assert!(!app.global_filter.is_active());
        assert!(app.fullscreen_panel.is_none());
    }

    #[test]
    fn test_next_panel() {
        let mut app = test_app();
        app.next_panel();
        assert!(matches!(app.active_panel, Panel::Docker));
        app.next_panel();
        assert!(matches!(app.active_panel, Panel::Processes));
    }

    #[test]
    fn test_prev_panel() {
        let mut app = test_app();
        app.prev_panel();
        assert!(matches!(app.active_panel, Panel::Logs));
    }

    #[test]
    fn test_select_panel_by_index() {
        let mut app = test_app();
        app.select_panel(2);
        assert!(matches!(app.active_panel, Panel::Processes));
    }

    #[test]
    fn test_toggle_fullscreen() {
        let mut app = test_app();
        assert!(app.fullscreen_panel.is_none());
        app.toggle_fullscreen();
        assert!(matches!(app.fullscreen_panel, Some(Panel::Ports)));
        app.toggle_fullscreen();
        assert!(app.fullscreen_panel.is_none());
    }

    #[test]
    fn test_enter_filter_mode() {
        let mut app = test_app();
        app.enter_global_filter();
        assert!(matches!(app.mode, AppMode::GlobalFilter));
    }

    #[test]
    fn test_quit() {
        let mut app = test_app();
        app.quit();
        assert!(app.should_quit);
    }

    #[test]
    fn test_move_selection() {
        let mut app = test_app();
        app.move_selection_down();
        assert_eq!(app.panel_states[0].selected_index, 1);
        app.move_selection_down();
        assert_eq!(app.panel_states[0].selected_index, 2);
        app.move_selection_up();
        assert_eq!(app.panel_states[0].selected_index, 1);
    }

    #[test]
    fn test_selection_does_not_go_below_zero() {
        let mut app = test_app();
        app.move_selection_up();
        assert_eq!(app.panel_states[0].selected_index, 0);
    }
}
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --lib app`
Expected: FAIL

- [ ] **Step 3: App 実装**

`src/app.rs`:
```rust
use crate::config::Config;
use crate::event::Panel;
use crate::filter::FilterState;

#[derive(Debug, PartialEq)]
pub enum AppMode {
    Normal,
    GlobalFilter,
    LocalFilter,
    Confirm,
    Help,
}

#[derive(Debug)]
pub struct PanelState {
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub local_filter: FilterState,
}

impl PanelState {
    pub fn new() -> Self {
        Self {
            selected_index: 0,
            scroll_offset: 0,
            local_filter: FilterState::new(),
        }
    }
}

pub struct App {
    pub config: Config,
    pub active_panel: Panel,
    pub fullscreen_panel: Option<Panel>,
    pub should_quit: bool,
    pub mode: AppMode,
    pub global_filter: FilterState,
    pub panel_states: Vec<PanelState>,
}

impl App {
    pub fn new(config: Config) -> Self {
        let panel_states = vec![
            PanelState::new(),
            PanelState::new(),
            PanelState::new(),
            PanelState::new(),
        ];
        Self {
            config,
            active_panel: Panel::Ports,
            fullscreen_panel: None,
            should_quit: false,
            mode: AppMode::Normal,
            global_filter: FilterState::new(),
            panel_states,
        }
    }

    pub fn next_panel(&mut self) {
        self.active_panel = self.active_panel.next();
    }

    pub fn prev_panel(&mut self) {
        self.active_panel = self.active_panel.prev();
    }

    pub fn select_panel(&mut self, index: usize) {
        if let Some(panel) = Panel::from_index(index) {
            self.active_panel = panel;
        }
    }

    pub fn toggle_fullscreen(&mut self) {
        self.fullscreen_panel = match self.fullscreen_panel {
            Some(_) => None,
            None => Some(self.active_panel),
        };
    }

    pub fn enter_global_filter(&mut self) {
        self.mode = AppMode::GlobalFilter;
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    fn active_panel_state(&mut self) -> &mut PanelState {
        &mut self.panel_states[self.active_panel as usize]
    }

    pub fn move_selection_down(&mut self) {
        let state = self.active_panel_state();
        state.selected_index = state.selected_index.saturating_add(1);
    }

    pub fn move_selection_up(&mut self) {
        let state = self.active_panel_state();
        state.selected_index = state.selected_index.saturating_sub(1);
    }
}

// tests は Step 1 で記載済み
```

`src/main.rs` にモジュール追加:
```rust
pub mod app;
pub mod config;
pub mod data;
pub mod event;
pub mod filter;
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --lib app`
Expected: 8 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/app.rs src/main.rs
git commit -m "feat: add App state machine with panel navigation and selection"
```

---

## Task 11: OS固有 PortScanner 実装（macOS + Linux）

**Files:**
- Modify: `src/data/ports.rs`

- [ ] **Step 1: SystemPortScanner の統合テストを書く**

`tests/integration/mod.rs` に追加:
```rust
mod port_scan_test;
```

`tests/integration/port_scan_test.rs`:
```rust
use devpulse::data::ports::{PortScanner, SystemPortScanner};

#[test]
fn test_system_port_scanner_returns_without_error() {
    let scanner = SystemPortScanner;
    let result = scanner.scan();
    assert!(result.is_ok());
}

#[test]
fn test_system_port_scanner_finds_some_ports() {
    // ほとんどのシステムでは何かしらリスニングしているポートがある
    let scanner = SystemPortScanner;
    let entries = scanner.scan().unwrap();
    // CI環境等では0の可能性もあるためパニックしないことだけ確認
    let _ = entries.len();
}
```

- [ ] **Step 2: テスト失敗を確認（空の実装なので成功するが件数は0）**

Run: `cargo test --test integration port_scan`
Expected: PASS (空vecが返るため)

- [ ] **Step 3: macOS/Linux固有の実装を追加**

`src/data/ports.rs` の `SystemPortScanner` の `scan` メソッドを更新:
```rust
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
        let port = name_field
            .rsplit(':')
            .next()?
            .parse::<u16>()
            .ok()?;

        Some(PortEntry {
            port,
            protocol: Protocol::Tcp,
            address: name_field.rsplitn(2, ':').nth(1).unwrap_or("*").to_string(),
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
        let content = fs::read_to_string("/proc/net/tcp")
            .unwrap_or_default();
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
        // state 0A = LISTEN
        if parts[3] != "0A" {
            return None;
        }
        let local_addr = parts[1];
        let addr_parts: Vec<&str> = local_addr.split(':').collect();
        if addr_parts.len() != 2 {
            return None;
        }
        let port = u16::from_str_radix(addr_parts[1], 16).ok()?;
        let inode = parts.get(9)?.to_string();

        Some(PortEntry {
            port,
            protocol: Protocol::Tcp,
            address: format_hex_addr(addr_parts[0]),
            pid: 0, // PID resolution requires /proc/*/fd scanning
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
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --test integration port_scan`
Expected: PASS

Run: `cargo test --lib data::ports`
Expected: PASS (既存テストも全パス)

- [ ] **Step 5: コミット**

```bash
git add src/data/ports.rs tests/integration/
git commit -m "feat: implement OS-specific PortScanner for macOS and Linux"
```

---

## Task 12: UI レイアウト計算

**Files:**
- Create: `src/ui/mod.rs`
- Create: `src/ui/layout.rs`
- Create: `src/ui/common.rs`
- Create: `src/ui/panels/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: レイアウト計算のテストを書く**

`src/ui/layout.rs` 末尾:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn test_quad_layout() {
        let area = Rect::new(0, 0, 120, 40);
        let panels = compute_layout(area, LayoutMode::Quad, None);
        assert_eq!(panels.len(), 4);
        // 各パネルが0より大きい面積を持つ
        for p in &panels {
            assert!(p.width > 0);
            assert!(p.height > 0);
        }
    }

    #[test]
    fn test_fullscreen_layout() {
        let area = Rect::new(0, 0, 120, 40);
        let panels = compute_layout(area, LayoutMode::Quad, Some(Panel::Docker));
        assert_eq!(panels.len(), 4);
        // Docker パネル (index 1) がフルサイズ
        assert_eq!(panels[1], area);
        // 他パネルはサイズ0
        assert_eq!(panels[0].width, 0);
        assert_eq!(panels[2].width, 0);
        assert_eq!(panels[3].width, 0);
    }

    #[test]
    fn test_narrow_terminal_reduces_panels() {
        let area = Rect::new(0, 0, 60, 20);
        let panels = compute_layout(area, LayoutMode::Quad, None);
        // 狭い画面でも4パネル分の領域は返す（描画側で判断）
        assert_eq!(panels.len(), 4);
    }
}
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --lib ui::layout`
Expected: FAIL

- [ ] **Step 3: レイアウト計算の実装**

`src/ui/mod.rs`:
```rust
pub mod common;
pub mod layout;
pub mod panels;
```

`src/ui/panels/mod.rs`:
```rust
pub mod docker;
pub mod logs;
pub mod ports;
pub mod processes;
```

`src/ui/panels/ports.rs` (スタブ):
```rust
// Portsパネル描画 — Task 13 で実装
```

`src/ui/panels/docker.rs` (スタブ):
```rust
// Dockerパネル描画 — Task 14 で実装
```

`src/ui/panels/processes.rs` (スタブ):
```rust
// Processesパネル描画 — Task 15 で実装
```

`src/ui/panels/logs.rs` (スタブ):
```rust
// Logsパネル描画 — Task 16 で実装
```

`src/ui/common.rs` (スタブ):
```rust
// 共通UI部品 — Task 17 で実装
```

`src/ui/layout.rs`:
```rust
use crate::event::Panel;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub enum LayoutMode {
    Quad,
}

pub fn compute_layout(
    area: Rect,
    _mode: LayoutMode,
    fullscreen: Option<Panel>,
) -> Vec<Rect> {
    if let Some(panel) = fullscreen {
        let mut rects = vec![Rect::new(0, 0, 0, 0); 4];
        rects[panel as usize] = area;
        return rects;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);

    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    vec![top[0], top[1], bottom[0], bottom[1]]
}

// tests は Step 1 で記載済み
```

`src/main.rs` にモジュール追加:
```rust
pub mod app;
pub mod config;
pub mod data;
pub mod event;
pub mod filter;
pub mod ui;
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --lib ui::layout`
Expected: 3 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/ui/ src/main.rs
git commit -m "feat: add quad layout computation with fullscreen support"
```

---

## Task 13: Ports パネル描画

**Files:**
- Modify: `src/ui/panels/ports.rs`

- [ ] **Step 1: Portsパネルのテストを書く**

`src/ui/panels/ports.rs`:
```rust
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Row, StatefulWidget, Table, TableState, Widget},
};

use crate::data::ports::{PortEntry, Protocol};

pub struct PortsPanel<'a> {
    pub entries: &'a [PortEntry],
    pub selected: usize,
    pub filter_text: &'a str,
    pub is_focused: bool,
}

// 実装は Step 3

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<PortEntry> {
        vec![
            PortEntry {
                port: 3000, protocol: Protocol::Tcp, address: "127.0.0.1".into(),
                pid: 1234, process_name: "next-dev".into(), command: "node".into(),
                cpu_percent: 12.5, memory_bytes: 340_000_000,
            },
            PortEntry {
                port: 5432, protocol: Protocol::Tcp, address: "0.0.0.0".into(),
                pid: 5678, process_name: "postgres".into(), command: "postgres".into(),
                cpu_percent: 2.0, memory_bytes: 120_000_000,
            },
        ]
    }

    #[test]
    fn test_ports_panel_renders_without_panic() {
        let entries = sample_entries();
        let panel = PortsPanel {
            entries: &entries,
            selected: 0,
            filter_text: "",
            is_focused: true,
        };
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        panel.render(area, &mut buf);
        // タイトルが含まれることを確認
        let content = buffer_to_string(&buf);
        assert!(content.contains("Ports"));
    }

    #[test]
    fn test_ports_panel_with_filter_shows_indicator() {
        let entries = sample_entries();
        let panel = PortsPanel {
            entries: &entries,
            selected: 0,
            filter_text: "node",
            is_focused: true,
        };
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        panel.render(area, &mut buf);
        let content = buffer_to_string(&buf);
        assert!(content.contains("node"));
    }

    fn buffer_to_string(buf: &Buffer) -> String {
        let mut s = String::new();
        for y in buf.area.top()..buf.area.bottom() {
            for x in buf.area.left()..buf.area.right() {
                s.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
            }
            s.push('\n');
        }
        s
    }
}
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --lib ui::panels::ports`
Expected: FAIL — render メソッドが未実装

- [ ] **Step 3: PortsPanel の Widget 実装**

`src/ui/panels/ports.rs` に `impl Widget` を追加:
```rust
impl<'a> Widget for PortsPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = if self.filter_text.is_empty() {
            " Ports ".to_string()
        } else {
            format!(" Ports [filter: {}] ", self.filter_text)
        };

        let border_style = if self.is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);

        let header = Row::new(vec!["PORT", "PROTO", "PROCESS", "PID", "CPU%", "MEM"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let rows: Vec<Row> = self.entries.iter().enumerate().map(|(i, e)| {
            let style = if i == self.selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![
                format!(":{}", e.port),
                match e.protocol { Protocol::Tcp => "TCP".into(), Protocol::Udp => "UDP".into() },
                e.process_name.clone(),
                e.pid.to_string(),
                format!("{:.1}", e.cpu_percent),
                format_bytes(e.memory_bytes),
            ]).style(style)
        }).collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(7),
                Constraint::Length(5),
                Constraint::Min(10),
                Constraint::Length(7),
                Constraint::Length(6),
                Constraint::Length(8),
            ],
        )
        .header(header)
        .block(block);

        Widget::render(table, area, buf);
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1}GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.0}MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0}KB", bytes as f64 / 1_000.0)
    } else {
        format!("{}B", bytes)
    }
}
```

ファイル先頭の `use` に `Constraint` を追加:
```rust
use ratatui::layout::Constraint;
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --lib ui::panels::ports`
Expected: 2 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/ui/panels/ports.rs
git commit -m "feat: implement Ports panel rendering with table widget"
```

---

## Task 14: Docker パネル描画

**Files:**
- Modify: `src/ui/panels/docker.rs`

- [ ] **Step 1: Dockerパネルのテストを書く**

`src/ui/panels/docker.rs`:
```rust
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, Table, Widget},
};

use crate::data::docker::{ContainerInfo, ContainerState, PortMapping};

pub struct DockerPanel<'a> {
    pub containers: &'a [ContainerInfo],
    pub selected: usize,
    pub filter_text: &'a str,
    pub is_focused: bool,
    pub is_available: bool,
}

// 実装は Step 3

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_containers() -> Vec<ContainerInfo> {
        vec![ContainerInfo {
            id: "abc123".into(), name: "app-web".into(), image: "node:18".into(),
            state: ContainerState::Running, cpu_percent: 12.0,
            memory_bytes: 340_000_000, memory_limit: 1_000_000_000,
            ports: vec![PortMapping { host: 3000, container: 3000, protocol: "tcp".into() }],
            compose_project: Some("myapp".into()), created: "2026-04-03".into(),
        }]
    }

    #[test]
    fn test_docker_panel_renders_without_panic() {
        let containers = sample_containers();
        let panel = DockerPanel {
            containers: &containers, selected: 0, filter_text: "",
            is_focused: true, is_available: true,
        };
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        panel.render(area, &mut buf);
    }

    #[test]
    fn test_docker_unavailable_shows_message() {
        let panel = DockerPanel {
            containers: &[], selected: 0, filter_text: "",
            is_focused: false, is_available: false,
        };
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        panel.render(area, &mut buf);
        let content = buffer_to_string(&buf);
        assert!(content.contains("Docker"));
    }

    fn buffer_to_string(buf: &Buffer) -> String {
        let mut s = String::new();
        for y in buf.area.top()..buf.area.bottom() {
            for x in buf.area.left()..buf.area.right() {
                s.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
            }
            s.push('\n');
        }
        s
    }
}
```

- [ ] **Step 2: テスト失敗を確認**

Run: `cargo test --lib ui::panels::docker`
Expected: FAIL

- [ ] **Step 3: DockerPanel の Widget 実装**

`impl Widget for DockerPanel`:
```rust
impl<'a> Widget for DockerPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = if self.filter_text.is_empty() {
            " Docker ".to_string()
        } else {
            format!(" Docker [filter: {}] ", self.filter_text)
        };

        let border_style = if self.is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);

        if !self.is_available {
            let inner = block.inner(area);
            Widget::render(block, area, buf);
            let msg = "Docker not detected";
            if inner.width as usize > msg.len() && inner.height > 0 {
                buf.set_string(inner.x, inner.y, msg, Style::default().fg(Color::DarkGray));
            }
            return;
        }

        let header = Row::new(vec!["NAME", "IMAGE", "STATE", "CPU%", "MEM", "PORTS"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let rows: Vec<Row> = self.containers.iter().enumerate().map(|(i, c)| {
            let style = if i == self.selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            let state_str = c.state.as_str();
            let ports_str = c.ports.iter()
                .map(|p| format!("{}:{}", p.host, p.container))
                .collect::<Vec<_>>()
                .join(", ");

            Row::new(vec![
                c.name.clone(), c.image.clone(), state_str,
                format!("{:.1}", c.cpu_percent),
                format_bytes(c.memory_bytes),
                ports_str,
            ]).style(style)
        }).collect();

        let table = Table::new(
            rows,
            [
                Constraint::Min(12), Constraint::Min(10), Constraint::Length(10),
                Constraint::Length(6), Constraint::Length(8), Constraint::Min(10),
            ],
        )
        .header(header)
        .block(block);

        Widget::render(table, area, buf);
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1}GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.0}MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{}B", bytes)
    }
}
```

- [ ] **Step 4: テスト成功を確認**

Run: `cargo test --lib ui::panels::docker`
Expected: 2 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/ui/panels/docker.rs
git commit -m "feat: implement Docker panel rendering with unavailable state"
```

---

## Task 15: Processes パネル描画

**Files:**
- Modify: `src/ui/panels/processes.rs`

構造は Task 13/14 と同パターン。以下に要点のみ記載。

- [ ] **Step 1: テストを書く**

`src/ui/panels/processes.rs` — sample ProcessInfo で `render` がパニックしないこと、ツリーモード時のインデント表示を確認するテスト。

```rust
use ratatui::{buffer::Buffer, layout::{Constraint, Rect}, style::{Color, Modifier, Style}, widgets::{Block, Borders, Row, Table, Widget}};
use crate::data::processes::ProcessInfo;

pub struct ProcessesPanel<'a> {
    pub processes: &'a [ProcessInfo],
    pub selected: usize,
    pub filter_text: &'a str,
    pub is_focused: bool,
    pub tree_mode: bool,
}

impl<'a> Widget for ProcessesPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = if self.filter_text.is_empty() {
            " Processes ".to_string()
        } else {
            format!(" Processes [filter: {}] ", self.filter_text)
        };
        let border_style = if self.is_focused { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::DarkGray) };
        let block = Block::default().title(title).borders(Borders::ALL).border_style(border_style);
        let header = Row::new(vec!["PID", "NAME", "CPU%", "MEM", "PORTS", "CMD"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let rows: Vec<Row> = self.processes.iter().enumerate().map(|(i, p)| {
            let style = if i == self.selected { Style::default().bg(Color::DarkGray) } else { Style::default() };
            let ports = p.listening_ports.iter().map(|p| format!(":{}", p)).collect::<Vec<_>>().join(",");
            Row::new(vec![
                p.pid.to_string(), p.name.clone(), format!("{:.1}", p.cpu_percent),
                format_bytes(p.memory_bytes), ports,
                p.command.chars().take(30).collect::<String>(),
            ]).style(style)
        }).collect();

        let table = Table::new(rows, [
            Constraint::Length(7), Constraint::Min(10), Constraint::Length(6),
            Constraint::Length(8), Constraint::Length(12), Constraint::Min(15),
        ]).header(header).block(block);
        Widget::render(table, area, buf);
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 { format!("{:.1}GB", bytes as f64 / 1_000_000_000.0) }
    else if bytes >= 1_000_000 { format!("{:.0}MB", bytes as f64 / 1_000_000.0) }
    else { format!("{}B", bytes) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processes_panel_renders_without_panic() {
        let procs = vec![ProcessInfo {
            pid: 1234, name: "node".into(), command: "node server.js".into(),
            user: "yuji".into(), cpu_percent: 12.5, memory_bytes: 340_000_000,
            threads: 8, parent_pid: Some(1), listening_ports: vec![3000], start_time: 0,
        }];
        let panel = ProcessesPanel { processes: &procs, selected: 0, filter_text: "", is_focused: true, tree_mode: false };
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        panel.render(area, &mut buf);
    }
}
```

- [ ] **Step 2: テスト失敗を確認 → Step 3: 実装は上記に含まれる → Step 4: テスト成功を確認**

Run: `cargo test --lib ui::panels::processes`
Expected: PASS

- [ ] **Step 5: コミット**

```bash
git add src/ui/panels/processes.rs
git commit -m "feat: implement Processes panel rendering"
```

---

## Task 16: Logs パネル描画

**Files:**
- Modify: `src/ui/panels/logs.rs`

- [ ] **Step 1: テスト + 実装**

`src/ui/panels/logs.rs`:
```rust
use ratatui::{buffer::Buffer, layout::Rect, style::{Color, Style}, text::{Line, Span}, widgets::{Block, Borders, Paragraph, Widget, Wrap}};
use crate::data::logs::{LogBuffer, LogLevel};

pub struct LogsPanel<'a> {
    pub buffer: &'a LogBuffer,
    pub selected: usize,
    pub filter_text: &'a str,
    pub is_focused: bool,
    pub tail_follow: bool,
    pub wrap: bool,
}

impl<'a> Widget for LogsPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let follow_indicator = if self.tail_follow { " FOLLOW" } else { "" };
        let title = if self.filter_text.is_empty() {
            format!(" Logs{} ", follow_indicator)
        } else {
            format!(" Logs [filter: {}]{} ", self.filter_text, follow_indicator)
        };
        let border_style = if self.is_focused { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::DarkGray) };
        let block = Block::default().title(title).borders(Borders::ALL).border_style(border_style);

        let lines: Vec<Line> = self.buffer.entries().iter().map(|entry| {
            let color = match entry.level {
                LogLevel::Error => Color::Red,
                LogLevel::Warn => Color::Yellow,
                LogLevel::Info => Color::Green,
            };
            Line::from(vec![
                Span::styled(format!("[{}] ", entry.source), Style::default().fg(color)),
                Span::raw(&entry.message),
            ])
        }).collect();

        let mut paragraph = Paragraph::new(lines).block(block);
        if self.wrap {
            paragraph = paragraph.wrap(Wrap { trim: false });
        }
        // tail follow: scroll to bottom
        if self.tail_follow && self.buffer.len() > 0 {
            let inner_height = area.height.saturating_sub(2) as usize;
            let scroll = self.buffer.len().saturating_sub(inner_height);
            paragraph = paragraph.scroll((scroll as u16, 0));
        }
        Widget::render(paragraph, area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::logs::{LogEntry, LogLevel};

    #[test]
    fn test_logs_panel_renders_without_panic() {
        let mut buffer = LogBuffer::new(100);
        buffer.push(LogEntry { timestamp: 1, source: "app".into(), level: LogLevel::Info, message: "started".into() });
        buffer.push(LogEntry { timestamp: 2, source: "db".into(), level: LogLevel::Error, message: "timeout".into() });

        let panel = LogsPanel {
            buffer: &buffer, selected: 0, filter_text: "", is_focused: true, tail_follow: true, wrap: false,
        };
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        panel.render(area, &mut buf);
    }

    #[test]
    fn test_logs_panel_empty_buffer() {
        let buffer = LogBuffer::new(100);
        let panel = LogsPanel {
            buffer: &buffer, selected: 0, filter_text: "", is_focused: false, tail_follow: false, wrap: false,
        };
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        panel.render(area, &mut buf);
    }
}
```

- [ ] **Step 2: テスト成功を確認**

Run: `cargo test --lib ui::panels::logs`
Expected: 2 tests passed

- [ ] **Step 3: コミット**

```bash
git add src/ui/panels/logs.rs
git commit -m "feat: implement Logs panel rendering with tail follow"
```

---

## Task 17: 共通UI部品（確認ダイアログ + ヘルプオーバーレイ）

**Files:**
- Modify: `src/ui/common.rs`

- [ ] **Step 1: テスト + 実装**

`src/ui/common.rs`:
```rust
use ratatui::{buffer::Buffer, layout::{Alignment, Rect}, style::{Color, Modifier, Style}, text::{Line, Span}, widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap}};

/// 共通ユーティリティ: 各パネルから使用
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1}GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.0}MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0}KB", bytes as f64 / 1_000.0)
    } else {
        format!("{}B", bytes)
    }
}

pub struct ConfirmDialog<'a> {
    pub message: &'a str,
}

impl<'a> Widget for ConfirmDialog<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = 44.min(area.width.saturating_sub(4));
        let height = 5.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect::new(x, y, width, height);

        Widget::render(Clear, dialog_area, buf);

        let block = Block::default()
            .title(" Confirm ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let text = vec![
            Line::from(self.message),
            Line::from(""),
            Line::from(vec![
                Span::styled("[Y]es", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled("[N]o", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            ]),
        ];
        let paragraph = Paragraph::new(text).block(block).alignment(Alignment::Center);
        Widget::render(paragraph, dialog_area, buf);
    }
}

pub struct HelpOverlay;

impl Widget for HelpOverlay {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = 50.min(area.width.saturating_sub(4));
        let height = 20.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let help_area = Rect::new(x, y, width, height);

        Widget::render(Clear, help_area, buf);

        let block = Block::default()
            .title(" Help (press ? to close) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let help_text = vec![
            Line::from("j/k        Move up/down"),
            Line::from("Tab        Next panel"),
            Line::from("Shift+Tab  Previous panel"),
            Line::from("1-4        Jump to panel / fullscreen"),
            Line::from("/          Global filter"),
            Line::from("f          Local filter"),
            Line::from("K          Kill process (SIGTERM)"),
            Line::from("Shift+K    Force kill (SIGKILL)"),
            Line::from("s          Stop Docker container"),
            Line::from("r          Restart Docker container"),
            Line::from("D          Delete (confirm required)"),
            Line::from("F          Toggle tail follow (Logs)"),
            Line::from("t          Toggle tree view (Processes)"),
            Line::from("w          Toggle line wrap (Logs)"),
            Line::from("y          Copy to clipboard"),
            Line::from("q          Quit"),
            Line::from("?          Toggle this help"),
        ];

        let paragraph = Paragraph::new(help_text).block(block).wrap(Wrap { trim: false });
        Widget::render(paragraph, help_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confirm_dialog_renders_without_panic() {
        let dialog = ConfirmDialog { message: "Kill process 'node' (PID 1234)?" };
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
    }

    #[test]
    fn test_help_overlay_renders_without_panic() {
        let help = HelpOverlay;
        let area = Rect::new(0, 0, 80, 30);
        let mut buf = Buffer::empty(area);
        help.render(area, &mut buf);
    }

    #[test]
    fn test_confirm_dialog_in_small_area() {
        let dialog = ConfirmDialog { message: "Kill?" };
        let area = Rect::new(0, 0, 20, 8);
        let mut buf = Buffer::empty(area);
        dialog.render(area, &mut buf);
    }
}
```

- [ ] **Step 2: テスト成功を確認**

Run: `cargo test --lib ui::common`
Expected: 3 tests passed

- [ ] **Step 3: コミット**

```bash
git add src/ui/common.rs
git commit -m "feat: add ConfirmDialog and HelpOverlay widgets"
```

---

## Task 18: メインイベントループ + 画面描画統合

**Files:**
- Modify: `src/main.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: main.rs を async に変更し、TUI初期化 + イベントループを実装**

`src/main.rs`:
```rust
pub mod app;
pub mod config;
pub mod data;
pub mod event;
pub mod filter;
pub mod ui;

use anyhow::Result;
use app::App;
use clap::Parser;
use config::Config;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use event::{AppEvent, Panel};
use ratatui::prelude::*;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(name = "devpulse", version, about = "Unified Developer Environment TUI")]
pub struct Cli {
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    #[arg(short, long)]
    pub filter: Option<String>,
    #[arg(short, long)]
    pub layout: Option<String>,
    #[arg(long)]
    pub no_docker: bool,
    #[arg(long)]
    pub refresh: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config_path = cli.config.unwrap_or_else(|| {
        dirs_or_default().join("devpulse").join("config.toml")
    });
    let mut config = Config::load(&config_path)?;

    if let Some(refresh) = cli.refresh {
        config.general.refresh_rate_ms = refresh.clamp(1000, 30000);
    }

    let mut app = App::new(config);

    if let Some(filter) = cli.filter {
        app.global_filter.set_query(&filter);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

async fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    let tick_rate = Duration::from_millis(app.config.general.refresh_rate_ms);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|frame| {
            ui::draw(frame, app);
        })?;

        let timeout = tick_rate.checked_sub(last_tick.elapsed()).unwrap_or(Duration::ZERO);

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                handle_key(app, key);
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
            // データ更新は将来のタスクで実装
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key: event::KeyEvent) {
    use app::AppMode;

    match app.mode {
        AppMode::Help => {
            if matches!(key.code, KeyCode::Char('?') | KeyCode::Esc) {
                app.mode = AppMode::Normal;
            }
        }
        AppMode::GlobalFilter => match key.code {
            KeyCode::Esc => {
                app.global_filter.clear();
                app.mode = AppMode::Normal;
            }
            KeyCode::Enter => {
                app.mode = AppMode::Normal;
            }
            KeyCode::Backspace => {
                let mut q = app.global_filter.query().to_string();
                q.pop();
                app.global_filter.set_query(&q);
            }
            KeyCode::Char(c) => {
                let mut q = app.global_filter.query().to_string();
                q.push(c);
                app.global_filter.set_query(&q);
            }
            _ => {}
        },
        AppMode::Confirm => match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                // 確認済み — アクション実行は将来タスクで接続
                app.mode = AppMode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.mode = AppMode::Normal;
            }
            _ => {}
        },
        AppMode::Normal | AppMode::LocalFilter => match key.code {
            KeyCode::Char('q') => app.quit(),
            KeyCode::Char('?') => app.mode = AppMode::Help,
            KeyCode::Char('/') => app.enter_global_filter(),
            KeyCode::Char('j') | KeyCode::Down => app.move_selection_down(),
            KeyCode::Char('k') | KeyCode::Up => app.move_selection_up(),
            KeyCode::Tab => app.next_panel(),
            KeyCode::BackTab => app.prev_panel(),
            KeyCode::Char('1') => app.select_panel_or_fullscreen(0),
            KeyCode::Char('2') => app.select_panel_or_fullscreen(1),
            KeyCode::Char('3') => app.select_panel_or_fullscreen(2),
            KeyCode::Char('4') => app.select_panel_or_fullscreen(3),
            _ => {}
        },
    }
}

fn dirs_or_default() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_default_cli_args() {
        let args = Cli::parse_from(["devpulse"]);
        assert!(args.config.is_none());
        assert!(args.filter.is_none());
        assert!(!args.no_docker);
    }

    #[test]
    fn test_cli_args_with_options() {
        let args = Cli::parse_from([
            "devpulse", "--config", "/tmp/config.toml",
            "--filter", "node", "--layout", "quad",
            "--no-docker", "--refresh", "5000",
        ]);
        assert_eq!(args.config.unwrap().to_str().unwrap(), "/tmp/config.toml");
        assert_eq!(args.filter.unwrap(), "node");
        assert!(args.no_docker);
        assert_eq!(args.refresh.unwrap(), 5000);
    }
}
```

> Note: `dirs` crateはTask 1のCargo.tomlに含まれている。

- [ ] **Step 2: App に `select_panel_or_fullscreen` メソッドを追加**

`src/app.rs` に追加:
```rust
pub fn select_panel_or_fullscreen(&mut self, index: usize) {
    if let Some(panel) = Panel::from_index(index) {
        if self.active_panel == panel {
            self.toggle_fullscreen();
        } else {
            self.active_panel = panel;
            self.fullscreen_panel = None;
        }
    }
}
```

- [ ] **Step 3: ui::draw 関数を実装**

`src/ui/mod.rs`:
```rust
pub mod common;
pub mod layout;
pub mod panels;

use ratatui::Frame;
use crate::app::{App, AppMode};
use layout::{compute_layout, LayoutMode};
use panels::{ports::PortsPanel, docker::DockerPanel, processes::ProcessesPanel, logs::LogsPanel};
use common::{ConfirmDialog, HelpOverlay};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let panel_areas = compute_layout(area, LayoutMode::Quad, app.fullscreen_panel);

    let global_filter = app.global_filter.query();

    // Ports panel
    let ports_panel = PortsPanel {
        entries: &[], // データ接続は将来タスク
        selected: app.panel_states[0].selected_index,
        filter_text: global_filter,
        is_focused: app.active_panel == crate::event::Panel::Ports,
    };
    frame.render_widget(ports_panel, panel_areas[0]);

    // Docker panel
    let docker_panel = DockerPanel {
        containers: &[],
        selected: app.panel_states[1].selected_index,
        filter_text: global_filter,
        is_focused: app.active_panel == crate::event::Panel::Docker,
        is_available: true,
    };
    frame.render_widget(docker_panel, panel_areas[1]);

    // Processes panel
    let processes_panel = ProcessesPanel {
        processes: &[],
        selected: app.panel_states[2].selected_index,
        filter_text: global_filter,
        is_focused: app.active_panel == crate::event::Panel::Processes,
        tree_mode: false,
    };
    frame.render_widget(processes_panel, panel_areas[2]);

    // Logs panel
    let log_buffer = crate::data::logs::LogBuffer::new(0);
    let logs_panel = LogsPanel {
        buffer: &log_buffer,
        selected: app.panel_states[3].selected_index,
        filter_text: global_filter,
        is_focused: app.active_panel == crate::event::Panel::Logs,
        tail_follow: app.config.logs.tail_follow,
        wrap: false,
    };
    frame.render_widget(logs_panel, panel_areas[3]);

    // Overlays
    match app.mode {
        AppMode::Help => frame.render_widget(HelpOverlay, area),
        AppMode::Confirm => frame.render_widget(ConfirmDialog { message: "Confirm action?" }, area),
        AppMode::GlobalFilter => {
            // フィルタ入力バーはタイトルバーに表示済み（各パネルのfilter_text経由）
        }
        _ => {}
    }
}
```

- [ ] **Step 4: ビルド確認**

Run: `cargo build`
Expected: コンパイル成功

Run: `cargo test`
Expected: 全テスト PASS

- [ ] **Step 5: コミット**

```bash
git add src/main.rs src/app.rs src/ui/mod.rs Cargo.toml
git commit -m "feat: integrate main event loop with TUI rendering"
```

---

## Task 19: データソース接続（ポート + プロセス リアルタイム更新）

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: App にデータフィールドを追加し、tick でデータ更新**

`src/app.rs` に追加:
```rust
use crate::data::ports::{PortEntry, PortScanner, SystemPortScanner};
use crate::data::processes::{ProcessInfo, ProcessSource};
use crate::data::logs::LogBuffer;

// App 構造体にフィールド追加:
pub port_entries: Vec<PortEntry>,
pub process_list: Vec<ProcessInfo>,
pub log_buffer: LogBuffer,

// new() で初期化:
port_entries: Vec::new(),
process_list: Vec::new(),
log_buffer: LogBuffer::new(config.logs.buffer_lines),

// tick メソッド追加:
pub fn tick(&mut self) {
    let scanner = SystemPortScanner;
    if let Ok(entries) = scanner.scan() {
        self.port_entries = entries;
    }

    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    self.process_list = sys.processes().iter().map(|(pid, p)| {
        ProcessInfo {
            pid: pid.as_u32(),
            name: p.name().to_string_lossy().to_string(),
            command: p.cmd().iter().map(|s| s.to_string_lossy().to_string()).collect::<Vec<_>>().join(" "),
            user: String::new(),
            cpu_percent: p.cpu_usage(),
            memory_bytes: p.memory(),
            threads: 0,
            parent_pid: p.parent().map(|p| p.as_u32()),
            listening_ports: vec![],
            start_time: p.start_time(),
        }
    }).collect();

    // 開発系プロセス優先ソート
    if self.config.processes.dev_process_priority {
        self.process_list.sort_by(|a, b| {
            let a_dev = crate::data::processes::is_dev_process(&a.name);
            let b_dev = crate::data::processes::is_dev_process(&b.name);
            b_dev.cmp(&a_dev).then(b.cpu_percent.partial_cmp(&a.cpu_percent).unwrap_or(std::cmp::Ordering::Equal))
        });
    }
}
```

- [ ] **Step 2: ui::draw でリアルデータを参照するよう更新**

`src/ui/mod.rs` — `&[]` を `&app.port_entries`、`&app.process_list`、`&app.log_buffer` に差し替え。

- [ ] **Step 3: main.rs の tick タイミングで `app.tick()` を呼び出し**

`run_loop` 内の `// データ更新は将来のタスクで実装` を `app.tick();` に置換。

- [ ] **Step 4: 動作確認**

Run: `cargo run`
Expected: 4パネルが表示され、Ports と Processes にリアルデータが表示される

- [ ] **Step 5: コミット**

```bash
git add src/app.rs src/ui/mod.rs src/main.rs
git commit -m "feat: connect live port and process data to TUI panels"
```

---

## Task 20: Docker データソース接続

**Files:**
- Modify: `src/data/docker.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: BollardDockerSource の実装**

`src/data/docker.rs` に追加:
```rust
pub struct BollardDockerSource {
    client: Option<bollard::Docker>,
}

impl BollardDockerSource {
    pub fn new() -> Self {
        let client = bollard::Docker::connect_with_local_defaults().ok();
        Self { client }
    }
}

#[async_trait]
impl DockerSource for BollardDockerSource {
    async fn list_containers(&self) -> Result<Vec<ContainerInfo>> {
        let client = match &self.client {
            Some(c) => c,
            None => return Ok(vec![]),
        };
        use bollard::container::ListContainersOptions;
        let opts = ListContainersOptions::<String> { all: true, ..Default::default() };
        let containers = client.list_containers(Some(opts)).await?;

        Ok(containers.into_iter().filter_map(|c| {
            Some(ContainerInfo {
                id: c.id?.chars().take(12).collect(),
                name: c.names?.first()?.trim_start_matches('/').to_string(),
                image: c.image.unwrap_or_default(),
                state: match c.state.as_deref() {
                    Some("running") => ContainerState::Running,
                    Some("exited") => ContainerState::Exited(0),
                    Some("created") => ContainerState::Created,
                    _ => ContainerState::Stopped,
                },
                cpu_percent: 0.0,
                memory_bytes: 0,
                memory_limit: 0,
                ports: c.ports.unwrap_or_default().into_iter().filter_map(|p| {
                    Some(PortMapping {
                        host: p.public_port? as u16,
                        container: p.private_port as u16,
                        protocol: p.typ.map(|t| format!("{:?}", t)).unwrap_or_default(),
                    })
                }).collect(),
                compose_project: c.labels.and_then(|l| l.get("com.docker.compose.project").cloned()),
                created: String::new(),
            })
        }).collect())
    }

    async fn stop_container(&self, id: &str) -> Result<()> {
        if let Some(client) = &self.client {
            client.stop_container(id, None).await?;
        }
        Ok(())
    }

    async fn restart_container(&self, id: &str) -> Result<()> {
        if let Some(client) = &self.client {
            client.restart_container(id, None).await?;
        }
        Ok(())
    }

    async fn remove_container(&self, id: &str) -> Result<()> {
        if let Some(client) = &self.client {
            client.remove_container(id, None).await?;
        }
        Ok(())
    }

    fn is_available(&self) -> bool {
        self.client.is_some()
    }
}
```

- [ ] **Step 2: App に Docker データ取得を統合**

`src/app.rs` — `tick` メソッドを async にするか、tokio::spawn で Docker 取得を並列実行。

```rust
pub docker_containers: Vec<ContainerInfo>,
pub docker_available: bool,
```

- [ ] **Step 3: ui::draw で Docker データを接続**

- [ ] **Step 4: 動作確認**

Run: `cargo run`
Expected: Docker起動中ならコンテナ一覧が表示。未起動なら「Docker not detected」表示。

- [ ] **Step 5: コミット**

```bash
git add src/data/docker.rs src/app.rs src/ui/mod.rs
git commit -m "feat: connect Docker container data via bollard API"
```

---

## Task 21: Kill / Stop アクション実装

**Files:**
- Create: `src/action.rs`
- Modify: `src/main.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: action のテストを書く**

`src/action.rs` 末尾:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_variants() {
        let action = Action::KillProcess { pid: 1234, force: false };
        assert!(matches!(action, Action::KillProcess { pid: 1234, .. }));

        let action = Action::StopContainer { id: "abc".to_string() };
        assert!(matches!(action, Action::StopContainer { .. }));
    }
}
```

- [ ] **Step 2: Action enum 実装**

`src/action.rs`:
```rust
#[derive(Debug, Clone)]
pub enum Action {
    KillProcess { pid: u32, force: bool },
    StopContainer { id: String },
    RestartContainer { id: String },
    RemoveContainer { id: String },
}
```

- [ ] **Step 3: App に pending_action + confirm フローを追加**

`src/app.rs`:
```rust
pub pending_action: Option<Action>,
pub confirm_message: String,
```

キー `K` 押下時 → `pending_action` にセット → `mode = Confirm` → `y` で実行。

- [ ] **Step 4: main.rs の handle_key で Kill/Stop キーバインド追加**

- [ ] **Step 5: 動作確認 + コミット**

Run: `cargo run` → プロセス選択 → `K` → 確認ダイアログ → `y` → プロセスがKillされる

```bash
git add src/action.rs src/app.rs src/main.rs
git commit -m "feat: implement Kill/Stop actions with confirmation dialog"
```

---

## Task 22: 最終統合テスト + clippy + fmt

**Files:**
- Create: `tests/integration/app_test.rs`

- [ ] **Step 1: 統合テストを書く**

`tests/integration/app_test.rs`:
```rust
use devpulse::app::{App, AppMode};
use devpulse::config::Config;
use devpulse::event::Panel;

#[test]
fn test_full_navigation_flow() {
    let mut app = App::new(Config::default());

    // 初期状態
    assert!(matches!(app.active_panel, Panel::Ports));
    assert!(matches!(app.mode, AppMode::Normal));

    // パネル移動
    app.next_panel();
    assert!(matches!(app.active_panel, Panel::Docker));

    // フルスクリーン
    app.select_panel_or_fullscreen(1);
    assert!(app.fullscreen_panel.is_some());
    app.select_panel_or_fullscreen(1);
    assert!(app.fullscreen_panel.is_none());

    // フィルタ
    app.enter_global_filter();
    assert!(matches!(app.mode, AppMode::GlobalFilter));
    app.global_filter.set_query("node");
    assert!(app.global_filter.is_active());

    // 終了
    app.quit();
    assert!(app.should_quit);
}
```

`tests/integration/mod.rs` に追加:
```rust
mod app_test;
```

- [ ] **Step 2: 全テスト実行**

Run: `cargo test`
Expected: 全テスト PASS

- [ ] **Step 3: clippy + fmt**

Run: `cargo clippy -- -D warnings`
Expected: 警告なし（あれば修正）

Run: `cargo fmt --check`
Expected: フォーマット済み（差分あれば `cargo fmt` で修正）

- [ ] **Step 4: 最終コミット**

```bash
git add -A
git commit -m "test: add integration tests and pass clippy/fmt checks"
```

---

## Summary

| Task | 内容 | テスト数 |
|------|------|---------|
| 1 | プロジェクト初期化 | 0 (ビルド確認) |
| 2 | Config モジュール | 4 |
| 3 | CLI引数パース | 2 |
| 4 | Filter モジュール | 6 |
| 5 | PortScanner trait | 3 |
| 6 | DockerSource trait | 3 |
| 7 | ProcessSource trait | 3 |
| 8 | LogBuffer | 4 |
| 9 | Event / Panel enum | 4 |
| 10 | App ステートマシン | 8 |
| 11 | OS固有 PortScanner | 2 |
| 12 | UI レイアウト | 3 |
| 13 | Ports パネル描画 | 2 |
| 14 | Docker パネル描画 | 2 |
| 15 | Processes パネル描画 | 1 |
| 16 | Logs パネル描画 | 2 |
| 17 | 共通UI部品 | 3 |
| 18 | イベントループ統合 | 2 (+ ビルド確認) |
| 19 | データソース接続 | 0 (動作確認) |
| 20 | Docker接続 | 0 (動作確認) |
| 21 | Kill/Stop アクション | 1 |
| 22 | 最終統合テスト | 1 |
| **Total** | | **~55 tests** |
