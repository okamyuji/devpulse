//! Docker endpoint resolution and connection helper.
//!
//! Mirrors the Docker CLI's endpoint selection so DevPulse can
//! talk to non-Docker-Desktop daemons (Colima, OrbStack, Rancher, rootless).

use crate::config::DockerConfig;
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerEndpoint {
    UnixSocket(PathBuf),
    Http(String),
    NamedPipe(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointSource {
    EnvVar,
    Config,
    CliContext(String),
    Probe(String),
    Default,
}

#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub endpoint: DockerEndpoint,
    pub source: EndpointSource,
    pub context_name: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ResolutionReport {
    pub tried: Vec<(EndpointSource, DockerEndpoint)>,
    pub resolved: Option<ResolvedEndpoint>,
}

impl ResolutionReport {
    pub fn summary_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (src, ep) in &self.tried {
            out.push(format!("{}: {}", source_label(src), endpoint_label(ep)));
        }
        out
    }
}

fn source_label(src: &EndpointSource) -> &'static str {
    match src {
        EndpointSource::EnvVar => "DOCKER_HOST",
        EndpointSource::Config => "config.toml",
        EndpointSource::CliContext(_) => "docker context",
        EndpointSource::Probe(_) => "probe",
        EndpointSource::Default => "default",
    }
}

fn endpoint_label(ep: &DockerEndpoint) -> String {
    match ep {
        DockerEndpoint::UnixSocket(p) => format!("unix://{}", p.display()),
        DockerEndpoint::Http(u) => u.clone(),
        DockerEndpoint::NamedPipe(n) => format!("npipe://{}", n),
    }
}

/// Abstraction over the environment so tests can inject fixtures.
pub trait Env {
    fn var(&self, name: &str) -> Option<String>;
    fn home_dir(&self) -> Option<PathBuf>;
    fn path_exists(&self, p: &Path) -> bool;
    fn read_to_string(&self, p: &Path) -> std::io::Result<String>;
    fn read_dir(&self, p: &Path) -> std::io::Result<Vec<PathBuf>>;
}

pub struct RealEnv;

impl Env for RealEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
    fn home_dir(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }
    fn path_exists(&self, p: &Path) -> bool {
        p.exists()
    }
    fn read_to_string(&self, p: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(p)
    }
    fn read_dir(&self, p: &Path) -> std::io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(p)? {
            out.push(entry?.path());
        }
        Ok(out)
    }
}

/// Parse a Docker endpoint string like `unix:///var/run/docker.sock`,
/// `tcp://...`, `http://...`, `npipe://...`, or a bare absolute path.
pub fn parse_endpoint(raw: &str) -> Option<DockerEndpoint> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(rest) = raw.strip_prefix("unix://") {
        return Some(DockerEndpoint::UnixSocket(PathBuf::from(rest)));
    }
    if raw.starts_with("tcp://") || raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(DockerEndpoint::Http(raw.to_string()));
    }
    if let Some(rest) = raw.strip_prefix("npipe://") {
        return Some(DockerEndpoint::NamedPipe(rest.to_string()));
    }
    if raw.starts_with('/') {
        return Some(DockerEndpoint::UnixSocket(PathBuf::from(raw)));
    }
    None
}

/// Well-known socket probe candidates, in priority order.
fn probe_candidates(home: &Path, xdg_runtime: Option<&Path>) -> Vec<(&'static str, PathBuf)> {
    let mut v = vec![
        ("colima", home.join(".colima/default/docker.sock")),
        ("orbstack", home.join(".orbstack/run/docker.sock")),
        ("rancher-desktop", home.join(".rd/docker.sock")),
    ];
    if let Some(xdg) = xdg_runtime {
        v.push(("rootless", xdg.join("docker.sock")));
    }
    v.push(("docker-desktop", home.join(".docker/run/docker.sock")));
    v
}

const DEFAULT_UNIX_SOCKET: &str = "/var/run/docker.sock";

/// Resolve the active Docker endpoint using the documented priority order.
pub fn resolve_endpoint<E: Env>(cfg: &DockerConfig, env: &E) -> ResolutionReport {
    let mut report = ResolutionReport::default();

    // 1. DOCKER_HOST
    if let Some(raw) = env.var("DOCKER_HOST") {
        if let Some(ep) = parse_endpoint(&raw) {
            report.tried.push((EndpointSource::EnvVar, ep.clone()));
            report.resolved = Some(ResolvedEndpoint {
                endpoint: ep,
                source: EndpointSource::EnvVar,
                context_name: None,
            });
            return report;
        }
    }

    // 2. config.toml socket_path
    if cfg.socket_path != "auto" && !cfg.socket_path.is_empty() {
        if let Some(ep) = parse_endpoint(&cfg.socket_path) {
            report.tried.push((EndpointSource::Config, ep.clone()));
            report.resolved = Some(ResolvedEndpoint {
                endpoint: ep,
                source: EndpointSource::Config,
                context_name: None,
            });
            return report;
        }
    }

    // 3. Docker CLI context
    if let Some(home) = env.home_dir() {
        if let Some((ctx_name, ep)) = resolve_cli_context(&home, env) {
            report
                .tried
                .push((EndpointSource::CliContext(ctx_name.clone()), ep.clone()));
            report.resolved = Some(ResolvedEndpoint {
                endpoint: ep,
                source: EndpointSource::CliContext(ctx_name.clone()),
                context_name: Some(ctx_name),
            });
            return report;
        }

        // 4. Probe well-known sockets
        let xdg = env.var("XDG_RUNTIME_DIR").map(PathBuf::from);
        for (label, path) in probe_candidates(&home, xdg.as_deref()) {
            if env.path_exists(&path) {
                let ep = DockerEndpoint::UnixSocket(path);
                report
                    .tried
                    .push((EndpointSource::Probe(label.to_string()), ep.clone()));
                report.resolved = Some(ResolvedEndpoint {
                    endpoint: ep,
                    source: EndpointSource::Probe(label.to_string()),
                    context_name: Some(label.to_string()),
                });
                return report;
            }
        }
    }

    // 5. Default /var/run/docker.sock
    let default_path = PathBuf::from(DEFAULT_UNIX_SOCKET);
    if env.path_exists(&default_path) {
        let ep = DockerEndpoint::UnixSocket(default_path);
        report.tried.push((EndpointSource::Default, ep.clone()));
        report.resolved = Some(ResolvedEndpoint {
            endpoint: ep,
            source: EndpointSource::Default,
            context_name: None,
        });
        return report;
    }

    // Nothing resolved — record the default we *would have* tried so the UI can
    // surface a useful error.
    report.tried.push((
        EndpointSource::Default,
        DockerEndpoint::UnixSocket(PathBuf::from(DEFAULT_UNIX_SOCKET)),
    ));
    report
}

fn resolve_cli_context<E: Env>(home: &Path, env: &E) -> Option<(String, DockerEndpoint)> {
    let config_path = home.join(".docker/config.json");
    if !env.path_exists(&config_path) {
        return None;
    }
    let content = env.read_to_string(&config_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let current = value.get("currentContext")?.as_str()?.to_string();
    if current.is_empty() || current == "default" {
        return None;
    }
    let meta_root = home.join(".docker/contexts/meta");
    if !env.path_exists(&meta_root) {
        return None;
    }
    let entries = env.read_dir(&meta_root).ok()?;
    for dir in entries {
        let meta_path = dir.join("meta.json");
        if !env.path_exists(&meta_path) {
            continue;
        }
        let Ok(meta_str) = env.read_to_string(&meta_path) else {
            continue;
        };
        let Ok(meta): Result<serde_json::Value, _> = serde_json::from_str(&meta_str) else {
            continue;
        };
        let Some(name) = meta.get("Name").and_then(|v| v.as_str()) else {
            continue;
        };
        if name != current {
            continue;
        }
        let host = meta
            .pointer("/Endpoints/docker/Host")
            .and_then(|v| v.as_str())?;
        let ep = parse_endpoint(host)?;
        return Some((current, ep));
    }
    None
}

/// Connect to the resolved endpoint.
pub fn connect(endpoint: &DockerEndpoint) -> Result<bollard::Docker> {
    match endpoint {
        DockerEndpoint::UnixSocket(p) => {
            let s = p.to_str().ok_or_else(|| anyhow!("non-utf8 socket path"))?;
            Ok(bollard::Docker::connect_with_unix(
                s,
                120,
                bollard::API_DEFAULT_VERSION,
            )?)
        }
        DockerEndpoint::Http(url) => Ok(bollard::Docker::connect_with_http(
            url,
            120,
            bollard::API_DEFAULT_VERSION,
        )?),
        #[cfg(windows)]
        DockerEndpoint::NamedPipe(pipe) => Ok(bollard::Docker::connect_with_named_pipe(
            pipe,
            120,
            bollard::API_DEFAULT_VERSION,
        )?),
        #[cfg(not(windows))]
        DockerEndpoint::NamedPipe(_) => Err(anyhow!(
            "named pipe endpoints are only supported on Windows"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeEnv {
        vars: HashMap<String, String>,
        home: Option<PathBuf>,
        existing: Vec<PathBuf>,
        files: HashMap<PathBuf, String>,
        dirs: HashMap<PathBuf, Vec<PathBuf>>,
    }

    impl FakeEnv {
        fn new() -> Self {
            Self {
                vars: HashMap::new(),
                home: None,
                existing: Vec::new(),
                files: HashMap::new(),
                dirs: HashMap::new(),
            }
        }
        fn with_home(mut self, p: &Path) -> Self {
            self.home = Some(p.to_path_buf());
            self
        }
        fn with_var(mut self, k: &str, v: &str) -> Self {
            self.vars.insert(k.into(), v.into());
            self
        }
        fn with_existing(mut self, p: &Path) -> Self {
            self.existing.push(p.to_path_buf());
            self
        }
        fn with_file(mut self, p: &Path, body: &str) -> Self {
            self.files.insert(p.to_path_buf(), body.to_string());
            self.existing.push(p.to_path_buf());
            self
        }
        fn with_dir(mut self, p: &Path, entries: Vec<PathBuf>) -> Self {
            self.dirs.insert(p.to_path_buf(), entries);
            self.existing.push(p.to_path_buf());
            self
        }
    }

    impl Env for FakeEnv {
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }
        fn home_dir(&self) -> Option<PathBuf> {
            self.home.clone()
        }
        fn path_exists(&self, p: &Path) -> bool {
            self.existing.iter().any(|e| e == p)
        }
        fn read_to_string(&self, p: &Path) -> std::io::Result<String> {
            self.files.get(p).cloned().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, p.display().to_string())
            })
        }
        fn read_dir(&self, p: &Path) -> std::io::Result<Vec<PathBuf>> {
            self.dirs.get(p).cloned().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, p.display().to_string())
            })
        }
    }

    fn cfg_auto() -> DockerConfig {
        DockerConfig {
            socket_path: "auto".into(),
            show_stopped: true,
        }
    }

    #[test]
    fn parse_unix_url() {
        assert_eq!(
            parse_endpoint("unix:///tmp/s.sock"),
            Some(DockerEndpoint::UnixSocket("/tmp/s.sock".into()))
        );
    }

    #[test]
    fn parse_tcp_url() {
        assert_eq!(
            parse_endpoint("tcp://1.2.3.4:2375"),
            Some(DockerEndpoint::Http("tcp://1.2.3.4:2375".into()))
        );
    }

    #[test]
    fn parse_bare_path() {
        assert_eq!(
            parse_endpoint("/var/run/docker.sock"),
            Some(DockerEndpoint::UnixSocket("/var/run/docker.sock".into()))
        );
    }

    #[test]
    fn parse_rejects_empty_and_relative() {
        assert_eq!(parse_endpoint(""), None);
        assert_eq!(parse_endpoint("relative/path"), None);
    }

    #[test]
    fn env_var_wins_over_everything() {
        let env = FakeEnv::new().with_var("DOCKER_HOST", "tcp://example:2375");
        let r = resolve_endpoint(&cfg_auto(), &env);
        let resolved = r.resolved.unwrap();
        assert_eq!(resolved.source, EndpointSource::EnvVar);
        assert_eq!(
            resolved.endpoint,
            DockerEndpoint::Http("tcp://example:2375".into())
        );
        assert!(resolved.context_name.is_none());
    }

    #[test]
    fn config_path_wins_over_context_and_probe() {
        let cfg = DockerConfig {
            socket_path: "/tmp/custom.sock".into(),
            show_stopped: true,
        };
        let env = FakeEnv::new();
        let r = resolve_endpoint(&cfg, &env);
        let resolved = r.resolved.unwrap();
        assert_eq!(resolved.source, EndpointSource::Config);
        assert_eq!(
            resolved.endpoint,
            DockerEndpoint::UnixSocket("/tmp/custom.sock".into())
        );
    }

    #[test]
    fn auto_config_is_ignored() {
        let env = FakeEnv::new();
        let r = resolve_endpoint(&cfg_auto(), &env);
        // No home, no default socket -> unresolved.
        assert!(r.resolved.is_none());
    }

    #[test]
    fn cli_context_resolves_colima() {
        let home = PathBuf::from("/home/u");
        let config_json = r#"{"currentContext":"colima"}"#;
        let meta = r#"{
            "Name":"colima",
            "Endpoints":{"docker":{"Host":"unix:///home/u/.colima/default/docker.sock"}}
        }"#;
        let meta_dir = home.join(".docker/contexts/meta/abc");
        let env = FakeEnv::new()
            .with_home(&home)
            .with_file(&home.join(".docker/config.json"), config_json)
            .with_dir(&home.join(".docker/contexts/meta"), vec![meta_dir.clone()])
            .with_file(&meta_dir.join("meta.json"), meta);
        let r = resolve_endpoint(&cfg_auto(), &env);
        let resolved = r.resolved.unwrap();
        assert_eq!(resolved.source, EndpointSource::CliContext("colima".into()));
        assert_eq!(resolved.context_name.as_deref(), Some("colima"));
        assert_eq!(
            resolved.endpoint,
            DockerEndpoint::UnixSocket("/home/u/.colima/default/docker.sock".into())
        );
    }

    #[test]
    fn cli_context_skipped_when_default() {
        let home = PathBuf::from("/home/u");
        let config_json = r#"{"currentContext":"default"}"#;
        let env = FakeEnv::new()
            .with_home(&home)
            .with_file(&home.join(".docker/config.json"), config_json);
        let r = resolve_endpoint(&cfg_auto(), &env);
        // No probe sockets, no default -> unresolved
        assert!(r.resolved.is_none());
    }

    #[test]
    fn probe_finds_colima_when_no_context() {
        let home = PathBuf::from("/home/u");
        let colima = home.join(".colima/default/docker.sock");
        let env = FakeEnv::new().with_home(&home).with_existing(&colima);
        let r = resolve_endpoint(&cfg_auto(), &env);
        let resolved = r.resolved.unwrap();
        assert_eq!(resolved.source, EndpointSource::Probe("colima".into()));
        assert_eq!(resolved.endpoint, DockerEndpoint::UnixSocket(colima));
    }

    #[test]
    fn probe_prefers_colima_over_orbstack() {
        let home = PathBuf::from("/home/u");
        let colima = home.join(".colima/default/docker.sock");
        let orb = home.join(".orbstack/run/docker.sock");
        let env = FakeEnv::new()
            .with_home(&home)
            .with_existing(&colima)
            .with_existing(&orb);
        let r = resolve_endpoint(&cfg_auto(), &env);
        assert_eq!(
            r.resolved.unwrap().source,
            EndpointSource::Probe("colima".into())
        );
    }

    #[test]
    fn probe_rootless_uses_xdg_runtime_dir() {
        let home = PathBuf::from("/home/u");
        let xdg = PathBuf::from("/run/user/1000");
        let rootless = xdg.join("docker.sock");
        let env = FakeEnv::new()
            .with_home(&home)
            .with_var("XDG_RUNTIME_DIR", xdg.to_str().unwrap())
            .with_existing(&rootless);
        let r = resolve_endpoint(&cfg_auto(), &env);
        assert_eq!(
            r.resolved.unwrap().source,
            EndpointSource::Probe("rootless".into())
        );
    }

    #[test]
    fn falls_back_to_default_socket() {
        let home = PathBuf::from("/home/u");
        let default = PathBuf::from("/var/run/docker.sock");
        let env = FakeEnv::new().with_home(&home).with_existing(&default);
        let r = resolve_endpoint(&cfg_auto(), &env);
        assert_eq!(r.resolved.unwrap().source, EndpointSource::Default);
    }

    #[test]
    fn nothing_found_reports_default_attempt() {
        let env = FakeEnv::new();
        let r = resolve_endpoint(&cfg_auto(), &env);
        assert!(r.resolved.is_none());
        assert_eq!(r.tried.len(), 1);
        assert_eq!(r.tried[0].0, EndpointSource::Default);
    }

    #[test]
    fn summary_lines_formats_every_attempt() {
        let env = FakeEnv::new().with_var("DOCKER_HOST", "tcp://x:1");
        let r = resolve_endpoint(&cfg_auto(), &env);
        let lines = r.summary_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("DOCKER_HOST"));
        assert!(lines[0].contains("tcp://x:1"));
    }
}
