use crate::data::ports::{PortEntry, Protocol};
use crate::ui::common::format_bytes;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, Table, Widget},
};

pub struct PortsPanel<'a> {
    pub entries: &'a [PortEntry],
    pub selected: usize,
    pub filter_text: &'a str,
    pub is_focused: bool,
}

impl<'a> Widget for PortsPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = if self.filter_text.is_empty() {
            " Ports ".to_string()
        } else {
            format!(" Ports [filter: {}] ", self.filter_text)
        };
        let border_style = if self.is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let count_text = format!(" {} items ", self.entries.len());
        let block = Block::default()
            .title(title)
            .title_bottom(count_text)
            .borders(Borders::ALL)
            .border_style(border_style);
        let header = Row::new(vec!["PORT", "PROTO", "PROCESS", "PID", "CPU%", "MEM"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let rows: Vec<Row> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let style = if i == self.selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };
                Row::new(vec![
                    format!(":{}", e.port),
                    match e.protocol {
                        Protocol::Tcp => "TCP".into(),
                        Protocol::Udp => "UDP".into(),
                    },
                    e.process_name.clone(),
                    e.pid.to_string(),
                    format!("{:.1}", e.cpu_percent),
                    format_bytes(e.memory_bytes),
                ])
                .style(style)
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(7),
                Constraint::Length(5),
                Constraint::Min(10),
                Constraint::Length(7),
                Constraint::Length(6),
                Constraint::Length(8),
            ],
        )
        .header(header)
        .block(block);
        Widget::render(table, area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Vec<PortEntry> {
        vec![PortEntry {
            port: 3000,
            protocol: Protocol::Tcp,
            address: "127.0.0.1".into(),
            pid: 1234,
            process_name: "next-dev".into(),
            command: "node".into(),
            cpu_percent: 12.5,
            memory_bytes: 340_000_000,
        }]
    }
    #[test]
    fn test_render_no_panic() {
        let e = sample();
        let p = PortsPanel {
            entries: &e,
            selected: 0,
            filter_text: "",
            is_focused: true,
        };
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
    }
    #[test]
    fn test_render_with_filter() {
        let e = sample();
        let p = PortsPanel {
            entries: &e,
            selected: 0,
            filter_text: "node",
            is_focused: true,
        };
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
    }
}
