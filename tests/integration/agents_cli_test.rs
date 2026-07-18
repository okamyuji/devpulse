//! collectorサブコマンド `devpulse agents --json` の統合テスト（詳細設計7節）。

use assert_cmd::Command;
use predicates::prelude::*;

/// 統一行モデルのJSONフィールド名（詳細設計2節の属性名。activity_sourceは内部属性でskip）。
const SESSION_FIELDS: [&str; 17] = [
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
];

fn devpulse() -> Command {
    Command::cargo_bin("devpulse").unwrap()
}

fn assert_session_object_shape(session: &serde_json::Value) {
    let obj = session.as_object().expect("session row must be an object");
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut expected = SESSION_FIELDS.to_vec();
    expected.sort_unstable();
    assert_eq!(keys, expected, "session fields must match design section 2");
    assert!(session["id"].is_string());
    assert!(session["location"].is_string());
    // stateは単純分類なら文字列、quietは経過秒付きオブジェクト（詳細設計2節）
    let state = &session["state"];
    assert!(
        state.is_string() || state["quiet"]["elapsed_s"].is_u64(),
        "unexpected state shape: {state}"
    );
}

/// (a) 3フィールドの有効JSONを出力し終了コード0
#[test]
fn agents_json_outputs_three_fields_and_exits_zero() {
    let output = devpulse().args(["agents", "--json"]).output().unwrap();
    assert!(output.status.success(), "expected exit 0: {output:?}");
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON");
    let obj = json.as_object().expect("top level must be an object");
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["schema_version", "sessions", "source_errors"]);
    assert_eq!(json["schema_version"], 1);
    assert!(json["sessions"].is_array());
    assert!(json["source_errors"].is_array());
    for err in json["source_errors"].as_array().unwrap() {
        let err_obj = err.as_object().expect("source_error must be an object");
        let mut err_keys: Vec<&str> = err_obj.keys().map(String::as_str).collect();
        err_keys.sort_unstable();
        assert_eq!(err_keys, ["error", "source"]);
        assert!(!err["source"].as_str().unwrap().is_empty());
        assert!(!err["error"].as_str().unwrap().is_empty());
    }
}

/// (b) sessionsのフィールド名が詳細設計2節の属性名（snake_case）と一致（実環境出力）
#[test]
fn agents_json_session_rows_have_design_section2_fields() {
    let output = devpulse().args(["agents", "--json"]).output().unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    for session in json["sessions"].as_array().unwrap() {
        assert_session_object_shape(session);
    }
}

/// (b) 決定的検証: 統一行モデルの直列化キーを実環境に依存せず固定する
#[test]
fn agent_session_row_serializes_design_section2_field_names() {
    use devpulse::data::agents::model::{AgentKind, AgentSessionRow, SessionState, StateSource};
    let mut row = AgentSessionRow::new("s1", AgentKind::Claude, StateSource::ClaudeCli);
    row.state = SessionState::Quiet { elapsed_s: 481 };
    let value = serde_json::to_value(&row).unwrap();
    assert_session_object_shape(&value);
    assert_eq!(value["id"], "s1");
    assert_eq!(value["agent"], "claude");
    assert_eq!(value["state"]["quiet"]["elapsed_s"], 481);
    assert_eq!(value["state_source"], "claude_cli");
}

/// (c') 契約は`devpulse agents --json`。フラグ無しは非0終了しstdoutは汚さない
#[test]
fn agents_without_json_flag_exits_nonzero_with_clean_stdout() {
    let output = devpulse().arg("agents").output().unwrap();
    assert!(
        !output.status.success(),
        "expected nonzero exit: {output:?}"
    );
    assert!(
        output.stdout.is_empty(),
        "stdout must stay clean: {output:?}"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("--json"));
}

/// (c) 引数不正は非0終了
#[test]
fn agents_unknown_flag_exits_nonzero() {
    devpulse()
        .args(["agents", "--unknown-flag"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--unknown-flag"));
}

/// (d) 既存起動パスの回帰: --show-config は従来どおり動き、[agents]も含む
#[test]
fn show_config_still_works_and_includes_agents_section() {
    devpulse()
        .arg("--show-config")
        .assert()
        .success()
        .stdout(predicate::str::contains("[logs]"))
        .stdout(predicate::str::contains("[agents]"))
        .stdout(predicate::str::contains("quiet_threshold_s"));
}

/// (e) 全取得元失敗環境（PATH空・HOME退避）でも0終了し、source_errorsへ失敗が記録される
#[test]
fn agents_json_with_all_cli_sources_failed_exits_zero() {
    let empty_home = tempfile::tempdir().unwrap();
    let output = devpulse()
        .args(["agents", "--json"])
        .env("PATH", "")
        .env("HOME", empty_home.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "expected exit 0: {output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("stdout must be valid JSON even when all sources fail");
    assert_eq!(json["schema_version"], 1);
    let sources: Vec<&str> = json["source_errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["source"].as_str().unwrap())
        .collect();
    // CLI系3取得元とpsはPATH空/HOME退避で必ず失敗する
    for expected in ["cmux", "claude", "kimi", "ps"] {
        assert!(
            sources.contains(&expected),
            "missing {expected} in {sources:?}"
        );
    }
    // CLI取得元が全滅している以上、残る行はプロセス走査由来の推定行のみ
    for session in json["sessions"].as_array().unwrap() {
        assert_session_object_shape(session);
        assert_eq!(
            session["confidence"], "inferred",
            "with all CLI sources failed every row must be process-inferred: {session}"
        );
        assert_eq!(session["state_source"], "process_table");
    }
}
