use crate::data::logs::{LogBuffer, LogLevel};
use crate::filter::FilterState;
use crate::ui::common::render_panel_scrollbar;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

pub struct LogsPanel<'a> {
    pub buffer: &'a LogBuffer,
    pub selected: usize,
    pub filter_text: &'a str,
    /// Docker panel selected container name filter (None = show all)
    pub container_filter: Option<&'a str>,
    /// Log-panel-local AND filter
    pub log_filter: &'a FilterState,
    pub is_focused: bool,
    pub tail_follow: bool,
    pub wrap: bool,
    /// Manual scroll offset (used when `tail_follow` is false).
    pub scroll_offset: usize,
}

impl<'a> Widget for LogsPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let follow_indicator = if self.tail_follow { " FOLLOW" } else { "" };

        // Build title with container and filter info
        let mut title_parts = vec![" Logs".to_string()];
        if let Some(container) = self.container_filter {
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

        // Filter entries by container name AND log filter terms
        let filtered_entries: Vec<_> = self
            .buffer
            .entries()
            .iter()
            .filter(|entry| {
                // Container filter: match source name
                if let Some(container) = self.container_filter {
                    if entry.source != container {
                        return false;
                    }
                }
                // Log local filter: AND condition on source+message
                if self.log_filter.is_active() {
                    let text = format!("[{}] {}", entry.source, entry.message);
                    if !self.log_filter.matches_all_terms(&text) {
                        return false;
                    }
                }
                true
            })
            .collect();

        let count_text = format!(" {} lines ", filtered_entries.len());
        let block = Block::default()
            .title(title)
            .title_bottom(count_text)
            .borders(Borders::ALL)
            .border_style(border_style);

        let lines: Vec<Line> = filtered_entries
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
        let max_scroll = lines.len().saturating_sub(inner_height);
        let scroll = if self.tail_follow {
            max_scroll
        } else {
            self.scroll_offset.min(max_scroll)
        };
        let mut paragraph = Paragraph::new(lines.clone()).block(block);
        if self.wrap {
            paragraph = paragraph.wrap(Wrap { trim: false });
        }
        if !lines.is_empty() && scroll > 0 {
            paragraph = paragraph.scroll((scroll as u16, 0));
        }
        Widget::render(paragraph, area, buf);

        render_panel_scrollbar(buf, area, lines.len(), inner_height, scroll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::logs::{LogEntry, LogLevel};

    fn make_log_buffer() -> LogBuffer {
        let mut buffer = LogBuffer::new(100);
        buffer.push(LogEntry {
            timestamp: 1,
            source: "app-web".into(),
            level: LogLevel::Info,
            message: "request started".into(),
        });
        buffer.push(LogEntry {
            timestamp: 2,
            source: "app-db".into(),
            level: LogLevel::Error,
            message: "connection timeout".into(),
        });
        buffer.push(LogEntry {
            timestamp: 3,
            source: "app-web".into(),
            level: LogLevel::Warn,
            message: "slow query detected".into(),
        });
        buffer
    }

    #[test]
    fn test_render_no_panic() {
        let buffer = make_log_buffer();
        let filter = FilterState::new();
        let p = LogsPanel {
            buffer: &buffer,
            selected: 0,
            filter_text: "",
            container_filter: None,
            log_filter: &filter,
            is_focused: true,
            tail_follow: true,
            wrap: false,
            scroll_offset: 0,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
        p.render(Rect::new(0, 0, 60, 10), &mut buf);
    }

    #[test]
    fn test_empty_buffer() {
        let buffer = LogBuffer::new(100);
        let filter = FilterState::new();
        let p = LogsPanel {
            buffer: &buffer,
            selected: 0,
            filter_text: "",
            container_filter: None,
            log_filter: &filter,
            is_focused: false,
            tail_follow: false,
            wrap: false,
            scroll_offset: 0,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
        p.render(Rect::new(0, 0, 60, 10), &mut buf);
    }

    #[test]
    fn test_container_filter_shows_only_matching_source() {
        let buffer = make_log_buffer();
        let filter = FilterState::new();
        let p = LogsPanel {
            buffer: &buffer,
            selected: 0,
            filter_text: "",
            container_filter: Some("app-web"),
            log_filter: &filter,
            is_focused: true,
            tail_follow: false,
            wrap: false,
            scroll_offset: 0,
        };
        // Render and check line count in title_bottom
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
        // The title_bottom should show "2 lines" (only app-web entries)
        let rendered = buf_to_string(&buf);
        assert!(
            rendered.contains("2 lines"),
            "Expected '2 lines' for container filter 'app-web', got: {}",
            rendered
        );
    }

    #[test]
    fn test_log_filter_and_condition() {
        let buffer = make_log_buffer();
        let mut filter = FilterState::new();
        filter.set_query("app-web slow");
        let p = LogsPanel {
            buffer: &buffer,
            selected: 0,
            filter_text: "app-web slow",
            container_filter: None,
            log_filter: &filter,
            is_focused: true,
            tail_follow: false,
            wrap: false,
            scroll_offset: 0,
        };
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
        let rendered = buf_to_string(&buf);
        // Only entry 3 matches: source=app-web, message contains "slow"
        assert!(
            rendered.contains("1 lines"),
            "Expected '1 lines' for AND filter 'app-web slow', got: {}",
            rendered
        );
    }

    #[test]
    fn test_container_filter_plus_log_filter() {
        let buffer = make_log_buffer();
        let mut filter = FilterState::new();
        filter.set_query("request");
        let p = LogsPanel {
            buffer: &buffer,
            selected: 0,
            filter_text: "request",
            container_filter: Some("app-web"),
            log_filter: &filter,
            is_focused: true,
            tail_follow: false,
            wrap: false,
            scroll_offset: 0,
        };
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
        let rendered = buf_to_string(&buf);
        // container=app-web (2 entries), then AND filter "request" → 1 entry
        assert!(
            rendered.contains("1 lines"),
            "Expected '1 lines', got: {}",
            rendered
        );
    }

    #[test]
    fn test_no_filter_shows_all() {
        let buffer = make_log_buffer();
        let filter = FilterState::new();
        let p = LogsPanel {
            buffer: &buffer,
            selected: 0,
            filter_text: "",
            container_filter: None,
            log_filter: &filter,
            is_focused: true,
            tail_follow: false,
            wrap: false,
            scroll_offset: 0,
        };
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
    fn test_scrollbar_visible_when_log_overflow() {
        let mut buffer = LogBuffer::new(200);
        for i in 0..50 {
            buffer.push(LogEntry {
                timestamp: i,
                source: "svc".into(),
                level: LogLevel::Info,
                message: format!("line {}", i),
            });
        }
        let filter = FilterState::new();
        let p = LogsPanel {
            buffer: &buffer,
            selected: 0,
            filter_text: "",
            container_filter: None,
            log_filter: &filter,
            is_focused: true,
            tail_follow: true,
            wrap: false,
            scroll_offset: 0,
        };
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
        // tail_follow OFF + scroll_offset > 0 should make the paragraph
        // skip leading lines so the visible window moves down.
        let mut buffer = LogBuffer::new(200);
        for i in 0..40 {
            buffer.push(LogEntry {
                timestamp: i,
                source: "svc".into(),
                level: LogLevel::Info,
                message: format!("line-{:02}", i),
            });
        }
        let filter = FilterState::new();
        let p = LogsPanel {
            buffer: &buffer,
            selected: 0,
            filter_text: "",
            container_filter: None,
            log_filter: &filter,
            is_focused: true,
            tail_follow: false,
            wrap: false,
            scroll_offset: 10,
        };
        let area = Rect::new(0, 0, 60, 8);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
        let rendered = buf_to_string(&buf);
        // Lines 0..9 should be scrolled past, the visible area should show
        // line-10 onward.
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
        // With tail_follow ON the paragraph scrolls to the last line; the
        // scrollbar thumb must end at the bottom-most cell of the bar.
        let mut buffer = LogBuffer::new(200);
        for i in 0..40 {
            buffer.push(LogEntry {
                timestamp: i,
                source: "svc".into(),
                level: LogLevel::Info,
                message: format!("line-{:02}", i),
            });
        }
        let filter = FilterState::new();
        let p = LogsPanel {
            buffer: &buffer,
            selected: 0,
            filter_text: "",
            container_filter: None,
            log_filter: &filter,
            is_focused: true,
            tail_follow: true,
            wrap: false,
            scroll_offset: 0,
        };
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
        let buffer = make_log_buffer();
        let filter = FilterState::new();
        let p = LogsPanel {
            buffer: &buffer,
            selected: 0,
            filter_text: "",
            container_filter: None,
            log_filter: &filter,
            is_focused: true,
            tail_follow: false,
            wrap: false,
            scroll_offset: 0,
        };
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
