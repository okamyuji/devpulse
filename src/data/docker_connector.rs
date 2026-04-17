//! Docker endpoint resolution and connection helper.
//!
//! Mirrors the Docker CLI's endpoint selection so DevPulse can
//! talk to non-Docker-Desktop daemons (Colima, OrbStack, Rancher, rootless).

use crate::config::DockerConfig;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

/// A Docker daemon endpoint in one of the three transport shapes
/// `bollard` understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerEndpoint {
    /// Unix domain socket at the given path.
    UnixSocket(PathBuf),
    /// HTTP(S) URL. `tcp://` inputs are normalized to `http://` by
    /// [`parse_endpoint`]; TLS endpoints must be passed as `https://`.
    Http(String),
    /// Windows named pipe (e.g. `\\.\pipe\docker_engine`).
    NamedPipe(String),
}

/// Where a resolved endpoint came from. Used by the UI to label the
/// active Docker source and to render the try-list on failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointSource {
    /// Resolved from the `DOCKER_HOST` environment variable.
    EnvVar,
    /// Resolved from `config.toml`'s `docker.socket_path`.
    Config,
    /// Resolved from the Docker CLI context (either `DOCKER_CONTEXT`
    /// env var or `~/.docker/config.json`'s `currentContext`).
    CliContext(String),
    /// Resolved by probing a well-known socket path. The inner string is
    /// the probe label (e.g. `"colima"`, `"orbstack"`).
    Probe(String),
    /// Fell back to `/var/run/docker.sock`.
    Default,
}

/// Endpoint resolution result plus metadata useful to the UI.
#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub endpoint: DockerEndpoint,
    pub source: EndpointSource,
    /// Human-readable context/probe name (`"colima"`, `"orbstack"`, …)
    /// for UI display. `None` for `DOCKER_HOST` / config / default
    /// sources since those have no inherent name.
    pub context_name: Option<String>,
}

/// Outcome of a single call to [`resolve_endpoint`].
///
/// `tried` lists every endpoint the resolver actually pushed into its
/// search — used by the UI to explain what happened when nothing worked.
/// `warnings` collects non-fatal issues such as a malformed
/// `config.socket_path` or a later connect failure.
#[derive(Debug, Default, Clone)]
pub struct ResolutionReport {
    pub tried: Vec<(EndpointSource, DockerEndpoint)>,
    pub resolved: Option<ResolvedEndpoint>,
    pub warnings: Vec<String>,
}

impl ResolutionReport {
    /// Human-readable one-line-per-attempt summary for the Docker panel
    /// error view. Warnings are prefixed with `! `.
    pub fn summary_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (src, ep) in &self.tried {
            out.push(format!("{}: {}", source_label(src), endpoint_label(ep)));
        }
        for w in &self.warnings {
            out.push(format!("! {}", w));
        }
        out
    }
}

fn source_label(src: &EndpointSource) -> String {
    match src {
        EndpointSource::EnvVar => "DOCKER_HOST".to_string(),
        EndpointSource::Config => "config.toml".to_string(),
        EndpointSource::CliContext(name) => format!("docker context ({})", name),
        EndpointSource::Probe(name) => format!("probe ({})", name),
        EndpointSource::Default => "default".to_string(),
    }
}

fn endpoint_label(ep: &DockerEndpoint) -> String {
    match ep {
        DockerEndpoint::UnixSocket(p) => format!("unix://{}", p.display()),
        DockerEndpoint::Http(u) => u.clone(),
        DockerEndpoint::NamedPipe(n) => format!("npipe://{}", n),
    }
}

/// Abstraction over the parts of the environment the resolver needs
/// (env vars, `$HOME`, filesystem reads). Production code uses
/// [`RealEnv`]; tests substitute an in-memory fake so resolution logic
/// can be exercised without touching real env or disk.
pub trait Env {
    /// Look up an environment variable. `None` if unset or non-UTF-8.
    fn var(&self, name: &str) -> Option<String>;
    /// User home directory, typically `$HOME` / `%USERPROFILE%`.
    fn home_dir(&self) -> Option<PathBuf>;
    /// Whether the given path exists (file *or* directory).
    fn path_exists(&self, p: &Path) -> bool;
    /// Read a file to a UTF-8 string.
    fn read_to_string(&self, p: &Path) -> std::io::Result<String>;
    /// List a directory's entries as absolute paths.
    fn read_dir(&self, p: &Path) -> std::io::Result<Vec<PathBuf>>;
}

/// Default [`Env`] implementation that consults the real process env
/// and filesystem.
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

/// Parse a Docker endpoint string in any of the common shapes:
///
/// - `unix:///path/to/docker.sock` → [`DockerEndpoint::UnixSocket`]
/// - `tcp://host:port` → [`DockerEndpoint::Http`] (normalized to `http://`)
/// - `http://...` / `https://...` → [`DockerEndpoint::Http`]
/// - `npipe://...` → [`DockerEndpoint::NamedPipe`] (slashes normalized
///   to backslashes for Windows API compatibility)
/// - bare `\\.\pipe\...` → [`DockerEndpoint::NamedPipe`]
/// - bare absolute path (`/...` or `Path::is_absolute()`) →
///   [`DockerEndpoint::UnixSocket`]
///
/// Leading/trailing whitespace is trimmed. Returns `None` for empty,
/// relative, or otherwise unrecognized input.
pub fn parse_endpoint(raw: &str) -> Option<DockerEndpoint> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(rest) = raw.strip_prefix("unix://") {
        return Some(DockerEndpoint::UnixSocket(PathBuf::from(rest)));
    }
    // Docker accepts tcp:// but hyper::Uri (used by bollard) only understands
    // http(s)://, so normalize tcp:// to http:// here.
    if let Some(rest) = raw.strip_prefix("tcp://") {
        return Some(DockerEndpoint::Http(format!("http://{}", rest)));
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(DockerEndpoint::Http(raw.to_string()));
    }
    if let Some(rest) = raw.strip_prefix("npipe://") {
        // Windows named pipes require backslash separators; Docker URLs are
        // sometimes written with forward slashes, so normalize here.
        return Some(DockerEndpoint::NamedPipe(rest.replace('/', "\\")));
    }
    // Bare Windows named-pipe path (e.g. `\\.\pipe\docker_engine`) must come
    // before the generic absolute-path check so it's not misclassified as a
    // Unix socket.
    if raw.starts_with(r"\\.\pipe\") {
        return Some(DockerEndpoint::NamedPipe(raw.to_string()));
    }
    // Unix-style leading '/' OR the host's own notion of absolute paths:
    // treat both as a Unix socket path so bare `/var/run/docker.sock` works
    // even on Windows hosts (WSL-adjacent setups, cross-compile scenarios).
    if raw.starts_with('/') || Path::new(raw).is_absolute() {
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

/// Connection timeout for the Docker daemon. Kept short because this is a
/// local-first TUI: on a healthy machine the socket answers in milliseconds,
/// and we don't want the UI to stall for two minutes when the daemon is down.
const CONNECT_TIMEOUT_SECS: u64 = 30;

/// Resolve the active Docker endpoint using the same priority Docker
/// CLI does:
///
/// 1. `DOCKER_HOST` environment variable
/// 2. `cfg.socket_path` (trimmed, case-insensitive `"auto"` / empty = skip)
/// 3. Docker CLI context — `DOCKER_CONTEXT` env var takes precedence
///    over `currentContext` in `~/.docker/config.json`
/// 4. Well-known socket probes (Colima → OrbStack → Rancher Desktop →
///    `$XDG_RUNTIME_DIR/docker.sock` if set → Docker Desktop)
/// 5. `/var/run/docker.sock` as a final fallback
///
/// Returns a [`ResolutionReport`] even on complete failure, so the UI
/// can show what was attempted. Kubernetes-only CLI contexts and
/// malformed `meta.json` files are skipped without aborting resolution.
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

    // 2. config.toml socket_path (trimmed; `"auto"` compared case-insensitively)
    let socket_path = cfg.socket_path.trim();
    if !socket_path.is_empty() && !socket_path.eq_ignore_ascii_case("auto") {
        match parse_endpoint(socket_path) {
            Some(ep) => {
                report.tried.push((EndpointSource::Config, ep.clone()));
                report.resolved = Some(ResolvedEndpoint {
                    endpoint: ep,
                    source: EndpointSource::Config,
                    context_name: None,
                });
                return report;
            }
            None => {
                report.warnings.push(format!(
                    "config.toml docker.socket_path is not a valid endpoint: {}",
                    cfg.socket_path
                ));
            }
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
    // Docker CLI priority: DOCKER_CONTEXT env var > ~/.docker/config.json currentContext.
    let current = match env.var("DOCKER_CONTEXT") {
        Some(v) if !v.is_empty() => v,
        _ => {
            let config_path = home.join(".docker/config.json");
            if !env.path_exists(&config_path) {
                return None;
            }
            let content = env.read_to_string(&config_path).ok()?;
            let value: serde_json::Value = serde_json::from_str(&content).ok()?;
            value.get("currentContext")?.as_str()?.to_string()
        }
    };
    if current.is_empty() || current == "default" {
        return None;
    }
    let meta_root = home.join(".docker/contexts/meta");
    if !env.path_exists(&meta_root) {
        return None;
    }
    let mut entries = match env.read_dir(&meta_root) {
        Ok(e) => e,
        Err(err) => {
            tracing::warn!(
                "failed to read Docker contexts dir {}: {}",
                meta_root.display(),
                err
            );
            return None;
        }
    };
    // Stable order across filesystems (read_dir is unordered) so which
    // meta.json wins is deterministic when several carry the same Name.
    entries.sort();
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
        // This context matched by name but may not define a Docker endpoint
        // (e.g. Kubernetes-only contexts). Fall through rather than
        // abandoning resolution entirely if this particular context is
        // unsuitable for Docker.
        let Some(host) = meta
            .pointer("/Endpoints/docker/Host")
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        let Some(ep) = parse_endpoint(host) else {
            continue;
        };
        return Some((current, ep));
    }
    None
}

/// Connect to the resolved endpoint.
/// Build a `bollard::Docker` client for the given endpoint.
///
/// Connection errors include the attempted endpoint for easier debugging.
/// Named-pipe endpoints return an error on non-Windows targets.
pub fn connect(endpoint: &DockerEndpoint) -> Result<bollard::Docker> {
    match endpoint {
        DockerEndpoint::UnixSocket(p) => {
            let s = p
                .to_str()
                .ok_or_else(|| anyhow!("non-utf8 unix socket path: {:?}", p))?;
            bollard::Docker::connect_with_unix(
                s,
                CONNECT_TIMEOUT_SECS,
                bollard::API_DEFAULT_VERSION,
            )
            .with_context(|| format!("connect_with_unix({}) failed", s))
        }
        DockerEndpoint::Http(url) => bollard::Docker::connect_with_http(
            url,
            CONNECT_TIMEOUT_SECS,
            bollard::API_DEFAULT_VERSION,
        )
        .with_context(|| format!("connect_with_http({}) failed", url)),
        #[cfg(windows)]
        DockerEndpoint::NamedPipe(pipe) => bollard::Docker::connect_with_named_pipe(
            pipe,
            CONNECT_TIMEOUT_SECS,
            bollard::API_DEFAULT_VERSION,
        )
        .with_context(|| format!("connect_with_named_pipe({}) failed", pipe)),
        #[cfg(not(windows))]
        DockerEndpoint::NamedPipe(pipe) => Err(anyhow!(
            "named pipe endpoints ({}) are only supported on Windows",
            pipe
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
    fn parse_tcp_url_normalizes_to_http() {
        assert_eq!(
            parse_endpoint("tcp://1.2.3.4:2375"),
            Some(DockerEndpoint::Http("http://1.2.3.4:2375".into()))
        );
    }

    #[test]
    fn parse_http_url_preserved() {
        assert_eq!(
            parse_endpoint("http://1.2.3.4:2375"),
            Some(DockerEndpoint::Http("http://1.2.3.4:2375".into()))
        );
    }

    #[test]
    fn parse_https_url_preserved() {
        assert_eq!(
            parse_endpoint("https://docker.example:2376"),
            Some(DockerEndpoint::Http("https://docker.example:2376".into()))
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
    fn parse_bare_windows_named_pipe() {
        assert_eq!(
            parse_endpoint(r"\\.\pipe\docker_engine"),
            Some(DockerEndpoint::NamedPipe(r"\\.\pipe\docker_engine".into()))
        );
    }

    #[test]
    fn parse_npipe_url_normalizes_slashes_to_backslashes() {
        assert_eq!(
            parse_endpoint(r"npipe:////./pipe/docker_engine"),
            Some(DockerEndpoint::NamedPipe(r"\\.\pipe\docker_engine".into()))
        );
    }

    #[test]
    fn env_var_wins_over_everything() {
        let env = FakeEnv::new().with_var("DOCKER_HOST", "tcp://example:2375");
        let r = resolve_endpoint(&cfg_auto(), &env);
        let resolved = r.resolved.unwrap();
        assert_eq!(resolved.source, EndpointSource::EnvVar);
        assert_eq!(
            resolved.endpoint,
            DockerEndpoint::Http("http://example:2375".into())
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
    fn cli_context_without_docker_endpoint_falls_through_to_probe() {
        // A Kubernetes-only context has no Endpoints.docker.Host; we should
        // keep going and pick up the probe candidate instead of erroring out.
        let home = PathBuf::from("/home/u");
        let config_json = r#"{"currentContext":"k8s-only"}"#;
        let meta = r#"{
            "Name":"k8s-only",
            "Endpoints":{"kubernetes":{"Host":"https://k8s.example"}}
        }"#;
        let meta_dir = home.join(".docker/contexts/meta/xyz");
        let colima = home.join(".colima/default/docker.sock");
        let env = FakeEnv::new()
            .with_home(&home)
            .with_file(&home.join(".docker/config.json"), config_json)
            .with_dir(&home.join(".docker/contexts/meta"), vec![meta_dir.clone()])
            .with_file(&meta_dir.join("meta.json"), meta)
            .with_existing(&colima);
        let resolved = resolve_endpoint(&cfg_auto(), &env).resolved.unwrap();
        assert_eq!(resolved.source, EndpointSource::Probe("colima".into()));
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
        assert!(lines[0].contains("http://x:1"));
    }

    #[test]
    fn summary_lines_includes_cli_context_name() {
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
        let lines = resolve_endpoint(&cfg_auto(), &env).summary_lines();
        assert!(
            lines.iter().any(|l| l.contains("docker context (colima)")),
            "expected context name in summary, got: {:?}",
            lines
        );
    }

    #[test]
    fn summary_lines_includes_probe_name() {
        let home = PathBuf::from("/home/u");
        let colima = home.join(".colima/default/docker.sock");
        let env = FakeEnv::new().with_home(&home).with_existing(&colima);
        let lines = resolve_endpoint(&cfg_auto(), &env).summary_lines();
        assert!(
            lines.iter().any(|l| l.contains("probe (colima)")),
            "expected probe label in summary, got: {:?}",
            lines
        );
    }

    #[test]
    fn docker_context_env_var_overrides_config_json() {
        // DOCKER_CONTEXT points at "colima" even though config.json has a different currentContext.
        let home = PathBuf::from("/home/u");
        let config_json = r#"{"currentContext":"desktop-linux"}"#;
        let meta = r#"{
            "Name":"colima",
            "Endpoints":{"docker":{"Host":"unix:///home/u/.colima/default/docker.sock"}}
        }"#;
        let meta_dir = home.join(".docker/contexts/meta/abc");
        let env = FakeEnv::new()
            .with_home(&home)
            .with_var("DOCKER_CONTEXT", "colima")
            .with_file(&home.join(".docker/config.json"), config_json)
            .with_dir(&home.join(".docker/contexts/meta"), vec![meta_dir.clone()])
            .with_file(&meta_dir.join("meta.json"), meta);
        let resolved = resolve_endpoint(&cfg_auto(), &env).resolved.unwrap();
        assert_eq!(resolved.source, EndpointSource::CliContext("colima".into()));
    }

    #[test]
    fn config_socket_path_auto_is_case_insensitive_and_trimmed() {
        let home = PathBuf::from("/home/u");
        let colima = home.join(".colima/default/docker.sock");
        for variant in ["  auto  ", "AUTO", "Auto", "  AUTO\n"] {
            let cfg = DockerConfig {
                socket_path: variant.into(),
                show_stopped: true,
            };
            let env = FakeEnv::new().with_home(&home).with_existing(&colima);
            let resolved = resolve_endpoint(&cfg, &env).resolved.unwrap();
            assert_eq!(
                resolved.source,
                EndpointSource::Probe("colima".into()),
                "variant {:?} should be treated as auto",
                variant
            );
        }
    }

    #[test]
    fn malformed_docker_config_json_is_skipped() {
        // Malformed JSON must not abort resolution; we should fall through.
        let home = PathBuf::from("/home/u");
        let colima = home.join(".colima/default/docker.sock");
        let env = FakeEnv::new()
            .with_home(&home)
            .with_file(&home.join(".docker/config.json"), "{ not valid json")
            .with_existing(&colima);
        let resolved = resolve_endpoint(&cfg_auto(), &env).resolved.unwrap();
        assert_eq!(resolved.source, EndpointSource::Probe("colima".into()));
    }

    #[test]
    fn malformed_meta_json_is_skipped_and_next_dir_is_tried() {
        // Two meta dirs: the first is junk, the second matches — after the
        // stable sort the first one comes lexicographically first.
        let home = PathBuf::from("/home/u");
        let config_json = r#"{"currentContext":"colima"}"#;
        let bad_meta = "{ this is not valid";
        let good_meta = r#"{
            "Name":"colima",
            "Endpoints":{"docker":{"Host":"unix:///home/u/.colima/default/docker.sock"}}
        }"#;
        let bad_dir = home.join(".docker/contexts/meta/aaa");
        let good_dir = home.join(".docker/contexts/meta/bbb");
        let env = FakeEnv::new()
            .with_home(&home)
            .with_file(&home.join(".docker/config.json"), config_json)
            .with_dir(
                &home.join(".docker/contexts/meta"),
                vec![bad_dir.clone(), good_dir.clone()],
            )
            .with_file(&bad_dir.join("meta.json"), bad_meta)
            .with_file(&good_dir.join("meta.json"), good_meta);
        let resolved = resolve_endpoint(&cfg_auto(), &env).resolved.unwrap();
        assert_eq!(resolved.source, EndpointSource::CliContext("colima".into()));
    }

    #[test]
    fn empty_current_context_is_skipped() {
        // currentContext present but empty string → treated as unset.
        let home = PathBuf::from("/home/u");
        let config_json = r#"{"currentContext":""}"#;
        let env = FakeEnv::new()
            .with_home(&home)
            .with_file(&home.join(".docker/config.json"), config_json);
        assert!(resolve_endpoint(&cfg_auto(), &env).resolved.is_none());
    }

    #[test]
    fn missing_current_context_key_is_skipped() {
        // config.json without the currentContext key — we just fall through.
        let home = PathBuf::from("/home/u");
        let config_json = r#"{"auths":{}}"#;
        let env = FakeEnv::new()
            .with_home(&home)
            .with_file(&home.join(".docker/config.json"), config_json);
        assert!(resolve_endpoint(&cfg_auto(), &env).resolved.is_none());
    }

    #[test]
    fn invalid_config_socket_path_is_recorded_and_falls_through() {
        let cfg = DockerConfig {
            socket_path: "mydocker.sock".into(), // relative path, not parseable
            show_stopped: true,
        };
        let home = PathBuf::from("/home/u");
        let colima = home.join(".colima/default/docker.sock");
        let env = FakeEnv::new().with_home(&home).with_existing(&colima);
        let report = resolve_endpoint(&cfg, &env);
        // Falls through to the probe.
        let resolved = report.resolved.as_ref().unwrap();
        assert_eq!(resolved.source, EndpointSource::Probe("colima".into()));
        // And the bad config is surfaced in warnings.
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("mydocker.sock") && w.contains("socket_path")),
            "expected warning about socket_path, got: {:?}",
            report.warnings
        );
        // Summary includes the warning with ! prefix.
        let lines = report.summary_lines();
        assert!(
            lines.iter().any(|l| l.starts_with("! ")),
            "expected warning in summary lines, got: {:?}",
            lines
        );
    }
}
