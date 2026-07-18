# 著者セルフレビュー記録

| 日付 | 対象 | 版数 | チェック結果 |
|---|---|---|---|
| 2026-07-19 | docs/design/agent-sessions-detailed-design.md | v1.1 | 実装フィードバック6件（バックログのv1.1候補5件＋シール時残件1件）の反映後に再点検。記載事実をsrc/data/agents/claude.rs（2スキーマ、id導出、statusのidle対応、pid両形）、merge.rs（pid照合が優先0、実行順はtty先行、cwd照合からpid保持行を除外、apply_activityのsession_id照合と接尾辞一致）、cmux.rs（--cursor-file、--limit 200、--no-heartbeat、seq 0シード、ackによるstale検出）と突き合わせて一致を確認。2節の内部属性3件がmodel.rsのserde(skip)属性と一致することを確認。4.2節のpid・session_id・source_kindの参照先（5節優先0、2節）の実在を確認。10節の鮮度テスト行に対応する実装テスト（app.rsの鮮度判定、ui/panels/agents.rsの停止表示）の実在を確認。変更履歴がv0.1〜v1.1の昇順であることを確認。バックログの反映6件を処理済みの記録へ移動。doclint 0/0/0/0 |
| 2026-07-18 | docs/design/agent-sessions-detailed-design.md | v0.4 | 3巡目残存4件（ユーザー承認済み）の修正後に再点検。8.4節の追記2行が基本設計10節の継続要件と7.2節のサーバー非起動規則の両方を参照して閉じていることを確認。「常駐」定義が4.1節・11節の否定表現と8.4節の背景タスクを両立させることを確認。変更履歴がv0.1〜v0.4の昇順であることを確認。doclint 0/0/0/0 |
| 2026-07-18 | docs/design/agent-sessions-detailed-design.md | v0.3 | 2巡目指摘8件の修正後に再点検。ttyの保持先（統一行モデルのみ）が1節・2節・4.4節で一貫することを確認。8.4節の「収集サイクル」定義が4.4節の参照と一致することを確認。8.3〜8.5の節番号繰り下げ後の参照（10節から8.3節への参照）の整合を確認。doclint 0/0/0/0 |
| 2026-07-18 | docs/design/agent-sessions-detailed-design.md | v0.2 | 1巡目指摘13件の修正後に再点検。基本設計7.3節「状態はtreeとeventsから得る」と4.1節の収集2コマンドの一致を確認。6節の到達可能性注記が7分類すべてを網羅していることを確認。2節tty追加と4.4節ps併用と5節照合キー1の三者が閉じていることを確認。7節の出力3フィールドと10節のテスト記述の整合を確認。doclint 0/0/0/0 |
| 2026-07-18 | docs/design/agent-sessions-detailed-design.md | v0.1 | 執筆前に事実を追加検証（cmux tree --jsonの実在と構造、claude agents --jsonのキーとstate実測値blocked/done/failed、Kimi state.jsonの8キー、main.rsがサブコマンドなし、Serialize導出なし、serde/serde_json/async-trait依存済み）。基本設計v1.0との突き合わせ（MVP取得元4種、状態7分類、断定回避、パス正規化、git手段の決定踏襲）を節ごとに確認。doclint 0/0/0/0 |
| 2026-07-18 | docs/design/agent-sessions-basic-design.md | v0.1 | 下表のとおり全10項目を確認済み |
| 2026-07-18 | docs/design/agent-sessions-basic-design.md | v0.3 | 2巡目指摘3件の修正後に再点検。2節の判定範囲が7.3節の取得元4種の列挙と一致することを確認。orphanedの用語（workspace、surface、pane）が26行目の用語対応と一致することを確認。パス正規化の一文が8節の照合と7.1節補足の両方から参照可能な位置にあることを確認。doclint 0/0/0/0 |
| 2026-07-18 | docs/design/agent-sessions-basic-design.md | v0.2 | 指摘9件の修正後に再点検。境界スペースはdoclint 0/0/0/0で確認。修正で追加した参照（7.1節の補足、3.2節、10節）の実在を確認。MVP境界は修正により2節、3.2節、6節、7.1節が「MVP＝cmux軸の第一歩、拡張は第2段階」で一貫。git照会の追加が決定表と7.1節補足と8節で同一結論であることを確認 |

## v0.1 のチェック内訳

| 項目 | 結果 |
|---|---|
| 1 境界スペース | doclint 0/0/0/0 で確認済み（合成違反での発火テストも実施済み） |
| 2 相互参照 | 文中のセクション参照（状態分類の節など）は実在を確認。コード参照（4パネル固定の3箇所）はevent.rs、layout.rs、app.rsで確認済み |
| 3 用語統一 | worktree、取得元、アダプタ、collectorの表記を統一 |
| 4 決定矛盾 | 本リポジトリにADRディレクトリは存在せず、矛盾対象なし |
| 5 状態機械 | 状態は分類であり遷移は定義していないため対象外 |
| 6 不変条件 | 対象外（本書はデータ保全仕様を持たない） |
| 7 契約伝播 | 対象外（DDLやAPI契約の変更なし） |
| 8 置換安全 | 機械置換は未実施 |
| 9 MVP境界 | 3.2の除外項目（tmux、dmux、DevFleet、app-server、5パネル化）が7.3、9、13で第2段階として一貫していることを確認 |
| 10 ルーブリック鮮度 | QUALITY_RUBRIC.md v1.0を本書のセクション構成（14項目）と同時に作成 |
