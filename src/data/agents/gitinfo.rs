//! git照会による属性補完（詳細設計4.5節）。取得元ではなく補完器。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::model::AgentSessionRow;
use super::CommandRunner;

/// 1回のrev-parseで得るworktreeルートとcommon directoryの組。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitInfo {
    pub worktree: PathBuf,
    pub git_common_dir: PathBuf,
}

/// cwd単位のキャッシュを持つgit補完器。キャッシュは1収集サイクル内でのみ有効
/// （サイクルごとに`clear_cache`を呼ぶか新しいインスタンスを作る）。
pub struct GitEnricher {
    runner: Arc<dyn CommandRunner>,
    timeout_ms: u64,
    cache: HashMap<PathBuf, Option<GitInfo>>,
}

impl GitEnricher {
    pub fn new(runner: Arc<dyn CommandRunner>, timeout_ms: u64) -> Self {
        Self {
            runner,
            timeout_ms,
            cache: HashMap::new(),
        }
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// git -C cwd rev-parse --show-toplevel --git-common-dir を1回で実行し2値を得る。
    /// 失敗時（gitリポジトリ外、タイムアウト）はNoneのまま補完しない。
    pub async fn lookup(&mut self, cwd: &Path) -> Option<GitInfo> {
        if let Some(cached) = self.cache.get(cwd) {
            return cached.clone();
        }
        let cwd_s = cwd.to_string_lossy().into_owned();
        let result = match self
            .runner
            .run(
                "git",
                &[
                    "-C",
                    &cwd_s,
                    "rev-parse",
                    "--show-toplevel",
                    "--git-common-dir",
                ],
                self.timeout_ms,
            )
            .await
        {
            Ok(out) if out.success && !out.timed_out => parse_rev_parse_output(cwd, &out.stdout),
            _ => None,
        };
        self.cache.insert(cwd.to_path_buf(), result.clone());
        result
    }

    /// 統合後の各行のcwdへgit情報を補完する。
    pub async fn enrich(&mut self, rows: &mut [AgentSessionRow]) {
        for row in rows.iter_mut() {
            let Some(cwd) = row.cwd.clone() else { continue };
            if let Some(info) = self.lookup(&cwd).await {
                row.worktree = Some(info.worktree);
                row.git_common_dir = Some(info.git_common_dir);
            }
        }
    }
}

/// rev-parseの2行出力を解析する。出力は呼び出し位置により相対パスと絶対パスが
/// 混在するため（実測確認済み。T4回帰4）、cwdを基準に絶対パスへ正規化する。
pub fn parse_rev_parse_output(cwd: &Path, stdout: &str) -> Option<GitInfo> {
    let mut lines = stdout.lines().map(str::trim).filter(|l| !l.is_empty());
    let toplevel = lines.next()?;
    let common_dir = lines.next()?;
    Some(GitInfo {
        worktree: normalize_path(cwd, toplevel),
        git_common_dir: normalize_path(cwd, common_dir),
    })
}

/// パスをcwd基準で絶対化し、`.`と`..`を字句的に解決する。
pub fn normalize_path(cwd: &Path, raw: &str) -> PathBuf {
    let raw_path = Path::new(raw);
    let absolute = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        cwd.join(raw_path)
    };
    // 「.」「..」を字句的に解決する（シンボリックリンクは辿らない）
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    let mut root = PathBuf::new();
    for component in absolute.components() {
        use std::path::Component;
        match component {
            Component::RootDir | Component::Prefix(_) => {
                root.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(part) => parts.push(part.to_os_string()),
        }
    }
    let mut result = root;
    for part in parts {
        result.push(part);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::agents::{CommandOutput, SystemCommandRunner};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn normalize_keeps_absolute_and_resolves_relative() {
        let cwd = Path::new("/Users/dev/devs/repo");
        assert_eq!(
            normalize_path(cwd, "/Users/dev/devs/repo"),
            PathBuf::from("/Users/dev/devs/repo")
        );
        // 相対（実測の".git"）はcwd基準で絶対化する
        assert_eq!(
            normalize_path(cwd, ".git"),
            PathBuf::from("/Users/dev/devs/repo/.git")
        );
        // 「..」は字句的に解決する
        assert_eq!(
            normalize_path(cwd, "../main/.git"),
            PathBuf::from("/Users/dev/devs/main/.git")
        );
        assert_eq!(
            normalize_path(cwd, "./.git/worktrees/../.."),
            PathBuf::from("/Users/dev/devs/repo")
        );
    }

    #[test]
    fn parse_rev_parse_two_lines_with_normalization() {
        let cwd = Path::new("/Users/dev/devs/repo");
        let info = parse_rev_parse_output(cwd, "/Users/dev/devs/repo\n.git\n").unwrap();
        assert_eq!(info.worktree, PathBuf::from("/Users/dev/devs/repo"));
        assert_eq!(
            info.git_common_dir,
            PathBuf::from("/Users/dev/devs/repo/.git")
        );
        // 2行に満たない出力は失敗として扱う
        assert!(parse_rev_parse_output(cwd, "/only/one/line\n").is_none());
        assert!(parse_rev_parse_output(cwd, "").is_none());
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // T7: gitのテストはモックではなくtempfileで実リポジトリと実worktreeを作る
    #[tokio::test]
    async fn regression_git_common_dir_relative_absolute_mixed_normalizes_to_same_path() {
        // T4回帰4: --git-common-dirの出力は呼び出し位置により相対と絶対が混在する。
        // 正規化後は本体とworktreeで同一の絶対パスに一致しなければならない。
        let tmp = tempfile::tempdir().unwrap();
        // macOSの/var→/private/varシンボリックリンク差異を避けるため先に正規化する
        let tmp_root = tmp.path().canonicalize().unwrap();
        let main = tmp_root.join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(
            &main,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "--allow-empty",
                "-q",
                "-m",
                "init",
            ],
        );
        let wt = tmp_root.join("wt1");
        git(&main, &["worktree", "add", "-q", wt.to_str().unwrap()]);

        let mut enricher = GitEnricher::new(Arc::new(SystemCommandRunner), 3000);
        let main_canon = main.clone();
        let wt_canon = wt.clone();
        let info_main = enricher.lookup(&main_canon).await.expect("main lookup");
        let info_wt = enricher.lookup(&wt_canon).await.expect("worktree lookup");
        // どちらも絶対パスに正規化される
        assert!(info_main.git_common_dir.is_absolute());
        assert!(info_wt.git_common_dir.is_absolute());
        // 本体とworktreeでcommon dirが同一の絶対パスへ収束する
        assert_eq!(info_main.git_common_dir, info_wt.git_common_dir);
        assert_eq!(info_main.git_common_dir, main_canon.join(".git"));
        // worktreeルートはそれぞれ自分のトップレベル
        assert_eq!(info_main.worktree, main_canon);
        assert_eq!(info_wt.worktree, wt_canon);
    }

    #[tokio::test]
    async fn lookup_outside_git_repo_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let mut enricher = GitEnricher::new(Arc::new(SystemCommandRunner), 3000);
        assert!(enricher.lookup(tmp.path()).await.is_none());
    }

    /// SystemCommandRunnerへ委譲しつつ呼び出し回数だけ数えるラッパ（実gitを使う）。
    struct CountingRunner {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl CommandRunner for CountingRunner {
        async fn run(
            &self,
            program: &str,
            args: &[&str],
            timeout_ms: u64,
        ) -> Result<CommandOutput> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            SystemCommandRunner.run(program, args, timeout_ms).await
        }
        fn exists(&self, program: &str) -> bool {
            SystemCommandRunner.exists(program)
        }
    }

    /// 固定のCommandOutputを返すrunner（失敗・タイムアウトの再現用）。
    struct StaticRunner {
        output: CommandOutput,
    }

    #[async_trait]
    impl CommandRunner for StaticRunner {
        async fn run(&self, _p: &str, _a: &[&str], _t: u64) -> Result<CommandOutput> {
            Ok(self.output.clone())
        }
        fn exists(&self, _p: &str) -> bool {
            true
        }
    }

    fn row_with_cwd(id: &str, cwd: Option<&Path>) -> AgentSessionRow {
        use crate::data::agents::model::{AgentKind, StateSource};
        let mut row = AgentSessionRow::new(id, AgentKind::Claude, StateSource::ClaudeCli);
        row.cwd = cwd.map(Path::to_path_buf);
        row
    }

    #[tokio::test]
    async fn enrich_fills_worktree_attributes_only_for_rows_inside_repo() {
        // T7慣例: tempfileで実リポジトリを作り、enrichが実際に属性を埋めることを確認
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        let outside = root.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let mut rows = vec![
            row_with_cwd("in-repo", Some(&repo)),
            row_with_cwd("outside", Some(&outside)),
            row_with_cwd("no-cwd", None),
        ];
        let mut enricher = GitEnricher::new(Arc::new(SystemCommandRunner), 3000);
        enricher.enrich(&mut rows).await;
        // リポジトリ内の行はworktreeとgit_common_dirが埋まる
        assert_eq!(rows[0].worktree, Some(repo.clone()));
        assert_eq!(rows[0].git_common_dir, Some(repo.join(".git")));
        // リポジトリ外・cwd無しの行は埋めない
        assert!(rows[1].worktree.is_none());
        assert!(rows[1].git_common_dir.is_none());
        assert!(rows[2].worktree.is_none());
        assert!(rows[2].git_common_dir.is_none());
    }

    #[tokio::test]
    async fn enrich_does_not_fill_on_command_failure_or_timeout() {
        // 2行のstdoutがあっても失敗（exit非0）なら補完しない
        let failed = StaticRunner {
            output: CommandOutput {
                stdout: "/repo\n/repo/.git\n".into(),
                timed_out: false,
                success: false,
            },
        };
        let mut rows = vec![row_with_cwd("r1", Some(Path::new("/repo")))];
        GitEnricher::new(Arc::new(failed), 100)
            .enrich(&mut rows)
            .await;
        assert!(rows[0].worktree.is_none());
        assert!(rows[0].git_common_dir.is_none());
        // タイムアウト時も部分出力からは補完しない
        let timed = StaticRunner {
            output: CommandOutput {
                stdout: "/repo\n/repo/.git\n".into(),
                timed_out: true,
                success: true,
            },
        };
        let mut rows = vec![row_with_cwd("r2", Some(Path::new("/repo")))];
        GitEnricher::new(Arc::new(timed), 100)
            .enrich(&mut rows)
            .await;
        assert!(rows[0].worktree.is_none());
        assert!(rows[0].git_common_dir.is_none());
    }

    #[tokio::test]
    async fn clear_cache_forces_fresh_lookup_next_cycle() {
        // clear_cache後は同一cwdでも再照会する（キャッシュ残留の防止）
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q"]);
        let runner = Arc::new(CountingRunner {
            calls: AtomicUsize::new(0),
        });
        let mut enricher = GitEnricher::new(runner.clone(), 3000);
        let _ = enricher.lookup(&main).await;
        let _ = enricher.lookup(&main).await;
        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
        enricher.clear_cache();
        let _ = enricher.lookup(&main).await;
        assert_eq!(runner.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn lookup_caches_per_cwd_within_a_cycle() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q"]);
        let runner = Arc::new(CountingRunner {
            calls: AtomicUsize::new(0),
        });
        let mut enricher = GitEnricher::new(runner.clone(), 3000);
        let a = enricher.lookup(&main).await;
        let b = enricher.lookup(&main).await;
        assert_eq!(a, b);
        // 同一cwdへの照会は1収集サイクル内で1度だけ実行する
        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
        // 失敗結果もキャッシュされる
        let outside = tmp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        assert!(enricher.lookup(&outside).await.is_none());
        assert!(enricher.lookup(&outside).await.is_none());
        assert_eq!(runner.calls.load(Ordering::SeqCst), 2);
    }
}
