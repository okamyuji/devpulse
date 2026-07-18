# Mutation Testing Baseline — src/data/agents

## 実行記録

- 実行日: 2026-07-18
- コマンド: `cargo mutants --file 'src/data/agents/*' --no-times`
- ツール: cargo-mutants 27.1.0

## 最終結果（基準値）

| 項目 | 件数 |
|---|---|
| tested | 118 |
| caught | 92 |
| missed | 1 |
| unviable | 25 |
| timeout | 0 |

- スコア: caught / (caught + missed) = 92 / 93 = **98.9%**
- 初回実行は missed 15 件。テスト追加（11件）により 14 件を caught 化した。

## 運用ルール

この結果を基準値（ベースライン）とする。以後の判定は「悪化したか」で行う。

- missed が 1 件（下記の等価変異のみ）を超えたら悪化。原因の変異を殺すテストを追加してから取り込む。
- スコアが 98.9% を下回ったら悪化として扱う。
- 新規コード追加で分母が増えるのは正常。missed に新顔が現れたときのみ対処する。

## 殺さずに残した変異とその理由

| 変異 | 理由 |
|---|---|
| `src/data/agents/mod.rs:126:9: replace AgentSource::activity -> Vec<ActivityEvent> with vec![]` | 等価変異。トレイト既定実装の本体がすでに `Vec::new()`（空Vec）を返しており、`vec![]` への置換は同一動作。テストで区別できず、殺す価値がない。 |
