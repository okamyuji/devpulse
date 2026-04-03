use crate::data::processes::ProcessInfo;
use crate::ui::common::format_bytes;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, Table, Widget},
};

pub struct ProcessesPanel<'a> {
    pub processes: &'a [ProcessInfo],
    pub selected: usize,
    pub filter_text: &'a str,
    pub is_focused: bool,
    pub tree_mode: bool,
}

impl<'a> Widget for ProcessesPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = if self.filter_text.is_empty() {
            " Processes ".to_string()
        } else {
            format!(" Processes [filter: {}] ", self.filter_text)
        };
        let border_style = if self.is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let count_text = format!(" {} items ", self.processes.len());
        let block = Block::default()
            .title(title)
            .title_bottom(count_text)
            .borders(Borders::ALL)
            .border_style(border_style);
        let header = Row::new(vec!["PID", "NAME", "CPU%", "MEM", "PORTS", "CMD"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let rows: Vec<Row> = self
            .processes
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let style = if i == self.selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };
                let ports = p
                    .listening_ports
                    .iter()
                    .map(|p| format!(":{}", p))
                    .collect::<Vec<_>>()
                    .join(",");
                Row::new(vec![
                    p.pid.to_string(),
                    p.name.clone(),
                    format!("{:.1}", p.cpu_percent),
                    format_bytes(p.memory_bytes),
                    ports,
                    p.command.chars().take(30).collect::<String>(),
                ])
                .style(style)
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(7),
                Constraint::Min(10),
                Constraint::Length(6),
                Constraint::Length(8),
                Constraint::Length(12),
                Constraint::Min(15),
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
    #[test]
    fn test_render_no_panic() {
        let procs = vec![ProcessInfo {
            pid: 1234,
            name: "node".into(),
            command: "node server.js".into(),
            user: "yuji".into(),
            cpu_percent: 12.5,
            memory_bytes: 340_000_000,
            threads: 8,
            parent_pid: Some(1),
            listening_ports: vec![3000],
            start_time: 0,
        }];
        let p = ProcessesPanel {
            processes: &procs,
            selected: 0,
            filter_text: "",
            is_focused: true,
            tree_mode: false,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 10));
        p.render(Rect::new(0, 0, 80, 10), &mut buf);
    }
}
