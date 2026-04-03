use crate::data::logs::{LogBuffer, LogLevel};
use crate::filter::FilterState;
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

        let mut paragraph = Paragraph::new(lines.clone()).block(block);
        if self.wrap {
            paragraph = paragraph.wrap(Wrap { trim: false });
        }
        if self.tail_follow && !lines.is_empty() {
            let inner_height = area.height.saturating_sub(2) as usize;
            let scroll = lines.len().saturating_sub(inner_height);
            paragraph = paragraph.scroll((scroll as u16, 0));
        }
        Widget::render(paragraph, area, buf);
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
