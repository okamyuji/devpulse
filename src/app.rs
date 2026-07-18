use crate::action::Action;
use crate::config::Config;
use crate::data::agents::model::{AgentKind, AgentSessionRow, SessionState};
use crate::data::agents::AgentsSnapshot;
use crate::data::docker::ContainerInfo;
#[cfg(not(test))]
use crate::data::docker::{BollardDockerSource, DockerSource};
use crate::data::docker_connector::DockerEndpoint;
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
    Error,
}

/// Diagnostic context surfaced when a Docker action (currently: Start) fails.
///
/// Captured at execute-time and shown via `ErrorOverlay` until the user
/// dismisses it with any keypress.
#[derive(Debug, Clone, Default)]
pub struct ErrorState {
    pub title: String,
    pub message: String,
    pub log_tail: Vec<String>,
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

/// Processes枠の表示ビュー（詳細設計8.1節）。パネル数は4のまま、枠内でビューを切り替える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessesViewMode {
    Processes,
    AgentSessions,
}

/// エージェントセッションビューのソート列（詳細設計8.1節。process用ソート状態から独立）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSortColumn {
    State,
    Agent,
    Task,
    Location,
    Cpu,
    Memory,
}

impl AgentSortColumn {
    pub fn next(self) -> Self {
        match self {
            Self::State => Self::Agent,
            Self::Agent => Self::Task,
            Self::Task => Self::Location,
            Self::Location => Self::Cpu,
            Self::Cpu => Self::Memory,
            Self::Memory => Self::State,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::State => Self::Memory,
            Self::Agent => Self::State,
            Self::Task => Self::Agent,
            Self::Location => Self::Task,
            Self::Cpu => Self::Location,
            Self::Memory => Self::Cpu,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::State => "STATE",
            Self::Agent => "AGENT",
            Self::Task => "TASK",
            Self::Location => "LOCATION",
            Self::Cpu => "CPU",
            Self::Memory => "MEM",
        }
    }
}

/// エージェント種別の表示名（フィルタ・ソートに使う）。
pub fn agent_kind_label(kind: &AgentKind) -> &str {
    match kind {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Kimi => "kimi",
        AgentKind::Other(s) => s.as_str(),
    }
}

/// SessionStateのソート順位（基本設計6節の7分類。稼働状態を先頭に置く）。
fn state_rank(state: &SessionState) -> u8 {
    match state {
        SessionState::Running => 0,
        SessionState::Waiting => 1,
        SessionState::Idle => 2,
        SessionState::Quiet { .. } => 3,
        SessionState::Failed => 4,
        SessionState::Orphaned => 5,
        SessionState::Unknown => 6,
    }
}

/// グローバルフィルタをエージェント行へ適用する（詳細設計8.1節:
/// task_title・エージェント種別・locationへの一致。Processesビューと同じ照合器を使う）。
pub fn filter_agent_rows(rows: &[AgentSessionRow], filter: &FilterState) -> Vec<AgentSessionRow> {
    if !filter.is_active() {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|r| {
            filter.matches(r.task_title.as_deref().unwrap_or(""))
                || filter.matches(agent_kind_label(&r.agent))
                || filter.matches(&r.location)
        })
        .cloned()
        .collect()
}

/// 描画用複製に対する並べ替え（詳細設計8.1節。共有スナップショットには適用しない）。
pub fn sort_agent_rows(rows: &mut [AgentSessionRow], col: AgentSortColumn, dir: SortDirection) {
    rows.sort_by(|a, b| {
        let ord = match col {
            AgentSortColumn::State => state_rank(&a.state).cmp(&state_rank(&b.state)),
            AgentSortColumn::Agent => agent_kind_label(&a.agent).cmp(agent_kind_label(&b.agent)),
            AgentSortColumn::Task => a
                .task_title
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b.task_title.as_deref().unwrap_or("").to_lowercase()),
            AgentSortColumn::Location => a.location.cmp(&b.location),
            AgentSortColumn::Cpu => a
                .cpu_percent
                .unwrap_or(0.0)
                .partial_cmp(&b.cpu_percent.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal),
            AgentSortColumn::Memory => a
                .memory_bytes
                .unwrap_or(0)
                .cmp(&b.memory_bytes.unwrap_or(0)),
        };
        match dir {
            SortDirection::Asc => ord,
            SortDirection::Desc => ord.reverse(),
        }
    });
}

/// 収集時刻がagents.refresh_msの2周期を超えて古いか（詳細設計8.4節の鮮度判定）。
pub fn is_snapshot_stale(
    collected_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
    refresh_ms: u64,
) -> bool {
    let limit = chrono::Duration::milliseconds(refresh_ms.saturating_mul(2) as i64);
    now.signed_duration_since(collected_at) > limit
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
    pub docker_endpoint: Option<DockerEndpoint>,
    // Actions
    pub pending_action: Option<Action>,
    pub confirm_message: String,
    /// Set when an action fails and the user should see diagnostic context.
    /// Cleared on any keypress while in [`AppMode::Error`].
    pub error_overlay: Option<ErrorState>,
    // Toggle states
    pub tail_follow: bool,
    pub wrap_logs: bool,
    pub tree_mode: bool,
    // Sort states
    pub process_sort: ProcessSortColumn,
    pub process_sort_dir: SortDirection,
    pub port_sort: PortSortColumn,
    pub port_sort_dir: SortDirection,
    // Agent sessions view (詳細設計8.1節: ビュー識別・選択・ソートをprocessと独立に保持)
    pub processes_view: ProcessesViewMode,
    pub agents_panel_state: PanelState,
    pub agent_sort: AgentSortColumn,
    pub agent_sort_dir: SortDirection,
    /// 背景収集タスクが置いた最新スナップショット（詳細設計8.4節）。
    pub agents_snapshot: Option<AgentsSnapshot>,
    /// 直近フレームでstalled表示だったか（遷移時のみログを出すための記憶）。
    agents_stalled: bool,
    // Agent sessions collection receiver (ログ収集と同型のチャネル方式)
    agents_rx: Option<mpsc::Receiver<AgentsSnapshot>>,
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
        #[cfg(not(test))]
        let docker_endpoint = docker_source.endpoint().cloned();
        #[cfg(test)]
        let docker_available = false;
        #[cfg(test)]
        let docker_context_name: Option<String> = None;
        #[cfg(test)]
        let docker_resolution_summary: Vec<String> = Vec::new();
        #[cfg(test)]
        let docker_endpoint: Option<DockerEndpoint> = None;

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
            docker_endpoint,
            pending_action: None,
            confirm_message: String::new(),
            error_overlay: None,
            log_filter: FilterState::new(),
            tail_follow,
            wrap_logs: false,
            tree_mode: false,
            process_sort: ProcessSortColumn::Cpu,
            process_sort_dir: SortDirection::Desc,
            port_sort: PortSortColumn::Port,
            port_sort_dir: SortDirection::Asc,
            processes_view: ProcessesViewMode::Processes,
            agents_panel_state: PanelState::new(),
            agent_sort: AgentSortColumn::State,
            agent_sort_dir: SortDirection::Asc,
            agents_snapshot: None,
            agents_stalled: false,
            agents_rx: None,
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
    /// Processes枠のビューを交互に切り替える（詳細設計8.1節、aキー）。
    pub fn toggle_processes_view(&mut self) {
        self.processes_view = match self.processes_view {
            ProcessesViewMode::Processes => ProcessesViewMode::AgentSessions,
            ProcessesViewMode::AgentSessions => ProcessesViewMode::Processes,
        };
        let view = match self.processes_view {
            ProcessesViewMode::Processes => "processes",
            ProcessesViewMode::AgentSessions => "agent_sessions",
        };
        tracing::info!(view, "processes view switched");
    }

    /// Processes枠がエージェントセッションビューを表示中か。
    pub fn agents_view_active(&self) -> bool {
        self.active_panel == Panel::Processes
            && self.processes_view == ProcessesViewMode::AgentSessions
    }

    /// エージェントセッションの行数（スナップショット未取得時は0）。
    pub fn agent_sessions_len(&self) -> usize {
        self.agents_snapshot
            .as_ref()
            .map(|s| s.sessions.len())
            .unwrap_or(0)
    }

    pub fn active_panel_data_len(&self) -> usize {
        match self.active_panel {
            Panel::Ports => self.port_entries.len(),
            Panel::Docker => self.docker_containers.len(),
            Panel::Processes => match self.processes_view {
                ProcessesViewMode::Processes => self.process_list.len(),
                ProcessesViewMode::AgentSessions => self.agent_sessions_len(),
            },
            Panel::Logs => self.log_buffer.len(),
        }
    }
    pub fn move_selection_down(&mut self) {
        if self.agents_view_active() {
            let max = self.agent_sessions_len().saturating_sub(1);
            let state = &mut self.agents_panel_state;
            if state.selected_index < max {
                state.selected_index += 1;
            }
            return;
        }
        if self.active_panel == Panel::Logs {
            // Logs uses a free-running scroll offset (paragraph based) rather
            // than a selected row. Manual scrolling also disengages tail-follow
            // so the user can read older lines without them being shoved off
            // the bottom by new entries.
            self.tail_follow = false;
            let state = &mut self.panel_states[Panel::Logs as usize];
            state.scroll_offset = state.scroll_offset.saturating_add(1);
            return;
        }
        let max = self.active_panel_data_len().saturating_sub(1);
        let state = &mut self.panel_states[self.active_panel as usize];
        if state.selected_index < max {
            state.selected_index += 1;
        }
    }
    pub fn move_selection_up(&mut self) {
        if self.agents_view_active() {
            let state = &mut self.agents_panel_state;
            state.selected_index = state.selected_index.saturating_sub(1);
            return;
        }
        if self.active_panel == Panel::Logs {
            self.tail_follow = false;
            let state = &mut self.panel_states[Panel::Logs as usize];
            state.scroll_offset = state.scroll_offset.saturating_sub(1);
            return;
        }
        let state = &mut self.panel_states[self.active_panel as usize];
        state.selected_index = state.selected_index.saturating_sub(1);
    }

    /// Cycle sort column forward (`>` key)
    pub fn sort_next(&mut self) {
        if self.agents_view_active() {
            // 並べ替えの実行は描画用複製に対して行う（visible_agent_rows）
            self.agent_sort = self.agent_sort.next();
            return;
        }
        match self.active_panel {
            Panel::Processes => self.process_sort = self.process_sort.next(),
            Panel::Ports => self.port_sort = self.port_sort.next(),
            _ => {}
        }
        self.apply_sort();
    }

    /// Cycle sort column backward (`<` key)
    pub fn sort_prev(&mut self) {
        if self.agents_view_active() {
            self.agent_sort = self.agent_sort.prev();
            return;
        }
        match self.active_panel {
            Panel::Processes => self.process_sort = self.process_sort.prev(),
            Panel::Ports => self.port_sort = self.port_sort.prev(),
            _ => {}
        }
        self.apply_sort();
    }

    /// Toggle sort direction (asc/desc) for current panel
    pub fn sort_toggle_direction(&mut self) {
        if self.agents_view_active() {
            self.agent_sort_dir = match self.agent_sort_dir {
                SortDirection::Asc => SortDirection::Desc,
                SortDirection::Desc => SortDirection::Asc,
            };
            return;
        }
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
            self.docker_endpoint.clone(),
            self.config.logs.buffer_lines,
        );
        self.log_rx = Some(rx);
    }

    /// Drain any pending log entries from background collectors into the buffer.
    /// Drain buffered log entries into the log buffer for this tick.
    ///
    /// Capped at [`Self::MAX_LOG_DRAIN_PER_TICK`] entries per call so a
    /// high-volume producer (e.g. dozens of Colima containers all emitting
    /// logs) can't starve the event loop. Any remaining entries are
    /// processed on the next iteration.
    pub fn drain_logs(&mut self) {
        if let Some(rx) = &mut self.log_rx {
            for _ in 0..Self::MAX_LOG_DRAIN_PER_TICK {
                match rx.try_recv() {
                    Ok(entry) => self.log_buffer.push(entry),
                    Err(_) => break,
                }
            }
        }
    }

    /// Maximum log entries drained per event-loop iteration. Tuned to be
    /// well under a millisecond of work so keyboard input is always serviced
    /// promptly even when many containers produce logs concurrently.
    pub const MAX_LOG_DRAIN_PER_TICK: usize = 512;

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
                    cwd: p.cwd().map(|c| c.to_path_buf()),
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
        // エージェントセッションビューの選択クランプ（詳細設計8.1節の分岐対象）
        let agents_len = self.agent_sessions_len();
        let state = &mut self.agents_panel_state;
        if agents_len == 0 {
            state.selected_index = 0;
        } else if state.selected_index >= agents_len {
            state.selected_index = agents_len - 1;
        }
    }

    /// 描画用のエージェント行複製を返す（グローバルフィルタ適用+ソート実行、詳細設計8.1節）。
    /// 共有スナップショット（agents_snapshot）は変更しない。
    pub fn visible_agent_rows(&self) -> Vec<AgentSessionRow> {
        let Some(snapshot) = &self.agents_snapshot else {
            return Vec::new();
        };
        let mut rows = filter_agent_rows(&snapshot.sessions, &self.global_filter);
        sort_agent_rows(&mut rows, self.agent_sort, self.agent_sort_dir);
        rows
    }

    /// エージェントセッションビューの選択行（描画順=フィルタ+ソート後の順で参照する）。
    pub fn selected_agent_row(&self) -> Option<AgentSessionRow> {
        self.visible_agent_rows()
            .into_iter()
            .nth(self.agents_panel_state.selected_index)
    }

    /// スナップショットがrefresh_msの2周期を超えて古いか（詳細設計8.4節）。
    /// 未取得（起動直後や無効化時）は停止表示の対象にしない。
    pub fn agents_snapshot_stale(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        match &self.agents_snapshot {
            Some(s) => is_snapshot_stale(s.collected_at, now, self.config.agents.refresh_ms),
            None => false,
        }
    }

    /// 鮮度の遷移を検知してログを出す（毎フレーム呼んでよい。遷移時のみ出力）。
    /// stalledへの遷移はwarn、回復はinfo。
    pub fn update_agents_freshness(&mut self, now: chrono::DateTime<chrono::Utc>) {
        let stale = self.agents_snapshot_stale(now);
        if stale && !self.agents_stalled {
            tracing::warn!("agent sessions snapshot stalled");
        } else if !stale && self.agents_stalled {
            tracing::info!("agent sessions snapshot recovered");
        }
        self.agents_stalled = stale;
    }

    /// エージェントセッションの背景収集タスクを起動する（詳細設計8.4節）。
    /// tokioランタイム上で呼ぶこと。agents.enabled=falseなら起動しない。
    /// タスク内のpanicはtokio::spawnに閉じ込められ、TUI本体へは伝播しない
    /// （収集の停止として扱われ、鮮度表示が停止を可視化する）。
    pub fn start_agent_collection(&mut self) {
        if !self.config.agents.enabled {
            return;
        }
        self.agents_rx = Some(spawn_agent_collector(self.config.agents.clone()));
    }

    /// 背景収集タスクが起動済みか。
    pub fn agent_collection_started(&self) -> bool {
        self.agents_rx.is_some()
    }

    /// 背景収集タスクからの最新スナップショットを取り込む（drain_logsと同型）。
    pub fn drain_agents(&mut self) {
        if let Some(rx) = &mut self.agents_rx {
            while let Ok(snapshot) = rx.try_recv() {
                self.agents_snapshot = Some(snapshot);
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
            Action::StartContainer { id } => {
                #[cfg(not(test))]
                {
                    if let Err(e) = self.docker_source.start_container(id).await {
                        let log_tail = self
                            .docker_source
                            .tail_logs(id, Self::ERROR_LOG_TAIL_LINES)
                            .await
                            .unwrap_or_default();
                        self.set_start_error(e.to_string(), log_tail);
                    }
                }
                let _ = id;
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

    /// Number of recent log lines surfaced when a Docker action fails.
    ///
    /// Chosen large enough to capture stack traces and multi-line panics
    /// (typical Java/Python crashes can run 100+ lines), but small enough
    /// to render in a single overlay without paging.
    pub const ERROR_LOG_TAIL_LINES: usize = 200;

    /// Record a Start failure and transition into the error overlay mode.
    ///
    /// Extracted from [`Self::execute_action`] so unit tests can verify the
    /// failure-path UI state without instantiating a live Docker source.
    pub fn set_start_error(&mut self, message: impl Into<String>, log_tail: Vec<String>) {
        self.error_overlay = Some(ErrorState {
            title: "Start failed".to_string(),
            message: message.into(),
            log_tail,
        });
        self.mode = AppMode::Error;
    }

    /// Dismiss the error overlay and return to normal mode.
    pub fn dismiss_error_overlay(&mut self) {
        self.error_overlay = None;
        if matches!(self.mode, AppMode::Error) {
            self.mode = AppMode::Normal;
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

/// エージェントセッション収集の背景タスクを起動し、受信側チャネルを返す
/// （詳細設計8.4節。log_collector::spawn_log_collectorsと同型の構造）。
/// 1周期で全アダプタの収集・ps実行・git補完・統合まで行い、結果をチャネルへ送る。
fn spawn_agent_collector(cfg: crate::config::AgentsConfig) -> mpsc::Receiver<AgentsSnapshot> {
    use crate::data::agents::{self, CollectOptions};

    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(async move {
        let opts = CollectOptions {
            command_timeout_ms: cfg.command_timeout_ms,
            quiet_threshold_s: cfg.quiet_threshold_s,
        };
        let sources = agents::default_sources(&opts);
        let process_source = crate::data::processes::SysinfoProcessSource::new();
        let tty_provider = agents::process::PsTtyProvider {
            timeout_ms: opts.command_timeout_ms,
        };
        let mut git = agents::gitinfo::GitEnricher::new(
            std::sync::Arc::new(agents::SystemCommandRunner),
            opts.command_timeout_ms,
        );
        loop {
            let snapshot = agents::collect_snapshot(
                &sources,
                &process_source,
                &tty_provider,
                &mut git,
                &opts,
                chrono::Utc::now(),
            )
            .await;
            if tx.send(snapshot).await.is_err() {
                break; // 受信側（App）が破棄されたら終了する
            }
            tokio::time::sleep(std::time::Duration::from_millis(cfg.refresh_ms.max(100))).await;
        }
    });
    rx
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
    fn test_logs_panel_j_k_scrolls_offset_and_disables_follow() {
        // On the Logs panel j/k must advance the panel's scroll_offset and
        // turn off tail_follow so the user can read older entries.
        let mut app = test_app();
        app.active_panel = Panel::Logs;
        app.tail_follow = true;

        app.move_selection_down();
        app.move_selection_down();
        app.move_selection_down();
        assert!(!app.tail_follow, "manual scroll must disable tail_follow");
        assert_eq!(app.panel_states[Panel::Logs as usize].scroll_offset, 3);
        assert_eq!(
            app.panel_states[Panel::Logs as usize].selected_index,
            0,
            "selected_index for Logs should stay at 0 — scrolling drives scroll_offset"
        );

        app.move_selection_up();
        assert_eq!(app.panel_states[Panel::Logs as usize].scroll_offset, 2);
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
    fn test_drain_logs_is_capped_per_tick() {
        // A flood of log entries (e.g. dozens of Colima containers all
        // chattering) must not stall the event loop: drain_logs stops
        // after MAX_LOG_DRAIN_PER_TICK entries and leaves the rest for
        // the next iteration.
        use crate::data::logs::{LogEntry, LogLevel};
        let mut app = test_app();
        let flood = App::MAX_LOG_DRAIN_PER_TICK * 3;
        let (tx, rx) = tokio::sync::mpsc::channel(flood + 16);
        app.log_rx = Some(rx);
        for i in 0..flood {
            tx.try_send(LogEntry {
                timestamp: i as u64,
                source: "test".into(),
                level: LogLevel::Info,
                message: format!("entry{}", i),
            })
            .unwrap();
        }

        app.drain_logs();
        assert_eq!(app.log_buffer.len(), App::MAX_LOG_DRAIN_PER_TICK);

        // Subsequent calls continue draining the backlog.
        app.drain_logs();
        assert_eq!(app.log_buffer.len(), App::MAX_LOG_DRAIN_PER_TICK * 2);
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

    #[test]
    fn test_set_start_error_transitions_to_error_mode() {
        let mut app = test_app();
        app.set_start_error(
            "no such image".to_string(),
            vec!["pull access denied".to_string()],
        );
        assert!(matches!(app.mode, AppMode::Error));
        let overlay = app.error_overlay.as_ref().expect("overlay must be set");
        assert_eq!(overlay.title, "Start failed");
        assert_eq!(overlay.message, "no such image");
        assert_eq!(overlay.log_tail, vec!["pull access denied".to_string()]);
    }

    #[test]
    fn test_dismiss_error_overlay_clears_state_and_returns_to_normal() {
        let mut app = test_app();
        app.set_start_error("boom".to_string(), vec!["log line".to_string()]);
        assert!(matches!(app.mode, AppMode::Error));
        assert!(app.error_overlay.is_some());

        app.dismiss_error_overlay();
        assert!(matches!(app.mode, AppMode::Normal));
        assert!(app.error_overlay.is_none());
    }

    #[test]
    fn test_pending_start_container_action() {
        let mut app = test_app();
        app.pending_action = Some(Action::StartContainer {
            id: "deadbeef0000".into(),
        });
        let action = app.pending_action.as_ref().unwrap();
        assert!(action.description().starts_with("Start container"));
    }
}

/// エージェントセッションビュー統合のテスト（詳細設計8.1・8.4節、テスト計画10節、T6）。
#[cfg(test)]
mod agents_view_tests {
    use super::*;
    use crate::data::agents::model::{AgentKind, AgentSessionRow, SessionState, StateSource};
    use crate::data::agents::AgentsSnapshot;
    use chrono::Utc;

    fn test_app() -> App {
        App::new(Config::default())
    }

    fn arow(id: &str, agent: AgentKind, task: &str, location: &str, cpu: f32) -> AgentSessionRow {
        let mut r = AgentSessionRow::new(id, agent, StateSource::ClaudeCli);
        r.task_title = Some(task.to_string());
        r.location = location.to_string();
        r.cpu_percent = Some(cpu);
        r
    }

    fn snapshot_of(rows: Vec<AgentSessionRow>) -> AgentsSnapshot {
        AgentsSnapshot {
            collected_at: Utc::now(),
            sessions: rows,
            source_errors: vec![],
        }
    }

    fn three_rows() -> Vec<AgentSessionRow> {
        vec![
            arow("a", AgentKind::Claude, "fix parser", "ws:1", 1.0),
            arow("b", AgentKind::Codex, "write docs", "ws:2", 3.0),
            arow("c", AgentKind::Kimi, "misc task", "surface:9", 2.0),
        ]
    }

    fn app_with_rows(rows: Vec<AgentSessionRow>) -> App {
        let mut app = test_app();
        app.agents_snapshot = Some(snapshot_of(rows));
        app
    }

    fn push_procs(app: &mut App, n: usize) {
        use crate::data::processes::ProcessInfo;
        for i in 0..n {
            app.process_list.push(ProcessInfo {
                pid: 100 + i as u32,
                name: format!("p{i}"),
                command: "cmd".into(),
                user: String::new(),
                cpu_percent: 0.0,
                memory_bytes: 0,
                threads: 0,
                parent_pid: None,
                listening_ports: vec![],
                start_time: 0,
                cwd: None,
            });
        }
    }

    #[test]
    fn toggle_processes_view_emits_info_event_with_view_name() {
        let cap = crate::logging::capture::Capture::new();
        let _guard = tracing::subscriber::set_default(cap.subscriber());
        let mut app = test_app();
        app.toggle_processes_view();
        let ev = cap
            .find("processes view switched")
            .expect("view switch event emitted");
        assert_eq!(ev.level, tracing::Level::INFO);
        assert!(ev.fields.contains("view=agent_sessions"), "{}", ev.fields);
        app.toggle_processes_view();
        let last = cap.events().pop().unwrap();
        assert!(last.fields.contains("view=processes"), "{}", last.fields);
    }

    #[test]
    fn stalled_transition_emits_warn_once_and_recovery_info_once() {
        let cap = crate::logging::capture::Capture::new();
        let _guard = tracing::subscriber::set_default(cap.subscriber());
        let mut app = test_app();
        let now = Utc::now();
        // 収集時刻が十分古いスナップショット → stalled
        app.agents_snapshot = Some(AgentsSnapshot {
            collected_at: now - chrono::Duration::seconds(3600),
            sessions: vec![],
            source_errors: vec![],
        });
        app.update_agents_freshness(now);
        app.update_agents_freshness(now); // 2フレーム連続でも1回だけ
        assert_eq!(cap.count(tracing::Level::WARN, "stalled"), 1);
        // 新鮮なスナップショットへ回復
        app.agents_snapshot = Some(snapshot_of(vec![]));
        app.update_agents_freshness(Utc::now());
        app.update_agents_freshness(Utc::now());
        assert_eq!(cap.count(tracing::Level::INFO, "recovered"), 1);
        assert_eq!(cap.count(tracing::Level::WARN, "stalled"), 1);
    }

    #[test]
    fn default_view_is_processes_and_toggle_alternates() {
        let mut app = test_app();
        assert_eq!(app.processes_view, ProcessesViewMode::Processes);
        app.toggle_processes_view();
        assert_eq!(app.processes_view, ProcessesViewMode::AgentSessions);
        app.toggle_processes_view();
        assert_eq!(app.processes_view, ProcessesViewMode::Processes);
    }

    #[test]
    fn data_len_branches_on_view() {
        let mut app = app_with_rows(three_rows());
        push_procs(&mut app, 1);
        app.active_panel = Panel::Processes;
        assert_eq!(app.active_panel_data_len(), 1);
        app.toggle_processes_view();
        assert_eq!(app.active_panel_data_len(), 3);
    }

    #[test]
    fn selection_is_independent_per_view() {
        let mut app = app_with_rows(three_rows());
        push_procs(&mut app, 5);
        app.active_panel = Panel::Processes;
        app.move_selection_down();
        app.move_selection_down();
        app.move_selection_down();
        assert_eq!(
            app.panel_states[Panel::Processes as usize].selected_index,
            3
        );

        app.toggle_processes_view();
        assert_eq!(app.agents_panel_state.selected_index, 0);
        app.move_selection_down();
        assert_eq!(app.agents_panel_state.selected_index, 1);
        // process側の選択は動いていない
        assert_eq!(
            app.panel_states[Panel::Processes as usize].selected_index,
            3
        );

        app.toggle_processes_view();
        assert_eq!(
            app.panel_states[Panel::Processes as usize].selected_index,
            3
        );
        // エージェント側の選択も保持される
        assert_eq!(app.agents_panel_state.selected_index, 1);
    }

    #[test]
    fn agent_selection_does_not_overflow() {
        let mut app = app_with_rows(vec![arow("a", AgentKind::Claude, "t", "l", 0.0)]);
        app.active_panel = Panel::Processes;
        app.toggle_processes_view();
        app.move_selection_down();
        app.move_selection_down();
        assert_eq!(app.agents_panel_state.selected_index, 0);
        app.move_selection_up();
        assert_eq!(app.agents_panel_state.selected_index, 0);
    }

    #[test]
    fn sort_state_is_independent_per_view() {
        let mut app = app_with_rows(three_rows());
        app.active_panel = Panel::Processes;
        let initial_proc_sort = app.process_sort;

        app.toggle_processes_view();
        let initial_agent_sort = app.agent_sort;
        app.sort_next();
        assert_ne!(app.agent_sort, initial_agent_sort);
        assert_eq!(
            app.process_sort, initial_proc_sort,
            "process側ソート列は不変"
        );

        app.sort_toggle_direction();
        assert_eq!(app.agent_sort_dir, SortDirection::Desc);
        assert_eq!(
            app.process_sort_dir,
            SortDirection::Desc,
            "既定値のまま不変"
        );

        let agent_sort_now = app.agent_sort;
        app.toggle_processes_view();
        app.sort_next();
        assert_ne!(app.process_sort, initial_proc_sort);
        assert_eq!(app.agent_sort, agent_sort_now, "agent側ソート列は不変");
    }

    #[test]
    fn sort_prev_cycles_back_on_agent_view() {
        let mut app = app_with_rows(three_rows());
        app.active_panel = Panel::Processes;
        app.toggle_processes_view();
        let initial = app.agent_sort;
        app.sort_next();
        app.sort_prev();
        assert_eq!(app.agent_sort, initial);
    }

    #[test]
    fn visible_rows_apply_global_filter_on_task_agent_and_location() {
        let mut app = app_with_rows(three_rows());
        app.global_filter.set_query("parser");
        let rows = app.visible_agent_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "a");

        app.global_filter.set_query("codex");
        let rows = app.visible_agent_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "b");

        app.global_filter.set_query("surface");
        let rows = app.visible_agent_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "c");

        app.global_filter.clear();
        assert_eq!(app.visible_agent_rows().len(), 3);
    }

    #[test]
    fn visible_rows_sort_does_not_mutate_shared_snapshot() {
        let mut app = app_with_rows(three_rows());
        app.agent_sort = AgentSortColumn::Cpu;
        app.agent_sort_dir = SortDirection::Desc;
        let rows = app.visible_agent_rows();
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c", "a"]);
        // 共有スナップショット側の並びは元のまま（描画用複製のみ並べ替え）
        let orig: Vec<&str> = app
            .agents_snapshot
            .as_ref()
            .unwrap()
            .sessions
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(orig, vec!["a", "b", "c"]);
    }

    #[test]
    fn sort_agent_rows_covers_all_columns() {
        let mut rows = three_rows();
        rows[0].memory_bytes = Some(300);
        rows[1].memory_bytes = Some(100);
        rows[2].memory_bytes = Some(200);
        rows[0].state = SessionState::Unknown;
        rows[1].state = SessionState::Running;
        rows[2].state = SessionState::Waiting;

        let mut v = rows.clone();
        sort_agent_rows(&mut v, AgentSortColumn::Memory, SortDirection::Asc);
        assert_eq!(v[0].id, "b");
        assert_eq!(v[2].id, "a");

        let mut v = rows.clone();
        sort_agent_rows(&mut v, AgentSortColumn::State, SortDirection::Asc);
        assert_eq!(v[0].id, "b", "runningが先頭");
        assert_eq!(v[2].id, "a", "unknownが末尾");

        let mut v = rows.clone();
        sort_agent_rows(&mut v, AgentSortColumn::Agent, SortDirection::Asc);
        assert_eq!(v[0].id, "a", "claude < codex < kimi");

        let mut v = rows.clone();
        sort_agent_rows(&mut v, AgentSortColumn::Task, SortDirection::Asc);
        assert_eq!(v[0].id, "a", "fix parser が先頭");

        let mut v = rows.clone();
        sort_agent_rows(&mut v, AgentSortColumn::Location, SortDirection::Asc);
        assert_eq!(v[0].id, "c", "surface:9 が昇順先頭");
    }

    #[test]
    fn clamp_selections_clamps_agent_selection() {
        let mut app = app_with_rows(vec![
            arow("a", AgentKind::Claude, "t", "l", 0.0),
            arow("b", AgentKind::Claude, "t", "l", 0.0),
        ]);
        app.agents_panel_state.selected_index = 5;
        app.clamp_selections();
        assert_eq!(app.agents_panel_state.selected_index, 1);

        app.agents_snapshot = Some(snapshot_of(vec![]));
        app.clamp_selections();
        assert_eq!(app.agents_panel_state.selected_index, 0);
    }

    #[test]
    fn selected_agent_row_follows_sorted_filtered_order() {
        let mut app = app_with_rows(three_rows());
        app.agent_sort = AgentSortColumn::Cpu;
        app.agent_sort_dir = SortDirection::Desc;
        app.agents_panel_state.selected_index = 0;
        assert_eq!(app.selected_agent_row().unwrap().id, "b");
        app.agents_panel_state.selected_index = 2;
        assert_eq!(app.selected_agent_row().unwrap().id, "a");
        app.agents_panel_state.selected_index = 3;
        assert!(app.selected_agent_row().is_none());
    }

    #[test]
    fn snapshot_staleness_uses_two_refresh_periods() {
        let now = Utc::now();
        let refresh_ms = 5000u64;
        // ちょうど2周期は停止扱いにしない（「超えて古い」場合のみ）
        let at_boundary = now - chrono::Duration::milliseconds(10_000);
        assert!(!is_snapshot_stale(at_boundary, now, refresh_ms));
        let beyond = now - chrono::Duration::milliseconds(10_001);
        assert!(is_snapshot_stale(beyond, now, refresh_ms));

        let mut app = test_app();
        assert!(
            !app.agents_snapshot_stale(now),
            "スナップショット未取得は停止表示しない"
        );
        app.agents_snapshot = Some(AgentsSnapshot {
            collected_at: beyond,
            sessions: vec![],
            source_errors: vec![],
        });
        assert!(app.agents_snapshot_stale(now));
    }

    #[test]
    fn drain_agents_updates_snapshot_to_latest() {
        let mut app = test_app();
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        app.agents_rx = Some(rx);
        tx.try_send(snapshot_of(vec![arow(
            "a",
            AgentKind::Claude,
            "t",
            "l",
            0.0,
        )]))
        .unwrap();
        tx.try_send(snapshot_of(three_rows())).unwrap();
        app.drain_agents();
        assert_eq!(app.agents_snapshot.as_ref().unwrap().sessions.len(), 3);
    }

    #[test]
    fn start_agent_collection_disabled_does_not_start() {
        let mut app = test_app();
        app.config.agents.enabled = false;
        app.start_agent_collection();
        assert!(!app.agent_collection_started());
    }

    #[tokio::test]
    async fn start_agent_collection_enabled_sets_receiver() {
        let mut app = test_app();
        app.config.agents.enabled = true;
        app.start_agent_collection();
        assert!(app.agent_collection_started());
    }

    /// T6並行性: 背景収集タスクとスナップショット読み取りの同時動作を反復し、
    /// パニック・不整合がないことを検査する（反復回数30、unsafe不使用）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn t6_concurrent_collection_and_reads_have_no_panic_or_inconsistency() {
        use crate::data::agents::{
            collect_snapshot, gitinfo::GitEnricher, process::TtyProvider, AgentSource,
            CollectOptions, CommandOutput, CommandRunner,
        };
        use crate::data::processes::{ProcessInfo, ProcessSource};
        use async_trait::async_trait;
        use std::collections::HashMap;
        use std::sync::Arc;

        struct FakeSource;
        #[async_trait]
        impl AgentSource for FakeSource {
            fn name(&self) -> &'static str {
                "fake"
            }
            async fn is_available(&self) -> bool {
                true
            }
            async fn collect(&self) -> anyhow::Result<Vec<AgentSessionRow>> {
                Ok(vec![
                    arow("a", AgentKind::Claude, "fix parser", "ws:1", 1.0),
                    arow("b", AgentKind::Codex, "write docs", "ws:2", 3.0),
                    arow("c", AgentKind::Kimi, "misc task", "surface:9", 2.0),
                ])
            }
        }

        struct NoProc;
        impl ProcessSource for NoProc {
            fn list_processes(&self) -> anyhow::Result<Vec<ProcessInfo>> {
                Ok(vec![])
            }
            fn kill_process(&self, _pid: u32, _force: bool) -> anyhow::Result<()> {
                Ok(())
            }
        }

        struct NoTty;
        #[async_trait]
        impl TtyProvider for NoTty {
            async fn tty_by_pid(&self) -> anyhow::Result<HashMap<u32, String>> {
                Ok(HashMap::new())
            }
        }

        struct NoRunner;
        #[async_trait]
        impl CommandRunner for NoRunner {
            async fn run(
                &self,
                _program: &str,
                _args: &[&str],
                _timeout_ms: u64,
            ) -> anyhow::Result<CommandOutput> {
                Ok(CommandOutput {
                    stdout: String::new(),
                    timed_out: false,
                    success: false,
                })
            }
            fn exists(&self, _program: &str) -> bool {
                false
            }
        }

        const ITERATIONS: usize = 30;
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let handle = tokio::spawn(async move {
            let sources: Vec<Box<dyn AgentSource>> = vec![Box::new(FakeSource)];
            let mut git = GitEnricher::new(Arc::new(NoRunner), 100);
            for _ in 0..ITERATIONS {
                let snap = collect_snapshot(
                    &sources,
                    &NoProc,
                    &NoTty,
                    &mut git,
                    &CollectOptions::default(),
                    Utc::now(),
                )
                .await;
                if tx.send(snap).await.is_err() {
                    return;
                }
                tokio::task::yield_now().await;
            }
        });

        let mut app = test_app();
        app.agents_rx = Some(rx);
        app.active_panel = Panel::Processes;
        app.processes_view = ProcessesViewMode::AgentSessions;
        loop {
            app.drain_agents();
            if let Some(snap) = &app.agents_snapshot {
                // 不整合検査: 収集は常に3行を返すので読み取りも常に3行
                assert_eq!(snap.sessions.len(), 3);
                assert_eq!(app.visible_agent_rows().len(), 3);
                app.clamp_selections();
                app.sort_next();
                app.move_selection_down();
            }
            if handle.is_finished() {
                break;
            }
            tokio::task::yield_now().await;
        }
        // 背景タスク内のpanicはここで検出される
        handle.await.expect("collector task must not panic");
        app.drain_agents();
        assert_eq!(app.agents_snapshot.as_ref().unwrap().sessions.len(), 3);
    }
}
