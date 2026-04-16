use crate::action::Action;
use crate::config::Config;
use crate::data::docker::ContainerInfo;
#[cfg(not(test))]
use crate::data::docker::{BollardDockerSource, DockerSource};
use crate::data::log_collector;
use crate::data::logs::{LogBuffer, LogEntry};
use crate::data::ports::{PortEntry, SystemPortScanner};
use crate::data::processes::ProcessInfo;
use crate::event::Panel;
use crate::filter::FilterState;
use tokio::sync::mpsc;

#[derive(Debug, PartialEq)]
pub enum AppMode {
    Normal,
    GlobalFilter,
    LocalFilter,
    LogFilter,
    Confirm,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessSortColumn {
    Pid,
    Name,
    Cpu,
    Memory,
    Ports,
}

impl ProcessSortColumn {
    pub fn next(self) -> Self {
        match self {
            Self::Pid => Self::Name,
            Self::Name => Self::Cpu,
            Self::Cpu => Self::Memory,
            Self::Memory => Self::Ports,
            Self::Ports => Self::Pid,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::Pid => Self::Ports,
            Self::Name => Self::Pid,
            Self::Cpu => Self::Name,
            Self::Memory => Self::Cpu,
            Self::Ports => Self::Memory,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Pid => "PID",
            Self::Name => "NAME",
            Self::Cpu => "CPU%",
            Self::Memory => "MEM",
            Self::Ports => "PORTS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PortSortColumn {
    Port,
    Process,
    Cpu,
    Memory,
}

impl PortSortColumn {
    pub fn next(self) -> Self {
        match self {
            Self::Port => Self::Process,
            Self::Process => Self::Cpu,
            Self::Cpu => Self::Memory,
            Self::Memory => Self::Port,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::Port => Self::Memory,
            Self::Process => Self::Port,
            Self::Cpu => Self::Process,
            Self::Memory => Self::Cpu,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Port => "PORT",
            Self::Process => "PROCESS",
            Self::Cpu => "CPU%",
            Self::Memory => "MEM",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
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
    pub docker_context_name: Option<String>,
    pub docker_resolution_summary: Vec<String>,
    // Actions
    pub pending_action: Option<Action>,
    pub confirm_message: String,
    // Toggle states
    pub tail_follow: bool,
    pub wrap_logs: bool,
    pub tree_mode: bool,
    // Sort states
    pub process_sort: ProcessSortColumn,
    pub process_sort_dir: SortDirection,
    pub port_sort: PortSortColumn,
    pub port_sort_dir: SortDirection,
    // Log-panel-local filter (AND condition, separate from global filter)
    pub log_filter: FilterState,
    // Log collection receiver
    log_rx: Option<mpsc::Receiver<LogEntry>>,
    // Internal data sources
    sys: sysinfo::System,
    #[cfg(not(test))]
    docker_source: BollardDockerSource,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("active_panel", &self.active_panel)
            .field("mode", &self.mode)
            .field("should_quit", &self.should_quit)
            .finish_non_exhaustive()
    }
}

impl App {
    pub fn new(config: Config) -> Self {
        let log_capacity = config.logs.buffer_lines;
        let tail_follow = config.logs.tail_follow;
        #[cfg(not(test))]
        let docker_source = BollardDockerSource::new(&config.docker);
        #[cfg(not(test))]
        let docker_available = docker_source.is_available();
        #[cfg(not(test))]
        let docker_context_name = docker_source.context_name().map(|s| s.to_string());
        #[cfg(not(test))]
        let docker_resolution_summary = docker_source.report().summary_lines();
        #[cfg(test)]
        let docker_available = false;
        #[cfg(test)]
        let docker_context_name: Option<String> = None;
        #[cfg(test)]
        let docker_resolution_summary: Vec<String> = Vec::new();

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
            docker_context_name,
            docker_resolution_summary,
            pending_action: None,
            confirm_message: String::new(),
            log_filter: FilterState::new(),
            tail_follow,
            wrap_logs: false,
            tree_mode: false,
            process_sort: ProcessSortColumn::Cpu,
            process_sort_dir: SortDirection::Desc,
            port_sort: PortSortColumn::Port,
            port_sort_dir: SortDirection::Asc,
            log_rx: None,
            sys: sysinfo::System::new(),
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
    pub fn active_panel_data_len(&self) -> usize {
        match self.active_panel {
            Panel::Ports => self.port_entries.len(),
            Panel::Docker => self.docker_containers.len(),
            Panel::Processes => self.process_list.len(),
            Panel::Logs => self.log_buffer.len(),
        }
    }
    pub fn move_selection_down(&mut self) {
        let max = self.active_panel_data_len().saturating_sub(1);
        let state = &mut self.panel_states[self.active_panel as usize];
        if state.selected_index < max {
            state.selected_index += 1;
        }
    }
    pub fn move_selection_up(&mut self) {
        let state = &mut self.panel_states[self.active_panel as usize];
        state.selected_index = state.selected_index.saturating_sub(1);
    }

    /// Cycle sort column forward (`>` key)
    pub fn sort_next(&mut self) {
        match self.active_panel {
            Panel::Processes => self.process_sort = self.process_sort.next(),
            Panel::Ports => self.port_sort = self.port_sort.next(),
            _ => {}
        }
        self.apply_sort();
    }

    /// Cycle sort column backward (`<` key)
    pub fn sort_prev(&mut self) {
        match self.active_panel {
            Panel::Processes => self.process_sort = self.process_sort.prev(),
            Panel::Ports => self.port_sort = self.port_sort.prev(),
            _ => {}
        }
        self.apply_sort();
    }

    /// Toggle sort direction (asc/desc) for current panel
    pub fn sort_toggle_direction(&mut self) {
        match self.active_panel {
            Panel::Processes => {
                self.process_sort_dir = match self.process_sort_dir {
                    SortDirection::Asc => SortDirection::Desc,
                    SortDirection::Desc => SortDirection::Asc,
                };
            }
            Panel::Ports => {
                self.port_sort_dir = match self.port_sort_dir {
                    SortDirection::Asc => SortDirection::Desc,
                    SortDirection::Desc => SortDirection::Asc,
                };
            }
            _ => {}
        }
        self.apply_sort();
    }

    /// Apply current sort settings to data
    pub fn apply_sort(&mut self) {
        // Sort processes
        let pcol = self.process_sort;
        let pdir = self.process_sort_dir;
        self.process_list.sort_by(|a, b| {
            let ord = match pcol {
                ProcessSortColumn::Pid => a.pid.cmp(&b.pid),
                ProcessSortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                ProcessSortColumn::Cpu => a
                    .cpu_percent
                    .partial_cmp(&b.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal),
                ProcessSortColumn::Memory => a.memory_bytes.cmp(&b.memory_bytes),
                ProcessSortColumn::Ports => a.listening_ports.len().cmp(&b.listening_ports.len()),
            };
            match pdir {
                SortDirection::Asc => ord,
                SortDirection::Desc => ord.reverse(),
            }
        });

        // Sort ports
        let scol = self.port_sort;
        let sdir = self.port_sort_dir;
        self.port_entries.sort_by(|a, b| {
            let ord = match scol {
                PortSortColumn::Port => a.port.cmp(&b.port),
                PortSortColumn::Process => a
                    .process_name
                    .to_lowercase()
                    .cmp(&b.process_name.to_lowercase()),
                PortSortColumn::Cpu => a
                    .cpu_percent
                    .partial_cmp(&b.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal),
                PortSortColumn::Memory => a.memory_bytes.cmp(&b.memory_bytes),
            };
            match sdir {
                SortDirection::Asc => ord,
                SortDirection::Desc => ord.reverse(),
            }
        });
    }

    /// Start background log collection tasks. Must be called from a tokio runtime.
    pub fn start_log_collection(&mut self) {
        let rx = log_collector::spawn_log_collectors(
            &self.config.logs.sources,
            &self.config.docker,
            self.config.logs.buffer_lines,
        );
        self.log_rx = Some(rx);
    }

    /// Drain any pending log entries from background collectors into the buffer.
    pub fn drain_logs(&mut self) {
        if let Some(rx) = &mut self.log_rx {
            while let Ok(entry) = rx.try_recv() {
                self.log_buffer.push(entry);
            }
        }
    }

    /// Refresh live data from system sources (ports, processes, docker)
    pub fn tick(&mut self) {
        // Scan ports
        let scanner = SystemPortScanner;
        if let Ok(entries) = crate::data::ports::PortScanner::scan(&scanner) {
            self.port_entries = entries;
        }

        // Scan processes via sysinfo (reuse self.sys so CPU deltas are computed)
        self.sys
            .refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let processes: Vec<ProcessInfo> = self
            .sys
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

        self.process_list = processes;

        // Apply user-selected sort
        self.apply_sort();

        self.clamp_selections();
    }

    pub fn clamp_selections(&mut self) {
        let lengths = [
            self.port_entries.len(),
            self.docker_containers.len(),
            self.process_list.len(),
            self.log_buffer.len(),
        ];
        for (i, &len) in lengths.iter().enumerate() {
            if len == 0 {
                self.panel_states[i].selected_index = 0;
            } else if self.panel_states[i].selected_index >= len {
                self.panel_states[i].selected_index = len - 1;
            }
        }
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

    /// Get the selected Docker container name (if any)
    pub fn selected_container_name(&self) -> Option<String> {
        let idx = self.panel_states[Panel::Docker as usize].selected_index;
        self.docker_containers.get(idx).map(|c| c.name.clone())
    }

    /// Enter log filter mode (f key on Logs panel)
    pub fn enter_log_filter(&mut self) {
        self.mode = AppMode::LogFilter;
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
        // Add some fake port entries so movement is allowed
        use crate::data::ports::{PortEntry, Protocol};
        for i in 0..5 {
            app.port_entries.push(PortEntry {
                port: 3000 + i,
                protocol: Protocol::Tcp,
                address: "127.0.0.1".into(),
                pid: 100 + i as u32,
                process_name: "test".into(),
                command: "test".into(),
                cpu_percent: 0.0,
                memory_bytes: 0,
            });
        }
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
    fn test_selection_no_overflow_on_empty() {
        let mut app = test_app();
        app.move_selection_down();
        assert_eq!(app.panel_states[0].selected_index, 0);
    }
    #[test]
    fn test_drain_logs_with_receiver() {
        use crate::data::logs::{LogEntry, LogLevel};
        let mut app = test_app();
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        app.log_rx = Some(rx);

        // Send some log entries
        tx.try_send(LogEntry {
            timestamp: 1,
            source: "test".into(),
            level: LogLevel::Info,
            message: "hello".into(),
        })
        .unwrap();
        tx.try_send(LogEntry {
            timestamp: 2,
            source: "test".into(),
            level: LogLevel::Error,
            message: "error".into(),
        })
        .unwrap();

        app.drain_logs();
        assert_eq!(app.log_buffer.len(), 2);
        assert_eq!(app.log_buffer.entries()[0].message, "hello");
        assert_eq!(app.log_buffer.entries()[1].message, "error");
    }

    #[test]
    fn test_drain_logs_without_receiver() {
        let mut app = test_app();
        // log_rx is None in test mode; should not panic
        app.drain_logs();
        assert_eq!(app.log_buffer.len(), 0);
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
    fn test_log_filter_independent_from_global() {
        let mut app = test_app();
        app.global_filter.set_query("node");
        app.log_filter.set_query("error timeout");
        assert_eq!(app.global_filter.query(), "node");
        assert_eq!(app.log_filter.query(), "error timeout");
        assert!(app.log_filter.matches_all_terms("[app] error timeout"));
        assert!(!app.log_filter.matches_all_terms("[app] error only"));
    }

    #[test]
    fn test_enter_log_filter_mode() {
        let mut app = test_app();
        app.active_panel = Panel::Logs;
        app.enter_log_filter();
        assert!(matches!(app.mode, AppMode::LogFilter));
    }

    #[test]
    fn test_selected_container_name() {
        use crate::data::docker::{ContainerInfo, ContainerState};
        let mut app = test_app();
        app.docker_containers.push(ContainerInfo {
            id: "abc123".into(),
            name: "app-web".into(),
            image: "node:18".into(),
            state: ContainerState::Running,
            cpu_percent: 0.0,
            memory_bytes: 0,
            memory_limit: 0,
            ports: vec![],
            compose_project: None,
            created: "2026-01-01".into(),
        });
        app.docker_containers.push(ContainerInfo {
            id: "def456".into(),
            name: "app-db".into(),
            image: "postgres:15".into(),
            state: ContainerState::Running,
            cpu_percent: 0.0,
            memory_bytes: 0,
            memory_limit: 0,
            ports: vec![],
            compose_project: None,
            created: "2026-01-01".into(),
        });
        app.panel_states[Panel::Docker as usize].selected_index = 1;
        assert_eq!(app.selected_container_name(), Some("app-db".to_string()));
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
