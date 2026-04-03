use devpulse::app::{App, AppMode};
use devpulse::config::Config;
use devpulse::event::Panel;

#[test]
fn test_full_navigation_flow() {
    let mut app = App::new(Config::default());
    assert!(matches!(app.active_panel, Panel::Ports));
    assert!(matches!(app.mode, AppMode::Normal));

    app.next_panel();
    assert!(matches!(app.active_panel, Panel::Docker));

    app.select_panel_or_fullscreen(1);
    assert!(app.fullscreen_panel.is_some());

    app.select_panel_or_fullscreen(1);
    assert!(app.fullscreen_panel.is_none());

    app.enter_global_filter();
    assert!(matches!(app.mode, AppMode::GlobalFilter));

    app.global_filter.set_query("node");
    assert!(app.global_filter.is_active());

    app.quit();
    assert!(app.should_quit);
}

#[test]
fn test_panel_cycling() {
    let mut app = App::new(Config::default());
    // Cycle through all panels
    assert!(matches!(app.active_panel, Panel::Ports));
    app.next_panel();
    assert!(matches!(app.active_panel, Panel::Docker));
    app.next_panel();
    assert!(matches!(app.active_panel, Panel::Processes));
    app.next_panel();
    assert!(matches!(app.active_panel, Panel::Logs));
    app.next_panel();
    assert!(matches!(app.active_panel, Panel::Ports));

    // Reverse cycle
    app.prev_panel();
    assert!(matches!(app.active_panel, Panel::Logs));
}

#[test]
fn test_fullscreen_toggle_with_panel_switch() {
    let mut app = App::new(Config::default());
    // Select panel 1 (Ports)
    app.select_panel_or_fullscreen(0);
    assert!(app.fullscreen_panel.is_some());
    assert!(matches!(app.fullscreen_panel.unwrap(), Panel::Ports));

    // Select different panel clears fullscreen
    app.select_panel_or_fullscreen(2);
    assert!(app.fullscreen_panel.is_none());
    assert!(matches!(app.active_panel, Panel::Processes));
}

#[test]
fn test_selection_movement() {
    use devpulse::data::ports::{PortEntry, Protocol};
    let mut app = App::new(Config::default());
    // Add fake data so selection movement works
    for i in 0..10 {
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
    app.move_selection_down();
    app.move_selection_down();
    assert_eq!(app.panel_states[Panel::Ports as usize].selected_index, 3);
    app.move_selection_up();
    assert_eq!(app.panel_states[Panel::Ports as usize].selected_index, 2);
    // Test underflow protection
    app.move_selection_up();
    app.move_selection_up();
    app.move_selection_up();
    assert_eq!(app.panel_states[Panel::Ports as usize].selected_index, 0);
}

#[test]
fn test_global_filter_state() {
    let mut app = App::new(Config::default());
    assert!(!app.global_filter.is_active());
    app.global_filter.set_query("docker");
    assert!(app.global_filter.is_active());
    assert_eq!(app.global_filter.query(), "docker");
    app.global_filter.clear();
    assert!(!app.global_filter.is_active());
}

#[test]
fn test_action_module() {
    use devpulse::action::Action;

    let kill = Action::KillProcess {
        pid: 1234,
        force: false,
    };
    let desc = kill.description();
    assert!(desc.contains("1234"));
    assert!(desc.contains("SIGTERM"));

    let stop = Action::StopContainer {
        id: "abc123def456".to_string(),
    };
    assert!(stop.description().contains("Stop"));
}
