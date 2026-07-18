//! ファイルへのログ出力初期化（TUIのためstdout/stderrへは一切出さない）。
//!
//! - 出力先: `~/.local/share/devpulse/devpulse.log`（固定名、ローテーションなし）
//! - レベル: 環境変数 `DEVPULSE_LOG`（EnvFilter構文）。未設定時は `info`
//! - 初期化に失敗してもパニックせず `None` を返し、アプリはログなしで動作を続ける

use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;

/// 既定のログディレクトリ `~/.local/share/devpulse`。
pub fn default_log_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".local").join("share").join("devpulse"))
}

/// 既定の場所でロギングを初期化する。返り値のguardはmain終了まで保持すること
/// （dropでバッファをフラッシュする）。
pub fn init() -> Option<WorkerGuard> {
    init_at(&default_log_dir()?)
}

/// 指定ディレクトリ配下の `devpulse.log` へ書くsubscriberをグローバル登録する。
/// ディレクトリ作成・ファイルオープン・登録のいずれに失敗しても `None`
/// を返すだけでパニックしない（要件: ログなしでも起動継続）。
pub fn init_at(dir: &Path) -> Option<WorkerGuard> {
    std::fs::create_dir_all(dir).ok()?;
    // セッションパスを含むログを他ローカルユーザーへ露出させない（所有者のみ）。
    // 既存の緩い権限も初期化時に締め直す。失敗しても起動は継続する。
    #[cfg(unix)]
    restrict_permissions(dir, 0o700);
    let log_path = dir.join("devpulse.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()?;
    #[cfg(unix)]
    restrict_permissions(&log_path, 0o600);
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = tracing_subscriber::EnvFilter::try_from_env("DEVPULSE_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok()?;
    Some(guard)
}

/// 所有者のみのモードへ権限を絞る（失敗は無視。非パニック契約の維持）。
#[cfg(unix)]
fn restrict_permissions(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

/// テスト専用のイベント収集レイヤー（外部crate不使用の最小実装）。
/// `tracing::subscriber::set_default` / `with_default` と組み合わせて使う。
#[cfg(test)]
pub mod capture {
    use std::fmt::Write as _;
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing::{Event, Level, Subscriber};
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::{Layer, Registry};

    /// 捕捉した1イベント。fieldsは `key=value ` 連結の文字列。
    #[derive(Debug, Clone)]
    pub struct CapturedEvent {
        pub level: Level,
        pub message: String,
        pub fields: String,
    }

    /// イベント収集ハンドル。cloneしてもバッファは共有される。
    #[derive(Default, Clone)]
    pub struct Capture {
        events: Arc<Mutex<Vec<CapturedEvent>>>,
    }

    impl Capture {
        pub fn new() -> Self {
            Self::default()
        }

        /// このCaptureへイベントを積むsubscriberを作る。
        pub fn subscriber(&self) -> impl Subscriber {
            Registry::default().with(CaptureLayer {
                events: Arc::clone(&self.events),
            })
        }

        pub fn events(&self) -> Vec<CapturedEvent> {
            self.events.lock().expect("capture lock").clone()
        }

        /// 指定レベルかつメッセージ部分一致のイベント数。
        pub fn count(&self, level: Level, message_contains: &str) -> usize {
            self.events()
                .iter()
                .filter(|e| e.level == level && e.message.contains(message_contains))
                .count()
        }

        /// メッセージ部分一致の最初のイベントを返す。
        pub fn find(&self, message_contains: &str) -> Option<CapturedEvent> {
            self.events()
                .into_iter()
                .find(|e| e.message.contains(message_contains))
        }
    }

    struct CaptureLayer {
        events: Arc<Mutex<Vec<CapturedEvent>>>,
    }

    impl<S: Subscriber> Layer<S> for CaptureLayer {
        fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
            let mut visitor = FieldVisitor::default();
            event.record(&mut visitor);
            self.events
                .lock()
                .expect("capture lock")
                .push(CapturedEvent {
                    level: *event.metadata().level(),
                    message: visitor.message,
                    fields: visitor.fields,
                });
        }
    }

    #[derive(Default)]
    struct FieldVisitor {
        message: String,
        fields: String,
    }

    impl Visit for FieldVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "message" {
                self.message = value.to_string();
            } else {
                let _ = write!(self.fields, "{}={} ", field.name(), value);
            }
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.message = format!("{value:?}");
            } else {
                let _ = write!(self.fields, "{}={:?} ", field.name(), value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_at_unwritable_dir_returns_none_without_panic() {
        // /dev/null配下はディレクトリを作れないため必ず失敗する
        let result = init_at(Path::new("/dev/null/devpulse-test-cannot-exist"));
        assert!(result.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn init_at_creates_owner_only_dir_and_file() {
        use std::os::unix::fs::PermissionsExt;
        // セッションパスを含むログを他ローカルユーザーへ露出させない
        // （ディレクトリ0700・ファイル0600。返り値はグローバル登録競合で
        // Noneになり得るため権限のみ検証する）
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("logs");
        let _guard = init_at(&dir);
        let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        let file_mode = std::fs::metadata(dir.join("devpulse.log"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn init_at_tightens_preexisting_world_readable_dir_and_file() {
        use std::os::unix::fs::PermissionsExt;
        // 既存の緩い権限（0755/0644）も初期化時に締め直す
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("logs");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let file = dir.join("devpulse.log");
        std::fs::write(&file, b"old\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _guard = init_at(&dir);
        let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        let file_mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
    }

    #[test]
    fn default_log_dir_points_under_local_share() {
        let dir = default_log_dir().expect("home dir");
        assert!(dir.ends_with(".local/share/devpulse"));
    }
}
