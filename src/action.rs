#[derive(Debug, Clone)]
pub enum Action {
    KillProcess { pid: u32, force: bool },
    StopContainer { id: String },
    RestartContainer { id: String },
    RemoveContainer { id: String },
}

impl Action {
    pub fn description(&self) -> String {
        match self {
            Action::KillProcess { pid, force } => {
                let signal = if *force { "SIGKILL" } else { "SIGTERM" };
                format!("Kill process PID {} ({})?", pid, signal)
            }
            Action::StopContainer { id } => format!("Stop container {}?", &id[..12.min(id.len())]),
            Action::RestartContainer { id } => {
                format!("Restart container {}?", &id[..12.min(id.len())])
            }
            Action::RemoveContainer { id } => {
                format!("Remove container {}?", &id[..12.min(id.len())])
            }
        }
    }
}

#[cfg(unix)]
pub fn kill_process(pid: u32, force: bool) {
    use std::process::Command;
    let signal = if force { "KILL" } else { "TERM" };
    let _ = Command::new("kill")
        .args([&format!("-{}", signal), &pid.to_string()])
        .output();
}

#[cfg(not(unix))]
pub fn kill_process(_pid: u32, _force: bool) {
    // No-op on non-Unix platforms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_variants() {
        let kill = Action::KillProcess {
            pid: 1234,
            force: false,
        };
        assert!(matches!(kill, Action::KillProcess { pid: 1234, .. }));

        let stop = Action::StopContainer {
            id: "abc123".to_string(),
        };
        assert!(matches!(stop, Action::StopContainer { .. }));

        let restart = Action::RestartContainer {
            id: "abc123".to_string(),
        };
        assert!(matches!(restart, Action::RestartContainer { .. }));

        let remove = Action::RemoveContainer {
            id: "abc123".to_string(),
        };
        assert!(matches!(remove, Action::RemoveContainer { .. }));
    }

    #[test]
    fn test_action_description() {
        let kill = Action::KillProcess {
            pid: 1234,
            force: false,
        };
        assert!(kill.description().contains("1234"));
        assert!(kill.description().contains("SIGTERM"));

        let kill_force = Action::KillProcess {
            pid: 5678,
            force: true,
        };
        assert!(kill_force.description().contains("SIGKILL"));

        let stop = Action::StopContainer {
            id: "abc123def456".to_string(),
        };
        assert!(stop.description().contains("Stop"));
    }
}
