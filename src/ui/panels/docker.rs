use crate::data::docker::ContainerInfo;
use crate::ui::common::format_bytes;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, Table, Widget},
};

pub struct DockerPanel<'a> {
    pub containers: &'a [ContainerInfo],
    pub selected: usize,
    pub filter_text: &'a str,
    pub is_focused: bool,
    pub is_available: bool,
}

impl<'a> Widget for DockerPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = if self.filter_text.is_empty() {
            " Docker ".to_string()
        } else {
            format!(" Docker [filter: {}] ", self.filter_text)
        };
        let border_style = if self.is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);
        if !self.is_available {
            let inner = block.inner(area);
            Widget::render(block, area, buf);
            if inner.width > 18 && inner.height > 0 {
                buf.set_string(
                    inner.x,
                    inner.y,
                    "Docker not detected",
                    Style::default().fg(Color::DarkGray),
                );
            }
            return;
        }
        let header = Row::new(vec!["NAME", "IMAGE", "STATE", "CPU%", "MEM", "PORTS"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let rows: Vec<Row> = self
            .containers
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let style = if i == self.selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };
                let ports_str = c
                    .ports
                    .iter()
                    .map(|p| format!("{}:{}", p.host, p.container))
                    .collect::<Vec<_>>()
                    .join(", ");
                Row::new(vec![
                    c.name.clone(),
                    c.image.clone(),
                    c.state.as_str(),
                    format!("{:.1}", c.cpu_percent),
                    format_bytes(c.memory_bytes),
                    ports_str,
                ])
                .style(style)
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Min(12),
                Constraint::Min(10),
                Constraint::Length(10),
                Constraint::Length(6),
                Constraint::Length(8),
                Constraint::Min(10),
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
    use crate::data::docker::{ContainerState, PortMapping};
    #[test]
    fn test_render_no_panic() {
        let c = vec![ContainerInfo {
            id: "abc".into(),
            name: "app-web".into(),
            image: "node:18".into(),
            state: ContainerState::Running,
            cpu_percent: 12.0,
            memory_bytes: 340_000_000,
            memory_limit: 1_000_000_000,
            ports: vec![PortMapping {
                host: 3000,
                container: 3000,
                protocol: "tcp".into(),
            }],
            compose_project: Some("myapp".into()),
            created: "2026-04-03".into(),
        }];
        let p = DockerPanel {
            containers: &c,
            selected: 0,
            filter_text: "",
            is_focused: true,
            is_available: true,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
        p.render(Rect::new(0, 0, 60, 10), &mut buf);
    }
    #[test]
    fn test_unavailable() {
        let p = DockerPanel {
            containers: &[],
            selected: 0,
            filter_text: "",
            is_focused: false,
            is_available: false,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
        p.render(Rect::new(0, 0, 60, 10), &mut buf);
    }
}
