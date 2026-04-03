use crate::action::Action;
use crate::config::Config;
use crate::data::docker::ContainerInfo;
#[cfg(not(test))]
use crate::data::docker::{BollardDockerSource, DockerSource};
use crate::data::logs::LogBuffer;
use crate::data::ports::{PortEntry, SystemPortScanner};
use crate::data::processes::{is_dev_process, ProcessInfo};
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

impl Default for PanelState {
    fn default() -> Self {
        Self::new()
    }
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
    // Live data
    pub port_entries: Vec<PortEntry>,
    pub process_list: Vec<ProcessInfo>,
    pub log_buffer: LogBuffer,
    pub docker_containers: Vec<ContainerInfo>,
    pub docker_available: bool,
    // Actions
    pub pending_action: Option<Action>,
    pub confirm_message: String,
    // Internal data sources
    #[cfg(not(test))]
    docker_source: BollardDockerSource,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("active_panel", &self.active_panel)
            .field("mode", &self.mode)
            .field("should_quit", &self.should_quit)
            .finish()
    }
}

impl App {
    pub fn new(config: Config) -> Self {
        let log_capacity = config.logs.buffer_lines;
        #[cfg(not(test))]
        let docker_source = BollardDockerSource::new();
        #[cfg(not(test))]
        let docker_available = docker_source.is_available();
        #[cfg(test)]
        let docker_available = false;

        Self {
            config,
            active_panel: Panel::Ports,
            fullscreen_panel: None,
            should_quit: false,
            mode: AppMode::Normal,
            global_filter: FilterState::new(),
            panel_states: vec![
                PanelState::new(),
                PanelState::new(),
                PanelState::new(),
                PanelState::new(),
            ],
            port_entries: Vec::new(),
            process_list: Vec::new(),
            log_buffer: LogBuffer::new(log_capacity),
            docker_containers: Vec::new(),
            docker_available,
            pending_action: None,
            confirm_message: String::new(),
            #[cfg(not(test))]
            docker_source,
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
    pub fn enter_global_filter(&mut self) {
        self.mode = AppMode::GlobalFilter;
    }
    pub fn quit(&mut self) {
        self.should_quit = true;
    }
    pub fn move_selection_down(&mut self) {
        let state = &mut self.panel_states[self.active_panel as usize];
        state.selected_index = state.selected_index.saturating_add(1);
    }
    pub fn move_selection_up(&mut self) {
        let state = &mut self.panel_states[self.active_panel as usize];
        state.selected_index = state.selected_index.saturating_sub(1);
    }

    /// Refresh live data from system sources (ports, processes, docker)
    pub fn tick(&mut self) {
        // Scan ports
        let scanner = SystemPortScanner;
        if let Ok(entries) = crate::data::ports::PortScanner::scan(&scanner) {
            self.port_entries = entries;
        }

        // Scan processes via sysinfo
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let dev_priority = self.config.processes.dev_process_priority;
        let mut processes: Vec<ProcessInfo> = sys
            .processes()
            .values()
            .map(|p| {
                let name = p.name().to_string_lossy().to_string();
                let cmd_parts: Vec<String> = p
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().to_string())
                    .collect();
                let command = cmd_parts.join(" ");
                ProcessInfo {
                    pid: p.pid().as_u32(),
                    name,
                    command,
                    user: String::new(),
                    cpu_percent: p.cpu_usage(),
                    memory_bytes: p.memory(),
                    threads: 0,
                    parent_pid: p.parent().map(|pp| pp.as_u32()),
                    listening_ports: Vec::new(),
                    start_time: p.start_time(),
                }
            })
            .collect();

        if dev_priority {
            processes.sort_by(|a, b| {
                let a_dev = is_dev_process(&a.name);
                let b_dev = is_dev_process(&b.name);
                b_dev.cmp(&a_dev).then_with(|| {
                    b.cpu_percent
                        .partial_cmp(&a.cpu_percent)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            });
        } else {
            processes.sort_by(|a, b| {
                b.cpu_percent
                    .partial_cmp(&a.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        self.process_list = processes;
    }

    /// Fetch Docker containers (async, call from tokio context)
    pub async fn tick_docker(&mut self) {
        #[cfg(not(test))]
        {
            self.docker_available = self.docker_source.is_available();
            if self.docker_available {
                match self.docker_source.list_containers().await {
                    Ok(containers) => self.docker_containers = containers,
                    Err(_) => {
                        self.docker_available = false;
                        self.docker_containers.clear();
                    }
                }
            }
        }
    }

    /// Execute a pending action
    pub async fn execute_action(&mut self, action: &Action) {
        match action {
            Action::KillProcess { pid, force } => {
                crate::action::kill_process(*pid, *force);
            }
            Action::StopContainer { id } => {
                #[cfg(not(test))]
                {
                    let _ = self.docker_source.stop_container(id).await;
                }
                let _ = id;
            }
            Action::RestartContainer { id } => {
                #[cfg(not(test))]
                {
                    let _ = self.docker_source.restart_container(id).await;
                }
                let _ = id;
            }
            Action::RemoveContainer { id } => {
                #[cfg(not(test))]
                {
                    let _ = self.docker_source.remove_container(id).await;
                }
                let _ = id;
            }
        }
    }

    /// Get the selected port entry's PID (if any)
    pub fn selected_port_pid(&self) -> Option<u32> {
        let idx = self.panel_states[Panel::Ports as usize].selected_index;
        self.port_entries.get(idx).map(|e| e.pid)
    }

    /// Get the selected process PID (if any)
    pub fn selected_process_pid(&self) -> Option<u32> {
        let idx = self.panel_states[Panel::Processes as usize].selected_index;
        self.process_list.get(idx).map(|p| p.pid)
    }

    /// Get the selected Docker container ID (if any)
    pub fn selected_container_id(&self) -> Option<String> {
        let idx = self.panel_states[Panel::Docker as usize].selected_index;
        self.docker_containers.get(idx).map(|c| c.id.clone())
    }
}

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
    fn test_selection_no_underflow() {
        let mut app = test_app();
        app.move_selection_up();
        assert_eq!(app.panel_states[0].selected_index, 0);
    }
    #[test]
    fn test_live_data_fields_initialized() {
        let app = test_app();
        assert!(app.port_entries.is_empty());
        assert!(app.process_list.is_empty());
        assert!(app.docker_containers.is_empty());
        assert_eq!(app.log_buffer.len(), 0);
        assert!(app.pending_action.is_none());
        assert!(app.confirm_message.is_empty());
    }
    #[test]
    fn test_pending_action() {
        let mut app = test_app();
        app.pending_action = Some(Action::KillProcess {
            pid: 1234,
            force: false,
        });
        app.confirm_message = "Kill process 1234?".to_string();
        app.mode = AppMode::Confirm;
        assert!(matches!(app.mode, AppMode::Confirm));
        assert!(app.pending_action.is_some());
    }
}
