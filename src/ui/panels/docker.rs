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
    pub context_name: Option<&'a str>,
    pub resolution_summary: &'a [String],
}

impl<'a> Widget for DockerPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let base_title = match self.context_name {
            Some(ctx) => format!(" Docker [{}] ", ctx),
            None => " Docker ".to_string(),
        };
        let title = if self.filter_text.is_empty() {
            base_title
        } else {
            format!("{}[filter: {}] ", base_title.trim_end(), self.filter_text)
        };
        let border_style = if self.is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let count_text = format!(" {} items ", self.containers.len());
        let block = Block::default()
            .title(title)
            .title_bottom(count_text)
            .borders(Borders::ALL)
            .border_style(border_style);
        if !self.is_available {
            let inner = block.inner(area);
            Widget::render(block, area, buf);
            if inner.height == 0 {
                return;
            }
            let header = "No Docker daemon found. Tried:";
            let short = "Docker not found";
            let header_text = if inner.width as usize >= header.len() {
                header
            } else {
                short
            };
            buf.set_stringn(
                inner.x,
                inner.y,
                header_text,
                inner.width as usize,
                Style::default().fg(Color::Red),
            );
            let max_rows = inner.height.saturating_sub(1) as usize;
            for (i, line) in self.resolution_summary.iter().take(max_rows).enumerate() {
                let row_y = inner.y + 1 + i as u16;
                buf.set_stringn(
                    inner.x,
                    row_y,
                    line,
                    inner.width as usize,
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
        let summary: Vec<String> = Vec::new();
        let p = DockerPanel {
            containers: &c,
            selected: 0,
            filter_text: "",
            is_focused: true,
            is_available: true,
            context_name: Some("colima"),
            resolution_summary: &summary,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
        p.render(Rect::new(0, 0, 60, 10), &mut buf);
    }
    #[test]
    fn test_unavailable() {
        let summary = vec![
            "DOCKER_HOST: unix:///tmp/x".to_string(),
            "default: unix:///var/run/docker.sock".to_string(),
        ];
        let p = DockerPanel {
            containers: &[],
            selected: 0,
            filter_text: "",
            is_focused: false,
            is_available: false,
            context_name: None,
            resolution_summary: &summary,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
        p.render(Rect::new(0, 0, 60, 10), &mut buf);
    }
}
