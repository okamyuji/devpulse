use ratatui::{buffer::Buffer, layout::{Alignment, Rect}, style::{Color, Modifier, Style}, text::{Line, Span}, widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap}};

pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 { format!("{:.1}GB", bytes as f64 / 1_000_000_000.0) }
    else if bytes >= 1_000_000 { format!("{:.0}MB", bytes as f64 / 1_000_000.0) }
    else if bytes >= 1_000 { format!("{:.0}KB", bytes as f64 / 1_000.0) }
    else { format!("{}B", bytes) }
}

pub struct ConfirmDialog<'a> { pub message: &'a str }

impl<'a> Widget for ConfirmDialog<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = 44.min(area.width.saturating_sub(4));
        let height = 5.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect::new(x, y, width, height);
        Widget::render(Clear, dialog_area, buf);
        let block = Block::default().title(" Confirm ").borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow));
        let text = vec![
            Line::from(self.message),
            Line::from(""),
            Line::from(vec![
                Span::styled("[Y]es", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled("[N]o", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            ]),
        ];
        Widget::render(Paragraph::new(text).block(block).alignment(Alignment::Center), dialog_area, buf);
    }
}

pub struct HelpOverlay;

impl Widget for HelpOverlay {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = 50.min(area.width.saturating_sub(4));
        let height = 20.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let help_area = Rect::new(x, y, width, height);
        Widget::render(Clear, help_area, buf);
        let block = Block::default().title(" Help (press ? to close) ").borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan));
        let help_text = vec![
            Line::from("j/k        Move up/down"), Line::from("Tab        Next panel"),
            Line::from("Shift+Tab  Previous panel"), Line::from("1-4        Jump to panel / fullscreen"),
            Line::from("/          Global filter"), Line::from("f          Local filter"),
            Line::from("K          Kill process (SIGTERM)"), Line::from("Shift+K    Force kill (SIGKILL)"),
            Line::from("s          Stop Docker container"), Line::from("r          Restart Docker container"),
            Line::from("D          Delete (confirm required)"), Line::from("F          Toggle tail follow (Logs)"),
            Line::from("t          Toggle tree view (Processes)"), Line::from("w          Toggle line wrap (Logs)"),
            Line::from("y          Copy to clipboard"), Line::from("q          Quit"), Line::from("?          Toggle this help"),
        ];
        Widget::render(Paragraph::new(help_text).block(block).wrap(Wrap { trim: false }), help_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_confirm_dialog() {
        let d = ConfirmDialog { message: "Kill process 'node' (PID 1234)?" };
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        d.render(Rect::new(0, 0, 80, 24), &mut buf);
    }
    #[test] fn test_help_overlay() {
        let h = HelpOverlay;
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 30));
        h.render(Rect::new(0, 0, 80, 30), &mut buf);
    }
    #[test] fn test_confirm_small_area() {
        let d = ConfirmDialog { message: "Kill?" };
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
        d.render(Rect::new(0, 0, 20, 8), &mut buf);
    }
    #[test] fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500B");
        assert_eq!(format_bytes(1_500), "2KB");
        assert_eq!(format_bytes(1_500_000), "2MB");
        assert_eq!(format_bytes(1_500_000_000), "1.5GB");
    }
}
