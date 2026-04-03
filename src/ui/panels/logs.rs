use ratatui::{buffer::Buffer, layout::Rect, style::{Color, Style}, text::{Line, Span}, widgets::{Block, Borders, Paragraph, Widget, Wrap}};
use crate::data::logs::{LogBuffer, LogLevel};

pub struct LogsPanel<'a> {
    pub buffer: &'a LogBuffer,
    pub selected: usize,
    pub filter_text: &'a str,
    pub is_focused: bool,
    pub tail_follow: bool,
    pub wrap: bool,
}

impl<'a> Widget for LogsPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let follow_indicator = if self.tail_follow { " FOLLOW" } else { "" };
        let title = if self.filter_text.is_empty() { format!(" Logs{} ", follow_indicator) }
        else { format!(" Logs [filter: {}]{} ", self.filter_text, follow_indicator) };
        let border_style = if self.is_focused { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::DarkGray) };
        let block = Block::default().title(title).borders(Borders::ALL).border_style(border_style);
        let lines: Vec<Line> = self.buffer.entries().iter().map(|entry| {
            let color = match entry.level { LogLevel::Error => Color::Red, LogLevel::Warn => Color::Yellow, LogLevel::Info => Color::Green };
            Line::from(vec![
                Span::styled(format!("[{}] ", entry.source), Style::default().fg(color)),
                Span::raw(&entry.message),
            ])
        }).collect();
        let mut paragraph = Paragraph::new(lines).block(block);
        if self.wrap { paragraph = paragraph.wrap(Wrap { trim: false }); }
        if self.tail_follow && self.buffer.len() > 0 {
            let inner_height = area.height.saturating_sub(2) as usize;
            let scroll = self.buffer.len().saturating_sub(inner_height);
            paragraph = paragraph.scroll((scroll as u16, 0));
        }
        Widget::render(paragraph, area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::logs::{LogEntry, LogLevel};
    #[test] fn test_render_no_panic() {
        let mut buffer = LogBuffer::new(100);
        buffer.push(LogEntry { timestamp: 1, source: "app".into(), level: LogLevel::Info, message: "started".into() });
        buffer.push(LogEntry { timestamp: 2, source: "db".into(), level: LogLevel::Error, message: "timeout".into() });
        let p = LogsPanel { buffer: &buffer, selected: 0, filter_text: "", is_focused: true, tail_follow: true, wrap: false };
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
        p.render(Rect::new(0, 0, 60, 10), &mut buf);
    }
    #[test] fn test_empty_buffer() {
        let buffer = LogBuffer::new(100);
        let p = LogsPanel { buffer: &buffer, selected: 0, filter_text: "", is_focused: false, tail_follow: false, wrap: false };
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
        p.render(Rect::new(0, 0, 60, 10), &mut buf);
    }
}
