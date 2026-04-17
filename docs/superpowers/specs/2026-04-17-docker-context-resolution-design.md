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
- Windows の npipe 対応（最低限パースできるが優先検証対象ではない）
- TLS 証明書付き tcp エンドポイントの詳細設定（将来拡張）

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
   - `"auto"` または空ならスキップ
   - 絶対パス or `unix://` / `tcp://` URL を許可
3. **Docker CLI context**
   - `DOCKER_CONTEXT` 環境変数が優先（Docker CLI と同じ挙動）
   - 未設定時は `$HOME/.docker/config.json` を読み `currentContext` を取得
   - 空 or `"default"` の場合はスキップ
   - `$HOME/.docker/contexts/meta/` 配下の各 `meta.json` を走査し、`Name == currentContext` の `Endpoints.docker.Host` を採用
4. **既知ソケットのプローブ**（最初に見つかったもの）
   - `$HOME/.colima/default/docker.sock` — Colima
   - `$HOME/.orbstack/run/docker.sock` — OrbStack
   - `$HOME/.rd/docker.sock` — Rancher Desktop
   - `$XDG_RUNTIME_DIR/docker.sock` — rootless（Linux）
   - `$HOME/.docker/run/docker.sock` — Docker Desktop
   - プローブは「ファイルが存在する」を条件とする（UDS の実接続まではしない）
5. **最終フォールバック**: `/var/run/docker.sock`
6. 全失敗時は `ResolutionReport::resolved = None` を返す

### 接続フロー

```text
┌─ App::new(config) ─────────────────────────────────────────┐
│  let report = resolve_endpoint(&cfg, &RealEnv);            │
│  match report.resolved {                                   │
│      Some(r) => connect(&r.endpoint) and capture errors    │
│                  → BollardDockerSource { client, endpoint, │
│                                          context_name,     │
│                                          report }          │
│      None    => record default attempt in report           │
│  }                                                         │
└────────────────────────────────────────────────────────────┘
```

- `BollardDockerSource` は `client`, `endpoint`, `context_name`, `report` を保持
- `App::new` は `docker_source.endpoint()` を 1 回だけ取り出し、`log_collector::spawn_log_collectors` に `Option<DockerEndpoint>` として渡す（resolve は 1 回のみ）

### エラー取り扱い

- 解決に成功したが接続でエラーが出た場合 → `client = None`、`docker_available = false`、`tracing::warn!` でログし `report.warnings` に記録
- `config.socket_path` が parse できない値の場合 → `report.warnings` に記録し、次の優先順位（CLI context）にフォールスルー
- UI はパネルに `No Docker daemon found. Tried:` ヘッダとともに試行エンドポイント一覧 + warnings を表示（狭いパネルでは `Docker not found` にフォールバック）

### プラットフォーム上の注意

- `docker.socket_path` のパース: `unix://`, `http(s)://`, `npipe://`, `tcp://` のスキーム付き URL に加え、`Path::is_absolute()` を満たす絶対パスを Unix socket として扱う
- Windows: Unix socket は基本的に使えないので、`npipe://` URL または `tcp://` URL を使うことを推奨。`C:\...` 形式の絶対パスは Unix socket として扱われるため、connect 時にエラーになる

### UI 変更

- Docker パネルタイトル: `Docker` → `Docker [colima]`（context 名が取得できた場合のみ）
- Docker 利用不可時のメッセージを現行の固定文言から、試行候補の簡易サマリ入りに差し替え

## テスト戦略

### ユニットテスト（`docker_connector::tests`）

テーブル駆動で以下のケースを検証:

| # | 事前状態 | 期待 source | 期待 endpoint |
|---|---|---|---|
| 1 | `DOCKER_HOST=tcp://1.2.3.4:2375` | EnvVar | Http |
| 2 | env 未設定、`config.socket_path = "/tmp/custom.sock"` | Config | UnixSocket |
| 3 | env/config なし、`config.json` に `currentContext=colima`、meta に Colima 定義 | CliContext("colima") | UnixSocket |
| 4 | env/config/context なし、Colima ソケットのみ存在 | Probe("colima") | UnixSocket |
| 5 | すべて不在 | — | resolved=None |

**I/O 分離:** `resolve_endpoint` は `DockerConfig` と「ファイルシステム / 環境変数アクセサ」トレイト or 関数引数を受ける純粋関数構造にし、テストでは tempdir で偽の `$HOME` / 偽 socket ファイルを差し込む。

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
