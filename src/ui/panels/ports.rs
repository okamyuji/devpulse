use crate::app::{PortSortColumn, SortDirection};
use crate::data::ports::{PortEntry, Protocol};
use crate::ui::common::{format_bytes, render_panel_scrollbar};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, StatefulWidget, Table, TableState, Widget},
};

pub struct PortsPanel<'a> {
    pub entries: &'a [PortEntry],
    pub selected: usize,
    pub filter_text: &'a str,
    pub is_focused: bool,
    pub sort_column: PortSortColumn,
    pub sort_direction: SortDirection,
}

impl<'a> Widget for PortsPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let selected = self.selected;
        let mut state = TableState::default().with_selected(Some(selected));
        StatefulWidget::render(self, area, buf, &mut state);
    }
}

impl<'a> StatefulWidget for PortsPanel<'a> {
    type State = TableState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut TableState) {
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
        let arrow = match self.sort_direction {
            SortDirection::Asc => "▲",
            SortDirection::Desc => "▼",
        };
        let sort_info = format!(
            " {} items  Sort: {}{} (,/.:col S:dir) ",
            self.entries.len(),
            self.sort_column.label(),
            arrow
        );
        let block = Block::default()
            .title(title)
            .title_bottom(sort_info)
            .borders(Borders::ALL)
            .border_style(border_style);
        let header_cols: Vec<String> = ["PORT", "PROTO", "PROCESS", "PID", "CPU%", "MEM"]
            .iter()
            .map(|&col| {
                let sort_col = match col {
                    "PORT" => Some(PortSortColumn::Port),
                    "PROCESS" => Some(PortSortColumn::Process),
                    "CPU%" => Some(PortSortColumn::Cpu),
                    "MEM" => Some(PortSortColumn::Memory),
                    _ => None,
                };
                if sort_col == Some(self.sort_column) {
                    format!("{}{}", col, arrow)
                } else {
                    col.to_string()
                }
            })
            .collect();
        let header = Row::new(header_cols).style(Style::default().add_modifier(Modifier::BOLD));
        let rows: Vec<Row> = self
            .entries
            .iter()
            .map(|e| {
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
        .block(block)
        .row_highlight_style(Style::default().bg(Color::DarkGray));
        StatefulWidget::render(table, area, buf, state);

        let visible = (area.height as usize).saturating_sub(3);
        render_panel_scrollbar(buf, area, self.entries.len(), visible, state.offset());
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
            sort_column: PortSortColumn::Port,
            sort_direction: SortDirection::Asc,
        };
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        Widget::render(p, area, &mut buf);
    }
    #[test]
    fn test_render_with_filter() {
        let e = sample();
        let p = PortsPanel {
            entries: &e,
            selected: 0,
            filter_text: "node",
            is_focused: true,
            sort_column: PortSortColumn::Cpu,
            sort_direction: SortDirection::Desc,
        };
        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        Widget::render(p, area, &mut buf);
    }

    #[test]
    fn test_stateful_render_scrolls_to_keep_selected_visible() {
        let entries: Vec<PortEntry> = (0..40)
            .map(|i| PortEntry {
                port: 3000 + i as u16,
                protocol: Protocol::Tcp,
                address: "127.0.0.1".into(),
                pid: 1000 + i as u32,
                process_name: format!("svc-{:02}", i),
                command: "x".into(),
                cpu_percent: 0.0,
                memory_bytes: 0,
            })
            .collect();
        let p = PortsPanel {
            entries: &entries,
            selected: 35,
            filter_text: "",
            is_focused: true,
            sort_column: PortSortColumn::Port,
            sort_direction: SortDirection::Asc,
        };
        let area = Rect::new(0, 0, 80, 8);
        let mut buf = Buffer::empty(area);
        let mut state = TableState::default().with_selected(Some(35)).with_offset(0);
        StatefulWidget::render(p, area, &mut buf, &mut state);
        assert!(
            state.offset() > 0,
            "expected scroll offset to advance, got {}",
            state.offset()
        );
        let rendered = buf_to_string(&buf);
        assert!(
            rendered.contains("svc-35"),
            "expected selected row 'svc-35' to be visible, got:\n{}",
            rendered
        );
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
}
