//! Kimi取得元アダプタ（詳細設計4.3節）。

use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};

use super::model::AgentSessionRow;
use super::AgentSource;

/// Kimiのセッションディレクトリ（2階層構造）を走査するアダプタ。
pub struct KimiAdapter {
    sessions_dir: PathBuf,
}

impl KimiAdapter {
    pub fn new(sessions_dir: PathBuf) -> Self {
        Self { sessions_dir }
    }
}

/// 既定のセッションディレクトリ（~/.kimi-code/sessions）。
pub fn default_sessions_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".kimi-code")
        .join("sessions")
}

/// sessionsディレクトリ配下を2階層走査して各session_ディレクトリのstate.jsonを読む。
///
/// 直下走査では0件になる（実測確認済み。T4回帰5）。読み取り失敗・JSONパース失敗・
/// キー欠落は行単位で握りつぶさず、該当セッションをunsupported相当の1行で返す。
pub fn scan_sessions(sessions_dir: &Path) -> Vec<AgentSessionRow> {
    let mut rows = Vec::new();
    let Ok(level1) = std::fs::read_dir(sessions_dir) else {
        return rows;
    };
    for wd_entry in level1.flatten() {
        if !wd_entry.path().is_dir() {
            continue;
        }
        let Ok(level2) = std::fs::read_dir(wd_entry.path()) else {
            continue;
        };
        for session_entry in level2.flatten() {
            let session_dir = session_entry.path();
            let session_id = session_entry.file_name().to_string_lossy().to_string();
            if !session_dir.is_dir() || !session_id.starts_with("session_") {
                continue;
            }
            let state_path = session_dir.join("state.json");
            if !state_path.exists() {
                continue;
            }
            let parsed = std::fs::read_to_string(&state_path)
                .map_err(anyhow::Error::from)
                .and_then(|body| parse_state_json(&session_id, &body));
            match parsed {
                Ok(row) => rows.push(row),
                Err(e) => rows.push(unsupported_row(&session_id, &e)),
            }
        }
    }
    rows
}

/// 読めない・解析できないセッションをunsupported相当の1行として可視化する
/// （詳細設計4.3節の保護。行単位で握りつぶさない）。
fn unsupported_row(session_id: &str, error: &anyhow::Error) -> AgentSessionRow {
    tracing::warn!(session_id, %error, "kimi state.json unreadable");
    let mut row = AgentSessionRow::new(
        session_id,
        super::model::AgentKind::Kimi,
        super::model::StateSource::KimiMetadata,
    );
    row.location = session_id.to_string();
    row.task_title = Some(format!("(unreadable state.json: {error})"));
    row
}

/// 単一のstate.json本文を統一行モデルへ解析する。idはsession_ディレクトリ名。
///
/// agentsは配列ではなくオブジェクトで、main型の自分自身を含む（実測確認済み。
/// T4回帰1）。child_agentsはtypeがmainの要素を除いた数とする。
pub fn parse_state_json(session_id: &str, body: &str) -> Result<AgentSessionRow> {
    use anyhow::Context;
    let root: serde_json::Value = serde_json::from_str(body).context("invalid JSON")?;
    let title = root
        .get("title")
        .and_then(|v| v.as_str())
        .context("missing key: title")?;
    let updated_at = root
        .get("updatedAt")
        .and_then(|v| v.as_str())
        .context("missing key: updatedAt")?;
    let agents = root
        .get("agents")
        .and_then(|v| v.as_object())
        .context("missing key: agents (object)")?;

    let mut row = AgentSessionRow::new(
        session_id,
        super::model::AgentKind::Kimi,
        super::model::StateSource::KimiMetadata,
    );
    row.location = session_id.to_string();
    row.task_title = Some(title.to_string());
    // 不正な時刻文字列は行を保ったままlast_activity_atだけ欠落させる
    row.last_activity_at = updated_at.parse::<chrono::DateTime<chrono::Utc>>().ok();
    row.activity_source = row
        .last_activity_at
        .map(|_| super::model::StateSource::KimiMetadata);
    row.cwd = root
        .get("workDir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    // agentsはオブジェクトで、main型の自分自身を含む（T4回帰1）。mainを除いた数。
    row.child_agents = Some(
        agents
            .values()
            .filter(|a| a.get("type").and_then(|t| t.as_str()) != Some("main"))
            .count(),
    );
    Ok(row)
}

#[async_trait]
impl AgentSource for KimiAdapter {
    fn name(&self) -> &'static str {
        "kimi"
    }

    async fn is_available(&self) -> bool {
        self.sessions_dir.is_dir()
    }

    async fn collect(&self) -> Result<Vec<AgentSessionRow>> {
        Ok(scan_sessions(&self.sessions_dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::agents::model::{AgentKind, Confidence, SessionState, StateSource};
    use chrono::{TimeZone, Utc};

    const WITH_WORKDIR: &str = include_str!("fixtures/kimi_state_with_workdir.json");
    const NO_WORKDIR: &str = include_str!("fixtures/kimi_state_no_workdir.json");
    const SUBAGENTS: &str = include_str!("fixtures/kimi_state_subagents.json");

    fn write_session(root: &Path, wd: &str, sid: &str, body: &str) {
        let dir = root.join(wd).join(sid);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("state.json"), body).unwrap();
    }

    #[test]
    fn parse_state_with_workdir_maps_attributes() {
        let row = parse_state_json("session_08280add", WITH_WORKDIR).unwrap();
        assert_eq!(row.id, "session_08280add");
        assert_eq!(row.agent, AgentKind::Kimi);
        assert_eq!(row.task_title.as_deref(), Some("New Session"));
        // workDir → cwd（存在する場合のみ）
        assert_eq!(
            row.cwd,
            Some(PathBuf::from("/Users/dev/devs/ruby/big_tree_table"))
        );
        // updatedAt → last_activity_at（2026-07-16T23:22:22.509Z）
        assert_eq!(
            row.last_activity_at,
            Some(
                Utc.with_ymd_and_hms(2026, 7, 16, 23, 22, 22).unwrap()
                    + chrono::Duration::milliseconds(509)
            )
        );
        assert_eq!(row.activity_source, Some(StateSource::KimiMetadata));
        // main型のみ → child_agentsは0
        assert_eq!(row.child_agents, Some(0));
        // メタデータのみでは実行状態を報告できない（状態は決定表に委ねる）
        assert_eq!(row.state, SessionState::Unknown);
        assert_ne!(row.confidence, Confidence::Reported);
        assert_eq!(row.state_source, StateSource::KimiMetadata);
    }

    #[test]
    fn parse_state_without_workdir_leaves_cwd_none() {
        let row = parse_state_json("session_03e14dbb", NO_WORKDIR).unwrap();
        assert!(row.cwd.is_none());
        // 絵文字と日本語を含むタイトル（T3文字種）
        assert_eq!(
            row.task_title.as_deref(),
            Some("kimiコマンドの動作確認をお願いします🚀")
        );
    }

    #[test]
    fn regression_kimi_agents_object_excludes_main_from_child_count() {
        // T4回帰1: agentsは配列ではなくオブジェクトで、main型の自分自身を含む。
        let row = parse_state_json("session_ac3c0715", SUBAGENTS).unwrap();
        // フィクスチャはmain 1 + sub 7の計8要素。mainを除いた7がchild_agents。
        assert_eq!(row.child_agents, Some(7));
    }

    #[test]
    fn parse_state_with_invalid_updated_at_keeps_row_without_activity() {
        // T3: 不正な時刻文字列。行は残しlast_activity_atだけ欠落させる。
        let body = r#"{"createdAt":"x","updatedAt":"not-a-time","title":"t","isCustomTitle":false,"agents":{"main":{"type":"main"}},"custom":{}}"#;
        let row = parse_state_json("session_x", body).unwrap();
        assert!(row.last_activity_at.is_none());
        assert_eq!(row.task_title.as_deref(), Some("t"));
    }

    #[test]
    fn parse_state_rejects_missing_required_keys() {
        // titleとagentsを欠く
        assert!(parse_state_json("s", r#"{"updatedAt":"2026-01-01T00:00:00Z"}"#).is_err());
        assert!(parse_state_json("s", "{ broken").is_err());
    }

    #[test]
    fn regression_kimi_sessions_live_two_levels_deep() {
        // T4回帰5: セッションはsessions直下ではなく2階層目にある。
        let tmp = tempfile::tempdir().unwrap();
        write_session(tmp.path(), "wd_proj_abc123", "session_aaa", WITH_WORKDIR);
        write_session(tmp.path(), "wd_proj_abc123", "session_bbb", NO_WORKDIR);
        write_session(tmp.path(), "wd_other_def456", "session_ccc", SUBAGENTS);
        // 直下に置いたstate.jsonは対象外（1階層目はディレクトリのみ走査）
        std::fs::write(tmp.path().join("state.json"), WITH_WORKDIR).unwrap();
        let mut rows = scan_sessions(tmp.path());
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["session_aaa", "session_bbb", "session_ccc"]);
    }

    #[test]
    fn scan_reports_unreadable_state_json_as_unsupported_row() {
        // T1: 不正JSON。他セッションは正常に残り、壊れたものは1行で可視化する。
        let tmp = tempfile::tempdir().unwrap();
        write_session(tmp.path(), "wd_a_1", "session_good", WITH_WORKDIR);
        write_session(tmp.path(), "wd_a_1", "session_broken", "{ not json");
        let rows = scan_sessions(tmp.path());
        assert_eq!(rows.len(), 2);
        let broken = rows.iter().find(|r| r.id == "session_broken").unwrap();
        assert_eq!(broken.state, SessionState::Unknown);
        assert_eq!(broken.agent, AgentKind::Kimi);
        assert!(broken.task_title.as_deref().unwrap().contains("unreadable"));
        let good = rows.iter().find(|r| r.id == "session_good").unwrap();
        assert_eq!(good.task_title.as_deref(), Some("New Session"));
    }

    #[cfg(unix)]
    #[test]
    fn scan_reports_permission_denied_state_json_as_unsupported_row() {
        use std::os::unix::fs::PermissionsExt;
        // T3: 権限（読み取り不能ファイル）。rootで走ると常に読めるためスキップする。
        if nix_is_root() {
            eprintln!("SKIP: running as root; permission test skipped");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        write_session(tmp.path(), "wd_a_1", "session_locked", WITH_WORKDIR);
        let file = tmp.path().join("wd_a_1/session_locked/state.json");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
        let rows = scan_sessions(tmp.path());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0]
            .task_title
            .as_deref()
            .unwrap()
            .contains("unreadable"));
    }

    #[cfg(unix)]
    fn nix_is_root() -> bool {
        // SAFETY不要: 標準コマンドで判定する
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
            .unwrap_or(false)
    }

    #[test]
    fn scan_returns_empty_for_empty_or_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(scan_sessions(tmp.path()).is_empty());
        assert!(scan_sessions(&tmp.path().join("does-not-exist")).is_empty());
    }

    #[test]
    fn default_sessions_dir_ends_with_kimi_code_sessions() {
        // 既定値が~/.kimi-code/sessions配下を指す（空パス化の防止）
        assert!(default_sessions_dir().ends_with(".kimi-code/sessions"));
    }

    #[test]
    fn scan_skips_directories_not_named_session_even_with_state_json() {
        // session_接頭辞を持たないディレクトリはstate.jsonがあっても対象外
        let tmp = tempfile::tempdir().unwrap();
        write_session(tmp.path(), "wd_a_1", "not_a_session", WITH_WORKDIR);
        assert!(scan_sessions(tmp.path()).is_empty());
    }

    #[tokio::test]
    async fn adapter_collect_returns_rows_from_fixture_directory_structure() {
        // collectが実フィクスチャのディレクトリ構造を走査して行を返す（空固定の防止）
        let tmp = tempfile::tempdir().unwrap();
        write_session(tmp.path(), "wd_proj_abc123", "session_aaa", WITH_WORKDIR);
        write_session(tmp.path(), "wd_other_def456", "session_bbb", SUBAGENTS);
        let adapter = KimiAdapter::new(tmp.path().to_path_buf());
        let mut rows = adapter.collect().await.unwrap();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "session_aaa");
        assert_eq!(rows[0].task_title.as_deref(), Some("New Session"));
        assert_eq!(rows[1].id, "session_bbb");
        assert_eq!(rows[1].child_agents, Some(7));
    }

    #[tokio::test]
    async fn adapter_is_available_only_when_dir_exists() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(
            KimiAdapter::new(tmp.path().to_path_buf())
                .is_available()
                .await
        );
        assert!(
            !KimiAdapter::new(tmp.path().join("nope"))
                .is_available()
                .await
        );
    }
}
