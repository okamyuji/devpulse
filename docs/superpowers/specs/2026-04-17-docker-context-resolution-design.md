# Docker Context Resolution Design

**Date:** 2026-04-17
**Status:** Approved
**Scope:** Docker CLI 互換のコンテナ検出（Colima / OrbStack / Rancher Desktop / rootless / Docker Desktop 対応）

## 背景と問題

DevPulse は起動時に `bollard::Docker::connect_with_local_defaults()` のみを使って Docker デーモンに接続している。この関数は以下しか参照しない:

1. `DOCKER_HOST` 環境変数
2. `/var/run/docker.sock`

そのため、次のようなごく一般的な構成で**コンテナが 0 件表示**になる:

- Colima を使用し、ソケットは `$HOME/.colima/default/docker.sock`
- OrbStack / Rancher Desktop / rootless docker
- Docker CLI context を `docker context use` で切り替えている

`config.toml` に `docker.socket_path = "auto"` という既定値があるものの、コード上からは**一切参照されていない** dead config になっている。

## ゴール

- `docker` CLI と同じ優先順位でエンドポイントを解決し、ユーザーが追加設定なしで Colima/OrbStack などを扱えるようにする
- ユーザーが `config.toml` で明示的にソケットを指定できるようにする（`socket_path` を dead config から復活させる）
- 検出に失敗した場合、試行した候補をユーザーに示してデバッグ可能にする
- テスト可能な構造（副作用と純粋ロジックの分離）にする

## 非ゴール

- 複数 Docker デーモンの同時集約（マルチコンテキスト表示）
- 起動中の動的な context 再解決・リコネクト
- Windows の npipe: `\\.\pipe\...` の bare path / `npipe://` URL のパースと connect までは実装（ただし実機動作検証は非対象）
- TLS 証明書付き `tcp://` エンドポイントの詳細設定（`tcp://` は常に `http://` に正規化、TLS が必要なら明示的に `https://` を使う）
- 接続タイムアウトの configurable 化（YAGNI。`CONNECT_TIMEOUT_SECS = 30` ハードコード）
- SHA-256 ハッシュによる `contexts/meta/` の O(1) lookup（通常 context 数が 1〜3 個で線形走査で十分、依存追加を避ける）

## 設計

### モジュール構成

新規モジュール `src/data/docker_connector.rs` に解決ロジックを集約する。

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerEndpoint {
    UnixSocket(PathBuf),
    Http(String),
    NamedPipe(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointSource {
    EnvVar,              // DOCKER_HOST
    Config,              // config.toml docker.socket_path
    CliContext(String),  // $HOME/.docker/config.json の currentContext
    Probe(String),       // 既知候補プローブ（"colima" 等）
    Default,             // 最終フォールバック
}

#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub endpoint: DockerEndpoint,
    pub source: EndpointSource,
    pub context_name: Option<String>, // UI 表示用 "colima" など
}

#[derive(Debug)]
pub struct ResolutionReport {
    pub tried: Vec<(EndpointSource, DockerEndpoint)>,
    pub resolved: Option<ResolvedEndpoint>,
    pub warnings: Vec<String>,
}

pub trait Env { /* env var / fs accessors, injectable for tests */ }
pub struct RealEnv;

pub fn resolve_endpoint<E: Env>(cfg: &DockerConfig, env: &E) -> ResolutionReport;
pub fn connect(endpoint: &DockerEndpoint) -> Result<bollard::Docker>;
```

### 解決順序（優先順位）

1. **`DOCKER_HOST` 環境変数**
   - `unix://`, `tcp://`, `npipe://` をパース → `DockerEndpoint`
   - 未設定ならスキップ
2. **`config.docker.socket_path`**
   - 値は trim され、`"auto"` と **大文字小文字を区別せず** 比較（`"AUTO"`・`"  auto  "` も自動扱い）
   - 空または `"auto"` ならスキップ
   - 絶対パス、`\\.\pipe\...`、`unix://` / `http(s)://` / `tcp://` / `npipe://` URL を許可
   - parse に失敗した値は `report.warnings` に記録し、次の優先順位へフォールスルー
3. **Docker CLI context**
   - `DOCKER_CONTEXT` 環境変数が優先（Docker CLI と同じ挙動）
   - 未設定時は `$HOME/.docker/config.json` を読み `currentContext` を取得
   - 値が空または `"default"` ならスキップ
   - `$HOME/.docker/contexts/meta/` 配下を **ソート済み順** で走査し、`Name == current` の `meta.json` から `Endpoints.docker.Host` を採用
   - マッチした context が Docker endpoint を持たない（Kubernetes-only 等）場合は `continue` して次の候補を探す
   - `config.json` / `meta.json` の読込失敗・JSON パースエラーは `tracing::warn` で記録しつつスキップ
4. **既知ソケットのプローブ**（最初に見つかったもの）
   - `$HOME/.colima/default/docker.sock` — Colima
   - `$HOME/.orbstack/run/docker.sock` — OrbStack
   - `$HOME/.rd/docker.sock` — Rancher Desktop
   - `$XDG_RUNTIME_DIR/docker.sock` — rootless（Linux、`XDG_RUNTIME_DIR` 未設定時はこの候補をスキップ）
   - `$HOME/.docker/run/docker.sock` — Docker Desktop
   - プローブは「ファイルが存在する」を条件とする（UDS の実接続まではしない）
5. **最終フォールバック**: `/var/run/docker.sock`
6. 全失敗時は `ResolutionReport::resolved = None` を返し、`tried` に default を記録する

### 接続フロー

```text
┌─ App::new(config) ─────────────────────────────────────────┐
│  let mut report = resolve_endpoint(&cfg, &RealEnv);        │
│  let client = match report.resolved {                      │
│      Some(r) => connect(&r.endpoint)                       │
│          .inspect_err(log & push to report.warnings)       │
│          .ok(),                                            │
│      None => None,                                         │
│  };                                                        │
│  BollardDockerSource { client, report }                    │
└────────────────────────────────────────────────────────────┘
```

- `BollardDockerSource` は `client` と `report` のみを保持。`endpoint()` / `context_name()` は `report.resolved` から派生して返す（state 重複なし）
- `App::new` は `docker_source.endpoint()` を 1 回だけ取り出し、`log_collector::spawn_log_collectors` に `Option<DockerEndpoint>` として渡す（resolve は 1 回のみ）

### エラー取り扱い

| 状況 | 対応 |
|---|---|
| `connect()` が失敗（後段） | `tracing::warn` + `report.warnings` に push。`client = None` / `docker_available = false` |
| `config.socket_path` が parse 不可 | `warnings` に記録してフォールスルー |
| `~/.docker/config.json` が破損 JSON | `resolve_cli_context` が `None` を返してフォールスルー |
| `contexts/meta/` の `meta.json` が破損 | 該当 dir を `continue` し次の dir を調べる |
| マッチ context が Docker endpoint 無し（Kubernetes-only 等） | `continue` してフォールスルー |
| `XDG_RUNTIME_DIR` 未設定 | rootless 候補をスキップするだけ |
| 接続時の `connect` エラーメッセージ | `anyhow::Context` で endpoint を含めてラップ（例: `connect_with_unix(/var/run/docker.sock) failed: ...`） |

- UI はパネルに `No Docker daemon found. Tried:` ヘッダと試行エンドポイント一覧 + warnings（`! ` 接頭辞）を表示
- パネル幅が狭い場合は `Docker not found` にフォールバック
- `BollardDockerSource::new` が connect 失敗しても `endpoint()` / `context_name()` は解決済みの値を返す（診断用）

### プラットフォーム上の注意

- `docker.socket_path` のパース: `unix://`, `http(s)://`, `npipe://`, `tcp://` のスキーム付き URL に加え、次のいずれかを満たす bare path は Unix socket として扱う:
  - 先頭が `/`（Unix-style の絶対パス、ホスト OS を問わず受理）
  - `\\.\pipe\` で始まる（Windows named pipe として `NamedPipe` にマッピング、`UnixSocket` 判定より優先）
  - `Path::is_absolute()` を満たす（ホスト OS ルールの絶対パス）
- Windows: Unix socket は基本的に使えないため `npipe://` / `tcp://` URL の利用を推奨。`/var/run/docker.sock` のような Unix 形式のパスを渡した場合はパースは成功するが connect 時に失敗する（`C:\...` 形式も同様）
- `npipe://` 経由で渡された path は `/` → `\` に正規化してから `bollard::connect_with_named_pipe` に渡す（Windows API 互換のため）
- `tcp://` は常に `http://` に正規化される（TLS は現状サポート外）。TLS 経由で接続する場合は `DOCKER_HOST=https://...` を直接指定する必要がある

### UI 変更

- Docker パネルタイトル: `Docker` → `Docker [colima]`（context 名が取得できた場合のみ）
- Docker 利用不可時のメッセージを現行の固定文言から、試行候補の簡易サマリ入りに差し替え

### タイムアウト

- `bollard::Docker::connect_with_*` は `CONNECT_TIMEOUT_SECS = 30` 秒を使用（ローカル用途として十分短く、UI が固まりにくい値）
- configurable にはしない（YAGNI、要求が出たら追加）

## テスト戦略

### ユニットテスト（`docker_connector::tests`）

純粋ロジックは `Env` trait を差し替える in-memory fake `FakeEnv` で検証し、テーブル駆動 + 個別ケースで以下を網羅:

**優先順位**

| # | 事前状態 | 期待 source | 期待 endpoint |
|---|---|---|---|
| 1 | `DOCKER_HOST=tcp://1.2.3.4:2375` | EnvVar | Http (`http://...` に正規化) |
| 2 | env 未設定、`config.socket_path = "/tmp/custom.sock"` | Config | UnixSocket |
| 3 | `DOCKER_CONTEXT=colima`（config.json と不一致でも env 優先） | CliContext("colima") | UnixSocket |
| 4 | env/config なし、config.json の currentContext=colima | CliContext("colima") | UnixSocket |
| 5 | env/config/context なし、Colima ソケットのみ存在 | Probe("colima") | UnixSocket |
| 6 | すべて不在 | — | resolved=None、tried=[Default] |

**入力正規化**

- `parse_endpoint` が `tcp://` を `http://` に正規化
- `parse_endpoint` が `npipe://` の `/` を `\` に正規化
- `parse_endpoint` が `\\.\pipe\` の bare path を NamedPipe として認識（Unix socket 判定より優先）
- `parse_endpoint` が `/var/run/docker.sock` を Unix socket と認識（host OS を問わず）
- `socket_path = "  AUTO  "` などは auto 扱い（trim + case-insensitive）

**エッジケース**

- `config.socket_path` が無効値 → `warnings` に記録され、次の優先順位にフォールスルー
- 現在 context が Docker endpoint を持たない（Kubernetes-only）→ プローブ候補にフォールスルー
- probe で Colima > OrbStack > Rancher Desktop の優先順序が守られる
- `XDG_RUNTIME_DIR` 未設定時 rootless 候補がスキップされる
- `summary_lines()` が context 名 / probe 名を含む（`docker context (colima)`、`probe (colima)`）
- `summary_lines()` が warnings を `! ` 接頭辞で出力

### 統合テスト

- `cargo test --lib` で docker_connector のテーブル駆動テストが全通過
- 既存の `data::docker::tests` / `data::logs` 系が通過（互換性確認）

### 実機確認

1. `cargo run` → Docker パネルに Colima 配下のコンテナ一覧が表示され、タイトルに `[colima]` が出る
2. `DOCKER_HOST=unix:///tmp/nonexistent cargo run` → env 優先でエラー扱い、パネルに試行サマリ
3. `HOME=/tmp/empty cargo run` 的な環境 → 最終フォールバック `/var/run/docker.sock` も失敗、`No Docker daemon found`

### 品質検証チェックリスト（後続で機械的に実施）

1. `cargo fmt --check`
2. `cargo clippy -- -D warnings`
3. `cargo test`
4. `cargo run` で実機確認（上記 1）

## 変更ファイル一覧

| ファイル | 種別 | 内容 |
|---|---|---|
| `src/data/docker_connector.rs` | 新規 | 解決ロジック + 接続ヘルパ |
| `src/data/mod.rs` | 変更 | `pub mod docker_connector;` 追加 |
| `src/data/docker.rs` | 変更 | `BollardDockerSource::new(&DockerConfig)` + `context_name()` アクセサ |
| `src/data/log_collector.rs` | 変更 | `connect_with_local_defaults` → `resolve_endpoint` 経由 |
| `src/app.rs` | 変更 | 呼び出し箇所の差し替え、`docker_context_name` 状態追加 |
| `src/ui/panels/docker.rs` | 変更 | タイトルに context 名、エラーメッセージ差し替え |
| `docs/superpowers/specs/2026-04-17-docker-context-resolution-design.md` | 新規 | 本仕様 |

## リスク

- `$HOME/.docker/contexts/meta/` の JSON スキーマが Docker の将来バージョンで変わる可能性 → 失敗時はスキップして次の候補に進める設計で緩和
- `config.socket_path` に不正値が入ると接続失敗するが、試行サマリで原因が分かる
- macOS/Linux 以外で動作テストしていない → 既知ソケットプローブは OS ごとに分岐しないが、存在しないパスは自然にスキップされるため副作用なし
