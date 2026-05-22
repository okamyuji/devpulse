use crate::data::docker::ContainerInfo;
use crate::ui::common::{format_bytes, render_panel_scrollbar};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Row, StatefulWidget, Table, TableState, Widget},
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
        let selected = self.selected;
        let mut state = TableState::default().with_selected(Some(selected));
        StatefulWidget::render(self, area, buf, &mut state);
    }
}

impl<'a> StatefulWidget for DockerPanel<'a> {
    type State = TableState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut TableState) {
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
            .map(|c| {
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
        .block(block)
        .row_highlight_style(Style::default().bg(Color::DarkGray));
        StatefulWidget::render(table, area, buf, state);

        // Scrollbar: borders (2) + header (1) consumed by the block, the rest
        // is data rows. Hide when everything fits.
        let visible = (area.height as usize).saturating_sub(3);
        render_panel_scrollbar(buf, area, self.containers.len(), visible, state.offset());
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
        Widget::render(p, Rect::new(0, 0, 60, 10), &mut buf);
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
        Widget::render(p, Rect::new(0, 0, 100, 20), &mut buf);

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
    fn test_stateful_render_scrolls_to_keep_selected_visible() {
        // 30 containers in a 10-row-high area: row 25 is well outside the
        // initial view. After rendering, TableState::offset must move so the
        // selected row is included in the rendered range.
        let containers: Vec<ContainerInfo> = (0..30)
            .map(|i| ContainerInfo {
                id: format!("id-{:02}", i),
                name: format!("c-{:02}", i),
                image: "img".into(),
                state: ContainerState::Running,
                cpu_percent: 0.0,
                memory_bytes: 0,
                memory_limit: 0,
                ports: vec![],
                compose_project: None,
                created: "2026-01-01".into(),
            })
            .collect();
        let summary: Vec<String> = Vec::new();
        let p = DockerPanel {
            containers: &containers,
            selected: 25,
            filter_text: "",
            is_focused: true,
            is_available: true,
            context_name: None,
            resolution_summary: &summary,
        };
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        let mut state = TableState::default().with_selected(Some(25)).with_offset(0);
        StatefulWidget::render(p, area, &mut buf, &mut state);
        assert!(
            state.offset() > 0,
            "expected scroll offset to advance, got {}",
            state.offset()
        );
        let rendered = buf_to_string(&buf);
        assert!(
            rendered.contains("c-25"),
            "expected selected row 'c-25' to be visible in rendered buffer, got:\n{}",
            rendered
        );
    }

    #[test]
    fn test_stateful_render_keeps_offset_when_selection_in_view() {
        // If selected row is inside the initial view, offset should stay at 0.
        let containers: Vec<ContainerInfo> = (0..30)
            .map(|i| ContainerInfo {
                id: format!("id-{:02}", i),
                name: format!("c-{:02}", i),
                image: "img".into(),
                state: ContainerState::Running,
                cpu_percent: 0.0,
                memory_bytes: 0,
                memory_limit: 0,
                ports: vec![],
                compose_project: None,
                created: "2026-01-01".into(),
            })
            .collect();
        let summary: Vec<String> = Vec::new();
        let p = DockerPanel {
            containers: &containers,
            selected: 1,
            filter_text: "",
            is_focused: true,
            is_available: true,
            context_name: None,
            resolution_summary: &summary,
        };
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        let mut state = TableState::default().with_selected(Some(1)).with_offset(0);
        StatefulWidget::render(p, area, &mut buf, &mut state);
        assert_eq!(state.offset(), 0);
    }

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

    fn right_edge_column(buf: &Buffer) -> String {
        let area = buf.area;
        let x = area.x + area.width - 1;
        let mut col = String::new();
        for y in area.y..area.y + area.height {
            col.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        col
    }

    #[test]
    fn test_scrollbar_visible_when_overflow() {
        let containers: Vec<ContainerInfo> = (0..30)
            .map(|i| ContainerInfo {
                id: format!("id-{:02}", i),
                name: format!("c-{:02}", i),
                image: "img".into(),
                state: ContainerState::Running,
                cpu_percent: 0.0,
                memory_bytes: 0,
                memory_limit: 0,
                ports: vec![],
                compose_project: None,
                created: "2026-01-01".into(),
            })
            .collect();
        let summary: Vec<String> = Vec::new();
        let p = DockerPanel {
            containers: &containers,
            selected: 0,
            filter_text: "",
            is_focused: true,
            is_available: true,
            context_name: None,
            resolution_summary: &summary,
        };
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        let mut state = TableState::default().with_selected(Some(0)).with_offset(0);
        StatefulWidget::render(p, area, &mut buf, &mut state);
        let col = right_edge_column(&buf);
        assert!(
            col.contains('█'),
            "expected scrollbar thumb '█' on right edge, got column: {:?}",
            col
        );
    }

    #[test]
    fn test_scrollbar_thumb_reaches_bottom_at_last_row() {
        // After selecting the last row, the table offset is total-visible
        // (= scrollable max). The scrollbar thumb must sit on the very last
        // cell of the bar — i.e. one row above the bottom border.
        let containers: Vec<ContainerInfo> = (0..30)
            .map(|i| ContainerInfo {
                id: format!("id-{:02}", i),
                name: format!("c-{:02}", i),
                image: "img".into(),
                state: ContainerState::Running,
                cpu_percent: 0.0,
                memory_bytes: 0,
                memory_limit: 0,
                ports: vec![],
                compose_project: None,
                created: "2026-01-01".into(),
            })
            .collect();
        let summary: Vec<String> = Vec::new();
        let p = DockerPanel {
            containers: &containers,
            selected: 29,
            filter_text: "",
            is_focused: true,
            is_available: true,
            context_name: None,
            resolution_summary: &summary,
        };
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        let mut state = TableState::default().with_selected(Some(29)).with_offset(0);
        StatefulWidget::render(p, area, &mut buf, &mut state);
        // scrollbar_area lives inside the borders: y = area.y+1 .. area.y+area.height-1.
        // Its last cell is area.y + area.height - 2.
        let last_y = area.y + area.height - 2;
        let last_x = area.x + area.width - 1;
        let cell = buf[(last_x, last_y)].symbol().to_string();
        assert_eq!(
            cell, "█",
            "expected scrollbar thumb on the very last bar cell (y={}), got {:?}",
            last_y, cell
        );
    }

    #[test]
    fn test_scrollbar_hidden_when_fits() {
        let containers: Vec<ContainerInfo> = (0..3)
            .map(|i| ContainerInfo {
                id: format!("id-{:02}", i),
                name: format!("c-{:02}", i),
                image: "img".into(),
                state: ContainerState::Running,
                cpu_percent: 0.0,
                memory_bytes: 0,
                memory_limit: 0,
                ports: vec![],
                compose_project: None,
                created: "2026-01-01".into(),
            })
            .collect();
        let summary: Vec<String> = Vec::new();
        let p = DockerPanel {
            containers: &containers,
            selected: 0,
            filter_text: "",
            is_focused: true,
            is_available: true,
            context_name: None,
            resolution_summary: &summary,
        };
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        let mut state = TableState::default().with_selected(Some(0)).with_offset(0);
        StatefulWidget::render(p, area, &mut buf, &mut state);
        let col = right_edge_column(&buf);
        assert!(
            !col.contains('█'),
            "expected no scrollbar thumb when content fits, got column: {:?}",
            col
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
        Widget::render(p, Rect::new(0, 0, 60, 10), &mut buf);
    }
}
