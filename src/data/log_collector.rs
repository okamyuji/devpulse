use crate::config::LogSourceConfig;
use crate::data::docker_connector::{self, DockerEndpoint};
use crate::data::logs::{LogEntry, LogLevel};
use tokio::sync::mpsc;

/// Spawn background log collection tasks based on config.
///
/// `docker_endpoint` should be pre-resolved by the caller
/// (typically via `BollardDockerSource::endpoint()`) so the Docker
/// connection is resolved exactly once per process.
pub fn spawn_log_collectors(
    sources: &[LogSourceConfig],
    docker_endpoint: Option<DockerEndpoint>,
    buffer_size: usize,
) -> mpsc::Receiver<LogEntry> {
    let (tx, rx) = mpsc::channel(buffer_size.min(1024));

    for source in sources {
        match source {
            LogSourceConfig::Docker { containers } => {
                let tx = tx.clone();
                let containers = containers.clone();
                let endpoint = docker_endpoint.clone();
                tokio::spawn(async move {
                    match endpoint {
                        Some(ep) => {
                            if let Err(e) = stream_docker_logs(tx, &ep, &containers).await {
                                tracing::warn!("Docker log stream ended: {}", e);
                            }
                        }
                        None => {
                            tracing::warn!("Docker log stream disabled: no endpoint resolved");
                        }
                    }
                });
            }
            LogSourceConfig::File { path } => {
                let tx = tx.clone();
                let path = path.clone();
                tokio::spawn(async move {
                    if let Err(e) = watch_file_logs(tx, &path).await {
                        tracing::warn!("File log watcher ended: {}", e);
                    }
                });
            }
        }
    }

    rx
}

/// Stream logs from Docker containers using bollard.
async fn stream_docker_logs(
    tx: mpsc::Sender<LogEntry>,
    endpoint: &DockerEndpoint,
    containers: &str,
) -> anyhow::Result<()> {
    use bollard::query_parameters::{ListContainersOptionsBuilder, LogsOptionsBuilder};
    use futures_util::StreamExt;

    let docker = docker_connector::connect(endpoint)?;

    // Determine which containers to stream
    let container_ids: Vec<String> = if containers == "all" {
        let mut filters = std::collections::HashMap::new();
        filters.insert("status", vec!["running"]);
        let options = ListContainersOptionsBuilder::default()
            .filters(&filters)
            .build();
        let containers = docker.list_containers(Some(options)).await?;
        containers.into_iter().filter_map(|c| c.id).collect()
    } else {
        containers
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    };

    if container_ids.is_empty() {
        return Ok(());
    }

    // Spawn a stream per container
    let mut handles = Vec::new();
    for id in container_ids {
        let docker = docker.clone();
        let tx = tx.clone();
        let handle = tokio::spawn(async move {
            let name = get_container_name(&docker, &id)
                .await
                .unwrap_or_else(|| id[..12.min(id.len())].to_string());
            let options = LogsOptionsBuilder::default()
                .follow(true)
                .stdout(true)
                .stderr(true)
                .tail("50")
                .build();
            let mut stream = docker.logs(&id, Some(options));
            while let Some(result) = stream.next().await {
                match result {
                    Ok(output) => {
                        let (level, message) = parse_docker_log_output(&output);
                        let entry = LogEntry {
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            source: name.clone(),
                            level,
                            message,
                        };
                        if tx.send(entry).await.is_err() {
                            break; // receiver dropped
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Docker log error for {}: {}", id, e);
                        break;
                    }
                }
            }
        });
        handles.push(handle);
    }

    // Wait for all streams
    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}

/// Get the human-readable container name.
async fn get_container_name(docker: &bollard::Docker, id: &str) -> Option<String> {
    let info = docker.inspect_container(id, None).await.ok()?;
    info.name.map(|n| n.trim_start_matches('/').to_string())
}

/// Parse a Docker log output line into a LogLevel and message string.
fn parse_docker_log_output(output: &bollard::container::LogOutput) -> (LogLevel, String) {
    use bollard::container::LogOutput;
    let (is_stderr, bytes) = match output {
        LogOutput::StdErr { message } => (true, message),
        LogOutput::StdOut { message } => (false, message),
        LogOutput::StdIn { message } => (false, message),
        LogOutput::Console { message } => (false, message),
    };
    let text = String::from_utf8_lossy(bytes).trim_end().to_string();
    let level = if is_stderr {
        detect_log_level_from_text(&text).unwrap_or(LogLevel::Error)
    } else {
        detect_log_level_from_text(&text).unwrap_or(LogLevel::Info)
    };
    (level, text)
}

/// Try to detect log level from common log format patterns.
fn detect_log_level_from_text(text: &str) -> Option<LogLevel> {
    let upper = text.to_uppercase();
    // Check for common log level indicators near the beginning.
    // Slice by *char boundary* — a byte slice would panic on multi-byte
    // characters (Japanese, emoji, etc.) in container logs.
    let prefix = truncate_char_boundary(&upper, 80);
    if prefix.contains("ERROR") || prefix.contains("FATAL") || prefix.contains("PANIC") {
        Some(LogLevel::Error)
    } else if prefix.contains("WARN") {
        Some(LogLevel::Warn)
    } else if prefix.contains("INFO") || prefix.contains("DEBUG") || prefix.contains("TRACE") {
        Some(LogLevel::Info)
    } else {
        None
    }
}

/// Return the longest prefix of `s` whose byte length is at most `max_bytes`,
/// stopping at the nearest UTF-8 character boundary.
fn truncate_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Watch file logs using the notify crate for file change detection.
async fn watch_file_logs(tx: mpsc::Sender<LogEntry>, glob_pattern: &str) -> anyhow::Result<()> {
    use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Seek, SeekFrom};
    use std::path::PathBuf;

    // Resolve glob pattern to files
    let paths = resolve_glob(glob_pattern);
    if paths.is_empty() {
        tracing::warn!("No files matched glob pattern: {}", glob_pattern);
        return Ok(());
    }

    // Track file positions
    let mut positions: HashMap<PathBuf, u64> = HashMap::new();

    // Read existing tail for each file (last 50 lines)
    for path in &paths {
        if let Ok(file) = std::fs::File::open(path) {
            let metadata = file.metadata()?;
            let size = metadata.len();
            positions.insert(path.clone(), size);

            // Read last 50 lines
            let reader = BufReader::new(&file);
            let all_lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
            let start = all_lines.len().saturating_sub(50);
            let source = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| glob_pattern.to_string());
            for line in &all_lines[start..] {
                if line.trim().is_empty() {
                    continue;
                }
                let level = detect_log_level_from_text(line).unwrap_or(LogLevel::Info);
                let entry = LogEntry {
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    source: source.clone(),
                    level,
                    message: line.clone(),
                };
                if tx.send(entry).await.is_err() {
                    return Ok(());
                }
            }
        }
    }

    // Set up file watcher
    let (notify_tx, mut notify_rx) = mpsc::channel::<notify::Event>(256);

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = notify_tx.blocking_send(event);
            }
        },
        Config::default(),
    )?;

    // Watch parent directories of all matched files
    let mut watched_dirs = std::collections::HashSet::new();
    for path in &paths {
        if let Some(parent) = path.parent() {
            if watched_dirs.insert(parent.to_path_buf()) {
                watcher.watch(parent, RecursiveMode::NonRecursive)?;
            }
        }
    }

    // Process file change events
    let paths_set: std::collections::HashSet<PathBuf> = paths
        .iter()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
        .collect();

    while let Some(event) = notify_rx.recv().await {
        if !matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
            continue;
        }
        for event_path in &event.paths {
            let canonical = event_path
                .canonicalize()
                .unwrap_or_else(|_| event_path.clone());
            if !paths_set.contains(&canonical) {
                continue;
            }
            let pos = positions.get(&canonical).copied().unwrap_or(0);
            if let Ok(mut file) = std::fs::File::open(&canonical) {
                if file.seek(SeekFrom::Start(pos)).is_ok() {
                    let reader = BufReader::new(&file);
                    let source = canonical
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| glob_pattern.to_string());
                    for line in reader.lines().map_while(Result::ok) {
                        if line.trim().is_empty() {
                            continue;
                        }
                        let level = detect_log_level_from_text(&line).unwrap_or(LogLevel::Info);
                        let entry = LogEntry {
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            source: source.clone(),
                            level,
                            message: line,
                        };
                        if tx.send(entry).await.is_err() {
                            return Ok(());
                        }
                    }
                }
                // Update position to end of file
                if let Ok(metadata) = canonical.metadata() {
                    positions.insert(canonical.clone(), metadata.len());
                }
            }
        }
    }

    Ok(())
}

/// Resolve a glob pattern to a list of file paths.
fn resolve_glob(pattern: &str) -> Vec<std::path::PathBuf> {
    glob::glob(pattern)
        .map(|paths| paths.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_log_level_error() {
        assert!(matches!(
            detect_log_level_from_text("2024-01-01 ERROR something failed"),
            Some(LogLevel::Error)
        ));
        assert!(matches!(
            detect_log_level_from_text("FATAL: out of memory"),
            Some(LogLevel::Error)
        ));
    }

    #[test]
    fn test_detect_log_level_warn() {
        assert!(matches!(
            detect_log_level_from_text("WARN: deprecated function"),
            Some(LogLevel::Warn)
        ));
    }

    #[test]
    fn test_detect_log_level_info() {
        assert!(matches!(
            detect_log_level_from_text("INFO: server started"),
            Some(LogLevel::Info)
        ));
    }

    #[test]
    fn test_detect_log_level_none() {
        assert!(detect_log_level_from_text("just a plain message").is_none());
    }

    #[test]
    fn test_detect_log_level_multibyte_prefix_does_not_panic() {
        // Regression: panicked with
        // "byte index 80 is not a char boundary; it is inside 'を'"
        // when a Japanese log line (Colima container logs) happened to
        // straddle the 80-byte truncation point.
        let msg = "ログ出力テスト：".repeat(20) + " ERROR occurred";
        let lvl = detect_log_level_from_text(&msg);
        // We don't care whether ERROR is found — only that it doesn't panic
        // and that a level (or None) is returned.
        let _ = lvl;
    }

    #[test]
    fn test_truncate_char_boundary_keeps_valid_utf8() {
        // "を" is a 3-byte char (E3 82 92). Truncating at byte 79 would
        // split it; the helper must back off to the previous boundary.
        let s = "a".repeat(78) + "を" + "bc"; // len = 78 + 3 + 2 = 83
        let out = truncate_char_boundary(&s, 80);
        // 80 bytes splits the multibyte; expect truncation back to byte 78.
        assert_eq!(out.len(), 78);
        assert!(out.chars().all(|c| c == 'a'));
    }

    #[test]
    fn test_truncate_char_boundary_shorter_than_max() {
        let s = "short";
        assert_eq!(truncate_char_boundary(s, 80), s);
    }

    #[test]
    fn test_resolve_glob_no_match() {
        let paths = resolve_glob("/nonexistent/path/*.log");
        assert!(paths.is_empty());
    }

    #[test]
    fn test_resolve_glob_with_tempfile() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        std::fs::write(&log_path, "hello\n").unwrap();
        let pattern = format!("{}/*.log", dir.path().display());
        let paths = resolve_glob(&pattern);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].file_name().unwrap().to_str().unwrap(), "test.log");
    }

    #[tokio::test]
    async fn test_spawn_log_collectors_with_file_source() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("app.log");
        std::fs::write(&log_path, "INFO: started\nERROR: crash\n").unwrap();

        let sources = vec![LogSourceConfig::File {
            path: format!("{}/*.log", dir.path().display()),
        }];
        let mut rx = spawn_log_collectors(&sources, None, 100);

        // Should receive the initial tail lines
        let entry1 = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout")
            .expect("no entry");
        assert_eq!(entry1.message, "INFO: started");

        let entry2 = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout")
            .expect("no entry");
        assert_eq!(entry2.message, "ERROR: crash");
        assert!(matches!(entry2.level, LogLevel::Error));
    }

    #[tokio::test]
    async fn test_file_watcher_detects_new_lines() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("watch.log");
        std::fs::write(&log_path, "").unwrap();

        let sources = vec![LogSourceConfig::File {
            path: format!("{}/*.log", dir.path().display()),
        }];
        let mut rx = spawn_log_collectors(&sources, None, 100);

        // Wait for watcher to initialize (macOS FSEvents can be slow)
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Append new content with explicit flush
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log_path)
                .unwrap();
            writeln!(f, "WARN: disk almost full").unwrap();
            f.flush().unwrap();
            f.sync_all().unwrap();
        }

        // macOS FSEvents may need extra time to deliver events
        let entry = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("timeout waiting for file watcher event")
            .expect("no entry received");
        assert_eq!(entry.message, "WARN: disk almost full");
        assert!(matches!(entry.level, LogLevel::Warn));
    }

    #[tokio::test]
    async fn test_spawn_with_empty_sources() {
        let sources: Vec<LogSourceConfig> = vec![];
        let mut rx = spawn_log_collectors(&sources, None, 100);

        // With no sources, receiver should eventually close (all senders dropped)
        let result = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
        // Either timeout or None is acceptable
        match result {
            Ok(None) => {} // channel closed
            Err(_) => {}   // timeout - also fine, original tx still exists
            Ok(Some(_)) => panic!("unexpected entry"),
        }
    }
}
