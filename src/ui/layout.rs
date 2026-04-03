use crate::event::Panel;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub enum LayoutMode {
    Quad,
}

pub fn compute_layout(area: Rect, _mode: LayoutMode, fullscreen: Option<Panel>) -> Vec<Rect> {
    if let Some(panel) = fullscreen {
        let mut rects = vec![Rect::new(0, 0, 0, 0); 4];
        rects[panel as usize] = area;
        return rects;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);
    vec![top[0], top[1], bottom[0], bottom[1]]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_quad_layout() {
        let area = Rect::new(0, 0, 120, 40);
        let panels = compute_layout(area, LayoutMode::Quad, None);
        assert_eq!(panels.len(), 4);
        for p in &panels {
            assert!(p.width > 0);
            assert!(p.height > 0);
        }
    }
    #[test]
    fn test_fullscreen_layout() {
        let area = Rect::new(0, 0, 120, 40);
        let panels = compute_layout(area, LayoutMode::Quad, Some(Panel::Docker));
        assert_eq!(panels[1], area);
        assert_eq!(panels[0].width, 0);
    }
    #[test]
    fn test_narrow_terminal() {
        let area = Rect::new(0, 0, 60, 20);
        let panels = compute_layout(area, LayoutMode::Quad, None);
        assert_eq!(panels.len(), 4);
    }
}
