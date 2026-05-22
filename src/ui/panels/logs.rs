use crate::data::logs::{LogEntry, LogLevel};
use crate::ui::common::render_panel_scrollbar;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

pub struct LogsPanel<'a> {
    /// Pre-filtered log entries (filtering is done in the draw layer).
    pub entries: &'a [&'a LogEntry],
    pub selected: usize,
    pub filter_text: &'a str,
    /// Docker panel selected container name (display only, for the title).
    pub container_label: Option<&'a str>,
    pub is_focused: bool,
    pub tail_follow: bool,
    pub wrap: bool,
    /// Manual scroll offset (used when `tail_follow` is false).
    pub scroll_offset: usize,
}

impl<'a> Widget for LogsPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let follow_indicator = if self.tail_follow { " FOLLOW" } else { "" };

        let mut title_parts = vec![" Logs".to_string()];
        if let Some(container) = self.container_label {
            title_parts.push(format!(" [{}]", container));
        }
        if !self.filter_text.is_empty() {
            title_parts.push(format!(" [filter: {}]", self.filter_text));
        }
        title_parts.push(format!("{} ", follow_indicator));
        let title = title_parts.join("");

        let border_style = if self.is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let count_text = format!(" {} lines ", self.entries.len());
        let block = Block::default()
            .title(title)
            .title_bottom(count_text)
            .borders(Borders::ALL)
            .border_style(border_style);

        let lines: Vec<Line> = self
            .entries
            .iter()
            .map(|entry| {
                let color = match entry.level {
                    LogLevel::Error => Color::Red,
                    LogLevel::Warn => Color::Yellow,
                    LogLevel::Info => Color::Green,
                };
                Line::from(vec![
                    Span::styled(format!("[{}] ", entry.source), Style::default().fg(color)),
                    Span::raw(&entry.message),
                ])
            })
            .collect();

        let inner_height = (area.height as usize).saturating_sub(2);
        let total_lines = lines.len();
        let max_scroll = total_lines.saturating_sub(inner_height);
        let scroll = if self.tail_follow {
            max_scroll
        } else {
            self.scroll_offset.min(max_scroll)
        };
        let mut paragraph = Paragraph::new(lines).block(block);
        if self.wrap {
            paragraph = paragraph.wrap(Wrap { trim: false });
        }
        if total_lines > 0 && scroll > 0 {
            paragraph = paragraph.scroll((scroll as u16, 0));
        }
        Widget::render(paragraph, area, buf);

        render_panel_scrollbar(buf, area, total_lines, inner_height, scroll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::logs::LogLevel;

    fn sample_entries() -> Vec<LogEntry> {
        vec![
            LogEntry {
                timestamp: 1,
                source: "app-web".into(),
                level: LogLevel::Info,
                message: "request started".into(),
            },
            LogEntry {
                timestamp: 2,
                source: "app-db".into(),
                level: LogLevel::Error,
                message: "connection timeout".into(),
            },
            LogEntry {
                timestamp: 3,
                source: "app-web".into(),
                level: LogLevel::Warn,
                message: "slow query detected".into(),
            },
        ]
    }

    fn make_panel<'a>(entries: &'a [&'a LogEntry]) -> LogsPanel<'a> {
        LogsPanel {
            entries,
            selected: 0,
            filter_text: "",
            container_label: None,
            is_focused: true,
            tail_follow: false,
            wrap: false,
            scroll_offset: 0,
        }
    }

    #[test]
    fn test_render_no_panic() {
        let entries = sample_entries();
        let refs: Vec<&LogEntry> = entries.iter().collect();
        let mut p = make_panel(&refs);
        p.tail_follow = true;
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
        p.render(Rect::new(0, 0, 60, 10), &mut buf);
    }

    #[test]
    fn test_empty_entries() {
        let refs: Vec<&LogEntry> = vec![];
        let p = make_panel(&refs);
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
        p.render(Rect::new(0, 0, 60, 10), &mut buf);
    }

    #[test]
    fn test_displays_correct_line_count() {
        let entries = sample_entries();
        let refs: Vec<&LogEntry> = entries.iter().collect();
        let p = make_panel(&refs);
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
        let rendered = buf_to_string(&buf);
        assert!(
            rendered.contains("3 lines"),
            "Expected '3 lines', got: {}",
            rendered
        );
    }

    #[test]
    fn test_pre_filtered_two_entries_shows_2_lines() {
        let entries = sample_entries();
        // Simulate container filter — only app-web entries
        let refs: Vec<&LogEntry> = entries.iter().filter(|e| e.source == "app-web").collect();
        let mut p = make_panel(&refs);
        p.container_label = Some("app-web");
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
        let rendered = buf_to_string(&buf);
        assert!(
            rendered.contains("2 lines"),
            "Expected '2 lines', got: {}",
            rendered
        );
    }

    #[test]
    fn test_scrollbar_visible_when_log_overflow() {
        let entries: Vec<LogEntry> = (0..50)
            .map(|i| LogEntry {
                timestamp: i,
                source: "svc".into(),
                level: LogLevel::Info,
                message: format!("line {}", i),
            })
            .collect();
        let refs: Vec<&LogEntry> = entries.iter().collect();
        let mut p = make_panel(&refs);
        p.tail_follow = true;
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
        let x = area.x + area.width - 1;
        let mut col = String::new();
        for y in area.y..area.y + area.height {
            col.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        assert!(
            col.contains('█'),
            "expected scrollbar thumb on right edge for overflowing logs, got: {:?}",
            col
        );
    }

    #[test]
    fn test_manual_scroll_offset_shifts_visible_logs() {
        let entries: Vec<LogEntry> = (0..40)
            .map(|i| LogEntry {
                timestamp: i,
                source: "svc".into(),
                level: LogLevel::Info,
                message: format!("line-{:02}", i),
            })
            .collect();
        let refs: Vec<&LogEntry> = entries.iter().collect();
        let mut p = make_panel(&refs);
        p.scroll_offset = 10;
        let area = Rect::new(0, 0, 60, 8);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
        let rendered = buf_to_string(&buf);
        assert!(
            rendered.contains("line-10"),
            "expected line-10 visible after scrolling 10 down, got:\n{}",
            rendered
        );
        assert!(
            !rendered.contains("line-00"),
            "expected line-00 to be scrolled off-screen, got:\n{}",
            rendered
        );
    }

    #[test]
    fn test_scrollbar_thumb_reaches_bottom_at_last_log() {
        let entries: Vec<LogEntry> = (0..40)
            .map(|i| LogEntry {
                timestamp: i,
                source: "svc".into(),
                level: LogLevel::Info,
                message: format!("line-{:02}", i),
            })
            .collect();
        let refs: Vec<&LogEntry> = entries.iter().collect();
        let mut p = make_panel(&refs);
        p.tail_follow = true;
        let area = Rect::new(0, 0, 60, 8);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
        let last_y = area.y + area.height - 2;
        let last_x = area.x + area.width - 1;
        let cell = buf[(last_x, last_y)].symbol().to_string();
        assert_eq!(
            cell, "█",
            "expected scrollbar thumb on the last bar cell when following the tail, got {:?}",
            cell
        );
    }

    #[test]
    fn test_scrollbar_hidden_when_logs_fit() {
        let entries = sample_entries();
        let refs: Vec<&LogEntry> = entries.iter().collect();
        let p = make_panel(&refs);
        let area = Rect::new(0, 0, 60, 20);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
        let x = area.x + area.width - 1;
        let mut col = String::new();
        for y in area.y..area.y + area.height {
            col.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        assert!(
            !col.contains('█'),
            "expected no scrollbar thumb when logs fit, got: {:?}",
            col
        );
    }

    /// Helper to convert Buffer to string for assertion
    fn buf_to_string(buf: &Buffer) -> String {
        let area = buf.area;
        let mut result = String::new();
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                result.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            result.push('\n');
        }
        result
    }
}
