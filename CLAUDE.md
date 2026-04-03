# DevPulse

Unified Developer Environment TUI — ポート・Docker・プロセス・ログを1画面に統合。

## Tech Stack

- Rust (edition 2021), ratatui + crossterm (TUI), tokio (async), bollard (Docker API)
- sysinfo (process), notify (file watch), clap (CLI), serde + toml (config)

## Commands

```bash
cargo build                    # ビルド
cargo test                     # 全テスト実行
cargo test --lib               # ユニットテストのみ
cargo test --test integration  # 統合テストのみ
cargo run                      # 起動
cargo clippy -- -D warnings    # lint
cargo fmt --check              # フォーマットチェック
```

## Architecture

```
src/
  main.rs           — エントリポイント、CLI引数パース、Appループ起動
  app.rs            — Appステートマシン、イベントループ
  config.rs         — TOML設定読み込み・デフォルト値
  event.rs          — キーイベント・tickイベントのディスパッチ
  ui/
    layout.rs       — パネルレイアウト計算
    panels/         — 各パネルの描画ロジック (ports, docker, processes, logs)
    common.rs       — パネル共通UI部品 (テーブル, フィルタバー, 確認ダイアログ)
  data/
    ports.rs        — ポート情報取得 (trait PortScanner)
    docker.rs       — Docker情報取得 (bollard)
    processes.rs    — プロセス情報取得 (sysinfo)
    logs.rs         — ログ収集 (Docker + ファイル)
  action.rs         — ユーザー操作 (kill, stop, restart, delete)
```

## Conventions

- テストファースト: 実装前にテストを書く
- エラー処理: `anyhow::Result` for application, `thiserror` for library errors
- 各データソースは trait で抽象化し、テストではモックを使用
- パネル描画は `Widget` trait 実装で統一
- コミットは小さく頻繁に。接頭辞: feat/fix/test/refactor/docs

## Key Docs

- `CONCEPT.md` — プロダクト仕様・機能ロジックツリー
- `docs/implementation-plan.md` — TDD実装計画（タスク分解・コード付き）
