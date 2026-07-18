# エージェントセッションパネル 手動動作検証手順

| 項目 | 内容 |
|---|---|
| 状態 | 確定版v2（ロギング実装を反映。各手順にログでの確認方法を併記） |
| 対象 | feat/agent-sessions-panel ブランチ |
| 実施者 | 別ターミナルでの手動検証 |

## 事前準備

1. ビルドします。

```bash
cd ~/devs/rust/devpulse
cargo build --release
```

2. 検証に使うエージェントを1つ以上動かしておきます。cmuxの別workspaceでClaude Codeを起動した状態が最も検証項目を多くカバーします。

3. ログ監視を用意します。検証は目視だけに頼らず、各手順の結果をログでも確認します。別ペインで次を実行してください。

```bash
tail -f ~/.local/share/devpulse/devpulse.log
```

devpulseの起動時は詳細ログを有効にします。

```bash
export DEVPULSE_LOG=debug
```

ログはこのファイルだけに書かれ、TUI画面やJSON出力には混ざりません。レベルは`DEVPULSE_LOG`のEnvFilter構文で制御でき（例: `devpulse::data::agents=debug`）、未設定時はinfoです。ログには件数・取得元名・パス・状態・キー名のみが記録され、タスク題名や会話内容は記録されません。NGが出た場合は該当時刻のログ行をそのまま報告に貼ってください。それが再現調査の起点になります。なおログのパスにはホームディレクトリ名やリポジトリ名が含まれます。このローカル検証ではローカルのセッションへそのまま貼って問題ありませんが、ローカルマシンの外へログ行を共有する場合は、ホームディレクトリ・ユーザー名・リポジトリ名などの識別情報を伏せてから貼ってください。

## 手順1: collectorの単体確認（TUIより先）

```bash
DEVPULSE_LOG=debug ./target/release/devpulse agents --json | python3 -m json.tool | head -60
```

tail側では次のログ行が出ることを確認します。

```text
INFO devpulse: agents collector run started
INFO devpulse::data::agents: agent collect cycle started
DEBUG devpulse::data::agents::merge: merge completed tty_matched=N cwd_matched=N unmerged=N
INFO devpulse::data::agents: agent collect cycle completed elapsed_ms=… rows=cmux=…,claude=…,kimi=…,process=… total_sessions=…
INFO devpulse: agents collector run finished sessions=… source_errors=0
```

確認する点は次のとおりです。

- トップレベルにschema_version、sessions、source_errorsの3フィールドがあること
- sessionsに、いま動かしているエージェント（このClaude Codeセッション自身を含む）が現れること
- cmux内で動いているセッションのlocationにworkspace参照とsurface参照が入っていること
- worktreeとgit_common_dirが、gitリポジトリ内で動いているセッションに絶対パスで入っていること
- state_sourceとconfidenceが全行に入っていること

## 手順2: TUIのビュー切り替え

```bash
./target/release/devpulse
```

1. Tabキーまたは3キーでProcessesパネルへフォーカスを移します
2. aキーを押します → Processesビューがエージェントセッションビューに切り替わること
3. もう一度aキー → Processesビューに戻ること
4. 表示列がSTATE、AGENT、TASK、LOCATION、WORKTREE、QUIET、CPU、MEMであること

期待されるログ行: 切り替えのたびに `INFO … view switched`（view=agent_sessionsまたはprocesses）が1行ずつ出ること。またTUI起動中は`agents.refresh_ms`（既定5秒）ごとに `agent collect cycle completed` が繰り返し出ること。

## 手順3: 状態表示の確認

- 作業中のClaude Codeセッションの行が存在し、stateとconfidenceの組が表示されていること
- `claude agents --json --all`で現在failedのセッションがある場合、failed行が表示されること
- 何も操作していないセッションのQUIET列が経過時間表示になっていること（既定は480秒超で表示）

## 手順4: worktree競合警告

別ターミナルで同一リポジトリのworktreeを2つ作り、それぞれでエージェントを起動します。

```bash
cd <検証用リポジトリ>
git worktree add /tmp/wt-a HEAD --detach
git worktree add /tmp/wt-b HEAD --detach
# それぞれのディレクトリで claude を起動
```

- 同一git_common_dirを持つ2行に警告記号「⚠」が付くこと
- 検証後は`git worktree remove`で片付けます

## 手順5: 読み取り専用の確認

エージェントセッションビューを表示した状態で、Processesビューの操作系キー（Kのkillなど）を押します。

- 何も実行されないこと（プロセスがkillされないこと）

期待されるログ行: K押下のたびに無効化を示すinfo行（押されたキー名を含む）が出ること。kill実行のActionに関するログや確認ダイアログが出ないこと。

## 手順6: 縮退の確認

- cmuxの外（plain Terminal）でエージェントを起動した場合、orchestratorがUnknownの行として現れること
- 取得元の一部が使えない環境（例: PATHからclaudeを外して起動）でも、DevPulse全体が起動し、パネル下部に取得不可の取得元が表示されること

期待されるログ行: 使えない取得元は毎周期の`agent collect cycle completed`のsource_errorsサマリに現れること。アダプタのcollect失敗時はwarn行が理由つきで出ること。収集が止まった場合（異常系）は`stalled`のwarn行が遷移時に1回だけ出て、回復時にinfo行が出ること。

## 手順7: 既存機能の無影響確認

- Ports、Docker、Logsパネルが従来どおり表示・操作できること
- Processesビュー（切り替え前）の選択・ソート・フィルタが従来どおり動くこと
- 1〜4キーのパネル選択と、同一パネル番号キーの再押下によるフルスクリーン切り替えが従来どおり動くこと

## 結果の記録

各手順の結果（OK/NG）をお知らせください。NGの場合は、画面の状況に加えて該当時刻のログ行（`~/.local/share/devpulse/devpulse.log`から該当部分をそのまま）を添えてください。ログ行が再現調査の起点になります。修正して再検証をお願いします。
