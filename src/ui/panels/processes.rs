use crate::app::{ProcessSortColumn, SortDirection};
use crate::data::processes::ProcessInfo;
use crate::ui::common::{format_bytes, render_panel_scrollbar};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, StatefulWidget, Table, TableState, Widget},
};

pub struct ProcessesPanel<'a> {
    pub processes: &'a [ProcessInfo],
    pub selected: usize,
    pub filter_text: &'a str,
    pub is_focused: bool,
    pub tree_mode: bool,
    pub sort_column: ProcessSortColumn,
    pub sort_direction: SortDirection,
}

impl<'a> Widget for ProcessesPanel<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let selected = self.selected;
        let mut state = TableState::default().with_selected(Some(selected));
        StatefulWidget::render(self, area, buf, &mut state);
    }
}

impl<'a> StatefulWidget for ProcessesPanel<'a> {
    type State = TableState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut TableState) {
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
        let arrow = match self.sort_direction {
            SortDirection::Asc => "▲",
            SortDirection::Desc => "▼",
        };
        let sort_info = format!(
            " {} items  Sort: {}{} (,/.:col S:dir) ",
            self.processes.len(),
            self.sort_column.label(),
            arrow
        );
        let count_text = sort_info;
        let block = Block::default()
            .title(title)
            .title_bottom(count_text)
            .borders(Borders::ALL)
            .border_style(border_style);
        let header_cols: Vec<String> = ["PID", "NAME", "CPU%", "MEM", "PORTS", "CMD"]
            .iter()
            .map(|&col| {
                let sort_col = match col {
                    "PID" => Some(ProcessSortColumn::Pid),
                    "NAME" => Some(ProcessSortColumn::Name),
                    "CPU%" => Some(ProcessSortColumn::Cpu),
                    "MEM" => Some(ProcessSortColumn::Memory),
                    "PORTS" => Some(ProcessSortColumn::Ports),
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
            .processes
            .iter()
            .map(|p| {
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
        .block(block)
        .row_highlight_style(Style::default().bg(Color::DarkGray));
        StatefulWidget::render(table, area, buf, state);

        let visible = (area.height as usize).saturating_sub(3);
        render_panel_scrollbar(buf, area, self.processes.len(), visible, state.offset());
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
            sort_column: ProcessSortColumn::Cpu,
            sort_direction: SortDirection::Desc,
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 10));
        Widget::render(p, Rect::new(0, 0, 80, 10), &mut buf);
    }

    #[test]
    fn test_stateful_render_scrolls_to_keep_selected_visible() {
        let processes: Vec<ProcessInfo> = (0..50)
            .map(|i| ProcessInfo {
                pid: 1000 + i as u32,
                name: format!("proc-{:02}", i),
                command: "x".into(),
                user: "u".into(),
                cpu_percent: 0.0,
                memory_bytes: 0,
                threads: 1,
                parent_pid: Some(1),
                listening_ports: vec![],
                start_time: 0,
            })
            .collect();
        let p = ProcessesPanel {
            processes: &processes,
            selected: 42,
            filter_text: "",
            is_focused: true,
            tree_mode: false,
            sort_column: ProcessSortColumn::Cpu,
            sort_direction: SortDirection::Desc,
        };
        let area = Rect::new(0, 0, 80, 8);
        let mut buf = Buffer::empty(area);
        let mut state = TableState::default().with_selected(Some(42)).with_offset(0);
        StatefulWidget::render(p, area, &mut buf, &mut state);
        assert!(
            state.offset() > 0,
            "expected scroll offset to advance, got {}",
            state.offset()
        );
        let rendered = buf_to_string(&buf);
        assert!(
            rendered.contains("proc-42"),
            "expected selected row 'proc-42' to be visible, got:\n{}",
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
