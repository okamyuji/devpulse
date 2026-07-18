# DevPulse エージェントセッション観測パネル 詳細設計書

| 項目 | 内容 |
|---|---|
| 版数 | v1.1 |
| 日付 | 2026-07-19 |
| 状態 | シール済み（v1.0で敵対的レビュー3巡＋ユーザー承認の最終確認1巡により収束。v1.1は実装と手動検証で判明した事実にもとづく訂正の反映版で、内容は実装済みコードと突き合わせ済み） |
| 正本 | 基本設計書（agent-sessions-basic-design.md v1.0、シール済み）を上位文書とし、本書はそのMVP範囲を実装可能な粒度まで具体化します |
| 記載方針 | 型と構造は表で定義し、ソースコードの断片は記載しません。本書の事実主張は2026-07-18に本環境で実測した出力と、v1.1で反映した実装時（2026-07-19まで）の実測に基づきます |

## 1. モジュール構成とファイル配置

既存の「data層とui層がパネル単位で対になる」構成（CLAUDE.mdの規約）に従い、次のファイルを追加します。

| ファイル | 責務 |
|---|---|
| src/data/agents/mod.rs | 公開インターフェースと収集の入口 |
| src/data/agents/model.rs | 統一行モデルと状態の型定義 |
| src/data/agents/cmux.rs | cmux取得元アダプタ |
| src/data/agents/claude.rs | Claude Code取得元アダプタ |
| src/data/agents/kimi.rs | Kimi取得元アダプタ |
| src/data/agents/process.rs | プロセス情報フォールバックアダプタ |
| src/data/agents/gitinfo.rs | git照会による属性補完（取得元ではなく補完器） |
| src/data/agents/merge.rs | 正規化と重複統合 |
| src/ui/panels/agents.rs | エージェントセッションビューの描画 |

既存ファイルへの変更は、main.rs（サブコマンド追加）、app.rs（ビュー状態の分離と分岐）、ui/mod.rs（切り替えビューの描画呼び出しとキー処理）、config.rs（設定セクション追加）、data/processes.rs（cwd属性の追加。制御端末は追加しない）、data/mod.rs（agentsモジュールの登録）、ui/panels/mod.rs（agentsパネルの登録）、ui/panels/processes.rs（ProcessInfoの属性追加に伴うテストフィクスチャの追随。表示列は変更しない）に限定します。mod.rs2件はRustのモジュール解決上の必須編集、processes.rsのテスト追随は型変更に伴う強制編集です。

## 2. 統一行モデルの型定義

基本設計5節のデータモデルを次の型で実装します。表記は「属性: 型」で、Option型は取得できない場合に欠落を表します。

| 属性 | 型 | 補足 |
|---|---|---|
| id | String | 取得元の識別子をそのまま保持する（衝突回避は5節の統合キーで行う） |
| agent | 列挙 AgentKind | Claude、Codex、Kimi、Other(String) |
| orchestrator | 列挙 Orchestrator | Cmux、Tmux、Dmux、DevFleet、InProcess、Unknown。MVPで生成するのはCmuxとUnknownのみ（基本設計7.3節の取得元範囲による） |
| location | String | cmux由来はworkspace:N/surface:N形式の参照。プロセス由来はpid:N形式 |
| cwd | Option PathBuf | |
| worktree | Option PathBuf | gitinfoが補完する（show-toplevelの絶対パス） |
| git_common_dir | Option PathBuf | gitinfoが補完する（git-common-dirの絶対パス正規化後） |
| task_title | Option String | |
| state | 列挙 SessionState | 基本設計6節の7分類。Quietは経過秒を保持する |
| state_source | 列挙 StateSource | CmuxCli、ClaudeCli、KimiMetadata、ProcessTable（状態の根拠となり得る取得元のみを列挙する。git照会は状態根拠にならないため含めない） |
| confidence | 列挙 Confidence | Reported（公開CLIが状態を報告）、Derived（メタデータからの導出）、Inferred（プロセス存在からの推定） |
| last_activity_at | Option 時刻 | UTCで保持し表示時にローカルへ変換する |
| cpu_percent | Option f32 | プロセス照合に成功した行のみ |
| memory_bytes | Option u64 | 同上 |
| child_agents | Option usize | 取得できた場合のみ（Kimiのagentsオブジェクトからmain型を除いた要素数など） |
| pid | Option u32 | 統合キーとして内部的に保持する |
| tty | Option String | 制御端末名（例: ttys001）。統合キーとして統一行モデルだけが保持し、ProcessInfoには追加しない（既存パネルへの型変更の波及をcwdの1属性に留めるため） |
| activity_source | Option StateSource | last_activity_atを与えた取得元。quiet判定のstate_source決定に使う。直列化されない内部属性（v1.1追加） |
| session_id | Option String | 公開CLIが報告したセッション識別子の完全形。cmux eventsとの照合キーに使う。直列化されない内部属性（v1.1追加） |
| source_kind | Option String | 取得元CLIが報告したセッション種別（claudeのbackgroundとinteractiveなど）。表示には使わない、直列化されない内部属性（v1.1追加） |

orchestratorのUnknownは基本設計5節の列挙（cmux、tmux、dmux、DevFleet、プロセス内）への詳細設計側の追加です。プロセス走査だけで見つかったセッションは実行環境を特定できないため、この値が必要になります。

## 3. 取得元アダプタの共通インターフェース

既存のPortScannerやProcessSourceと同じtrait方式で、次の契約を定めます。実装にはリポジトリが既に依存しているasync-traitを使います。

| 契約項目 | 内容 |
|---|---|
| 名前関数 | アダプタの表示名を返す（縮退表示に使う） |
| 収集関数 | 非同期で統一行モデルの配列を返す。失敗はエラー型で返し、呼び出し側が該当取得元の欠落として扱う |
| 可用性関数 | アダプタが現在使えるかの軽量判定（コマンド存在確認など）。不可ならUIに「取得不可」と表示する |
| タイムアウト | 収集関数の外部コマンド実行には上限時間を設ける（既定1000ミリ秒、設定で変更可能） |

テストは既存のMockProcessSourceと同じ形式で、各アダプタのパーサに実測出力から採取したフィクスチャを与えて検証します（10節）。

## 4. 各アダプタの詳細

### 4.1 cmuxアダプタ

実行するコマンドは正式形だけを使います（基本設計7.3節の決定による）。

| 項目 | 内容 |
|---|---|
| 検出 | cmuxコマンドの存在と応答（ping）で可用性を判定する |
| 収集 | tree --all --jsonと、eventsの単発読み取りの2コマンドを実行する。どちらも構造化出力であることを実測で確認済み。eventsはストリーム購読ではなく収集サイクルごとの単発取得とし、サーバー起動や常駐は行わない（基本設計7.3節の「状態はtreeとeventsから得る」に対応する）。--limit指定だけの読み取りは、保持イベント残数がlimit未満のときliveイベント待ちでブロックすることが実装時の実測で判明したため（--limit 30で2分待ち）採用しない。カーソルファイル方式（--cursor-fileに--limit 200と--no-heartbeatを併用）で前回の続きから読み、上限時間つきで実行してタイムアウト時はそれまでの部分出力を解析対象とする。カーソルファイル欠如時のcmuxはライブ購読になり保持イベントを再生しないため、初回はseq 0をシードして最古から追いつく。cmux再起動でseqが巻き戻った場合は、ackフレームのresume情報でstaleカーソルを検出してseq 0へ戻し、次周期に再生する（--no-ackを付けないのはこの検出のためで、ackフレームはoccurred_atを持たず解析が自然に無視する） |
| 解析対象（tree） | ツリー内の各ノードは自己参照をrefキーで持つ（全ノードで実測確認済み）。ノードの種別は階層位置（windows、workspaces、panes、surfacesの配列名）で決まり、typeキーを持つのはsurfaceノードだけである（terminal等のsurface種別を表す。window、workspace、paneノードにtypeキーは存在しないことを実測確認済み。surface_typeやwindow_refの形のキー名はトップレベルのactiveとcaller参照ブロックにのみ現れる）。workspaceノードのtitle、terminal型surfaceノードのttyを使う |
| 解析対象（events） | occurred_atは全イベントに存在するが、session_idはトップレベルには存在せずpayload内にのみ、かつ一部のイベント（実測ではsourceがclaudeのもの）に限って存在することを実測確認済み。抽出はpayloadのsession_idを用い、session_idを持たないイベントは無視する。行との紐付けはsession_idの一致だけで行い、cwdは照合に使わない（イベントのcwdはフック時の作業ディレクトリでありセッション一意でないため、cwd照合は他セッションの活動を誤帰属させることが手動検証の実測で判明した。v1.1で廃止）。イベントのsession_idは「claude-（uuid）」のように取得元接頭辞つきの形をとるため、完全一致に加えて接尾辞一致（「-行のsession_id」で終わる形）も同一セッションとみなす。一致した行のlast_activity_atを最新イベント時刻で更新し、activity_source（2節の内部属性）へ取得元を記録する |
| 生成する行 | terminal型surfaceを1行の候補とし、orchestratorをCmux、locationをworkspace参照とsurface参照の組で表現する。未統合のsurfaceの扱いは次のとおりとする（v1.1で明文化）。ttyがpsの端末集合に生きているのにエージェントプロセスが無いsurface（素のシェル等）は行にしない。ttyにプロセスが1つも存在しないsurfaceだけをorphanedの行として残す（6節の順5に対応する） |
| プロセスとの照合 | surfaceのttyとプロセスの制御端末を突き合わせ、そのsurface上で動くエージェントプロセスを特定する（5節） |
| 状態 | cmuxは実行状態そのものを報告しないため、stateの決定は6節の決定表に委ねる（eventsが与えるlast_activity_atにより順6のquietが、親参照とプロセスの突き合わせにより順5のorphanedが判定可能になる）。workspaceタイトルはtask_titleの候補とする |

### 4.2 Claude Codeアダプタ

| 項目 | 内容 |
|---|---|
| 検出 | claudeコマンドの存在で判定する |
| 収集 | agents --json --allを実行する。TTYなしで動作することを実測で確認済み。出力には2種類のスキーマが混在することを手動検証で実測確認済み（v1.1で追記） |
| 解析対象 | 出力の各エントリはbackground型とinteractive型の2スキーマに分かれる。background型はid、cwd、kind、name、sessionId、startedAt（エポックミリ秒）、stateを持つ。interactive型はidを持たず、pid（文字列と数値の両形が実測される）、status、kind=interactive、cwd、name、sessionIdを持つ。行のidは、background型のidがsessionIdの先頭8文字であることの実測に合わせ、interactive型ではsessionIdの先頭8文字から導出する。idもsessionIdも無いエントリだけを読み飛ばす |
| 状態対応 | 事前定義の対応表による。background型はstate、interactive型はstatusで状態を報告する。実測で観測済みの値はblocked、done、failed（state）とidle（status）。対応はblockedをwaiting、doneとidleをidle、failedをfailedとし、加えて実行中を自明に示す値runningをrunningへ対応させる（本環境の実測では未観測だが、対応表への事前定義であり推測による分類ではない。観測され次第フィクスチャを追加する）。いずれもstate_source=ClaudeCli、confidence=Reportedとする。対応表にない値はunknownへ落とし、生の値をログへ記録する（値域が公開仕様として網羅されていないため。改善バックログ記載の残件）。なおdoneのidle対応は基本設計6節のidle定義（idleと報告している）の拡大であり、7分類に完了状態が存在しないため完了報告をidle相当として扱うという判断をここで明示する |
| 属性対応 | nameをtask_title、cwdをcwd、startedAtを補助情報とする。interactive型のpidは統合キーとして保持する（5節の優先0のpid照合に使う）。sessionIdは完全形をsession_id（2節の内部属性）として保持し、cmux eventsの照合に使う。kindはsource_kind（2節の内部属性）として保持し、表示には使わない。last_activity_atはこのCLIからは得られないため設定しない |

### 4.3 Kimiアダプタ

| 項目 | 内容 |
|---|---|
| 検出 | セッションディレクトリの存在で判定する |
| 収集 | sessionsディレクトリ配下は中間ディレクトリを挟む2階層（作業ツリー名のディレクトリの下にsession_ディレクトリが並ぶ構造。直下走査では0件になることを実測確認済み）なので、2階層目の各session_ディレクトリのstate.jsonを読む。実測で確認済みのキーはagents、createdAt、custom、isCustomTitle、lastPrompt（任意）、title、updatedAt、workDir（任意） |
| 属性対応 | titleをtask_title、updatedAtをlast_activity_at、workDirをcwd（存在する場合のみ）とする。child_agentsは、agentsオブジェクト（実測では配列ではなくオブジェクトであり、main型のエージェント自身を含むことを22ファイルの全件パースで確認済み）からtypeがmainの要素を除いた数とする |
| 状態 | メタデータのみでは実行状態を報告できないため、このアダプタは状態を与えずlast_activity_atだけを確定させる。状態の決定は、プロセス照合の成否に関わらず、統合後の全行へ6節の決定表を無条件に適用して行う（updatedAt由来のlast_activity_atがあれば、Kimi単独行でも順6のquietに到達し得る） |
| 保護 | state.jsonの読み取りはJSONパース失敗やキー欠落を行単位で握りつぶさず、該当セッションをunsupported相当として1行で表示する |

### 4.4 プロセスフォールバックアダプタ

| 項目 | 内容 |
|---|---|
| 収集 | 既存のsysinfo基盤を流用し、プロセス名またはコマンドラインがエージェント（claude、codex、kimi）に該当するものを抽出する |
| cwd拡張 | 現行のProcessInfoにcwdが無いことを確認済みのため、sysinfoのcwd取得を使って属性を追加する（既存のProcessesパネルには表示列を追加しないが、ProcessInfoの構造体リテラルを持つui/panels/processes.rsのテストフィクスチャに追随の強制編集が生じる。1節の変更対象に含む） |
| 制御端末の取得 | sysinfo 0.38系に制御端末の取得APIが存在しないことをソース確認済みのため、psコマンド（pidとttyの列指定出力）を1収集サイクル（8.4節で定義する背景収集タスクの1周期）につき1回実行し、pidをキーに制御端末名を突き合わせる。突き合わせ結果は統一行モデルのtty属性にのみ格納し、ProcessInfoには持たせない。psが失敗した場合はtty照合（5節の優先1）を諦め、優先2のcwd照合へ縮退する。なおpid、tty（ps出力）、cmux treeのsurfaceのttyの三者が実機で一致することを突き合わせの成立根拠として実測済み |
| 生成する行 | 他の取得元に統合されなかったエージェントプロセスを、orchestrator=Unknown、confidence=Inferredの行として残す。これにより「cmux外のセッションはプロセス走査でしか見えない」という基本設計4節の要件を満たす |

### 4.5 git補完器

| 項目 | 内容 |
|---|---|
| 入力 | 統合後の各行のcwd |
| 実行 | git -C 対象cwd rev-parse --show-toplevel --git-common-dirを1回で実行し、worktreeルートとcommon directoryを同時に得る |
| 正規化 | 出力は呼び出し位置により相対パスと絶対パスが混在することを実測で確認済みのため、cwdを基準に絶対パスへ正規化してから保持する（基本設計7.1節の規定） |
| キャッシュ | 同一cwdへの照会は1回の収集サイクル内で1度だけ実行する |
| 縮退 | 失敗時（gitリポジトリ外、タイムアウト）はworktreeとgit_common_dirを欠落のままにする |

## 5. 正規化と重複統合

統合は次の照合キーを優先順に適用します。

| 優先 | 照合キー | 対象 |
|---|---|---|
| 0 | pid | 公開CLI行が報告するpidと、プロセス行またはtty照合済みcmux行のpidの完全一致（v1.1追加） |
| 1 | tty | cmuxのterminal型surfaceのttyと、プロセスの制御端末の一致 |
| 2 | cwdとエージェント種別 | Claude CLI行やKimi行と、プロセス行のcwdおよび種別の一致 |
| 3 | 一致なし | 統合せず個別の行として残す |

pid照合はv1.1で追加した最上位の照合キーです。claudeのinteractive型（4.2節）がpidを報告するため、cwd照合の曖昧さに依らず確実に統合できます。cmux行はtty照合で初めてpidを得るため、処理の実行順はtty照合が先になりますが、公開CLI行にとっての照合キーの優先はpidが最上位であり、pidを持つ行は優先2のcwd照合の対象にも競合にも含めません。pid一致でtty照合済みcmux行と統合する場合は、識別・位置・題名の表示をcmux側に保ち、公開CLIが報告した状態とsession_idを取り込みます。

統合時の属性の優先は「公開CLIの報告値が最優先、メタデータ導出が次点、プロセス推定は補完のみ」とします。confidenceは属性の出所ではなく状態の確度であり、6節の決定表が付与した値だけを正とします（属性を高確度の出所から採用していても、状態が推定であればconfidenceは推定側の値になります）。cwdによる照合は同一cwdの複数エージェントで曖昧になり得るため、種別の一致を必須条件に加え、それでも曖昧な場合は統合しません（誤統合より重複表示を選びます。基本設計の断定回避と同じ方針です）。

## 6. 状態決定ロジック

統合後の各行に対して、次の決定表を上から順に適用します。

| 順 | 条件 | 決定 |
|---|---|---|
| 1 | 公開CLIがfailedを報告 | failed（Reported） |
| 2 | 公開CLIがwaiting相当（blockedなど）を報告 | waiting（Reported） |
| 3 | 公開CLIがidle相当（doneなど）を報告 | idle（Reported） |
| 4 | 公開CLIがrunning相当を報告 | running（Reported） |
| 5 | orchestratorの親参照が実在するのに対応プロセスが不在、またはプロセスが実在するのに親参照が消滅 | orphaned（判定材料の出所に応じたconfidence） |
| 6 | last_activity_atがあり、経過が閾値（既定480秒、設定可能）を超過 | quiet 経過表示（Derived） |
| 7 | プロセスは実在するが上記いずれにも該当しない | unknown（Inferred） |
| 8 | いずれの材料もない | unknown |

waitingへの分類は公開CLIの報告がある場合に限ります（基本設計6節の規定）。quietの経過はlast_activity_atからの単純差分であり、故障の断定ではないため表示だけを行います。

決定表のMVPでの到達可能性は次のとおりです。順1、2、3はClaude CLIの実測値（failed、blocked、done）で到達します。順4はClaude CLIがrunning値を返した場合に到達します（4.2節の事前定義。実測では未観測）。順5はcmuxのtree、順6はcmuxのeventsとKimiのupdatedAt、順7と8はプロセス走査で到達します。したがって7分類のすべてに生成経路があります。

## 7. collectorサブコマンド

| 項目 | 内容 |
|---|---|
| 起動形式 | devpulse agents --json |
| CLI統合 | 現行のCli構造体はフラグのみでサブコマンドを持たないことを確認済み。clapのderiveでサブコマンド列挙を任意項目として追加し、未指定時は従来どおりTUIを起動する（既存の起動互換を壊さない） |
| 出力 | トップレベルをオブジェクトとするJSONを標準出力へ出す。構成はschema_version（初期値1）、sessions（統一行モデルの配列。フィールド名は2節の属性名）、source_errors（取得元別の失敗情報の配列。取得元名と失敗理由）の3フィールドとする |
| 直列化 | 現行コードにSerialize導出は存在しないが、serdeのderive機能とserde_jsonは依存済みであることを確認済みのため、model.rsの型にSerializeを導出する（新規依存なし） |
| 終了コード | 収集が1件以上成功すれば0。全取得元が失敗した場合も、空のsessionsとsource_errorsを出力して0で終える（観測できないこと自体は異常ではないため）。引数不正のみ非0 |
| 用途 | 基本設計4節の「JSONの出力内容で結び付きの正しさを先に実証する」ための最初の成果物であり、TUIビューはこの出力と同じ収集結果を使う |

## 8. TUIビューの統合

### 8.1 ビュー状態の分離

Processes枠の切り替えビューは、現行実装の「パネル1枚につきデータ1系列」前提と衝突することが確認済みのため、次の分離を行います。

| 項目 | 内容 |
|---|---|
| ビュー識別 | Processes枠の表示ビューを列挙（ProcessesまたはAgentSessions）で保持する |
| 状態分離 | 選択位置とソート条件をビューごとに独立して保持する。エージェントセッションビュー用の選択位置、ソート列、ソート方向を新設し、既存のprocess用状態には触れない |
| 分岐対象 | データ長参照、選択移動、ソート切替、ソートの実行（表示用リストの並べ替え。tick毎に呼ばれる既存処理が該当する。エージェントセッションビューでは8.4節の共有スナップショットを変更せず、描画用の複製に対して並べ替える）、選択クランプ、選択行の参照、グローバルフィルタの適用の7系統。現行でprocess_listの系列に直結している処理を、表示ビューに応じて分岐させる |
| グローバルフィルタ | フィルタ有効時、エージェントセッションビューにはtask_title、エージェント種別、locationへの部分一致で適用する（Processesビューと同じ操作感を保つため） |
| キー割り当て | Processes枠がフォーカスされているときのaキーでビューを交互に切り替える。aキーが現行のキーハンドラで未使用であることはgrepで確認済み |

### 8.2 描画

| 項目 | 内容 |
|---|---|
| ウィジェット | ui/panels/agents.rsに、既存のProcessesPanelと同型の借用データ方式で実装する |
| 表示列 | STATE、AGENT、TASK、LOCATION、WORKTREE、QUIET、CPU、MEM |
| 状態の表示 | stateとconfidenceを組で表示し、Reported以外の状態には確度が下がっていることが分かる表示を添える（基本設計10節の縮退可視化） |
| worktree列 | 同一git_common_dirの行が複数ある場合、該当行に警告記号を付ける（基本設計8節の競合検出） |
| 取得不可 | 使えない取得元がある場合、パネル下部に取得元名と取得不可の旨を1行で表示する |

### 8.3 操作系キーの扱い

エージェントセッションビューは読み取り専用であるため、表示中はProcessesビューの操作系キー（killを発行するKキーなど）を無効とし、押下時は何も実行しません。ビュー切り替えで操作系が誤発火しないことをテスト計画（10節）に含めます。

### 8.4 収集の駆動

エージェントセッションの収集は、既存の描画周期（tick）とは独立した背景の非同期タスクとして駆動します。現行実装のtickは単一タイマーによる同期処理であり、外部コマンドを複数実行する収集（各コマンドに上限時間あり）を同期実行するとUIが停止し得るためです。

| 項目 | 内容 |
|---|---|
| 周期 | agents.refresh_ms（9節、既定5000ミリ秒）。既存tickの周期とは独立とする |
| 実行形態 | 既存依存のtokioによる背景タスク。1周期で全アダプタの収集、ps実行、git補完、統合までを行い、結果のスナップショットを共有状態へ置く |
| 描画側 | tickと描画はスナップショットを読むだけとし、外部コマンドを実行しない |
| 収集サイクルの定義 | 4.4節などで言う「1収集サイクル」はこの背景タスクの1周期を指す |
| collectorとの関係 | devpulse agentsサブコマンドは同じ収集処理を1回だけ実行して出力する |
| タスク異常時の継続 | 背景タスクの異常終了（panicを含む）はTUI本体へ伝播させず、収集の停止として扱う。DevPulse全体は起動と表示を継続する（基本設計10節の継続要件に対応する） |
| スナップショットの鮮度 | スナップショットに収集時刻を含め、収集時刻がagents.refresh_msの2周期を超えて古い場合は、エージェントセッションビューに収集停止の旨と最終取得時刻を表示する（古い情報を最新のように見せない） |

なお4.1節と11節で退けた「常駐」とは、外部デーモンの起動やストリーム購読接続の維持を指します。自プロセス内で動くこの背景タスクはそれに該当せず、基本設計7.2節の「観測のためにサーバーを起動しない」規則とも矛盾しません。

### 8.5 レイアウト不変の確認

パネル数は4のまま変更しません。パネル列挙型、レイアウト計算、パネル状態配列の要素数、描画のインデックス参照、パネル選択キーには手を入れず、既存のレイアウトテストが変更なしで通ることを完了条件に含めます。

## 9. 設定

config.tomlに次のセクションを追加します。すべて省略可能で、省略時は既定値で動作します。

| キー | 既定値 | 意味 |
|---|---|---|
| agents.enabled | true | ビューとcollectorの有効化 |
| agents.refresh_ms | 5000 | 収集サイクルの間隔 |
| agents.quiet_threshold_s | 480 | quiet表示の閾値秒 |
| agents.command_timeout_ms | 1000 | 外部コマンド1回あたりの上限時間 |
| agents.private_store_fallback | false | 非公開保存形式のフォールバック。MVPでは実装せず、キーの予約のみ行う（基本設計7.1節の優先度4の第2段階化に対応） |

## 10. テスト計画

CLAUDE.mdのテストファースト規約に従い、実装前に次のテストを書きます。

| 対象 | 方式 |
|---|---|
| 各アダプタのパーサ | 実測出力から採取したフィクスチャ（cmux tree --jsonの出力、claude agents --jsonの出力、Kimi state.json）を入力とする単体テスト。未知キーや欠落キーのフィクスチャも用意する |
| 状態決定ロジック | 6節の決定表の各行に対応する入力を与える単体テスト。waitingが報告なしで発生しないことの反例テストを含める |
| 統合ロジック | tty一致、cwd一致、曖昧ケース（同一cwdに複数エージェント）で誤統合しないことの単体テスト |
| git補完器 | 一時リポジトリとworktreeを作成する統合テストで、絶対パス正規化後の同一性判定を検証する（レビューで実証済みの相対と絶対の混在ケースを固定化する） |
| collector | サブコマンド起動でトップレベルの3フィールド（schema_version、sessions、source_errors）を持つJSONが出力されること、sessionsのフィールド名が2節の属性名と一致すること、全取得元失敗時にsource_errorsへ失敗情報が載り終了コードが0であることの統合テスト |
| 操作系キーの無効化 | エージェントセッションビュー表示中にkill系キーが何も実行しないことの単体テスト（8.3節） |
| TUI | ビュー切り替え後の選択位置とソートが独立していることの単体テスト。既存レイアウトテストが無変更で通ることの確認 |
| スナップショット鮮度 | 収集時刻がagents.refresh_msの2周期を超えた場合に収集停止と判定されること、および収集停止の旨と最終取得時刻がパネルに表示されることの単体テスト（8.4節。v1.1追加） |

## 11. 決定表（詳細設計で確定した事項）

| 決定事項 | 検討した選択肢 | 採用 | 理由 |
|---|---|---|---|
| cmux出力の取得形式 | テキストツリーの解析、tree --all --jsonの解析 | tree --all --json | 構造化JSON出力の実在を実測で確認済みで、テキスト解析より仕様変化に強いため |
| cmuxとプロセスの照合キー | cwd照合のみ、ttyを第一キーにする | ttyを第一キー | terminal型surfaceにttyが付くことを実測で確認済みで、cwdより一意性が高いため |
| Claudeの状態対応 | 観測済み値のみ対応、網羅表を仮定 | 観測済み値のみ対応し未知値はunknown | state値域が公開仕様として網羅されていないため（改善バックログの残件と対応） |
| 曖昧な統合の扱い | 最も近い候補へ統合、統合しない | 統合しない | 誤統合は競合検出の誤報につながり、重複表示より害が大きいため |
| collectorの終了コード | 全取得元失敗を異常終了、正常終了 | 正常終了 | 観測対象が存在しないことは観測ツールにとって異常ではないため |
| gitライブラリ | git2系crateの導入、gitコマンド呼び出し | gitコマンド呼び出し | 基本設計11節の決定を踏襲 |
| cmuxのeventsの使い方 | ストリーム購読、収集サイクルごとの単発取得 | 単発取得 | 常駐や購読を持ち込まず、既存の周期収集の型に収めるため。基本設計7.3節の「状態はtreeとeventsから得る」を満たす最小の形 |
| プロセスの制御端末の取得 | sysinfoのAPI、psコマンド併用 | psコマンド併用 | sysinfo 0.38系に制御端末の取得APIが存在しないことをソース確認済みのため |

## 12. 機能拡張として考えられる内容

基本設計13節と同じ扱いで、実施を確約しない内容です。tmux、dmux、DevFleet、Codexのapp-serverのアダプタ追加はアダプタ1ファイルの追加に閉じる構造を1節が保証します。cmux eventsのストリーム購読による差分更新は、収集サイクル方式の実証後に検討する余地があります。

## 13. 変更履歴

| 版数 | 日付 | 変更内容 |
|---|---|---|
| v0.1 | 2026-07-18 | 初版作成（敵対的レビュー前のドラフト） |
| v0.2 | 2026-07-18 | 敵対的レビュー1巡目（3名、実在確認済み指摘13件）を反映。cmuxアダプタへeventsの単発取得を追加し状態7分類すべてに生成経路を確保、tty属性と制御端末のps併用取得を明記、Kimiのagentsをオブジェクトとして扱いmain型を除外、cmux JSONのキー名を実出力へ整合、変更ファイル一覧へmod.rs2件とprocesses.rsテスト追随を追加、confidenceを状態の確度に一本化、collector出力をオブジェクト形に確定、分岐対象へソート実行とフィルタ適用を追加、aキー未使用の確認済みを反映 |
| v0.3 | 2026-07-18 | 再レビュー2巡目（3名、実在確認済み指摘8件）を反映。ttyの保持先を統一行モデルに一意化しProcessInfoへは追加しないと明記、背景収集タスクの節（8.4）を新設し収集サイクルを定義、操作系キーの無効化の節（8.3）を新設、eventsの抽出パス（payload内、欠落許容）とtreeのtypeキーの実態（surfaceのみ）を実測へ整合、Kimiの2階層走査と6節無条件適用を明記、collectorテストを3フィールド構成へ追随 |
| v0.4 | 2026-07-18 | 3巡目の残存指摘4件（Medium1、Low3）をユーザー承認のもと反映。背景タスク異常終了時の継続とスナップショット鮮度表示を8.4節へ追記、「常駐」の定義を明記、ソート実行の対象を描画用複製に限定、変更履歴を版数順に整列 |
| v1.0 | 2026-07-18 | 最終確認1巡で修正4件の解消を確認し、残る単独Low1件（鮮度表示のテスト計画追記）をIMPROVEMENT_BACKLOGへ記録してシール |
| v1.1 | 2026-07-19 | 実装と手動検証からのフィードバック（IMPROVEMENT_BACKLOG記載のv1.1候補）を反映。4.1節のevents取得を--limit指定の単発読み取りからカーソルファイル方式（--cursor-file、--no-heartbeat、staleカーソル検出とseq 0リセット）へ訂正（--limitのみの読み取りは保持イベント残数がlimit未満のときliveイベント待ちでブロックする実測のため）、4.1節のevents紐付けからcwd照合を廃止しsession_id照合のみへ変更（cwd照合が他セッションの活動を誤帰属させる実測のため）、未統合surfaceの解釈（生きているttyのsurfaceは行にせず、tty不在のsurfaceのみorphaned）を4.1節へ明文化、4.2節へclaude agents出力の2スキーマ（background型とinteractive型。interactive型はidを持たずpidとstatusを報告し、行idはsessionIdの先頭8文字から導出、statusのidleはidleへ対応）を追記、2節へ内部属性3件（activity_source、session_id、source_kind）を追記、5節へpid完全一致を優先0として追加、10節へスナップショット鮮度のテスト行を追記（シール時残件の解消） |
