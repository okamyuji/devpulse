//! プロセス情報フォールバックアダプタ（詳細設計4.4節）。

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

use crate::data::processes::ProcessInfo;

use super::model::{AgentKind, AgentSessionRow};

/// pid→制御端末名の突き合わせを提供する（1収集サイクルにつき1回実行する）。
#[async_trait]
pub trait TtyProvider: Send + Sync {
    async fn tty_by_pid(&self) -> Result<HashMap<u32, String>>;
}

/// psコマンド（pidとttyの列指定出力）で制御端末を取得する実装。
/// sysinfo 0.38系に制御端末の取得APIが存在しないため（詳細設計11節）。
pub struct PsTtyProvider {
    pub timeout_ms: u64,
}

#[async_trait]
impl TtyProvider for PsTtyProvider {
    async fn tty_by_pid(&self) -> Result<HashMap<u32, String>> {
        use super::CommandRunner;
        let out = super::SystemCommandRunner
            .run("ps", &["-axo", "pid=,tty="], self.timeout_ms)
            .await?;
        if out.timed_out {
            anyhow::bail!("ps timed out after {}ms", self.timeout_ms);
        }
        Ok(parse_ps_tty(&out.stdout))
    }
}

/// ps -axo pid=,tty= の出力を解析する。制御端末を持たない行（??）は除外する。
pub fn parse_ps_tty(output: &str) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        let (Some(pid_s), Some(tty)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Ok(pid) = pid_s.parse::<u32>() else {
            continue;
        };
        if tty.contains('?') {
            continue; // 制御端末なし
        }
        map.insert(pid, tty.to_string());
    }
    map
}

/// プロセス名またはコマンドラインからエージェント種別を判定する。
/// 対象はclaude、codex、kimi（詳細設計4.4節）。
pub fn detect_agent_kind(name: &str, command: &str) -> Option<AgentKind> {
    fn kind_of(token: &str) -> Option<AgentKind> {
        match token {
            "claude" => Some(AgentKind::Claude),
            "codex" => Some(AgentKind::Codex),
            "kimi" => Some(AgentKind::Kimi),
            _ => None,
        }
    }
    // プロセス名そのもの、またはコマンドライン各トークンのbasenameが
    // エージェント名に完全一致する場合のみ（部分文字列の誤検出を避ける）
    if let Some(kind) = kind_of(&name.to_lowercase()) {
        return Some(kind);
    }
    command.split_whitespace().find_map(|token| {
        let base = token.rsplit('/').next().unwrap_or(token).to_lowercase();
        kind_of(&base)
    })
}

/// エージェントプロセスを統一行モデルの行にする。
/// orchestrator=Unknown、confidence=Inferredのフォールバック行を生成する。
pub fn build_rows(
    processes: &[ProcessInfo],
    tty_by_pid: &HashMap<u32, String>,
) -> Vec<AgentSessionRow> {
    processes
        .iter()
        .filter_map(|p| {
            let kind = detect_agent_kind(&p.name, &p.command)?;
            let mut row = AgentSessionRow::new(
                p.pid.to_string(),
                kind,
                super::model::StateSource::ProcessTable,
            );
            row.location = format!("pid:{}", p.pid);
            row.pid = Some(p.pid);
            row.cwd = p.cwd.clone();
            row.tty = tty_by_pid.get(&p.pid).cloned();
            row.cpu_percent = Some(p.cpu_percent);
            row.memory_bytes = Some(p.memory_bytes);
            Some(row)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::agents::model::{Confidence, Orchestrator, SessionState, StateSource};
    use std::path::PathBuf;

    fn proc(pid: u32, name: &str, command: &str, cwd: Option<&str>) -> ProcessInfo {
        ProcessInfo {
            pid,
            name: name.into(),
            command: command.into(),
            user: "dev".into(),
            cpu_percent: 3.5,
            memory_bytes: 200_000_000,
            threads: 4,
            parent_pid: Some(1),
            listening_ports: vec![],
            start_time: 0,
            cwd: cwd.map(PathBuf::from),
        }
    }

    #[test]
    fn parse_ps_tty_maps_pid_to_tty_and_drops_headerless_question_marks() {
        // 実出力の形（pid右寄せ、tty列、制御端末なしは??）
        let out =
            " 78610 ttys000 \n 91479 ttys000 \n 92001 ttys003 \n     1 ??      \n   103 ??      \n";
        let map = parse_ps_tty(out);
        assert_eq!(map.len(), 3);
        assert_eq!(map.get(&78610).map(String::as_str), Some("ttys000"));
        assert_eq!(map.get(&92001).map(String::as_str), Some("ttys003"));
        assert!(!map.contains_key(&1));
    }

    #[test]
    fn parse_ps_tty_ignores_garbage_lines() {
        let map = parse_ps_tty("garbage\n abc ttys000\n 42 ttys001\n");
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&42).map(String::as_str), Some("ttys001"));
    }

    #[test]
    fn detect_agent_kind_matches_claude_codex_kimi_only() {
        assert_eq!(
            detect_agent_kind("claude", "claude --resume"),
            Some(AgentKind::Claude)
        );
        assert_eq!(detect_agent_kind("codex", "codex"), Some(AgentKind::Codex));
        assert_eq!(
            detect_agent_kind("kimi", "/usr/local/bin/kimi chat"),
            Some(AgentKind::Kimi)
        );
        // コマンドライン側からの判定（nodeで起動されたclaude）
        assert_eq!(
            detect_agent_kind("node", "node /Users/dev/.local/bin/claude --continue"),
            Some(AgentKind::Claude)
        );
        assert_eq!(detect_agent_kind("node", "node server.js"), None);
        assert_eq!(detect_agent_kind("zsh", "-zsh"), None);
        // 部分文字列の誤検出をしない（claudia等）
        assert_eq!(detect_agent_kind("claudia", "claudia run"), None);
    }

    #[test]
    fn build_rows_creates_fallback_rows_with_tty_join() {
        let procs = vec![
            proc(91658, "claude", "claude", Some("/Users/dev/devs/zenn")),
            proc(92001, "kimi", "kimi", None),
            proc(500, "node", "node server.js", Some("/tmp")),
        ];
        let ttys = HashMap::from([(91658, "ttys000".to_string())]);
        let rows = build_rows(&procs, &ttys);
        // エージェントに該当しないnodeは行にならない
        assert_eq!(rows.len(), 2);
        let claude = rows.iter().find(|r| r.pid == Some(91658)).unwrap();
        assert_eq!(claude.id, "91658");
        assert_eq!(claude.agent, AgentKind::Claude);
        assert_eq!(claude.orchestrator, Orchestrator::Unknown);
        assert_eq!(claude.location, "pid:91658");
        assert_eq!(claude.cwd, Some(PathBuf::from("/Users/dev/devs/zenn")));
        assert_eq!(claude.tty.as_deref(), Some("ttys000"));
        assert_eq!(claude.cpu_percent, Some(3.5));
        assert_eq!(claude.memory_bytes, Some(200_000_000));
        assert_eq!(claude.state, SessionState::Unknown);
        assert_eq!(claude.state_source, StateSource::ProcessTable);
        assert_eq!(claude.confidence, Confidence::Inferred);
        // psに載らないpidはtty欠落のまま
        let kimi = rows.iter().find(|r| r.pid == Some(92001)).unwrap();
        assert!(kimi.tty.is_none());
        assert!(kimi.cwd.is_none());
    }

    #[test]
    fn build_rows_empty_input_yields_empty_output() {
        assert!(build_rows(&[], &HashMap::new()).is_empty());
    }

    #[tokio::test]
    async fn real_ps_provider_returns_tty_map() {
        // T7: psは実コマンドを実行して検査する
        let provider = PsTtyProvider { timeout_ms: 3000 };
        let map = provider.tty_by_pid().await.unwrap();
        // 自プロセスもしくは何らかのプロセスが載る。tty名の形式を確認する。
        for tty in map.values() {
            assert!(!tty.contains('?'), "?? must be excluded, got {tty}");
            assert!(!tty.trim().is_empty());
        }
    }
}
