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
    assert_eq!(config.docker.socket_path, "auto");
}

#[test]
fn test_load_nonexistent_file_returns_default() {
    let config =
        devpulse::config::Config::load(std::path::Path::new("/nonexistent/config.toml")).unwrap();
    assert_eq!(config.general.refresh_rate_ms, 2000);
}

// ---- [agents]セクション（詳細設計9節）----

#[test]
fn agents_defaults_match_design_section9() {
    let config = devpulse::config::Config::default();
    assert!(config.agents.enabled);
    assert_eq!(config.agents.refresh_ms, 5000);
    assert_eq!(config.agents.quiet_threshold_s, 480);
    assert_eq!(config.agents.command_timeout_ms, 1000);
    assert!(!config.agents.private_store_fallback);
}

#[test]
fn agents_section_omitted_uses_defaults() {
    let toml_content = r#"
[general]
refresh_rate_ms = 3000
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();
    let config = devpulse::config::Config::load(file.path()).unwrap();
    assert!(config.agents.enabled);
    assert_eq!(config.agents.refresh_ms, 5000);
    assert_eq!(config.agents.quiet_threshold_s, 480);
    assert_eq!(config.agents.command_timeout_ms, 1000);
    assert!(!config.agents.private_store_fallback);
}

#[test]
fn agents_partial_section_keeps_other_defaults() {
    let toml_content = r#"
[agents]
enabled = false
refresh_ms = 10000
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();
    let config = devpulse::config::Config::load(file.path()).unwrap();
    assert!(!config.agents.enabled);
    assert_eq!(config.agents.refresh_ms, 10000);
    // 未指定キーは既定値のまま
    assert_eq!(config.agents.quiet_threshold_s, 480);
    assert_eq!(config.agents.command_timeout_ms, 1000);
    assert!(!config.agents.private_store_fallback);
}

#[test]
fn agents_all_keys_load_from_toml() {
    let toml_content = r#"
[agents]
enabled = false
refresh_ms = 2000
quiet_threshold_s = 60
command_timeout_ms = 500
private_store_fallback = true
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();
    let config = devpulse::config::Config::load(file.path()).unwrap();
    assert!(!config.agents.enabled);
    assert_eq!(config.agents.refresh_ms, 2000);
    assert_eq!(config.agents.quiet_threshold_s, 60);
    assert_eq!(config.agents.command_timeout_ms, 500);
    assert!(config.agents.private_store_fallback);
}

#[test]
fn agents_negative_refresh_ms_is_rejected() {
    let toml_content = r#"
[agents]
refresh_ms = -5
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();
    let err = devpulse::config::Config::load(file.path()).unwrap_err();
    // u64への負数はTOMLパースエラーとして報告される（既存Config::loadの失敗伝播の慣例）
    assert!(err.to_string().contains("refresh_ms"), "error was: {err}");
}

#[test]
fn agents_type_mismatch_enabled_is_rejected() {
    let toml_content = r#"
[agents]
enabled = "yes"
"#;
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();
    let err = devpulse::config::Config::load(file.path()).unwrap_err();
    assert!(err.to_string().contains("enabled"), "error was: {err}");
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
    assert_eq!(config.general.refresh_rate_ms, 1000);
}
