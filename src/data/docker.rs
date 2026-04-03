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

#[cfg(test)]
mod tests {
    use super::*;

    struct MockDockerSource { containers: Vec<ContainerInfo> }

    #[async_trait]
    impl DockerSource for MockDockerSource {
        async fn list_containers(&self) -> Result<Vec<ContainerInfo>> { Ok(self.containers.clone()) }
        async fn stop_container(&self, _id: &str) -> Result<()> { Ok(()) }
        async fn restart_container(&self, _id: &str) -> Result<()> { Ok(()) }
        async fn remove_container(&self, _id: &str) -> Result<()> { Ok(()) }
        fn is_available(&self) -> bool { true }
    }

    #[tokio::test]
    async fn test_mock_list_containers() {
        let source = MockDockerSource {
            containers: vec![ContainerInfo {
                id: "abc123".into(), name: "app-web".into(), image: "node:18".into(),
                state: ContainerState::Running, cpu_percent: 12.0,
                memory_bytes: 340_000_000, memory_limit: 1_000_000_000,
                ports: vec![PortMapping { host: 3000, container: 3000, protocol: "tcp".into() }],
                compose_project: Some("myapp".into()), created: "2026-04-03T10:00:00Z".into(),
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
