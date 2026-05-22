pub mod common;
pub mod layout;
pub mod panels;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::TableState;
use ratatui::Frame;

use crate::action::Action;
use crate::app::{App, AppMode};
use crate::data::docker::ContainerInfo;
use crate::data::ports::PortEntry;
use crate::data::processes::ProcessInfo;
use crate::event::Panel;
use crate::ui::common::{ConfirmDialog, ErrorOverlay, HelpOverlay};
use crate::ui::layout::{compute_layout, LayoutMode};
use crate::ui::panels::docker::DockerPanel;
use crate::ui::panels::logs::LogsPanel;
use crate::ui::panels::ports::PortsPanel;
use crate::ui::panels::processes::ProcessesPanel;

/// Draw all panels and overlays onto the frame.
///
/// Takes `&mut App` because table panels persist their scroll offsets
/// across frames via `PanelState::scroll_offset` — ratatui's `TableState`
/// adjusts the offset during `render_stateful_widget` and we read the
/// updated value back so the next frame stays scrolled.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let (panel_areas, status_area) = compute_layout(area, LayoutMode::Quad, app.fullscreen_panel);
    let filter_text = app.global_filter.query();
    let filter_active = app.global_filter.is_active();

    // Filter data when global filter is active
    let filtered_ports: Vec<PortEntry> = if filter_active {
        app.port_entries
            .iter()
            .filter(|e| {
                app.global_filter.matches(&e.process_name)
                    || app.global_filter.matches(&format!(":{}", e.port))
            })
            .cloned()
            .collect()
    } else {
        app.port_entries.clone()
    };

    let filtered_containers: Vec<ContainerInfo> = if filter_active {
        app.docker_containers
            .iter()
            .filter(|c| app.global_filter.matches(&c.name) || app.global_filter.matches(&c.image))
            .cloned()
            .collect()
    } else {
        app.docker_containers.clone()
    };

    let filtered_processes: Vec<ProcessInfo> = if filter_active {
        app.process_list
            .iter()
            .filter(|p| app.global_filter.matches(&p.name) || app.global_filter.matches(&p.command))
            .cloned()
            .collect()
    } else {
        app.process_list.clone()
    };

    // Ports panel
    let ports_area = panel_areas[Panel::Ports as usize];
    if ports_area.width > 0 && ports_area.height > 0 {
        let selected = app.panel_states[Panel::Ports as usize].selected_index;
        let offset = app.panel_states[Panel::Ports as usize].scroll_offset;
        let ports_panel = PortsPanel {
            entries: &filtered_ports,
            selected,
            filter_text,
            is_focused: app.active_panel == Panel::Ports,
            sort_column: app.port_sort,
            sort_direction: app.port_sort_dir,
        };
        let mut state = TableState::default()
            .with_selected(Some(selected))
            .with_offset(offset);
        frame.render_stateful_widget(ports_panel, ports_area, &mut state);
        app.panel_states[Panel::Ports as usize].scroll_offset = state.offset();
    }

    // Docker panel
    let docker_area = panel_areas[Panel::Docker as usize];
    if docker_area.width > 0 && docker_area.height > 0 {
        let selected = app.panel_states[Panel::Docker as usize].selected_index;
        let offset = app.panel_states[Panel::Docker as usize].scroll_offset;
        let docker_panel = DockerPanel {
            containers: &filtered_containers,
            selected,
            filter_text,
            is_focused: app.active_panel == Panel::Docker,
            is_available: app.docker_available,
            context_name: app.docker_context_name.as_deref(),
            resolution_summary: &app.docker_resolution_summary,
        };
        let mut state = TableState::default()
            .with_selected(Some(selected))
            .with_offset(offset);
        frame.render_stateful_widget(docker_panel, docker_area, &mut state);
        app.panel_states[Panel::Docker as usize].scroll_offset = state.offset();
    }

    // Processes panel
    let processes_area = panel_areas[Panel::Processes as usize];
    if processes_area.width > 0 && processes_area.height > 0 {
        let selected = app.panel_states[Panel::Processes as usize].selected_index;
        let offset = app.panel_states[Panel::Processes as usize].scroll_offset;
        let processes_panel = ProcessesPanel {
            processes: &filtered_processes,
            selected,
            filter_text,
            is_focused: app.active_panel == Panel::Processes,
            tree_mode: app.tree_mode,
            sort_column: app.process_sort,
            sort_direction: app.process_sort_dir,
        };
        let mut state = TableState::default()
            .with_selected(Some(selected))
            .with_offset(offset);
        frame.render_stateful_widget(processes_panel, processes_area, &mut state);
        app.panel_states[Panel::Processes as usize].scroll_offset = state.offset();
    }

    // Logs panel — uses log-local filter (AND condition) + Docker container filter
    let logs_area = panel_areas[Panel::Logs as usize];
    if logs_area.width > 0 && logs_area.height > 0 {
        let container_filter = app.selected_container_name();
        let filtered_total = app.filtered_log_count();
        let inner_height = (logs_area.height as usize).saturating_sub(2);
        let max_scroll = filtered_total.saturating_sub(inner_height);
        // Clamp persisted manual scroll so it can't exceed the available range
        // (the user may have pressed `j` past the end before new entries
        // arrived, or the panel may have grown).
        let logs_state = &mut app.panel_states[Panel::Logs as usize];
        logs_state.scroll_offset = logs_state.scroll_offset.min(max_scroll);
        let scroll_offset = logs_state.scroll_offset;
        let logs_panel = LogsPanel {
            buffer: &app.log_buffer,
            selected: logs_state.selected_index,
            filter_text: app.log_filter.query(),
            container_filter: container_filter.as_deref(),
            log_filter: &app.log_filter,
            is_focused: app.active_panel == Panel::Logs,
            tail_follow: app.tail_follow,
            wrap: app.wrap_logs,
            scroll_offset,
        };
        frame.render_widget(logs_panel, logs_area);
    }

    // Status bar (always visible)
    if status_area.height > 0 {
        let status_line = build_status_line(app);
        frame.render_widget(status_line, status_area);
    }

    // Overlays (drawn on top of everything)
    match app.mode {
        AppMode::Confirm => {
            let dialog = ConfirmDialog {
                message: &app.confirm_message,
            };
            frame.render_widget(dialog, area);
        }
        AppMode::Help => {
            frame.render_widget(HelpOverlay, area);
        }
        AppMode::Error => {
            if let Some(err) = app.error_overlay.as_ref() {
                let overlay = ErrorOverlay {
                    title: &err.title,
                    message: &err.message,
                    log_tail: &err.log_tail,
                };
                frame.render_widget(overlay, area);
            }
        }
        AppMode::GlobalFilter | AppMode::LogFilter if status_area.height > 0 => {
            // Replace status bar with filter input
            let (label, color, query) = match app.mode {
                AppMode::GlobalFilter => (" / Filter: ", Color::Yellow, app.global_filter.query()),
                AppMode::LogFilter => (
                    " f Log Filter (AND): ",
                    Color::Green,
                    app.log_filter.query(),
                ),
                _ => unreachable!(),
            };
            let filter_bar = ratatui::widgets::Paragraph::new(ratatui::text::Line::from(vec![
                Span::styled(
                    label,
                    Style::default()
                        .fg(Color::Black)
                        .bg(color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{query}|"), Style::default().fg(color)),
            ]));
            frame.render_widget(filter_bar, status_area);
        }
        _ => {}
    }
}

fn build_status_line<'a>(app: &App) -> ratatui::widgets::Paragraph<'a> {
    use ratatui::text::Line;

    let panel_name = match app.active_panel {
        Panel::Ports => "Ports",
        Panel::Docker => "Docker",
        Panel::Processes => "Processes",
        Panel::Logs => "Logs",
    };

    // Context-sensitive hints based on active panel
    let context_hints: Vec<(&str, &str)> = match app.active_panel {
        Panel::Ports => vec![
            ("K", "Kill"),
            ("Ctrl+K", "Force Kill"),
            (",/.", "Sort"),
            ("S", "Sort Dir"),
        ],
        Panel::Docker => vec![
            ("u", "Start"),
            ("d", "Stop"),
            ("R", "Restart"),
            ("r", "Remove"),
        ],
        Panel::Processes => vec![
            ("K", "Kill"),
            ("Ctrl+K", "Force Kill"),
            ("t", "Tree"),
            (",/.", "Sort"),
            ("S", "Sort Dir"),
        ],
        Panel::Logs => vec![("f", "Filter"), ("F", "Follow"), ("w", "Wrap")],
    };

    // Common hints
    let common_hints: Vec<(&str, &str)> = vec![
        ("j/k", "Nav"),
        ("Tab", "Panel"),
        ("1-4", "Jump"),
        ("/", "Filter"),
        ("?", "Help"),
        ("q", "Quit"),
    ];

    let mut spans = Vec::new();

    // Panel indicator
    spans.push(Span::styled(
        format!(" {} ", panel_name),
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw(" "));

    // Context hints
    for (key, desc) in &context_hints {
        spans.push(Span::styled(
            format!(" {} ", key),
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ));
        spans.push(Span::styled(
            format!(" {} ", desc),
            Style::default().fg(Color::White),
        ));
    }

    // Separator
    spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));

    // Common hints
    for (key, desc) in &common_hints {
        spans.push(Span::styled(
            format!(" {} ", key),
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ));
        spans.push(Span::styled(
            format!(" {} ", desc),
            Style::default().fg(Color::Gray),
        ));
    }

    ratatui::widgets::Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Black))
}

/// Handle a key event, updating app state accordingly.
/// Returns true if the event was handled.
pub fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    match app.mode {
        AppMode::Normal => handle_normal_mode(app, key),
        AppMode::GlobalFilter => handle_global_filter_mode(app, key),
        AppMode::Confirm => handle_confirm_mode(app, key),
        AppMode::Help => handle_help_mode(app, key),
        AppMode::LocalFilter => false,
        AppMode::LogFilter => handle_log_filter_mode(app, key),
        AppMode::Error => handle_error_mode(app, key),
    }
}

fn handle_error_mode(app: &mut App, _key: KeyEvent) -> bool {
    // Any key dismisses the overlay and returns to Normal mode.
    app.dismiss_error_overlay();
    true
}

fn handle_normal_mode(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') => {
            app.quit();
            true
        }
        KeyCode::Char('?') => {
            app.mode = AppMode::Help;
            true
        }
        KeyCode::Char('/') => {
            app.enter_global_filter();
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_selection_down();
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.move_selection_up();
            true
        }
        KeyCode::Tab => {
            app.next_panel();
            true
        }
        KeyCode::BackTab => {
            app.prev_panel();
            true
        }
        KeyCode::Char('1') => {
            app.select_panel_or_fullscreen(0);
            true
        }
        KeyCode::Char('2') => {
            app.select_panel_or_fullscreen(1);
            true
        }
        KeyCode::Char('3') => {
            app.select_panel_or_fullscreen(2);
            true
        }
        KeyCode::Char('4') => {
            app.select_panel_or_fullscreen(3);
            true
        }
        KeyCode::Char('K') => {
            // K = SIGTERM, Ctrl+K = SIGKILL (force)
            let force = key.modifiers.contains(KeyModifiers::CONTROL);
            match app.active_panel {
                Panel::Ports => {
                    if let Some(pid) = app.selected_port_pid() {
                        let action = Action::KillProcess { pid, force };
                        app.confirm_message = action.description();
                        app.pending_action = Some(action);
                        app.mode = AppMode::Confirm;
                    }
                }
                Panel::Processes => {
                    if let Some(pid) = app.selected_process_pid() {
                        let action = Action::KillProcess { pid, force };
                        app.confirm_message = action.description();
                        app.pending_action = Some(action);
                        app.mode = AppMode::Confirm;
                    }
                }
                _ => {}
            }
            true
        }
        KeyCode::Char('u') => {
            if app.active_panel == Panel::Docker {
                if let Some(id) = app.selected_container_id() {
                    let action = Action::StartContainer { id };
                    app.confirm_message = action.description();
                    app.pending_action = Some(action);
                    app.mode = AppMode::Confirm;
                }
            }
            true
        }
        KeyCode::Char('d') => {
            if app.active_panel == Panel::Docker {
                if let Some(id) = app.selected_container_id() {
                    let action = Action::StopContainer { id };
                    app.confirm_message = action.description();
                    app.pending_action = Some(action);
                    app.mode = AppMode::Confirm;
                }
            }
            true
        }
        KeyCode::Char('R') => {
            if app.active_panel == Panel::Docker {
                if let Some(id) = app.selected_container_id() {
                    let action = Action::RestartContainer { id };
                    app.confirm_message = action.description();
                    app.pending_action = Some(action);
                    app.mode = AppMode::Confirm;
                }
            }
            true
        }
        KeyCode::Char('r') => {
            if app.active_panel == Panel::Docker {
                if let Some(id) = app.selected_container_id() {
                    let action = Action::RemoveContainer { id };
                    app.confirm_message = action.description();
                    app.pending_action = Some(action);
                    app.mode = AppMode::Confirm;
                }
            }
            true
        }
        KeyCode::Char('F') => {
            app.tail_follow = !app.tail_follow;
            true
        }
        KeyCode::Char('t') => {
            app.tree_mode = !app.tree_mode;
            true
        }
        KeyCode::Char('w') => {
            app.wrap_logs = !app.wrap_logs;
            true
        }
        KeyCode::Char('.') => {
            app.sort_next();
            true
        }
        KeyCode::Char(',') => {
            app.sort_prev();
            true
        }
        KeyCode::Char('S') => {
            app.sort_toggle_direction();
            true
        }
        KeyCode::Char('f') => {
            if app.active_panel == Panel::Logs {
                app.enter_log_filter();
            }
            true
        }
        _ => false,
    }
}

fn handle_global_filter_mode(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            app.global_filter.clear();
            app.mode = AppMode::Normal;
            true
        }
        KeyCode::Enter => {
            // Apply filter and return to normal mode
            app.mode = AppMode::Normal;
            true
        }
        KeyCode::Backspace => {
            let query = app.global_filter.query().to_string();
            if !query.is_empty() {
                let mut chars = query.chars();
                chars.next_back();
                let new_query = chars.as_str();
                app.global_filter.set_query(new_query);
            }
            true
        }
        KeyCode::Char(c) => {
            let mut query = app.global_filter.query().to_string();
            query.push(c);
            app.global_filter.set_query(&query);
            true
        }
        _ => false,
    }
}

fn handle_log_filter_mode(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            app.log_filter.clear();
            app.mode = AppMode::Normal;
            true
        }
        KeyCode::Enter => {
            app.mode = AppMode::Normal;
            true
        }
        KeyCode::Backspace => {
            let query = app.log_filter.query().to_string();
            if !query.is_empty() {
                let mut chars = query.chars();
                chars.next_back();
                let new_query = chars.as_str();
                app.log_filter.set_query(new_query);
            }
            true
        }
        KeyCode::Char(c) => {
            let mut query = app.log_filter.query().to_string();
            query.push(c);
            app.log_filter.set_query(&query);
            true
        }
        _ => false,
    }
}

fn handle_confirm_mode(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('y') | KeyCode::Enter => {
            // Action will be executed by the event loop (async)
            // We keep the pending_action for the event loop to pick up
            app.mode = AppMode::Normal;
            true
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            app.pending_action = None;
            app.confirm_message.clear();
            app.mode = AppMode::Normal;
            true
        }
        _ => false,
    }
}

fn handle_help_mode(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('?') | KeyCode::Esc => {
            app.mode = AppMode::Normal;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn test_handle_key_quit() {
        let mut app = App::new(Config::default());
        handle_key(&mut app, make_key(KeyCode::Char('q')));
        assert!(app.should_quit);
    }

    #[test]
    fn test_handle_key_help() {
        let mut app = App::new(Config::default());
        handle_key(&mut app, make_key(KeyCode::Char('?')));
        assert!(matches!(app.mode, AppMode::Help));
        handle_key(&mut app, make_key(KeyCode::Esc));
        assert!(matches!(app.mode, AppMode::Normal));
    }

    #[test]
    fn test_handle_key_filter() {
        let mut app = App::new(Config::default());
        handle_key(&mut app, make_key(KeyCode::Char('/')));
        assert!(matches!(app.mode, AppMode::GlobalFilter));
        handle_key(&mut app, make_key(KeyCode::Char('n')));
        handle_key(&mut app, make_key(KeyCode::Char('o')));
        assert_eq!(app.global_filter.query(), "no");
        handle_key(&mut app, make_key(KeyCode::Backspace));
        assert_eq!(app.global_filter.query(), "n");
        handle_key(&mut app, make_key(KeyCode::Enter));
        assert!(matches!(app.mode, AppMode::Normal));
        assert_eq!(app.global_filter.query(), "n");
    }

    #[test]
    fn test_handle_key_filter_esc_clears() {
        let mut app = App::new(Config::default());
        handle_key(&mut app, make_key(KeyCode::Char('/')));
        handle_key(&mut app, make_key(KeyCode::Char('x')));
        handle_key(&mut app, make_key(KeyCode::Esc));
        assert!(matches!(app.mode, AppMode::Normal));
        assert_eq!(app.global_filter.query(), "");
    }

    #[test]
    fn test_handle_key_navigation() {
        use crate::data::ports::{PortEntry, Protocol};
        let mut app = App::new(Config::default());
        // Add fake data so selection movement works
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
        handle_key(&mut app, make_key(KeyCode::Tab));
        assert!(matches!(app.active_panel, Panel::Docker));
        handle_key(&mut app, make_key(KeyCode::BackTab));
        assert!(matches!(app.active_panel, Panel::Ports));
        handle_key(&mut app, make_key(KeyCode::Char('j')));
        assert_eq!(app.panel_states[0].selected_index, 1);
        handle_key(&mut app, make_key(KeyCode::Char('k')));
        assert_eq!(app.panel_states[0].selected_index, 0);
    }

    #[test]
    fn test_handle_key_panel_select() {
        let mut app = App::new(Config::default());
        handle_key(&mut app, make_key(KeyCode::Char('3')));
        assert!(matches!(app.active_panel, Panel::Processes));
    }

    #[test]
    fn test_handle_key_confirm_cancel() {
        let mut app = App::new(Config::default());
        app.mode = AppMode::Confirm;
        app.pending_action = Some(Action::KillProcess {
            pid: 123,
            force: false,
        });
        handle_key(&mut app, make_key(KeyCode::Char('n')));
        assert!(matches!(app.mode, AppMode::Normal));
        assert!(app.pending_action.is_none());
    }

    #[test]
    fn test_handle_key_confirm_yes() {
        let mut app = App::new(Config::default());
        app.mode = AppMode::Confirm;
        app.pending_action = Some(Action::KillProcess {
            pid: 123,
            force: false,
        });
        handle_key(&mut app, make_key(KeyCode::Char('y')));
        assert!(matches!(app.mode, AppMode::Normal));
        // pending_action is kept for the event loop to execute
        assert!(app.pending_action.is_some());
    }

    #[test]
    fn test_draw_no_panic() {
        use ratatui::{backend::TestBackend, Terminal};
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(Config::default());
        terminal.draw(|f| draw(f, &mut app)).unwrap();
    }

    #[test]
    fn test_draw_with_help_overlay() {
        use ratatui::{backend::TestBackend, Terminal};
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(Config::default());
        app.mode = AppMode::Help;
        terminal.draw(|f| draw(f, &mut app)).unwrap();
    }

    #[test]
    fn test_draw_with_confirm_overlay() {
        use ratatui::{backend::TestBackend, Terminal};
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(Config::default());
        app.mode = AppMode::Confirm;
        app.confirm_message = "Kill process?".to_string();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
    }

    #[test]
    fn test_f_key_enters_log_filter_on_logs_panel() {
        let mut app = App::new(Config::default());
        app.active_panel = Panel::Logs;
        handle_key(&mut app, make_key(KeyCode::Char('f')));
        assert!(matches!(app.mode, AppMode::LogFilter));
    }

    #[test]
    fn test_f_key_does_nothing_on_other_panels() {
        let mut app = App::new(Config::default());
        app.active_panel = Panel::Ports;
        handle_key(&mut app, make_key(KeyCode::Char('f')));
        assert!(matches!(app.mode, AppMode::Normal));
    }

    #[test]
    fn test_log_filter_input_and_clear() {
        let mut app = App::new(Config::default());
        app.active_panel = Panel::Logs;
        handle_key(&mut app, make_key(KeyCode::Char('f')));
        assert!(matches!(app.mode, AppMode::LogFilter));
        handle_key(&mut app, make_key(KeyCode::Char('e')));
        handle_key(&mut app, make_key(KeyCode::Char('r')));
        handle_key(&mut app, make_key(KeyCode::Char('r')));
        assert_eq!(app.log_filter.query(), "err");
        // Enter confirms
        handle_key(&mut app, make_key(KeyCode::Enter));
        assert!(matches!(app.mode, AppMode::Normal));
        assert_eq!(app.log_filter.query(), "err");
    }

    #[test]
    fn test_log_filter_esc_clears() {
        let mut app = App::new(Config::default());
        app.active_panel = Panel::Logs;
        handle_key(&mut app, make_key(KeyCode::Char('f')));
        handle_key(&mut app, make_key(KeyCode::Char('x')));
        handle_key(&mut app, make_key(KeyCode::Esc));
        assert!(matches!(app.mode, AppMode::Normal));
        assert_eq!(app.log_filter.query(), "");
    }

    #[test]
    fn test_global_filter_does_not_affect_logs_panel() {
        // Verify that global filter text is NOT passed to logs panel
        let mut app = App::new(Config::default());
        app.global_filter.set_query("node");
        // The logs panel should use app.log_filter, not app.global_filter
        assert_eq!(app.log_filter.query(), "");
        assert_eq!(app.global_filter.query(), "node");
    }

    #[test]
    fn test_u_key_on_docker_enters_confirm_with_start_action() {
        use crate::data::docker::{ContainerInfo, ContainerState};
        let mut app = App::new(Config::default());
        app.active_panel = Panel::Docker;
        app.docker_containers.push(ContainerInfo {
            id: "deadbeef0000".into(),
            name: "stopped-svc".into(),
            image: "alpine".into(),
            state: ContainerState::Stopped,
            cpu_percent: 0.0,
            memory_bytes: 0,
            memory_limit: 0,
            ports: vec![],
            compose_project: None,
            created: "2026-01-01".into(),
        });
        handle_key(&mut app, make_key(KeyCode::Char('u')));
        assert!(matches!(app.mode, AppMode::Confirm));
        match app.pending_action.as_ref() {
            Some(Action::StartContainer { id }) => assert_eq!(id, "deadbeef0000"),
            other => panic!("expected StartContainer, got {:?}", other),
        }
        assert!(app.confirm_message.starts_with("Start container"));
    }

    #[test]
    fn test_u_key_on_other_panels_is_noop() {
        let mut app = App::new(Config::default());
        app.active_panel = Panel::Ports;
        handle_key(&mut app, make_key(KeyCode::Char('u')));
        assert!(matches!(app.mode, AppMode::Normal));
        assert!(app.pending_action.is_none());
    }

    #[test]
    fn test_error_mode_dismissed_by_any_key() {
        let mut app = App::new(Config::default());
        app.set_start_error("oops".to_string(), vec!["log".to_string()]);
        assert!(matches!(app.mode, AppMode::Error));
        // Any key — pick something unrelated to other handlers
        handle_key(&mut app, make_key(KeyCode::Char('x')));
        assert!(matches!(app.mode, AppMode::Normal));
        assert!(app.error_overlay.is_none());
    }

    #[test]
    fn test_draw_with_error_overlay() {
        use ratatui::{backend::TestBackend, Terminal};
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(Config::default());
        app.set_start_error(
            "no such image".to_string(),
            vec!["pull access denied".to_string()],
        );
        terminal.draw(|f| draw(f, &mut app)).unwrap();
    }

    #[test]
    fn test_draw_with_filter_bar() {
        use ratatui::{backend::TestBackend, Terminal};
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(Config::default());
        app.mode = AppMode::GlobalFilter;
        app.global_filter.set_query("node");
        terminal.draw(|f| draw(f, &mut app)).unwrap();
    }
}
