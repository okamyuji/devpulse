# DevPulse — Unified Developer Environment TUI

## 1. プロダクト概要

**DevPulse** は、ローカル開発環境の4つの柱（ポート・Docker・プロセス・ログ）を1つのターミナルUIに統合し、表示だけでなく管理アクション（kill/stop/delete）まで実行できるクロスプラットフォームTUIツールである。

### 1.1 解決する課題

開発者はローカル環境の状態把握に複数ツールを行き来している：

| 操作 | 現状 | 問題 |
|------|------|------|
| ポート確認 | `lsof -i :3000` / `netstat` | OS毎にコマンドが異なる、出力が読みにくい |
| Docker管理 | `docker ps` + lazydocker | ホストプロセスとの関連が見えない |
| プロセス監視 | htop / btop / Activity Monitor | ポートとの紐付けがない |
| ログ確認 | `tail -f` / `docker logs` | 複数ソースを横断できない |

**DevPulseは、これら4領域を1画面に統合し、コンテキストスイッチを排除する。**

### 1.2 ターゲットユーザー

- バックエンド/フルスタック開発者（複数サービスを同時に起動する人）
- マイクロサービス開発者（Docker + ローカルプロセスが混在する環境）
- DevOps/SRE（ローカル検証環境のトラブルシュートをする人）

### 1.3 非ターゲット

- 本番環境の監視（Grafana/Datadog等の領域）
- Kubernetes クラスタ管理（k9s/Lens等の領域）
- リモートサーバの監視（SSH越しの運用）

---

## 2. 機能ロジックツリー（MECE分解）

```
DevPulse
├── 2.1 データ取得層（What to show）
│   ├── 2.1.1 ポート情報
│   ├── 2.1.2 Docker情報
│   ├── 2.1.3 プロセス情報
│   └── 2.1.4 ログ情報
│
├── 2.2 表示層（How to show）
│   ├── 2.2.1 レイアウト
│   ├── 2.2.2 パネル共通設計
│   └── 2.2.3 テーマ・表示カスタマイズ
│
├── 2.3 操作層（What to do）
│   ├── 2.3.1 ポート操作
│   ├── 2.3.2 Docker操作
│   ├── 2.3.3 プロセス操作
│   ├── 2.3.4 ログ操作
│   └── 2.3.5 確認ダイアログ（破壊的操作共通）
│
├── 2.4 横断機能（Cross-cutting）
│   ├── 2.4.1 フィルタリング体系
│   ├── 2.4.2 キーバインド体系
│   ├── 2.4.3 設定管理
│   ├── 2.4.4 CLI引数
│   └── 2.4.5 エラーハンドリング・グレースフルデグラデーション
│
└── 2.5 非機能要件（Quality attributes）
    ├── 2.5.1 パフォーマンス
    ├── 2.5.2 クロスプラットフォーム
    ├── 2.5.3 インストール・配布
    └── 2.5.4 拡張性（Post-MVP）
```

---

### 2.1 データ取得層

#### 2.1.1 ポート情報

| 項目 | 詳細 |
|------|------|
| 取得内容 | リスニング中のTCP/UDPポート、バインドアドレス、プロトコル |
| 紐付け | ポート → PID → プロセス名 → コマンドライン引数 |
| リソース | 紐付きプロセスのCPU使用率、メモリ使用量 |
| 更新頻度 | デフォルト2秒（設定可能: 1-30秒） |
| OS実装 | Linux: `/proc/net/tcp` + procfs解析, macOS: `libproc` API, Windows: `GetExtendedTcpTable` API |
| 実装crate | OS固有モジュールで `trait PortScanner` を実装（`netstat2`はメンテ不安定のため不採用） |

#### 2.1.2 Docker情報

| 項目 | 詳細 |
|------|------|
| 取得内容 | コンテナ一覧、状態（Running/Stopped/Exited/Created）、イメージ名、作成日時 |
| リソース | CPU%、メモリ使用量/制限、ネットワークI/O |
| ポートマッピング | ホストポート ↔ コンテナポート の対応（Portsパネルと相互参照） |
| Compose認識 | `com.docker.compose.project` ラベルによるサービス名グルーピング |
| 接続方式 | Unix socket（Linux/macOS）、named pipe（Windows） |
| Docker非存在時 | Dockerパネルを「Docker未検出」と表示、他3パネルは正常動作（graceful degradation） |
| 実装crate | `bollard`（Docker Engine API クライアント） |

#### 2.1.3 プロセス情報

| 項目 | 詳細 |
|------|------|
| 取得内容 | PID、プロセス名、コマンドライン、ユーザー、起動時刻 |
| リソース | CPU%、メモリ使用量（RSS）、スレッド数 |
| 表示モード | フラットリスト / ツリー表示（親子関係） |
| スマートフィルタ | デフォルトで開発系プロセスを上位に表示（node, python, java, go, cargo, ruby, php, docker等） |
| ポート相互参照 | ポートを保持するプロセスにはポート番号をインライン表示 |
| 実装crate | `sysinfo` |

#### 2.1.4 ログ情報

| 項目 | 詳細 |
|------|------|
| ソース1 | Dockerコンテナログ（`bollard`の`logs` APIによるストリーミング） |
| ソース2 | ファイルログ（ユーザー指定パス、glob対応。`notify` crateでファイル変更検知） |
| 表示 | 統合タイムライン表示（ソース名を色分けプレフィックス） |
| テール追従 | デフォルトON。スクロール操作で自動停止、`F` キーで追従再開 |
| バッファ | メモリ内に直近N行保持（デフォルト10,000行、設定可能） |
| サイズ表示 | 各ログソースのディスク使用量を表示（ファイルログのみ） |

> **Note:** DevPulseはプロセスランチャーではないため、stdout/stderrキャプチャは行わない。既に起動中のプロセスのDockerログとファイルログのみを対象とする。

---

### 2.2 表示層

#### 2.2.1 レイアウト

```
デフォルトレイアウト（4パネル均等分割）:
┌──────────────────┬──────────────────┐
│   Ports          │   Docker         │
│                  │                  │
├──────────────────┼──────────────────┤
│   Processes      │   Logs           │
│                  │                  │
└──────────────────┴──────────────────┘

代替レイアウト（メイン+サイド）:
┌────────────────────────┬───────────┐
│                        │  Docker   │
│   Ports (メイン)       ├───────────┤
│                        │ Processes │
│                        ├───────────┤
│                        │   Logs    │
└────────────────────────┴───────────┘

フルスクリーンモード:
┌────────────────────────────────────┐
│   任意のパネルを全画面表示          │
│                                    │
│                                    │
└────────────────────────────────────┘
```

- `1`〜`4` キーで各パネルをフルスクリーン切替（トグル）
- `Tab` / `Shift+Tab` でパネル間フォーカス移動
- ターミナル幅80未満: 2パネル縦並び（アクティブ+直前のパネル）
- ターミナル幅40未満: 1パネル（アクティブのみ）

#### 2.2.2 パネル共通設計

各パネルは以下の共通構造を持つ：

```
┌─ Panel Title ── [filter: node] ── Sort: CPU▼ ─┐
│ PORT   PROCESS     PID    CPU   MEM           │  ← ヘッダー（カラム名+ソート方向）
│ :3000  next-dev    1234   12%   340MB    ←    │  ← 選択行ハイライト
│ :5432  postgres    5678    2%   120MB         │
│ :6379  redis       9012    1%    45MB         │
│ ...                                           │
├─ 3 items (filtered) ──────────────────────────┤  ← ステータスバー
│ [K]ill [F]ilter [S]ort [/]Global [?]Help     │  ← コンテキストヘルプ
└───────────────────────────────────────────────┘
```

- 選択行: 背景色ハイライト + `>` マーカー
- 複数選択: `Space` でトグル（バッチ操作対象）
- カラム幅: ターミナル幅に応じて自動調整、優先度低いカラムから非表示

#### 2.2.3 テーマ・表示カスタマイズ

| 項目 | MVP | Post-MVP |
|------|-----|----------|
| 組み込みテーマ | dark（デフォルト）、light | monokai、nord、gruvbox 追加 |
| カスタムテーマ | - | TOML設定で全色を上書き可能 |
| カラム表示/非表示 | - | パネルごとに設定可能 |
| リフレッシュレート | 全パネル共通で設定可能 | パネルごとに個別設定可能 |

---

### 2.3 操作層

#### 2.3.1 ポート操作

| アクション | キー | 詳細 |
|-----------|------|------|
| プロセスKill | `K` | 紐付きプロセスにSIGTERM送信（Win: TerminateProcess） |
| 強制Kill | `Shift+K` | SIGKILL送信（確認ダイアログ） |
| 詳細表示 | `Enter` | 紐付きプロセスの詳細をポップアップ（コマンドライン、ポートマッピング、起動時刻） |
| コピー | `y` | ポート番号をクリップボードにコピー |

#### 2.3.2 Docker操作

| アクション | キー | 詳細 |
|-----------|------|------|
| コンテナ停止 | `s` | `docker stop`（graceful shutdown、タイムアウト10秒） |
| コンテナ再起動 | `r` | `docker restart` |
| コンテナ削除 | `D` | 停止済みコンテナのみ対象（確認ダイアログ） |
| ログフォーカス | `l` | Logsパネルに切替 + 該当コンテナでフィルタ自動適用 |
| シェル接続 | `e` | 外部ターミナルで `docker exec -it <id> /bin/sh` 起動 |

#### 2.3.3 プロセス操作

| アクション | キー | 詳細 |
|-----------|------|------|
| SIGTERM | `K` | graceful終了（Win: TerminateProcess） |
| SIGKILL | `Shift+K` | 強制終了（確認ダイアログ） |
| シグナル選択 | `Ctrl+K` | シグナル一覧から選択送信（Unix系のみ） |
| ツリー/フラット切替 | `t` | トグル |
| ソート変更 | `<` / `>` | CPU→MEM→PID→名前 でソート循環 |

> **Windows対応:** Unixシグナル（SIGTERM/SIGKILL/その他）はWindowsに存在しない。Windowsでは `K` が `TerminateProcess` API呼び出し、`Ctrl+K`（シグナル選択）は非表示となる。

#### 2.3.4 ログ操作

| アクション | キー | 詳細 | MVPスコープ |
|-----------|------|------|------------|
| テール追従トグル | `F` | 最新行への自動スクロールON/OFF | MVP |
| 先頭/末尾ジャンプ | `g` / `G` | vim風 | MVP |
| ラップ切替 | `w` | 行折り返しトグル | MVP |
| ソース絞り込み | `f` | 特定ソースのログのみ表示 | MVP |
| ログファイル削除 | `D` | ファイルログのみ対象（確認ダイアログ、サイズ表示） | Post-MVP |
| ログクリア | `C` | Dockerコンテナログのtruncate | Post-MVP |

#### 2.3.5 確認ダイアログ（破壊的操作共通）

すべての破壊的操作（Kill、削除、クリア）は共通の確認ダイアログを経由する：

```
┌─ Confirm ──────────────────────────────┐
│                                        │
│  Kill process "next-dev" (PID 1234)?   │
│                                        │
│  [Y]es    [N]o    [A]lways             │
└────────────────────────────────────────┘
```

- `Y` / `Enter`: 実行
- `N` / `Esc`: キャンセル
- `A`: 今回のセッション中、同種の操作を確認なしで実行（上級者向け）

---

### 2.4 横断機能

#### 2.4.1 フィルタリング体系

フィルタは3段階の階層で構成され、相互に排他的なスコープを持つ：

| レベル | スコープ | 起動キー | マッチ方式 | 持続性 |
|--------|----------|---------|-----------|--------|
| グローバルフィルタ | 全4パネル横断 | `/` | fuzzy match（デフォルト）、`regex:` プレフィックスで正規表現 | `Esc` で解除 |
| パネルローカルフィルタ | アクティブパネルのみ | `f` | 部分一致、カラム指定可（例: `port:8080`） | パネル離脱で保持、`Esc` で解除 |
| プリセットフィルタ | アクティブパネルのみ | `p` | 定義済みセット | トグル |

動作例（グローバル `/node`）:
- Ports: nodeプロセスが使用するポートのみ表示
- Docker: 名前/イメージに "node" を含むコンテナのみ
- Processes: "node" を含むプロセスのみ
- Logs: nodeソースのログのみ

フィルタ適用中はタイトルバーに `[filter: node]` インジケータ表示。

#### 2.4.2 キーバインド体系

| カテゴリ | 原則 |
|----------|------|
| ナビゲーション | vim互換（`j/k` 上下、`g/G` 先頭/末尾、`Ctrl+u/d` ページ送り） |
| パネル切替 | `Tab` / `Shift+Tab` 順次、`1`-`4` 直接ジャンプ、数字再押下でフルスクリーントグル |
| 破壊的操作 | 大文字キー（`K` kill、`D` delete）→ 確認ダイアログ（2.3.5） |
| フィルタ | `/` グローバル、`f` ローカル、`p` プリセット |
| 表示制御 | `t` ツリートグル、`w` ラップ、`F` テール追従 |
| 情報 | `?` ヘルプオーバーレイ（全キーバインド一覧 + コンテキスト別ヒント） |
| 終了 | `q` 終了（確認なし）、`Ctrl+c` 即時終了 |

#### 2.4.3 設定管理

設定ファイルパス: `~/.config/devpulse/config.toml`

```toml
[general]
refresh_rate_ms = 2000         # データ更新間隔 (1000-30000)
default_layout = "quad"        # "quad" | "main-side"
confirm_destructive = true     # false にすると確認ダイアログをスキップ

[ports]
show_columns = ["port", "protocol", "pid", "process", "cpu", "memory"]
sort_by = "port"               # "port" | "pid" | "cpu" | "memory" | "process"

[docker]
socket_path = "auto"           # "auto" | "/var/run/docker.sock" 等
show_stopped = true            # 停止済みコンテナも表示

[processes]
default_view = "flat"          # "flat" | "tree"
dev_process_priority = true    # 開発系プロセスを上位にソート

[logs]
sources = [
    { type = "docker", containers = "all" },
    { type = "file", path = "/var/log/app/*.log" },
]
buffer_lines = 10000           # メモリ内保持行数
tail_follow = true             # 起動時のテール追従デフォルト

[theme]
name = "dark"                  # "dark" | "light"
```

- 設定ファイルが存在しない場合: 全項目デフォルト値で動作（ゼロコンフィグ）
- 設定ファイルのホットリロード: `notify` crateでファイル変更を検知し自動反映

#### 2.4.4 CLI引数

```
devpulse [OPTIONS]

OPTIONS:
    -c, --config <PATH>     設定ファイルパス（デフォルト: ~/.config/devpulse/config.toml）
    -l, --layout <LAYOUT>   レイアウト指定（quad | main-side）
    -f, --filter <QUERY>    起動時グローバルフィルタ（例: --filter node）
    --ports-only            Portsパネルのみ表示
    --docker-only           Dockerパネルのみ表示
    --no-docker             Dockerパネルを無効化
    --refresh <MS>          更新間隔（ms）
    -V, --version           バージョン表示
    -h, --help              ヘルプ表示
```

CLI引数は設定ファイルよりも優先される。

#### 2.4.5 エラーハンドリング・グレースフルデグラデーション

DevPulseは**部分障害でもクラッシュしない**設計を取る：

| 状況 | 動作 |
|------|------|
| Docker未インストール | Dockerパネル:「Docker未検出」表示、他3パネル正常動作 |
| Docker daemon停止中 | Dockerパネル:「Docker daemon未起動」表示 + 5秒毎に再接続試行 |
| 権限不足（ポート取得） | 取得可能な範囲のみ表示 + ステータスバーに「一部取得不可（権限不足）」 |
| 権限不足（プロセスkill） | 確認ダイアログにエラー表示:「Permission denied. Run with sudo?」 |
| ターミナル幅80未満 | 2パネル表示に自動縮小 |
| ターミナル幅40未満 | 1パネル表示に自動縮小 |
| ログファイル消失 | 「ファイル消失」表示 + 該当ソースの監視を自動停止 |
| Docker API タイムアウト | リトライ3回後、「接続タイムアウト」表示 + バックオフ再接続 |

---

### 2.5 非機能要件

#### 2.5.1 パフォーマンス

| 指標 | 目標値 |
|------|--------|
| 起動時間 | < 500ms（Docker接続を含む。Docker未検出時 < 200ms） |
| メモリ使用量 | < 50MB（通常時）、< 200MB（大量ログバッファ時） |
| CPU使用率 | < 2%（アイドル時）、< 5%（全パネルアクティブ時） |
| 描画 | 30fps（操作時）。データ更新は `refresh_rate_ms` に従う。差分描画でCPU負荷を低減 |

#### 2.5.2 クロスプラットフォーム

| OS | サポートレベル | 備考 |
|----|---------------|------|
| Linux (x86_64, aarch64) | Tier 1（MVP対象） | フル機能 |
| macOS (x86_64, aarch64) | Tier 1（MVP対象） | フル機能 |
| Windows (x86_64) | Tier 2（Post-MVP） | Docker Desktop named pipe対応、シグナルはTerminateProcess、Ctrl+Kシグナル選択なし |

OS抽象化戦略:

```
trait PortScanner    → Linux: procfs, macOS: libproc, Windows: GetExtendedTcpTable
trait ProcessKiller  → Unix: kill(2), Windows: TerminateProcess
Docker接続          → bollard (自動socket/pipe検出)
ファイル監視         → notify crate (全OS対応)
プロセス情報         → sysinfo crate (全OS対応)
```

#### 2.5.3 インストール・配布

| 方式 | コマンド | MVPスコープ |
|------|---------|------------|
| Cargo | `cargo install devpulse` | MVP |
| バイナリ直接DL | GitHub Releasesからシングルバイナリ | MVP |
| Homebrew | `brew install devpulse` | Post-MVP |
| AUR | `yay -S devpulse` | Post-MVP |
| Scoop (Windows) | `scoop install devpulse` | Post-MVP |

シングルバイナリ配布（ランタイム依存なし）を最優先とする。

#### 2.5.4 拡張性（Post-MVP）

| 拡張 | 概要 | 優先度 |
|------|------|--------|
| Windows対応 | Tier 2サポートの実装 | 高 |
| Podman対応 | Docker互換APIでPodmanもサポート | 中 |
| カスタムテーマ/キーバインド | TOML設定による完全カスタマイズ | 中 |
| プラグインシステム | カスタムパネルの追加（Lua/WASM） | 低 |
| リモート接続 | SSH越しの別マシン監視 | 低 |
| 通知・アラート | CPU/メモリ閾値超過時のデスクトップ通知 | 低 |

---

## 3. 技術スタック

| 要素 | 選定 | 理由 |
|------|------|------|
| 言語 | Rust | シングルバイナリ、高速、クロスプラットフォーム |
| TUIフレームワーク | ratatui + crossterm | 最も活発なRust TUIライブラリ。crossterm はクロスプラットフォームターミナルバックエンド |
| 非同期ランタイム | tokio | bollard(Docker)が依存、async I/Oでノンブロッキングデータ取得 |
| プロセス情報 | sysinfo | クロスプラットフォーム対応済み、CPU/メモリ/プロセス一括取得 |
| Docker API | bollard | Rust製Docker Engine APIクライアント、async対応 |
| ファイル監視 | notify | クロスプラットフォームファイルシステムイベント監視 |
| CLI引数 | clap | Rustエコシステム標準のCLIパーサ |
| 設定パーサ | toml + serde | Rustエコシステム標準 |
| クリップボード | arboard | クロスプラットフォームクリップボードアクセス |
| ログ（デバッグ用） | tracing + tracing-appender | ファイル出力の構造化ログ（TUI表示と干渉しない） |

---

## 4. 競合との差別化マトリクス

| 機能 | DevPulse | lazydocker | btop | bottom | glances | fkill |
|------|----------|------------|------|--------|---------|-------|
| ポート一覧+紐付き情報 | **Yes** | No | No | No | 部分的 | No |
| Docker管理 | **Yes** | Yes | No | No | 表示のみ | No |
| プロセス管理 | **Yes** | コンテナ内のみ | Yes | Yes | Yes | Yes |
| ログ統合表示 | **Yes** | コンテナのみ | No | No | No | No |
| 全領域でKill/Stop/Delete | **Yes** | Dockerのみ | プロセスのみ | プロセスのみ | プロセスのみ | プロセスのみ |
| グローバル横断フィルタ | **Yes** | No | No | Yes(単体) | No | fuzzyのみ |
| クロスプラットフォーム | **Yes** | Yes | Yes | Yes | Yes | Yes |
| シングルバイナリ | **Yes** | Yes | Yes | Yes | No(Python) | No(Node) |
| ゼロコンフィグ起動 | **Yes** | Yes | Yes | Yes | Yes | Yes |

**DevPulseの一言差別化: 「lazydocker + btop + fkill + tail -f を1画面に統合」**

---

## 5. MVP定義（v0.1.0）

最小限で価値を提供できるスコープ：

### MVP含む

- [ ] 4パネルレイアウト表示（Ports / Docker / Processes / Logs）
- [ ] 各パネルの基本データ取得・表示
- [ ] Portsパネル: リスニングポート一覧 + プロセス紐付け + Kill
- [ ] Dockerパネル: コンテナ一覧 + stop / restart
- [ ] Processesパネル: プロセス一覧 + Kill（SIGTERM/SIGKILL）
- [ ] Logsパネル: Dockerログ + ファイルログ統合表示 + テール追従
- [ ] パネルローカルフィルタ（`f` キー）
- [ ] グローバルフィルタ（`/` キー、fuzzy match）
- [ ] vim風ナビゲーション（j/k/g/G/Tab）
- [ ] フルスクリーントグル（`1`-`4` キー）
- [ ] 確認ダイアログ（破壊的操作）
- [ ] ヘルプオーバーレイ（`?` キー）
- [ ] TOML設定ファイル（基本項目）
- [ ] CLI引数（--config, --filter, --layout, --no-docker）
- [ ] Linux + macOS サポート
- [ ] dark / light テーマ

### MVP含まない（v0.2.0以降）

- [ ] Windows サポート
- [ ] カスタムテーマ / カスタムキーバインド
- [ ] ログファイル削除 / ログクリア
- [ ] プリセットフィルタ
- [ ] main-side レイアウト
- [ ] docker exec（シェル接続）
- [ ] 複数選択バッチ操作
- [ ] Podman対応
- [ ] Homebrew / AUR / Scoop パッケージ
