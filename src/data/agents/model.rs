//! 統一行モデルと状態の型定義（詳細設計2節）。

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::PathBuf;

/// エージェントの種別。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
    Kimi,
    Other(String),
}

impl AgentKind {
    /// JSON契約上の文字列表現。Otherは中身の文字列そのもの。
    pub fn as_str(&self) -> &str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Kimi => "kimi",
            AgentKind::Other(s) => s,
        }
    }
}

// agentフィールドは常にフラット文字列で直列化する（deriveだとOtherが
// {"other":"..."}のオブジェクトになりJSON契約を壊すため手書きする）。
impl Serialize for AgentKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for AgentKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "claude" => AgentKind::Claude,
            "codex" => AgentKind::Codex,
            "kimi" => AgentKind::Kimi,
            _ => AgentKind::Other(s),
        })
    }
}

/// セッションを実行しているオーケストレータ。MVPで生成するのはCmuxとUnknownのみ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Orchestrator {
    Cmux,
    Tmux,
    Dmux,
    DevFleet,
    InProcess,
    Unknown,
}

/// 基本設計6節の7分類。Quietは経過秒を保持する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Running,
    Waiting,
    Idle,
    Failed,
    Orphaned,
    Quiet { elapsed_s: u64 },
    Unknown,
}

/// 状態の根拠となり得る取得元。git照会は状態根拠にならないため含めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StateSource {
    CmuxCli,
    ClaudeCli,
    KimiMetadata,
    ProcessTable,
}

/// 状態の確度。属性の出所ではなく状態の確度を表す（詳細設計5節）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Reported,
    Derived,
    Inferred,
}

/// 統一行モデル（詳細設計2節）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentSessionRow {
    pub id: String,
    pub agent: AgentKind,
    pub orchestrator: Orchestrator,
    pub location: String,
    pub cwd: Option<PathBuf>,
    pub worktree: Option<PathBuf>,
    pub git_common_dir: Option<PathBuf>,
    pub task_title: Option<String>,
    pub state: SessionState,
    pub state_source: StateSource,
    pub confidence: Confidence,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub cpu_percent: Option<f32>,
    pub memory_bytes: Option<u64>,
    pub child_agents: Option<usize>,
    pub pid: Option<u32>,
    pub tty: Option<String>,
    /// last_activity_atを与えた取得元（quiet判定のstate_source決定に使う内部属性）。
    #[serde(skip)]
    pub activity_source: Option<StateSource>,
    /// 公開CLIが報告したセッション識別子（cmux eventsのsession_id照合に使う内部属性）。
    #[serde(skip)]
    pub session_id: Option<String>,
    /// 取得元CLIが報告したセッション種別（例: claudeのbackground/interactive）。
    /// 表示には使わない内部属性。
    #[serde(skip)]
    pub source_kind: Option<String>,
}

impl AgentSessionRow {
    /// 取得元アダプタが共通既定値から行を組み立てるためのコンストラクタ。
    pub fn new(id: impl Into<String>, agent: AgentKind, source: StateSource) -> Self {
        Self {
            id: id.into(),
            agent,
            orchestrator: Orchestrator::Unknown,
            location: String::new(),
            cwd: None,
            worktree: None,
            git_common_dir: None,
            task_title: None,
            state: SessionState::Unknown,
            state_source: source,
            confidence: Confidence::Inferred,
            last_activity_at: None,
            cpu_percent: None,
            memory_bytes: None,
            child_agents: None,
            pid: None,
            tty: None,
            activity_source: None,
            session_id: None,
            source_kind: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_row() -> AgentSessionRow {
        AgentSessionRow {
            id: "surface:1".into(),
            agent: AgentKind::Claude,
            orchestrator: Orchestrator::Cmux,
            location: "workspace:1/surface:1".into(),
            cwd: Some(PathBuf::from("/Users/dev/devs/zenn")),
            worktree: Some(PathBuf::from("/Users/dev/devs/zenn")),
            git_common_dir: Some(PathBuf::from("/Users/dev/devs/zenn/.git")),
            task_title: Some("⠐ AI開発ツールの改善案を整理".into()),
            state: SessionState::Quiet { elapsed_s: 512 },
            state_source: StateSource::CmuxCli,
            confidence: Confidence::Derived,
            last_activity_at: Some(Utc.with_ymd_and_hms(2026, 7, 18, 7, 58, 32).unwrap()),
            cpu_percent: Some(1.5),
            memory_bytes: Some(120_000_000),
            child_agents: None,
            pid: Some(91658),
            tty: Some("ttys000".into()),
            activity_source: Some(StateSource::CmuxCli),
            session_id: None,
            source_kind: None,
        }
    }

    #[test]
    fn serializes_with_section2_field_names() {
        let v = serde_json::to_value(sample_row()).unwrap();
        let obj = v.as_object().unwrap();
        for key in [
            "id",
            "agent",
            "orchestrator",
            "location",
            "cwd",
            "worktree",
            "git_common_dir",
            "task_title",
            "state",
            "state_source",
            "confidence",
            "last_activity_at",
            "cpu_percent",
            "memory_bytes",
            "child_agents",
            "pid",
            "tty",
        ] {
            assert!(obj.contains_key(key), "missing field {key}");
        }
        // 内部属性activity_sourceは直列化しない
        assert!(!obj.contains_key("activity_source"));
        // 値の中身まで確認する（T5）
        assert_eq!(obj["id"], "surface:1");
        assert_eq!(obj["agent"], "claude");
        assert_eq!(obj["orchestrator"], "cmux");
        assert_eq!(obj["cwd"], "/Users/dev/devs/zenn");
        assert_eq!(obj["state"]["quiet"]["elapsed_s"], 512);
        assert_eq!(obj["state_source"], "cmux_cli");
        assert_eq!(obj["confidence"], "derived");
        assert_eq!(obj["pid"], 91658);
        assert_eq!(obj["tty"], "ttys000");
    }

    #[test]
    fn agent_kind_serializes_every_variant_as_plain_string() {
        // JSON契約: agentフィールドは常にフラット文字列（Otherもオブジェクトにしない）
        for (kind, expected) in [
            (AgentKind::Claude, "claude"),
            (AgentKind::Codex, "codex"),
            (AgentKind::Kimi, "kimi"),
            (AgentKind::Other("foo".into()), "foo"),
        ] {
            assert_eq!(
                serde_json::to_value(&kind).unwrap(),
                serde_json::json!(expected)
            );
        }
    }

    #[test]
    fn agent_kind_round_trips_through_json() {
        for kind in [
            AgentKind::Claude,
            AgentKind::Codex,
            AgentKind::Kimi,
            AgentKind::Other("unknown".into()),
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: AgentKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn new_row_defaults_are_unknown_and_empty() {
        let row = AgentSessionRow::new("42", AgentKind::Kimi, StateSource::KimiMetadata);
        assert_eq!(row.id, "42");
        assert_eq!(row.agent, AgentKind::Kimi);
        assert_eq!(row.orchestrator, Orchestrator::Unknown);
        assert_eq!(row.state, SessionState::Unknown);
        assert_eq!(row.state_source, StateSource::KimiMetadata);
        assert_eq!(row.confidence, Confidence::Inferred);
        assert!(row.cwd.is_none());
        assert!(row.pid.is_none());
        assert!(row.tty.is_none());
    }
}
