use crate::data::docker::ContainerInfo;
use crate::ui::common::format_bytes;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Row, Table, Widget},
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
        let header = Row::new(vec!["S", "NAME", "IMAGE", "STATE", "CPU%", "MEM", "PORTS"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let rows: Vec<Row> = self
            .containers
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let row_style = if i == self.selected {
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
                let indicator = Cell::from(Line::from(Span::styled(
                    c.state.indicator_glyph().to_string(),
                    Style::default()
                        .fg(c.state.indicator_color())
                        .add_modifier(Modifier::BOLD),
                )));
                Row::new(vec![
                    indicator,
                    Cell::from(c.name.clone()),
                    Cell::from(c.image.clone()),
                    Cell::from(c.state.as_str()),
                    Cell::from(format!("{:.1}", c.cpu_percent)),
                    Cell::from(format_bytes(c.memory_bytes)),
                    Cell::from(ports_str),
                ])
                .style(row_style)
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(2),
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
    fn test_render_all_states_no_panic() {
        fn make(name: &str, state: ContainerState) -> ContainerInfo {
            ContainerInfo {
                id: format!("id-{}", name),
                name: name.into(),
                image: "img".into(),
                state,
                cpu_percent: 0.0,
                memory_bytes: 0,
                memory_limit: 0,
                ports: vec![],
                compose_project: None,
                created: "2026-04-03".into(),
            }
        }
        let containers = vec![
            make("run", ContainerState::Running),
            make("stp", ContainerState::Stopped),
            make("ok", ContainerState::Exited(0)),
            make("ng", ContainerState::Exited(137)),
            make("new", ContainerState::Created),
        ];
        let summary: Vec<String> = Vec::new();
        let p = DockerPanel {
            containers: &containers,
            selected: 0,
            filter_text: "",
            is_focused: true,
            is_available: true,
            context_name: Some("colima"),
            resolution_summary: &summary,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 20));
        p.render(Rect::new(0, 0, 100, 20), &mut buf);

        // Block border sits at x=0/y=0, so the inner area starts at (1, 1).
        // Row layout inside the inner area: y=1 is header, y=2 is the first
        // data row. The indicator column is the first column, so the glyph
        // lives at (1, 2). Both ▶ (Running) and ■ (non-Running) are valid
        // here since rows can be in any order; just confirm a glyph cell is
        // present in the indicator column.
        let first_row_glyph = buf.cell((1, 2)).map(|c| c.symbol().to_string());
        assert!(
            first_row_glyph == Some("▶".to_string()) || first_row_glyph == Some("■".to_string()),
            "expected indicator glyph in indicator column at (1,2), got {:?}",
            first_row_glyph
        );
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
