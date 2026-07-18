//! エージェントセッションビューの描画（詳細設計8.2節）。
//! ProcessesPanelと同型の借用データ方式。表示行は描画用複製（App::visible_agent_rows）を受け取る。

use crate::app::{agent_kind_label, AgentSortColumn, SortDirection};
use crate::data::agents::model::{AgentSessionRow, Confidence, SessionState};
use crate::data::agents::SourceError;
use crate::ui::common::{format_bytes, render_panel_scrollbar};
use chrono::{DateTime, Local, Utc};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, StatefulWidget, Table, TableState, Widget},
};
use std::collections::HashSet;
use std::path::PathBuf;

/// 同一git_common_dirを持つ行が複数ある場合の警告記号（基本設計8節の競合検出）。
pub const WORKTREE_CONFLICT_MARK: &str = "⚠";

/// stateの表示ラベル。
fn state_label(state: &SessionState) -> &'static str {
    match state {
        SessionState::Running => "running",
        SessionState::Waiting => "waiting",
        SessionState::Idle => "idle",
        SessionState::Failed => "failed",
        SessionState::Orphaned => "orphaned",
        SessionState::Quiet { .. } => "quiet",
        SessionState::Unknown => "unknown",
    }
}

/// STATE列: stateとconfidenceの組表示（詳細設計8.2節）。
/// Reported以外は確度低下が分かる記号を添える（Derived=~、Inferred=?）。
pub fn state_cell(state: &SessionState, confidence: Confidence) -> String {
    let suffix = match confidence {
        Confidence::Reported => "",
        Confidence::Derived => "~",
        Confidence::Inferred => "?",
    };
    format!("{}{}", state_label(state), suffix)
}

/// QUIET列: quiet状態の経過秒表示。quiet以外は"-"。
pub fn quiet_cell(state: &SessionState) -> String {
    match state {
        SessionState::Quiet { elapsed_s } => format!("{}m{:02}s", elapsed_s / 60, elapsed_s % 60),
        _ => "-".to_string(),
    }
}

/// 複数行に現れるgit_common_dirの集合（該当行へ警告記号を付けるため）。
pub fn duplicated_git_dirs(rows: &[AgentSessionRow]) -> HashSet<PathBuf> {
    let mut seen: HashSet<&PathBuf> = HashSet::new();
    let mut dup: HashSet<PathBuf> = HashSet::new();
    for row in rows {
        if let Some(dir) = &row.git_common_dir {
            if !seen.insert(dir) {
                dup.insert(dir.clone());
            }
        }
    }
    dup
}

/// WORKTREE列: worktree末尾要素。同一git_common_dir複数行には警告記号を付ける。
pub fn worktree_cell(row: &AgentSessionRow, dups: &HashSet<PathBuf>) -> String {
    let name = row
        .worktree
        .as_ref()
        .and_then(|w| w.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "-".to_string());
    let conflicted = row
        .git_common_dir
        .as_ref()
        .is_some_and(|d| dups.contains(d));
    if conflicted {
        format!("{WORKTREE_CONFLICT_MARK} {name}")
    } else {
        name
    }
}

/// パネル下部の取得不可表示（詳細設計8.2節: 取得元名と取得不可の旨を1行で）。
pub fn source_error_line(errors: &[SourceError]) -> Option<String> {
    if errors.is_empty() {
        return None;
    }
    let names: Vec<&str> = errors.iter().map(|e| e.source.as_str()).collect();
    Some(format!("unavailable: {}", names.join(",")))
}

/// 収集停止表示（詳細設計8.4節: 2周期超で停止の旨と最終取得時刻）。
pub fn staleness_line(collected_at: DateTime<Utc>) -> String {
    format!(
        "collection stalled (last {})",
        collected_at.with_timezone(&Local).format("%H:%M:%S")
    )
}

pub struct AgentsPanel<'a> {
    /// 描画用複製（フィルタ+ソート適用済み）。共有スナップショットではない。
    pub rows: &'a [AgentSessionRow],
    pub selected: usize,
    pub filter_text: &'a str,
    pub is_focused: bool,
    pub sort_column: AgentSortColumn,
    pub sort_direction: SortDirection,
    pub source_errors: &'a [SourceError],
    pub collected_at: Option<DateTime<Utc>>,
    pub stale: bool,
}

impl Widget for AgentsPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let selected = self.selected;
        let mut state = TableState::default().with_selected(Some(selected));
        StatefulWidget::render(self, area, buf, &mut state);
    }
}

impl StatefulWidget for AgentsPanel<'_> {
    type State = TableState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut TableState) {
        let title = if self.filter_text.is_empty() {
            " Agent Sessions ".to_string()
        } else {
            format!(" Agent Sessions [filter: {}] ", self.filter_text)
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
        let mut bottom = format!(
            " {} items  Sort: {}{} (a:view ,/.:col S:dir) ",
            self.rows.len(),
            self.sort_column.label(),
            arrow
        );
        if let Some(errs) = source_error_line(self.source_errors) {
            bottom.push_str(&format!(" {errs} "));
        }
        if self.stale {
            if let Some(at) = self.collected_at {
                bottom.push_str(&format!(" {} ", staleness_line(at)));
            }
        }
        let block = Block::default()
            .title(title)
            .title_bottom(bottom)
            .borders(Borders::ALL)
            .border_style(border_style);

        let header_cols: Vec<String> = [
            ("STATE", Some(AgentSortColumn::State)),
            ("AGENT", Some(AgentSortColumn::Agent)),
            ("TASK", Some(AgentSortColumn::Task)),
            ("LOCATION", Some(AgentSortColumn::Location)),
            ("WORKTREE", None),
            ("QUIET", None),
            ("CPU", Some(AgentSortColumn::Cpu)),
            ("MEM", Some(AgentSortColumn::Memory)),
        ]
        .iter()
        .map(|&(col, sort_col)| {
            if sort_col == Some(self.sort_column) {
                format!("{col}{arrow}")
            } else {
                col.to_string()
            }
        })
        .collect();
        let header = Row::new(header_cols).style(Style::default().add_modifier(Modifier::BOLD));

        let dups = duplicated_git_dirs(self.rows);
        let rows: Vec<Row> = self
            .rows
            .iter()
            .map(|r| {
                Row::new(vec![
                    state_cell(&r.state, r.confidence),
                    agent_kind_label(&r.agent).to_string(),
                    r.task_title
                        .as_deref()
                        .unwrap_or("-")
                        .chars()
                        .take(30)
                        .collect::<String>(),
                    r.location.chars().take(24).collect::<String>(),
                    worktree_cell(r, &dups),
                    quiet_cell(&r.state),
                    r.cpu_percent
                        .map(|c| format!("{c:.1}"))
                        .unwrap_or_else(|| "-".to_string()),
                    r.memory_bytes
                        .map(format_bytes)
                        .unwrap_or_else(|| "-".to_string()),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Length(8),
                Constraint::Min(12),
                Constraint::Min(10),
                Constraint::Min(8),
                Constraint::Length(8),
                Constraint::Length(6),
                Constraint::Length(8),
            ],
        )
        .header(header)
        .block(block)
        .row_highlight_style(Style::default().bg(Color::DarkGray));
        StatefulWidget::render(table, area, buf, state);

        let visible = (area.height as usize).saturating_sub(3);
        render_panel_scrollbar(buf, area, self.rows.len(), visible, state.offset());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::agents::model::{AgentKind, StateSource};

    fn row(id: &str) -> AgentSessionRow {
        let mut r = AgentSessionRow::new(id, AgentKind::Claude, StateSource::ClaudeCli);
        r.task_title = Some(format!("task-{id}"));
        r.location = format!("loc-{id}");
        r
    }

    #[test]
    fn state_cell_marks_reduced_confidence_for_non_reported() {
        assert_eq!(
            state_cell(&SessionState::Running, Confidence::Reported),
            "running"
        );
        assert_eq!(
            state_cell(&SessionState::Quiet { elapsed_s: 500 }, Confidence::Derived),
            "quiet~"
        );
        assert_eq!(
            state_cell(&SessionState::Orphaned, Confidence::Inferred),
            "orphaned?"
        );
        assert_eq!(
            state_cell(&SessionState::Unknown, Confidence::Inferred),
            "unknown?"
        );
    }

    #[test]
    fn quiet_cell_formats_elapsed_only_for_quiet() {
        assert_eq!(quiet_cell(&SessionState::Quiet { elapsed_s: 512 }), "8m32s");
        assert_eq!(quiet_cell(&SessionState::Quiet { elapsed_s: 60 }), "1m00s");
        assert_eq!(quiet_cell(&SessionState::Running), "-");
    }

    #[test]
    fn duplicated_git_dirs_detects_only_repeated_dirs() {
        let mut a = row("a");
        a.git_common_dir = Some(PathBuf::from("/repo/.git"));
        let mut b = row("b");
        b.git_common_dir = Some(PathBuf::from("/repo/.git"));
        let mut c = row("c");
        c.git_common_dir = Some(PathBuf::from("/other/.git"));
        let d = row("d"); // git情報なし
        let dups = duplicated_git_dirs(&[a, b, c, d]);
        assert_eq!(dups.len(), 1);
        assert!(dups.contains(&PathBuf::from("/repo/.git")));
    }

    #[test]
    fn worktree_cell_adds_warning_mark_for_conflicting_rows() {
        let mut a = row("a");
        a.worktree = Some(PathBuf::from("/repo/wt-a"));
        a.git_common_dir = Some(PathBuf::from("/repo/.git"));
        let dups: HashSet<PathBuf> = [PathBuf::from("/repo/.git")].into_iter().collect();
        assert_eq!(worktree_cell(&a, &dups), "⚠ wt-a");
        let empty = HashSet::new();
        assert_eq!(worktree_cell(&a, &empty), "wt-a");
        let no_wt = row("b");
        assert_eq!(worktree_cell(&no_wt, &empty), "-");
    }

    #[test]
    fn source_error_line_lists_unavailable_sources() {
        assert_eq!(source_error_line(&[]), None);
        let errs = vec![
            SourceError {
                source: "cmux".into(),
                error: "unavailable".into(),
            },
            SourceError {
                source: "kimi".into(),
                error: "boom".into(),
            },
        ];
        assert_eq!(
            source_error_line(&errs),
            Some("unavailable: cmux,kimi".to_string())
        );
    }

    #[test]
    fn render_shows_columns_rows_and_warning_mark() {
        let mut a = row("a");
        a.worktree = Some(PathBuf::from("/repo/wt-a"));
        a.git_common_dir = Some(PathBuf::from("/repo/.git"));
        a.state = SessionState::Quiet { elapsed_s: 512 };
        a.confidence = Confidence::Derived;
        let mut b = row("b");
        b.worktree = Some(PathBuf::from("/repo/wt-b"));
        b.git_common_dir = Some(PathBuf::from("/repo/.git"));
        let rows = vec![a, b];
        let panel = AgentsPanel {
            rows: &rows,
            selected: 0,
            filter_text: "",
            is_focused: true,
            sort_column: AgentSortColumn::State,
            sort_direction: SortDirection::Asc,
            source_errors: &[],
            collected_at: None,
            stale: false,
        };
        let area = Rect::new(0, 0, 140, 12);
        let mut buf = Buffer::empty(area);
        Widget::render(panel, area, &mut buf);
        let text = buf_to_string(&buf);
        for header in [
            "STATE", "AGENT", "TASK", "LOCATION", "WORKTREE", "QUIET", "CPU", "MEM",
        ] {
            assert!(text.contains(header), "missing header {header}:\n{text}");
        }
        assert!(text.contains("quiet~"), "確度低下表示が必要:\n{text}");
        assert!(
            text.matches(WORKTREE_CONFLICT_MARK).count() >= 2,
            "同一git_common_dirの2行へ警告記号:\n{text}"
        );
        assert!(text.contains("task-a"), "1行目が描画される");
        assert!(text.contains("task-b"), "2行目が描画される");
    }

    #[test]
    fn render_shows_source_errors_and_staleness_at_bottom() {
        let rows = vec![row("a")];
        let errs = vec![SourceError {
            source: "cmux".into(),
            error: "unavailable".into(),
        }];
        let collected = Utc::now() - chrono::Duration::seconds(60);
        let panel = AgentsPanel {
            rows: &rows,
            selected: 0,
            filter_text: "",
            is_focused: false,
            sort_column: AgentSortColumn::State,
            sort_direction: SortDirection::Asc,
            source_errors: &errs,
            collected_at: Some(collected),
            stale: true,
        };
        let area = Rect::new(0, 0, 140, 10);
        let mut buf = Buffer::empty(area);
        Widget::render(panel, area, &mut buf);
        let text = buf_to_string(&buf);
        assert!(
            text.contains("unavailable: cmux"),
            "取得不可の取得元:\n{text}"
        );
        assert!(text.contains("collection stalled"), "収集停止の旨:\n{text}");
    }

    #[test]
    fn render_empty_rows_no_panic() {
        let panel = AgentsPanel {
            rows: &[],
            selected: 0,
            filter_text: "",
            is_focused: false,
            sort_column: AgentSortColumn::State,
            sort_direction: SortDirection::Asc,
            source_errors: &[],
            collected_at: None,
            stale: false,
        };
        let area = Rect::new(0, 0, 80, 6);
        let mut buf = Buffer::empty(area);
        Widget::render(panel, area, &mut buf);
    }

    fn buf_to_string(buf: &Buffer) -> String {
        let area = buf.area;
        let mut result = String::new();
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                result.push_str(buf[(x, y)].symbol());
            }
            result.push('\n');
        }
        result
    }
}
