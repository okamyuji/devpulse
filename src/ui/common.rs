use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1}GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.0}MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0}KB", bytes as f64 / 1_000.0)
    } else {
        format!("{}B", bytes)
    }
}

pub struct ConfirmDialog<'a> {
    pub message: &'a str,
}

impl<'a> Widget for ConfirmDialog<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = 44.min(area.width.saturating_sub(4));
        let height = 5.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect::new(x, y, width, height);
        Widget::render(Clear, dialog_area, buf);
        let block = Block::default()
            .title(" Confirm ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));
        let text = vec![
            Line::from(self.message),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "[Y]es",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    "[N]o",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]),
        ];
        Widget::render(
            Paragraph::new(text)
                .block(block)
                .alignment(Alignment::Center),
            dialog_area,
            buf,
        );
    }
}

/// Modal dialog shown when an action (currently: Docker Start) fails.
///
/// Renders the error title, message, and the last N log lines fetched from
/// the offending container so the user can see why it failed without having
/// to leave the TUI. Dismissed by any keypress while `AppMode::Error`.
pub struct ErrorOverlay<'a> {
    pub title: &'a str,
    pub message: &'a str,
    pub log_tail: &'a [String],
}

impl<'a> Widget for ErrorOverlay<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = (area.width.saturating_mul(7) / 10).max(40).min(area.width);
        let height = (area.height.saturating_mul(6) / 10).max(8).min(area.height);
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect::new(x, y, width, height);
        Widget::render(Clear, dialog_area, buf);

        let title = format!(" {} ", self.title);
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        // Reserve space for: 2 border rows + message + 1 blank + header + 1
        // blank + footer ≈ 7 rows of overhead. Anything left over is log
        // area; show the *tail* of the captured logs there (newest at the
        // bottom, which is what users want when diagnosing a failure).
        let overhead_rows: u16 = 7;
        let log_area_rows = dialog_area.height.saturating_sub(overhead_rows) as usize;
        let total_logs = self.log_tail.len();
        let displayed: &[String] = if total_logs > log_area_rows && log_area_rows > 0 {
            &self.log_tail[total_logs - log_area_rows..]
        } else {
            self.log_tail
        };
        let truncated = total_logs.saturating_sub(displayed.len());

        let header_text = if total_logs == 0 {
            "Recent logs:".to_string()
        } else if truncated > 0 {
            format!(
                "Recent logs (showing last {} of {} lines):",
                displayed.len(),
                total_logs
            )
        } else {
            format!("Recent logs ({} lines):", total_logs)
        };

        let mut lines: Vec<Line> = Vec::with_capacity(displayed.len() + 5);
        lines.push(Line::from(Span::styled(
            self.message.to_string(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            header_text,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        if displayed.is_empty() {
            lines.push(Line::from(Span::styled(
                "(no log output captured)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for log_line in displayed {
                lines.push(Line::from(Span::styled(
                    log_line.clone(),
                    Style::default().fg(Color::Gray),
                )));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "[Press any key to close]",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        Widget::render(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false }),
            dialog_area,
            buf,
        );
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
        let block = Block::default()
            .title(" Help (press ? to close) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let help_text = vec![
            Line::from("j/k        Move up/down"),
            Line::from("Tab        Next panel"),
            Line::from("Shift+Tab  Previous panel"),
            Line::from("1-4        Jump to panel / fullscreen"),
            Line::from("/          Global filter"),
            Line::from("K          Kill process (SIGTERM)"),
            Line::from("Ctrl+K     Force kill (SIGKILL)"),
            Line::from("u          Start Docker container (Up)"),
            Line::from("d          Stop Docker container (Down)"),
            Line::from("R          Restart Docker container"),
            Line::from("r          Remove container (confirm)"),
            Line::from("F          Toggle tail follow (Logs)"),
            Line::from("t          Toggle tree view (Processes)"),
            Line::from("w          Toggle line wrap (Logs)"),
            Line::from("q          Quit"),
            Line::from("?          Toggle this help"),
        ];
        Widget::render(
            Paragraph::new(help_text)
                .block(block)
                .wrap(Wrap { trim: false }),
            help_area,
            buf,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_confirm_dialog() {
        let d = ConfirmDialog {
            message: "Kill process 'node' (PID 1234)?",
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        d.render(Rect::new(0, 0, 80, 24), &mut buf);
    }
    #[test]
    fn test_help_overlay() {
        let h = HelpOverlay;
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 30));
        h.render(Rect::new(0, 0, 80, 30), &mut buf);
    }
    #[test]
    fn test_confirm_small_area() {
        let d = ConfirmDialog { message: "Kill?" };
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
        d.render(Rect::new(0, 0, 20, 8), &mut buf);
    }
    #[test]
    fn test_error_overlay_no_panic_with_logs() {
        let logs = vec![
            "panic: oom".to_string(),
            "exit code 137".to_string(),
            "container died".to_string(),
        ];
        let o = ErrorOverlay {
            title: "Start failed",
            message: "no such image: foo:bar",
            log_tail: &logs,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 30));
        o.render(Rect::new(0, 0, 100, 30), &mut buf);
    }
    #[test]
    fn test_error_overlay_no_panic_empty_logs() {
        let o = ErrorOverlay {
            title: "Start failed",
            message: "connection refused",
            log_tail: &[],
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        o.render(Rect::new(0, 0, 80, 24), &mut buf);
    }
    #[test]
    fn test_error_overlay_handles_many_logs_without_panic() {
        // Caller may pass 200+ lines (ERROR_LOG_TAIL_LINES); overlay must
        // truncate to fit and still render without panic.
        let logs: Vec<String> = (0..250).map(|i| format!("log line {i}")).collect();
        let o = ErrorOverlay {
            title: "Start failed",
            message: "container exited",
            log_tail: &logs,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
        o.render(Rect::new(0, 0, 120, 30), &mut buf);
    }
    #[test]
    fn test_error_overlay_no_panic_small_area() {
        let logs = vec!["one log line".to_string()];
        let o = ErrorOverlay {
            title: "Start failed",
            message: "boom",
            log_tail: &logs,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 8));
        o.render(Rect::new(0, 0, 20, 8), &mut buf);
    }
    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500B");
        assert_eq!(format_bytes(1_500), "2KB");
        assert_eq!(format_bytes(1_500_000), "2MB");
        assert_eq!(format_bytes(1_500_000_000), "1.5GB");
    }
}
