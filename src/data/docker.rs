use crate::config::DockerConfig;
use crate::data::docker_connector::{self, DockerEndpoint, ResolutionReport};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use bollard::query_parameters::ListContainersOptionsBuilder;
use std::collections::HashMap;

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

/// Live Docker source backed by the `bollard` crate.
///
/// Resolution and connection happen once at `new()`. The resolved
/// endpoint, context name, and resolution report are derived from the
/// stored [`ResolutionReport`] — there is no duplicated state.
pub struct BollardDockerSource {
    client: Option<bollard::Docker>,
    report: ResolutionReport,
}

impl BollardDockerSource {
    /// Build a source using the documented resolution order.
    ///
    /// If resolution finds an endpoint but `bollard` fails to connect
    /// (bad URL, unsupported scheme on the current platform, etc.), the
    /// error is logged via `tracing::warn` and recorded in
    /// `report.warnings` so the UI can surface it.
    pub fn new(cfg: &DockerConfig) -> Self {
        let mut report = docker_connector::resolve_endpoint(cfg, &docker_connector::RealEnv);
        let client = match report.resolved.as_ref() {
            Some(r) => match docker_connector::connect(&r.endpoint) {
                Ok(c) => Some(c),
                Err(e) => {
                    let msg = format!("failed to connect to Docker: {}", e);
                    tracing::warn!("{}", msg);
                    report.warnings.push(msg);
                    None
                }
            },
            None => None,
        };
        Self { client, report }
    }

    /// Human-readable context name from the resolution, if any
    /// (e.g. `"colima"` when resolved via the Docker CLI context).
    pub fn context_name(&self) -> Option<&str> {
        self.report
            .resolved
            .as_ref()
            .and_then(|r| r.context_name.as_deref())
    }

    /// The resolved endpoint, if resolution succeeded.
    ///
    /// Present even when the subsequent `bollard` connect failed — in
    /// that case `is_available()` is false but this still reports which
    /// endpoint was attempted.
    pub fn endpoint(&self) -> Option<&DockerEndpoint> {
        self.report.resolved.as_ref().map(|r| &r.endpoint)
    }

    /// Full resolution report for diagnostics (tried candidates + warnings).
    pub fn report(&self) -> &ResolutionReport {
        &self.report
    }
}

#[async_trait]
impl DockerSource for BollardDockerSource {
    async fn list_containers(&self) -> Result<Vec<ContainerInfo>> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow!("Docker not available"))?;
        let mut filters = HashMap::new();
        filters.insert("status", vec!["running", "exited", "created", "paused"]);
        let options = ListContainersOptionsBuilder::default()
            .all(true)
            .filters(&filters)
            .build();
        let containers = client.list_containers(Some(options)).await?;
        let mut result = Vec::new();
        for c in containers {
            let name = c
                .names
                .as_ref()
                .and_then(|n| n.first())
                .map(|n| n.trim_start_matches('/').to_string())
                .unwrap_or_default();
            let image = c.image.clone().unwrap_or_default();
            use bollard::models::ContainerSummaryStateEnum;
            let state = match c.state {
                Some(ContainerSummaryStateEnum::RUNNING) => ContainerState::Running,
                Some(ContainerSummaryStateEnum::CREATED) => ContainerState::Created,
                Some(ContainerSummaryStateEnum::EXITED) => ContainerState::Exited(0),
                _ => ContainerState::Stopped,
            };
            let ports = c
                .ports
                .as_ref()
                .map(|ports| {
                    ports
                        .iter()
                        .filter_map(|p| {
                            Some(PortMapping {
                                host: p.public_port?,
                                container: p.private_port,
                                protocol: p.typ.map(|t| format!("{:?}", t)).unwrap_or_default(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let compose_project = c
                .labels
                .as_ref()
                .and_then(|l| l.get("com.docker.compose.project").cloned());
            result.push(ContainerInfo {
                id: c.id.clone().unwrap_or_default(),
                name,
                image,
                state,
                cpu_percent: 0.0,
                memory_bytes: 0,
                memory_limit: 0,
                ports,
                compose_project,
                created: c.created.map(|t| t.to_string()).unwrap_or_default(),
            });
        }
        Ok(result)
    }

    async fn stop_container(&self, id: &str) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow!("Docker not available"))?;
        client.stop_container(id, None).await?;
        Ok(())
    }

    async fn restart_container(&self, id: &str) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow!("Docker not available"))?;
        client.restart_container(id, None).await?;
        Ok(())
    }

    async fn remove_container(&self, id: &str) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow!("Docker not available"))?;
        client.remove_container(id, None).await?;
        Ok(())
    }

    fn is_available(&self) -> bool {
        self.client.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockDockerSource {
        containers: Vec<ContainerInfo>,
    }

    #[async_trait]
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
                id: "abc123".into(),
                name: "app-web".into(),
                image: "node:18".into(),
                state: ContainerState::Running,
                cpu_percent: 12.0,
                memory_bytes: 340_000_000,
                memory_limit: 1_000_000_000,
                ports: vec![PortMapping {
                    host: 3000,
                    container: 3000,
                    protocol: "tcp".into(),
                }],
                compose_project: Some("myapp".into()),
                created: "2026-04-03T10:00:00Z".into(),
            }],
        };
        let containers = source.list_containers().await.unwrap();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].name, "app-web");
        assert!(matches!(containers[0].state, ContainerState::Running));
    }

    #[tokio::test]
    async fn test_mock_stop() {
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
