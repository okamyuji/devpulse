use crate::event::Panel;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub enum LayoutMode {
    Quad,
}

/// Returns (panel_rects[4], status_bar_rect)
pub fn compute_layout(
    area: Rect,
    _mode: LayoutMode,
    fullscreen: Option<Panel>,
) -> (Vec<Rect>, Rect) {
    // Reserve 1 row at the bottom for the status bar
    let (main_area, status_bar) = if area.height > 3 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);
        (chunks[0], chunks[1])
    } else {
        (area, Rect::new(0, 0, 0, 0))
    };

    if let Some(panel) = fullscreen {
        let mut rects = vec![Rect::new(0, 0, 0, 0); 4];
        rects[panel as usize] = main_area;
        return (rects, status_bar);
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_area);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);
    (vec![top[0], top[1], bottom[0], bottom[1]], status_bar)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_quad_layout() {
        let area = Rect::new(0, 0, 120, 40);
        let (panels, status) = compute_layout(area, LayoutMode::Quad, None);
        assert_eq!(panels.len(), 4);
        for p in &panels {
            assert!(p.width > 0);
            assert!(p.height > 0);
        }
        assert_eq!(status.height, 1);
        assert_eq!(status.width, 120);
    }
    #[test]
    fn test_fullscreen_layout() {
        let area = Rect::new(0, 0, 120, 40);
        let (panels, status) = compute_layout(area, LayoutMode::Quad, Some(Panel::Docker));
        assert_eq!(panels[1].width, 120);
        assert!(panels[1].height > 0);
        assert_eq!(panels[0].width, 0);
        assert_eq!(status.height, 1);
    }
    #[test]
    fn test_narrow_terminal() {
        let area = Rect::new(0, 0, 60, 20);
        let (panels, _) = compute_layout(area, LayoutMode::Quad, None);
        assert_eq!(panels.len(), 4);
    }
}
