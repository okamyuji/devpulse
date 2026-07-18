//! 正規化と重複統合（詳細設計5節）および状態決定（詳細設計6節）。

use chrono::{DateTime, Utc};
use std::collections::HashSet;

use super::model::{AgentSessionRow, Confidence, SessionState, StateSource};
use super::ActivityEvent;

/// 統合の入口。照合キーを優先順（pid→tty→cwd+種別→統合しない）に適用する。
///
/// pid照合（優先0）はCLI行のpidとプロセス行/cmux tty照合済み行のpidの完全一致。
/// cmux行はtty照合で初めてpidを得るため、コード上はtty照合を先に実行するが、
/// CLI行にとっての照合キー優先順はpidが最上位（pidを持つ行はcwd照合へ進まない）。
///
/// - `cmux_rows`: terminal型surface由来の行の候補
/// - `cli_rows`: Claude CLI行とKimi行
/// - `process_rows`: プロセスフォールバック行
/// - `tty_universe`: psが返した全制御端末名。psが失敗した場合はNone（tty照合を
///   諦めcwd照合へ縮退する。詳細設計4.4節）
pub fn merge_rows(
    cmux_rows: Vec<AgentSessionRow>,
    cli_rows: Vec<AgentSessionRow>,
    process_rows: Vec<AgentSessionRow>,
    tty_universe: Option<&HashSet<String>>,
) -> Vec<AgentSessionRow> {
    let mut out = Vec::new();
    let mut procs: Vec<Option<AgentSessionRow>> = process_rows.into_iter().map(Some).collect();
    let mut pid_matched = 0usize;
    let mut tty_matched = 0usize;
    let mut cwd_matched = 0usize;

    // 優先1: tty照合（cmuxのterminal型surfaceとプロセスの制御端末）
    for mut cmux in cmux_rows {
        let Some(universe) = tty_universe else {
            // psが失敗した場合はtty照合を諦める（優先2のcwd照合へ縮退）。
            // cmux行はcwdを持たないため照合材料がなく出力しない。
            continue;
        };
        let Some(tty) = cmux.tty.clone() else {
            continue;
        };
        let matches: Vec<usize> = procs
            .iter()
            .enumerate()
            .filter(|(_, p)| p.as_ref().and_then(|p| p.tty.as_deref()) == Some(tty.as_str()))
            .map(|(i, _)| i)
            .collect();
        match matches.as_slice() {
            [only] => {
                let proc = procs[*only].take().expect("index from filter");
                tty_matched += 1;
                // 位置・題名はcmux、実体属性はプロセスから統合する
                cmux.agent = proc.agent;
                cmux.cwd = proc.cwd;
                cmux.pid = proc.pid;
                cmux.cpu_percent = proc.cpu_percent;
                cmux.memory_bytes = proc.memory_bytes;
                out.push(cmux);
            }
            [] if !universe.contains(&tty) => {
                // 親参照（surface）が実在するのに、そのttyにプロセスが1つも
                // 存在しない: orphanedの材料（詳細設計6節の順5）
                cmux.state = SessionState::Orphaned;
                cmux.state_source = StateSource::CmuxCli;
                cmux.confidence = Confidence::Derived;
                out.push(cmux);
            }
            // エージェント不在だがttyは生きている（素の端末）、または同一tty上に
            // 複数エージェントがある曖昧ケース: 統合しない（誤統合より重複表示）
            _ => {}
        }
    }

    // 優先0: pid照合（CLI行のpidとプロセス行/cmux tty照合済み行のpidの完全一致）。
    // claudeのinteractive型はpidを報告するため、cwd照合の曖昧さに依らず確実に統合できる。
    let mut cli_slots: Vec<Option<AgentSessionRow>> = cli_rows.into_iter().map(Some).collect();
    for slot in cli_slots.iter_mut() {
        let Some(pid) = slot.as_ref().and_then(|r| r.pid) else {
            continue;
        };
        // cmux tty照合済み行との一致: 表示（識別・位置・題名）はcmux側を保ち、
        // CLI報告の状態とsession_idを取り込む
        if let Some(target) = out.iter_mut().find(|r| r.pid == Some(pid)) {
            let row = slot.take().expect("slot checked above");
            pid_matched += 1;
            target.agent = row.agent;
            target.state = row.state;
            target.state_source = row.state_source;
            target.confidence = row.confidence;
            target.session_id = row.session_id;
            if target.cwd.is_none() {
                target.cwd = row.cwd;
            }
            if target.task_title.is_none() {
                target.task_title = row.task_title;
            }
            continue;
        }
        // プロセス行との一致: 実体属性をプロセスから統合する
        let matches: Vec<usize> = procs
            .iter()
            .enumerate()
            .filter(|(_, p)| p.as_ref().and_then(|p| p.pid) == Some(pid))
            .map(|(i, _)| i)
            .collect();
        if let [only] = matches.as_slice() {
            let proc = procs[*only].take().expect("index from filter");
            pid_matched += 1;
            let row = slot.as_mut().expect("slot checked above");
            row.tty = proc.tty;
            row.cpu_percent = proc.cpu_percent;
            row.memory_bytes = proc.memory_bytes;
            if row.cwd.is_none() {
                row.cwd = proc.cwd;
            }
        }
    }
    let cli_rows: Vec<AgentSessionRow> = cli_slots.into_iter().flatten().collect();

    // 優先2: cwdとエージェント種別の照合（Claude CLI行・Kimi行とプロセス行）。
    // pidを持つ行は優先0で決着済みのためcwd照合の対象にも競合にも含めない。
    for i in 0..cli_rows.len() {
        let mut row = cli_rows[i].clone();
        if let (None, Some(cwd)) = (row.pid, row.cwd.clone()) {
            let rival_cli = cli_rows
                .iter()
                .filter(|r| {
                    r.pid.is_none()
                        && r.agent == row.agent
                        && r.cwd.as_deref() == Some(cwd.as_path())
                })
                .count();
            let matches: Vec<usize> = procs
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    p.as_ref().is_some_and(|p| {
                        p.agent == row.agent && p.cwd.as_deref() == Some(cwd.as_path())
                    })
                })
                .map(|(idx, _)| idx)
                .collect();
            // 同一cwdの複数エージェントで曖昧な場合は統合しない（詳細設計5節）
            if rival_cli == 1 {
                if let [only] = matches.as_slice() {
                    let proc = procs[*only].take().expect("index from filter");
                    cwd_matched += 1;
                    row.pid = proc.pid;
                    row.tty = proc.tty;
                    row.cpu_percent = proc.cpu_percent;
                    row.memory_bytes = proc.memory_bytes;
                }
            }
        }
        out.push(row);
    }

    // 優先3: 一致なし。統合されなかったプロセス行を個別の行として残す
    let unmerged: Vec<AgentSessionRow> = procs.into_iter().flatten().collect();
    tracing::debug!(
        pid_matched,
        tty_matched,
        cwd_matched,
        unmerged = unmerged.len(),
        "merge completed"
    );
    out.extend(unmerged);
    out
}

/// cmux eventsの活動イベントを統合後の行へ適用する。
/// session_idが一致した行のlast_activity_atを最新イベント時刻で更新する（詳細設計4.1節）。
/// cwdはフック時の作業ディレクトリでセッション一意でなく誤帰属の実測があるため使わない。
///
/// イベントのsession_idは「claude-<uuid>」のように取得元接頭辞付きのため、
/// 完全一致に加え「-<行のsession_id>」の接尾辞一致も同一セッションとみなす。
pub fn apply_activity(rows: &mut [AgentSessionRow], events: &[ActivityEvent]) {
    for row in rows.iter_mut() {
        let Some(sid) = row.session_id.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };
        let suffix = format!("-{sid}");
        let latest = events
            .iter()
            .filter(|e| e.session_id == sid || e.session_id.ends_with(&suffix))
            .map(|e| e.occurred_at)
            .max();
        if let Some(occurred_at) = latest {
            // 既により新しい活動時刻を持つ行は巻き戻さない
            if row.last_activity_at.is_none_or(|t| t < occurred_at) {
                row.last_activity_at = Some(occurred_at);
                row.activity_source = Some(StateSource::CmuxCli);
            }
        }
    }
}

/// 状態決定の入力（詳細設計6節の決定表の判定材料）。
#[derive(Debug, Clone)]
pub struct StateInputs {
    /// 公開CLIが報告した状態（failed/waiting/idle/runningのみ。それ以外はNone）
    pub reported: Option<SessionState>,
    /// orphaned判定の材料（判定材料の出所と、その出所に応じたconfidence）
    pub orphan: Option<(StateSource, Confidence)>,
    pub last_activity_at: Option<DateTime<Utc>>,
    /// last_activity_atを与えた取得元
    pub activity_source: Option<StateSource>,
    pub has_process: bool,
    /// 行を生成した取得元（他に材料がない場合のstate_source）
    pub origin: StateSource,
}

/// 決定表（詳細設計6節）を上から順に適用する。
pub fn decide_state(
    inputs: &StateInputs,
    now: DateTime<Utc>,
    quiet_threshold_s: u64,
) -> (SessionState, StateSource, Confidence) {
    // 順1〜4: 公開CLIの報告（failed→waiting→idle→runningの順で判定するが、
    // reportedは単一値のためどれか1つ）
    if let Some(reported) = inputs.reported {
        if matches!(
            reported,
            SessionState::Failed
                | SessionState::Waiting
                | SessionState::Idle
                | SessionState::Running
        ) {
            return (reported, inputs.origin, Confidence::Reported);
        }
    }
    // 順5: orphaned（判定材料の出所に応じたconfidence）
    if let Some((source, confidence)) = inputs.orphan {
        return (SessionState::Orphaned, source, confidence);
    }
    // 順6: quiet（経過が閾値を超過。未来時刻は経過0として扱う）
    if let Some(last) = inputs.last_activity_at {
        let elapsed_s = (now - last).num_seconds().max(0) as u64;
        if elapsed_s > quiet_threshold_s {
            return (
                SessionState::Quiet { elapsed_s },
                inputs.activity_source.unwrap_or(inputs.origin),
                Confidence::Derived,
            );
        }
    }
    // 順7: プロセスは実在するが上記いずれにも該当しない
    if inputs.has_process {
        return (
            SessionState::Unknown,
            StateSource::ProcessTable,
            Confidence::Inferred,
        );
    }
    // 順8: いずれの材料もない
    (SessionState::Unknown, inputs.origin, Confidence::Inferred)
}

/// 統合後の全行へ決定表を無条件に適用する（詳細設計4.3節・6節）。
pub fn apply_state_decision(
    rows: &mut [AgentSessionRow],
    now: DateTime<Utc>,
    quiet_threshold_s: u64,
) {
    for row in rows.iter_mut() {
        let reported = (row.confidence == Confidence::Reported).then_some(row.state);
        let orphan =
            (row.state == SessionState::Orphaned).then_some((row.state_source, row.confidence));
        let inputs = StateInputs {
            reported,
            orphan,
            last_activity_at: row.last_activity_at,
            activity_source: row.activity_source,
            has_process: row.pid.is_some(),
            origin: row.state_source,
        };
        let (state, source, confidence) = decide_state(&inputs, now, quiet_threshold_s);
        row.state = state;
        row.state_source = source;
        row.confidence = confidence;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::agents::model::{AgentKind, Orchestrator};
    use chrono::{Duration, TimeZone};
    use std::path::PathBuf;

    fn cmux_row(id: &str, tty: &str, title: &str) -> AgentSessionRow {
        let mut row =
            AgentSessionRow::new(id, AgentKind::Other("unknown".into()), StateSource::CmuxCli);
        row.orchestrator = Orchestrator::Cmux;
        row.location = format!("workspace:1/{id}");
        row.task_title = Some(title.into());
        row.tty = Some(tty.into());
        row
    }

    fn proc_row(
        pid: u32,
        kind: AgentKind,
        cwd: Option<&str>,
        tty: Option<&str>,
    ) -> AgentSessionRow {
        let mut row = AgentSessionRow::new(pid.to_string(), kind, StateSource::ProcessTable);
        row.location = format!("pid:{pid}");
        row.pid = Some(pid);
        row.cwd = cwd.map(PathBuf::from);
        row.tty = tty.map(String::from);
        row.cpu_percent = Some(2.0);
        row.memory_bytes = Some(64_000_000);
        row
    }

    fn claude_row(id: &str, cwd: &str, state: SessionState) -> AgentSessionRow {
        let mut row = AgentSessionRow::new(id, AgentKind::Claude, StateSource::ClaudeCli);
        row.cwd = Some(PathBuf::from(cwd));
        row.state = state;
        if !matches!(state, SessionState::Unknown) {
            row.confidence = Confidence::Reported;
        }
        row
    }

    fn kimi_row(id: &str, cwd: Option<&str>, updated: Option<DateTime<Utc>>) -> AgentSessionRow {
        let mut row = AgentSessionRow::new(id, AgentKind::Kimi, StateSource::KimiMetadata);
        row.cwd = cwd.map(PathBuf::from);
        row.last_activity_at = updated;
        row.activity_source = updated.map(|_| StateSource::KimiMetadata);
        row
    }

    fn claude_interactive_row(sid: &str, pid: u32, cwd: &str) -> AgentSessionRow {
        // interactive型claude行: idはsessionId先頭8文字、pidと報告状態を持つ
        let mut row = AgentSessionRow::new(&sid[..8], AgentKind::Claude, StateSource::ClaudeCli);
        row.cwd = Some(PathBuf::from(cwd));
        row.state = SessionState::Idle;
        row.confidence = Confidence::Reported;
        row.pid = Some(pid);
        row.session_id = Some(sid.to_string());
        row
    }

    fn universe(ttys: &[&str]) -> HashSet<String> {
        ttys.iter().map(|s| s.to_string()).collect()
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap()
    }

    // ---- 統合（詳細設計5節） ----

    #[test]
    fn tty_match_merges_cmux_surface_with_agent_process() {
        // 優先1: cmuxのterminal型surfaceのttyとプロセスの制御端末の一致
        let cmux = vec![cmux_row(
            "surface:1",
            "ttys000",
            "⠐ AI開発ツールの改善案を整理",
        )];
        let procs = vec![proc_row(
            91658,
            AgentKind::Claude,
            Some("/Users/dev/devs/zenn"),
            Some("ttys000"),
        )];
        let rows = merge_rows(cmux, vec![], procs, Some(&universe(&["ttys000"])));
        assert_eq!(rows.len(), 1);
        let m = &rows[0];
        // 位置と題名はcmux、実体属性はプロセスから
        assert_eq!(m.id, "surface:1");
        assert_eq!(m.location, "workspace:1/surface:1");
        assert_eq!(
            m.task_title.as_deref(),
            Some("⠐ AI開発ツールの改善案を整理")
        );
        assert_eq!(m.orchestrator, Orchestrator::Cmux);
        assert_eq!(m.agent, AgentKind::Claude);
        assert_eq!(m.pid, Some(91658));
        assert_eq!(m.cwd, Some(PathBuf::from("/Users/dev/devs/zenn")));
        assert_eq!(m.tty.as_deref(), Some("ttys000"));
        assert_eq!(m.cpu_percent, Some(2.0));
        assert_eq!(m.memory_bytes, Some(64_000_000));
    }

    #[test]
    fn merge_emits_debug_event_with_match_counts() {
        let cap = crate::logging::capture::Capture::new();
        let _guard = tracing::subscriber::set_default(cap.subscriber());
        let cmux = vec![cmux_row("surface:1", "ttys000", "t")];
        let procs = vec![
            proc_row(1, AgentKind::Claude, Some("/x"), Some("ttys000")),
            proc_row(2, AgentKind::Codex, Some("/y"), Some("ttys001")),
        ];
        merge_rows(
            cmux,
            vec![],
            procs,
            Some(&universe(&["ttys000", "ttys001"])),
        );
        let ev = cap.find("merge completed").expect("merge debug event");
        assert_eq!(ev.level, tracing::Level::DEBUG);
        assert!(ev.fields.contains("pid_matched=0"), "{}", ev.fields);
        assert!(ev.fields.contains("tty_matched=1"), "{}", ev.fields);
        assert!(ev.fields.contains("cwd_matched=0"), "{}", ev.fields);
        assert!(ev.fields.contains("unmerged=1"), "{}", ev.fields);
    }

    #[test]
    fn unmatched_cmux_surface_with_live_tty_is_dropped_as_plain_terminal() {
        // エージェントプロセスは無いがttyは生きている（素のシェル）→ 行にしない
        let cmux = vec![cmux_row("surface:4", "ttys003", "noslp")];
        let rows = merge_rows(cmux, vec![], vec![], Some(&universe(&["ttys003"])));
        assert!(rows.is_empty());
    }

    #[test]
    fn cmux_surface_with_dead_tty_becomes_orphaned_candidate() {
        // 順5の材料: 親参照（surface）が実在するのに対応プロセスが不在
        // （そのttyにpsが1つもプロセスを載せていない）
        let cmux = vec![cmux_row("surface:9", "ttys007", "🚀 サンプル記事の下書き")];
        let rows = merge_rows(cmux, vec![], vec![], Some(&universe(&["ttys000"])));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, SessionState::Orphaned);
        assert_eq!(rows[0].state_source, StateSource::CmuxCli);
        assert_eq!(rows[0].confidence, Confidence::Derived);
    }

    #[test]
    fn ps_failure_degrades_to_cwd_matching_only() {
        // psが失敗した場合（universe=None）はtty照合を諦め、優先2のcwd照合へ縮退
        let cmux = vec![cmux_row("surface:1", "ttys000", "t")];
        let cli = vec![claude_row("aaa", "/Users/dev/proj", SessionState::Failed)];
        let procs = vec![proc_row(
            500,
            AgentKind::Claude,
            Some("/Users/dev/proj"),
            None,
        )];
        let rows = merge_rows(cmux, cli, procs, None);
        // cmux行は照合材料を失い出力されない。claude行はcwd+種別でプロセスと統合。
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "aaa");
        assert_eq!(rows[0].pid, Some(500));
    }

    #[test]
    fn cwd_and_kind_match_merges_cli_row_with_process() {
        // 優先2: cwdとエージェント種別の一致
        let cli = vec![claude_row(
            "aaa",
            "/Users/dev/devs/zenn",
            SessionState::Waiting,
        )];
        let procs = vec![
            proc_row(
                600,
                AgentKind::Claude,
                Some("/Users/dev/devs/zenn"),
                Some("ttys002"),
            ),
            proc_row(700, AgentKind::Kimi, Some("/Users/dev/devs/zenn"), None),
        ];
        let rows = merge_rows(vec![], cli, procs, Some(&universe(&["ttys002"])));
        // claude行は600と統合、kimiプロセス700は非統合で残る
        assert_eq!(rows.len(), 2);
        let merged = rows.iter().find(|r| r.id == "aaa").unwrap();
        assert_eq!(merged.pid, Some(600));
        assert_eq!(merged.tty.as_deref(), Some("ttys002"));
        // 公開CLIの報告値が最優先（stateはCLI報告のまま）
        assert_eq!(merged.state, SessionState::Waiting);
        assert_eq!(merged.state_source, StateSource::ClaudeCli);
        assert!(rows
            .iter()
            .any(|r| r.pid == Some(700) && r.agent == AgentKind::Kimi));
    }

    #[test]
    fn same_cwd_different_kind_does_not_merge() {
        let cli = vec![kimi_row("session_x", Some("/Users/dev/p"), None)];
        let procs = vec![proc_row(800, AgentKind::Claude, Some("/Users/dev/p"), None)];
        let rows = merge_rows(vec![], cli, procs, Some(&universe(&[])));
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|r| r.pid != Some(800) || r.agent == AgentKind::Claude));
    }

    #[test]
    fn ambiguous_multiple_processes_same_cwd_do_not_merge() {
        // 同一cwdの複数エージェント: 誤統合より重複表示を選ぶ（詳細設計5節）
        let cli = vec![claude_row("aaa", "/Users/dev/p", SessionState::Idle)];
        let procs = vec![
            proc_row(900, AgentKind::Claude, Some("/Users/dev/p"), None),
            proc_row(901, AgentKind::Claude, Some("/Users/dev/p"), None),
        ];
        let rows = merge_rows(vec![], cli, procs, Some(&universe(&[])));
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().find(|r| r.id == "aaa").unwrap().pid.is_none());
    }

    #[test]
    fn ambiguous_multiple_cli_rows_for_one_process_do_not_merge() {
        let cli = vec![
            claude_row("aaa", "/Users/dev/p", SessionState::Idle),
            claude_row("bbb", "/Users/dev/p", SessionState::Failed),
        ];
        let procs = vec![proc_row(902, AgentKind::Claude, Some("/Users/dev/p"), None)];
        let rows = merge_rows(vec![], cli, procs, Some(&universe(&[])));
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.id == "902" || r.pid.is_none()));
    }

    #[test]
    fn ambiguous_multiple_agent_processes_on_same_tty_do_not_merge() {
        let cmux = vec![cmux_row("surface:1", "ttys000", "t")];
        let procs = vec![
            proc_row(910, AgentKind::Claude, None, Some("ttys000")),
            proc_row(911, AgentKind::Codex, None, Some("ttys000")),
        ];
        let rows = merge_rows(cmux, vec![], procs, Some(&universe(&["ttys000"])));
        // 統合しない: プロセス2行が残り、cmux行は素の端末扱いで落ちる
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.orchestrator == Orchestrator::Unknown));
    }

    #[test]
    fn rival_count_requires_both_kind_and_cwd_to_match() {
        // 照合条件は「種別一致かつcwd一致」。片方だけの一致は競合に数えず、
        // 唯一のCLI行とプロセスは正しく統合される（&&→||の防止）。
        // ケース1: cwd一致・種別不一致のCLI行（kimi）がいても、claude行は統合される
        let cli = vec![
            claude_row("aaa", "/Users/dev/p", SessionState::Idle),
            kimi_row("session_k", Some("/Users/dev/p"), None),
        ];
        let procs = vec![proc_row(920, AgentKind::Claude, Some("/Users/dev/p"), None)];
        let rows = merge_rows(vec![], cli, procs, Some(&universe(&[])));
        assert_eq!(rows.len(), 2);
        let claude = rows.iter().find(|r| r.id == "aaa").unwrap();
        assert_eq!(claude.pid, Some(920));
        let kimi = rows.iter().find(|r| r.id == "session_k").unwrap();
        assert!(kimi.pid.is_none());
        // ケース2: 種別一致・cwd不一致のCLI行（別ディレクトリのclaude）がいても統合される
        let cli = vec![
            claude_row("aaa", "/Users/dev/a", SessionState::Idle),
            claude_row("bbb", "/Users/dev/b", SessionState::Idle),
        ];
        let procs = vec![proc_row(930, AgentKind::Claude, Some("/Users/dev/a"), None)];
        let rows = merge_rows(vec![], cli, procs, Some(&universe(&[])));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.iter().find(|r| r.id == "aaa").unwrap().pid, Some(930));
        assert!(rows.iter().find(|r| r.id == "bbb").unwrap().pid.is_none());
    }

    #[test]
    fn merge_handles_zero_one_and_bulk_inputs() {
        // T2: 0件、1件、大量（1000件以上）
        assert!(merge_rows(vec![], vec![], vec![], None).is_empty());
        let one = merge_rows(
            vec![],
            vec![],
            vec![proc_row(1, AgentKind::Kimi, None, None)],
            Some(&universe(&[])),
        );
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].id, "1");
        let many: Vec<_> = (0..1200)
            .map(|i| proc_row(2000 + i, AgentKind::Claude, Some("/Users/dev/bulk"), None))
            .collect();
        let rows = merge_rows(vec![], vec![], many, Some(&universe(&[])));
        assert_eq!(rows.len(), 1200);
        assert_eq!(rows[0].pid, Some(2000));
        assert_eq!(rows[1199].pid, Some(3199));
    }

    #[test]
    fn merge_is_idempotent_for_same_input() {
        // T3冪等性: 同一入力で2回実行して結果が同一、重複統合が発生しない
        let build = || {
            (
                vec![cmux_row("surface:1", "ttys000", "t")],
                vec![claude_row("aaa", "/Users/dev/p", SessionState::Idle)],
                vec![
                    proc_row(600, AgentKind::Claude, Some("/Users/dev/p"), None),
                    proc_row(601, AgentKind::Kimi, None, Some("ttys000")),
                ],
            )
        };
        let u = universe(&["ttys000"]);
        let (c1, l1, p1) = build();
        let (c2, l2, p2) = build();
        let r1 = merge_rows(c1, l1, p1, Some(&u));
        let r2 = merge_rows(c2, l2, p2, Some(&u));
        assert_eq!(r1, r2);
        assert_eq!(r1.len(), 2);
    }

    #[test]
    fn pid_match_merges_claude_interactive_row_with_process_row() {
        // 優先0: claude行のpidとプロセス行のpidの完全一致は最優先で統合する
        let cli = vec![claude_interactive_row(
            "e05d60a0-a0cb-44db-9206-6c6e5bf04d4c",
            81574,
            "/Users/dev/p",
        )];
        let procs = vec![
            proc_row(
                81574,
                AgentKind::Claude,
                Some("/Users/dev/p"),
                Some("ttys004"),
            ),
            proc_row(81575, AgentKind::Claude, Some("/Users/dev/p"), None),
        ];
        let rows = merge_rows(vec![], cli, procs, Some(&universe(&["ttys004"])));
        // 同一cwdに複数claudeプロセスがいてもpid一致は曖昧にならず統合される
        assert_eq!(rows.len(), 2);
        let merged = rows.iter().find(|r| r.id == "e05d60a0").unwrap();
        assert_eq!(merged.pid, Some(81574));
        assert_eq!(merged.tty.as_deref(), Some("ttys004"));
        assert_eq!(merged.cpu_percent, Some(2.0));
        assert_eq!(merged.memory_bytes, Some(64_000_000));
        // 公開CLIの報告値は保持される
        assert_eq!(merged.state, SessionState::Idle);
        assert_eq!(merged.confidence, Confidence::Reported);
        // pid不一致のプロセスは独立行として残る
        assert!(rows.iter().any(|r| r.pid == Some(81575)));
    }

    #[test]
    fn pid_match_merges_cmux_tty_matched_row_with_claude_interactive_row() {
        // 優先0: cmux tty照合済み行（ttyでプロセスと統合されpidを得た行）と
        // claude interactive行のpid完全一致。cmux workspace行のstateが
        // unknownでなくCLI報告値になる。
        let cmux = vec![cmux_row("surface:1", "ttys000", "devpulse-0a")];
        let procs = vec![proc_row(
            81574,
            AgentKind::Claude,
            Some("/Users/dev/p"),
            Some("ttys000"),
        )];
        let cli = vec![claude_interactive_row(
            "e05d60a0-a0cb-44db-9206-6c6e5bf04d4c",
            81574,
            "/Users/dev/p",
        )];
        let rows = merge_rows(cmux, cli, procs, Some(&universe(&["ttys000"])));
        assert_eq!(rows.len(), 1);
        let m = &rows[0];
        // 表示（識別・位置・題名）はcmux側を保つ
        assert_eq!(m.id, "surface:1");
        assert_eq!(m.location, "workspace:1/surface:1");
        assert_eq!(m.orchestrator, Orchestrator::Cmux);
        assert_eq!(m.pid, Some(81574));
        // 状態はCLI報告値
        assert_eq!(m.state, SessionState::Idle);
        assert_eq!(m.state_source, StateSource::ClaudeCli);
        assert_eq!(m.confidence, Confidence::Reported);
        // cmux eventsのsession_id照合に使えるようsession_idを引き継ぐ
        assert_eq!(
            m.session_id.as_deref(),
            Some("e05d60a0-a0cb-44db-9206-6c6e5bf04d4c")
        );
    }

    // ---- 活動イベントの適用（詳細設計4.1節） ----

    #[test]
    fn regression_events_join_by_session_id_only_not_cwd() {
        // 手動検証で発見: イベントのcwdはフック時の作業ディレクトリでありセッション一意
        // でない。cwd一致・session_id不一致のイベントが他セッションの行へ誤帰属し
        // QUIET 187mの誤表示になった。cwdが一致してもsession_idが一致しない限り付けない。
        let mut rows = vec![claude_row("aaa", "/Users/dev/devpulse", SessionState::Idle)];
        rows[0].session_id = Some("9e0112cf-9e45-4f76-b22c-64a0712ac9db".into());
        let events = vec![ActivityEvent {
            // cwdは行と同一だがsession_idは別セッション
            session_id: "claude-11111111-2222-3333-4444-555555555555".into(),
            cwd: PathBuf::from("/Users/dev/devpulse"),
            occurred_at: now() - Duration::seconds(60),
        }];
        apply_activity(&mut rows, &events);
        assert_eq!(rows[0].last_activity_at, None);
        assert_eq!(rows[0].activity_source, None);
    }

    #[test]
    fn activity_updates_last_activity_for_matching_session_id_with_latest_event() {
        // session_idが一致するイベントは従来どおり付く。イベントのsession_idは
        // 「claude-<uuid>」形式、行のsession_idは<uuid>のため接尾辞一致で照合する。
        // cwdが食い違っていてもsession_id一致だけで付く。
        let mut rows = vec![claude_row(
            "aaa",
            "/Users/dev/devs/zenn",
            SessionState::Idle,
        )];
        rows[0].session_id = Some("9e0112cf-9e45-4f76-b22c-64a0712ac9db".into());
        let t1 = now() - Duration::seconds(600);
        let t2 = now() - Duration::seconds(60);
        let events = vec![
            ActivityEvent {
                session_id: "claude-9e0112cf-9e45-4f76-b22c-64a0712ac9db".into(),
                cwd: PathBuf::from("/Users/dev/devs/zenn"),
                occurred_at: t1,
            },
            ActivityEvent {
                session_id: "claude-9e0112cf-9e45-4f76-b22c-64a0712ac9db".into(),
                // フック時の作業ディレクトリは行のcwdと食い違うことがある（実測）
                cwd: PathBuf::from("/Users/dev/elsewhere"),
                occurred_at: t2,
            },
            ActivityEvent {
                session_id: "claude-11111111-2222-3333-4444-555555555555".into(),
                cwd: PathBuf::from("/Users/dev/devs/zenn"),
                occurred_at: now(),
            },
        ];
        apply_activity(&mut rows, &events);
        assert_eq!(rows[0].last_activity_at, Some(t2));
        assert_eq!(rows[0].activity_source, Some(StateSource::CmuxCli));
    }

    #[test]
    fn activity_without_session_id_on_row_never_attaches() {
        // session_id照合の相手がいない行（cmux surface行・プロセス行）には付けない。
        // 誤ったquietより正直なunknownを選ぶ（設計の断定回避方針）。
        let mut rows = vec![claude_row("aaa", "/Users/dev/p", SessionState::Idle)];
        assert!(rows[0].session_id.is_none());
        let events = vec![ActivityEvent {
            session_id: "claude-9e0112cf-9e45-4f76-b22c-64a0712ac9db".into(),
            cwd: PathBuf::from("/Users/dev/p"),
            occurred_at: now() - Duration::seconds(60),
        }];
        apply_activity(&mut rows, &events);
        assert_eq!(rows[0].last_activity_at, None);
    }

    #[test]
    fn activity_does_not_regress_newer_metadata_timestamp() {
        let newer = now() - Duration::seconds(10);
        let mut rows = vec![kimi_row("session_x", Some("/Users/dev/p"), Some(newer))];
        rows[0].session_id = Some("s".into());
        let events = vec![ActivityEvent {
            session_id: "s".into(),
            cwd: PathBuf::from("/Users/dev/p"),
            occurred_at: now() - Duration::seconds(1000),
        }];
        apply_activity(&mut rows, &events);
        // Kimiメタデータの方が新しいので上書きしない
        assert_eq!(rows[0].last_activity_at, Some(newer));
        assert_eq!(rows[0].activity_source, Some(StateSource::KimiMetadata));
    }

    #[test]
    fn activity_equal_timestamp_does_not_overwrite_existing_source() {
        // 境界: 同時刻イベントは「より新しい」ではないため上書きしない（<→<=の防止）
        let t = now() - Duration::seconds(100);
        let mut rows = vec![kimi_row("session_x", Some("/Users/dev/p"), Some(t))];
        rows[0].session_id = Some("s".into());
        let events = vec![ActivityEvent {
            session_id: "s".into(),
            cwd: PathBuf::from("/Users/dev/p"),
            occurred_at: t,
        }];
        apply_activity(&mut rows, &events);
        assert_eq!(rows[0].last_activity_at, Some(t));
        assert_eq!(rows[0].activity_source, Some(StateSource::KimiMetadata));
    }

    #[test]
    fn activity_strictly_newer_event_updates_older_row_timestamp() {
        // 境界: 既存時刻より新しいイベントは更新する（<→==の防止）
        let older = now() - Duration::seconds(1000);
        let newer = now() - Duration::seconds(10);
        let mut rows = vec![kimi_row("session_x", Some("/Users/dev/p"), Some(older))];
        rows[0].session_id = Some("s".into());
        let events = vec![ActivityEvent {
            session_id: "s".into(),
            cwd: PathBuf::from("/Users/dev/p"),
            occurred_at: newer,
        }];
        apply_activity(&mut rows, &events);
        assert_eq!(rows[0].last_activity_at, Some(newer));
        assert_eq!(rows[0].activity_source, Some(StateSource::CmuxCli));
    }

    // ---- 状態決定（詳細設計6節の決定表。全8行: T2） ----

    fn base_inputs() -> StateInputs {
        StateInputs {
            reported: None,
            orphan: None,
            last_activity_at: None,
            activity_source: None,
            has_process: false,
            origin: StateSource::ProcessTable,
        }
    }

    #[test]
    fn decision_table_row_1_reported_failed() {
        let inputs = StateInputs {
            reported: Some(SessionState::Failed),
            origin: StateSource::ClaudeCli,
            // 他の材料が揃っていても報告が勝つ
            last_activity_at: Some(now() - Duration::seconds(10_000)),
            has_process: true,
            ..base_inputs()
        };
        let (state, source, conf) = decide_state(&inputs, now(), 480);
        assert_eq!(state, SessionState::Failed);
        assert_eq!(source, StateSource::ClaudeCli);
        assert_eq!(conf, Confidence::Reported);
    }

    #[test]
    fn decision_table_row_2_reported_waiting() {
        let inputs = StateInputs {
            reported: Some(SessionState::Waiting),
            origin: StateSource::ClaudeCli,
            ..base_inputs()
        };
        let (state, source, conf) = decide_state(&inputs, now(), 480);
        assert_eq!(state, SessionState::Waiting);
        assert_eq!(source, StateSource::ClaudeCli);
        assert_eq!(conf, Confidence::Reported);
    }

    #[test]
    fn decision_table_row_3_reported_idle() {
        let inputs = StateInputs {
            reported: Some(SessionState::Idle),
            origin: StateSource::ClaudeCli,
            ..base_inputs()
        };
        let (state, _, conf) = decide_state(&inputs, now(), 480);
        assert_eq!(state, SessionState::Idle);
        assert_eq!(conf, Confidence::Reported);
    }

    #[test]
    fn decision_table_row_4_reported_running() {
        let inputs = StateInputs {
            reported: Some(SessionState::Running),
            origin: StateSource::ClaudeCli,
            ..base_inputs()
        };
        let (state, _, conf) = decide_state(&inputs, now(), 480);
        assert_eq!(state, SessionState::Running);
        assert_eq!(conf, Confidence::Reported);
    }

    #[test]
    fn decision_table_row_5_orphaned_beats_quiet_and_process() {
        let inputs = StateInputs {
            orphan: Some((StateSource::CmuxCli, Confidence::Derived)),
            last_activity_at: Some(now() - Duration::seconds(10_000)),
            activity_source: Some(StateSource::CmuxCli),
            has_process: true,
            origin: StateSource::CmuxCli,
            ..base_inputs()
        };
        let (state, source, conf) = decide_state(&inputs, now(), 480);
        assert_eq!(state, SessionState::Orphaned);
        assert_eq!(source, StateSource::CmuxCli);
        assert_eq!(conf, Confidence::Derived);
    }

    #[test]
    fn decision_table_row_6_quiet_when_threshold_exceeded() {
        let inputs = StateInputs {
            last_activity_at: Some(now() - Duration::seconds(481)),
            activity_source: Some(StateSource::KimiMetadata),
            has_process: true,
            origin: StateSource::KimiMetadata,
            ..base_inputs()
        };
        let (state, source, conf) = decide_state(&inputs, now(), 480);
        assert_eq!(state, SessionState::Quiet { elapsed_s: 481 });
        assert_eq!(source, StateSource::KimiMetadata);
        assert_eq!(conf, Confidence::Derived);
    }

    #[test]
    fn decision_table_row_7_process_only_is_unknown_inferred() {
        let inputs = StateInputs {
            has_process: true,
            origin: StateSource::ProcessTable,
            ..base_inputs()
        };
        let (state, source, conf) = decide_state(&inputs, now(), 480);
        assert_eq!(state, SessionState::Unknown);
        assert_eq!(source, StateSource::ProcessTable);
        assert_eq!(conf, Confidence::Inferred);
    }

    #[test]
    fn decision_table_row_8_no_materials_is_unknown() {
        let inputs = StateInputs {
            origin: StateSource::KimiMetadata,
            ..base_inputs()
        };
        let (state, source, conf) = decide_state(&inputs, now(), 480);
        assert_eq!(state, SessionState::Unknown);
        assert_eq!(source, StateSource::KimiMetadata);
        assert_eq!(conf, Confidence::Inferred);
    }

    #[test]
    fn waiting_never_occurs_without_cli_report() {
        // 反例テスト（詳細設計10節）: 報告なしの行がwaitingへ分類されないこと
        for last in [None, Some(now() - Duration::seconds(100_000))] {
            let inputs = StateInputs {
                last_activity_at: last,
                activity_source: last.map(|_| StateSource::CmuxCli),
                has_process: true,
                ..base_inputs()
            };
            let (state, _, _) = decide_state(&inputs, now(), 480);
            assert_ne!(state, SessionState::Waiting);
        }
    }

    #[test]
    fn quiet_boundary_exactly_threshold_is_not_quiet() {
        // T3時刻依存: 閾値ちょうどは超過ではない
        let mk = |secs: i64| StateInputs {
            last_activity_at: Some(now() - Duration::seconds(secs)),
            activity_source: Some(StateSource::CmuxCli),
            origin: StateSource::CmuxCli,
            ..base_inputs()
        };
        let (at, _, _) = decide_state(&mk(480), now(), 480);
        assert_eq!(at, SessionState::Unknown);
        let (under, _, _) = decide_state(&mk(479), now(), 480);
        assert_eq!(under, SessionState::Unknown);
        let (over, _, _) = decide_state(&mk(481), now(), 480);
        assert_eq!(over, SessionState::Quiet { elapsed_s: 481 });
    }

    #[test]
    fn quiet_ignores_future_timestamps() {
        // T3時刻依存: 未来時刻は経過0として扱いquietにしない
        let inputs = StateInputs {
            last_activity_at: Some(now() + Duration::seconds(3600)),
            activity_source: Some(StateSource::CmuxCli),
            origin: StateSource::CmuxCli,
            ..base_inputs()
        };
        let (state, _, _) = decide_state(&inputs, now(), 480);
        assert_eq!(state, SessionState::Unknown);
    }

    #[test]
    fn apply_state_decision_covers_reported_orphan_quiet_and_unknown_rows() {
        let stale = now() - Duration::seconds(1000);
        let mut orphan = cmux_row("surface:9", "ttys007", "t");
        orphan.state = SessionState::Orphaned;
        orphan.state_source = StateSource::CmuxCli;
        orphan.confidence = Confidence::Derived;
        let mut rows = vec![
            claude_row("aaa", "/a", SessionState::Failed),
            orphan,
            kimi_row("session_q", Some("/b"), Some(stale)),
            proc_row(42, AgentKind::Codex, None, None),
        ];
        apply_state_decision(&mut rows, now(), 480);
        assert_eq!(rows[0].state, SessionState::Failed);
        assert_eq!(rows[0].confidence, Confidence::Reported);
        assert_eq!(rows[1].state, SessionState::Orphaned);
        assert_eq!(rows[1].confidence, Confidence::Derived);
        assert_eq!(rows[2].state, SessionState::Quiet { elapsed_s: 1000 });
        assert_eq!(rows[2].state_source, StateSource::KimiMetadata);
        assert_eq!(rows[2].confidence, Confidence::Derived);
        assert_eq!(rows[3].state, SessionState::Unknown);
        assert_eq!(rows[3].state_source, StateSource::ProcessTable);
        assert_eq!(rows[3].confidence, Confidence::Inferred);
    }
}
