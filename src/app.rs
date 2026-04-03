use crate::config::Config;
use crate::event::Panel;
use crate::filter::FilterState;

#[derive(Debug, PartialEq)]
pub enum AppMode { Normal, GlobalFilter, LocalFilter, Confirm, Help }

#[derive(Debug)]
pub struct PanelState {
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub local_filter: FilterState,
}

impl PanelState {
    pub fn new() -> Self {
        Self { selected_index: 0, scroll_offset: 0, local_filter: FilterState::new() }
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
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config, active_panel: Panel::Ports, fullscreen_panel: None,
            should_quit: false, mode: AppMode::Normal,
            global_filter: FilterState::new(),
            panel_states: vec![PanelState::new(), PanelState::new(), PanelState::new(), PanelState::new()],
        }
    }
    pub fn next_panel(&mut self) { self.active_panel = self.active_panel.next(); }
    pub fn prev_panel(&mut self) { self.active_panel = self.active_panel.prev(); }
    pub fn select_panel(&mut self, index: usize) {
        if let Some(panel) = Panel::from_index(index) { self.active_panel = panel; }
    }
    pub fn toggle_fullscreen(&mut self) {
        self.fullscreen_panel = match self.fullscreen_panel { Some(_) => None, None => Some(self.active_panel) };
    }
    pub fn select_panel_or_fullscreen(&mut self, index: usize) {
        if let Some(panel) = Panel::from_index(index) {
            if self.active_panel == panel { self.toggle_fullscreen(); }
            else { self.active_panel = panel; self.fullscreen_panel = None; }
        }
    }
    pub fn enter_global_filter(&mut self) { self.mode = AppMode::GlobalFilter; }
    pub fn quit(&mut self) { self.should_quit = true; }
    pub fn move_selection_down(&mut self) {
        let state = &mut self.panel_states[self.active_panel as usize];
        state.selected_index = state.selected_index.saturating_add(1);
    }
    pub fn move_selection_up(&mut self) {
        let state = &mut self.panel_states[self.active_panel as usize];
        state.selected_index = state.selected_index.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_app() -> App { App::new(Config::default()) }

    #[test] fn test_initial_state() {
        let app = test_app();
        assert!(matches!(app.active_panel, Panel::Ports));
        assert!(!app.should_quit);
        assert!(matches!(app.mode, AppMode::Normal));
        assert!(!app.global_filter.is_active());
        assert!(app.fullscreen_panel.is_none());
    }
    #[test] fn test_next_panel() {
        let mut app = test_app();
        app.next_panel(); assert!(matches!(app.active_panel, Panel::Docker));
        app.next_panel(); assert!(matches!(app.active_panel, Panel::Processes));
    }
    #[test] fn test_prev_panel() {
        let mut app = test_app();
        app.prev_panel(); assert!(matches!(app.active_panel, Panel::Logs));
    }
    #[test] fn test_select_panel_by_index() {
        let mut app = test_app();
        app.select_panel(2); assert!(matches!(app.active_panel, Panel::Processes));
    }
    #[test] fn test_toggle_fullscreen() {
        let mut app = test_app();
        assert!(app.fullscreen_panel.is_none());
        app.toggle_fullscreen();
        assert!(matches!(app.fullscreen_panel, Some(Panel::Ports)));
        app.toggle_fullscreen();
        assert!(app.fullscreen_panel.is_none());
    }
    #[test] fn test_enter_filter_mode() {
        let mut app = test_app();
        app.enter_global_filter();
        assert!(matches!(app.mode, AppMode::GlobalFilter));
    }
    #[test] fn test_quit() { let mut app = test_app(); app.quit(); assert!(app.should_quit); }
    #[test] fn test_move_selection() {
        let mut app = test_app();
        app.move_selection_down(); assert_eq!(app.panel_states[0].selected_index, 1);
        app.move_selection_down(); assert_eq!(app.panel_states[0].selected_index, 2);
        app.move_selection_up(); assert_eq!(app.panel_states[0].selected_index, 1);
    }
    #[test] fn test_selection_no_underflow() {
        let mut app = test_app();
        app.move_selection_up(); assert_eq!(app.panel_states[0].selected_index, 0);
    }
}
