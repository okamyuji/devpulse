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
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("devpulse.log"))
        .ok()?;
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

    #[test]
    fn default_log_dir_points_under_local_share() {
        let dir = default_log_dir().expect("home dir");
        assert!(dir.ends_with(".local/share/devpulse"));
    }
}
