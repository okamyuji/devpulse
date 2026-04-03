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
