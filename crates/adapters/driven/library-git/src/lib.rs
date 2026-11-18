//! The `TreeObserver` port over the git command line (step 10, U12): what
//! git sees of a library tree, a project checkout's `.ctxloom` or the home's.
//!
//! Every git is run by git-cli's runner (G9) in the caller's environment, its
//! repository-local variables scrubbed, as `git --no-optional-locks -c
//! core.longpaths=true ...`, so a read never refreshes or writes the index,
//! and nothing writes an object (`hash-object` without `-w`). Before its
//! first invocation the observer asks git its version, once: git missing is
//! `library-git-git-missing` and one older than 2.25 `library-git-git-too-old`
//! (G11), each a failure of the environment (exit 10).
//!
//! A path git prints is relative to its repository's root; the observer
//! hands each over relative to the directory it was asked about, with `/`
//! separators, so a project's root, a subdirectory of a repository and a
//! dotfiles home read alike (QUESTIONS.md I).

mod error;
mod parse;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use ctxloom_ports::{IgnoreRule, Motion, StatusEntry, TreeChange, TreeError, TreeObserver};
use file_system::{Fs, Open};
use git_cli::{Invocation, Runner};
use os::Mode;
use process_env::EnvSnapshot;

#[cfg(any(test, feature = "test-support"))]
pub use error::diagnostic_samples;
pub use error::{LibraryGitError, EN_US};

/// How long one git invocation may run: a `status` of a large repository is
/// the slowest, and worktree-git's thirty seconds bound the same commands.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// The oldest git the observer reads with (G11): `status --porcelain=v2`,
/// `rev-parse --git-path` and `check-ignore --no-index` are all older, and
/// 2.25 is what every supported platform ships or exceeds.
pub const FLOOR: (u32, u32) = (2, 25);

/// The global options every invocation has: no optional index refresh, and
/// paths past Windows' MAX_PATH (other platforms ignore it; no repository's
/// configuration changes).
const GLOBAL: [&str; 3] = ["--no-optional-locks", "-c", "core.longpaths=true"];

/// What the version check found, kept for the observer's life.
#[derive(Clone, Debug)]
enum Floor {
    Met,
    Missing,
    TooOld(String),
    Failed(LibraryGitError),
}

/// The observer: git on the caller's `PATH`, in the caller's environment.
pub struct GitTrees {
    fs: Arc<dyn Fs>,
    runner: Runner,
    floor: OnceLock<Floor>,
}

impl GitTrees {
    /// The git on `env`'s `PATH`, each invocation bounded by
    /// [`DEFAULT_TIMEOUT`]; `fs` is the filesystem the trees are on.
    pub fn new(fs: Arc<dyn Fs>, env: Arc<EnvSnapshot>) -> GitTrees {
        GitTrees::with_program(fs, "git", DEFAULT_TIMEOUT, env)
    }

    /// `program` (a path, or a name looked up on `env`'s `PATH`), each
    /// invocation bounded by `timeout`.
    pub fn with_program(
        fs: Arc<dyn Fs>,
        program: impl Into<PathBuf>,
        timeout: Duration,
        env: Arc<EnvSnapshot>,
    ) -> GitTrees {
        GitTrees {
            fs,
            runner: Runner::new(program, timeout, env),
            floor: OnceLock::new(),
        }
    }

    /// The version check, asked of git once.
    fn floor(&self) -> Result<(), TreeError> {
        let floor = self.floor.get_or_init(|| {
            match self.runner.run(&Invocation::new(None, &["--version"])) {
                Ok(ran) => {
                    let said = String::from_utf8_lossy(&ran.stdout).trim().to_string();
                    match parse::version(&said) {
                        Some(found) if found < FLOOR => Floor::TooOld(said),
                        _ => Floor::Met,
                    }
                }
                Err(failure) if failure.is_not_found() => Floor::Missing,
                Err(failure) => Floor::Failed(failed(&["--version"], Path::new(""), &failure)),
            }
        });
        match floor {
            Floor::Met => Ok(()),
            Floor::Missing => Err(TreeError::GitMissing(Box::new(LibraryGitError::GitMissing))),
            Floor::TooOld(said) => {
                Err(TreeError::GitTooOld(Box::new(LibraryGitError::GitTooOld {
                    version: parse::version_word(said).to_string(),
                })))
            }
            Floor::Failed(error) => Err(TreeError::Failed(Box::new(error.clone()))),
        }
    }

    /// `git <GLOBAL> args` in `dir`, fed `stdin`, with `extra` set: what it
    /// wrote on success, else the invocation's failure as it is (the caller
    /// judges an exit; anything else is [`failed`]).
    fn invoke(
        &self,
        dir: &Path,
        args: &[&str],
        stdin: Option<&[u8]>,
        extra: &[(OsString, OsString)],
    ) -> Result<Result<Vec<u8>, git_cli::Failure>, TreeError> {
        self.floor()?;
        let mut all: Vec<&str> = GLOBAL.to_vec();
        all.extend_from_slice(args);
        Ok(self
            .runner
            .run(&Invocation {
                dir: Some(dir),
                args: &all,
                stdin,
                extra_env: extra,
            })
            .map(|ran| ran.stdout))
    }

    /// [`invoke`](Self::invoke), any failure the diagnostic.
    fn git(&self, dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>, TreeError> {
        self.invoke(dir, args, stdin, &[])?
            .map_err(|failure| TreeError::Failed(Box::new(failed(args, dir, &failure))))
    }

    /// What `dir` is below its repository's root (`.ctxloom/`, or empty at
    /// the root itself).
    fn prefix(&self, dir: &Path) -> Result<String, TreeError> {
        let out = self.git(dir, &["rev-parse", "--show-prefix"], None)?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }
}

/// The failure of `git args` in `dir`, named by its subcommand and flags:
/// the arguments before `--`, after which only paths come.
fn failed(args: &[&str], dir: &Path, failure: &git_cli::Failure) -> LibraryGitError {
    let shown: Vec<&str> = args.iter().copied().take_while(|a| *a != "--").collect();
    let said = match failure {
        git_cli::Failure::Spawn(error) => error.io().to_string(),
        git_cli::Failure::TimedOut(limit) => {
            format!("it did not finish within {limit:?} and was killed")
        }
        git_cli::Failure::Exited { status, .. } => {
            let said = failure.said();
            if said.is_empty() {
                status.to_string()
            } else {
                said
            }
        }
    };
    LibraryGitError::Failed {
        args: shown.join(" "),
        dir: dir.display().to_string(),
        ext_err_txt: said,
    }
}

/// Whether `failure` is git exiting 1 having said nothing: the answer "no"
/// of a query (`rev-parse -q --verify`, `check-ignore`).
fn is_quiet_no(failure: &git_cli::Failure) -> bool {
    matches!(failure, git_cli::Failure::Exited { status, stderr, .. }
        if status.code() == Some(1) && stderr.iter().all(u8::is_ascii_whitespace))
}

/// `-- paths`, the arguments a pathspec is.
fn with_paths<'a>(args: &[&'a str], paths: &[&'a str]) -> Vec<&'a str> {
    let mut all = args.to_vec();
    all.push("--");
    all.extend_from_slice(paths);
    all
}

impl TreeObserver for GitTrees {
    fn work_tree(&self, dir: &str) -> Result<Option<String>, TreeError> {
        let dir = Path::new(dir);
        let args = ["rev-parse", "--show-toplevel"];
        // git's words in the C locale, so "not a git repository" reads the
        // same whatever her language.
        let c = [
            (OsString::from("LC_ALL"), OsString::from("C")),
            (OsString::from("LANGUAGE"), OsString::new()),
        ];
        match self.invoke(dir, &args, None, &c)? {
            Ok(out) => Ok(Some(String::from_utf8_lossy(&out).trim().to_string())),
            Err(failure) if failure.said().contains("not a git repository") => Ok(None),
            Err(failure) => Err(TreeError::Failed(Box::new(failed(&args, dir, &failure)))),
        }
    }

    fn check_ignore(&self, dir: &str, path: &str) -> Result<Option<IgnoreRule>, TreeError> {
        let dir = Path::new(dir);
        let args = ["check-ignore", "--no-index", "-v", "-z", "--stdin"];
        let mut fed = path.as_bytes().to_vec();
        fed.push(0);
        match self.invoke(dir, &args, Some(&fed), &[])? {
            Ok(out) => Ok(parse::ignore_rule(&out)),
            Err(failure) if is_quiet_no(&failure) => Ok(None),
            Err(failure) => Err(TreeError::Failed(Box::new(failed(&args, dir, &failure)))),
        }
    }

    fn init_repository(&self, dir: &str, ignore: Option<&str>) -> Result<(), TreeError> {
        let dir = Path::new(dir);
        // The ignore file first, so git never sees the directory without it,
        // and never over one that is there (another process's, or hers).
        if let Some(text) = ignore {
            let file = dir.join(".gitignore");
            match self.fs.open(&file, Open::CreateNew(Mode::FILE)) {
                Ok(mut handle) => {
                    use std::io::Write;
                    handle.write_all(text.as_bytes()).map_err(|e| {
                        TreeError::Failed(Box::new(LibraryGitError::Failed {
                            args: "init".to_string(),
                            dir: dir.display().to_string(),
                            ext_err_txt: format!("writing .gitignore: {e}"),
                        }))
                    })?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => {
                    return Err(TreeError::Failed(Box::new(LibraryGitError::Failed {
                        args: "init".to_string(),
                        dir: dir.display().to_string(),
                        ext_err_txt: format!("writing .gitignore: {e}"),
                    })))
                }
            }
        }
        self.git(dir, &["init", "-q"], None).map(|_| ())
    }

    fn head(&self, dir: &str) -> Result<Option<String>, TreeError> {
        self.verify(Path::new(dir), "HEAD^{commit}")
    }

    fn library_tree(&self, dir: &str, commit: &str) -> Result<Option<String>, TreeError> {
        self.verify(Path::new(dir), &format!("{commit}:./"))
    }

    fn status(&self, dir: &str, paths: &[&str]) -> Result<Vec<StatusEntry>, TreeError> {
        let dir = Path::new(dir);
        let prefix = self.prefix(dir)?;
        let args = with_paths(
            &["status", "--porcelain=v2", "-z", "-uall", "--renames"],
            paths,
        );
        let out = self.git(dir, &args, None)?;
        Ok(parse::status(&out, &prefix))
    }

    fn diff_tree(
        &self,
        dir: &str,
        from: &str,
        to: &str,
        paths: &[&str],
    ) -> Result<Vec<TreeChange>, TreeError> {
        let dir = Path::new(dir);
        let args = with_paths(
            &[
                "diff-tree",
                "-r",
                "-M",
                "-z",
                "--relative",
                "--no-commit-id",
                from,
                to,
            ],
            paths,
        );
        let out = self.git(dir, &args, None)?;
        Ok(parse::diff_tree(&out))
    }

    fn blob_ids(&self, dir: &str, paths: &[&str]) -> Result<BTreeMap<String, String>, TreeError> {
        let dir_path = Path::new(dir);
        let args = ["hash-object", "--stdin-paths"];
        if paths.iter().any(|p| p.contains(['\n', '\r'])) {
            return Err(TreeError::Failed(Box::new(LibraryGitError::Failed {
                args: args.join(" "),
                dir: dir.to_string(),
                ext_err_txt: "a path holds a line break, which hash-object cannot be given"
                    .to_string(),
            })));
        }
        // A path with no file is left out (absent), never fed to git, which
        // would fail the whole call on it.
        let present: Vec<&str> = paths
            .iter()
            .copied()
            .filter(|p| self.fs.exists(&dir_path.join(p)).unwrap_or(false))
            .collect();
        let paths = present.as_slice();
        if paths.is_empty() {
            return Ok(BTreeMap::new());
        }
        // git reads the names from its repository's root, whatever the
        // directory it runs in: each is given whole.
        let fed: String = paths
            .iter()
            .map(|p| format!("{}\n", dir_path.join(p).display()))
            .collect();
        let out = self.git(dir_path, &args, Some(fed.as_bytes()))?;
        let ids: Vec<String> = String::from_utf8_lossy(&out)
            .lines()
            .map(str::to_string)
            .collect();
        Ok(paths.iter().map(|p| p.to_string()).zip(ids).collect())
    }

    fn blob_id_of(&self, dir: &str, path: &str, bytes: &[u8]) -> Result<String, TreeError> {
        let dir_path = Path::new(dir);
        // `--path` names the file git's filters are chosen by, from the
        // repository's root whatever the directory it runs in: given whole.
        let named = format!("--path={}", dir_path.join(path).display());
        let out = self.git(dir_path, &["hash-object", "--stdin", &named], Some(bytes))?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }

    fn in_motion(&self, dir: &str) -> Result<Option<Motion>, TreeError> {
        let dir = Path::new(dir);
        // Most telling first: a stopped rebase or merge also leaves files a
        // later name would match.
        let marks: [(&str, Motion); 6] = [
            ("rebase-merge", Motion::Rebasing),
            ("rebase-apply", Motion::Rebasing),
            ("MERGE_HEAD", Motion::Merging),
            ("CHERRY_PICK_HEAD", Motion::CherryPicking),
            ("REVERT_HEAD", Motion::Reverting),
            ("index.lock", Motion::IndexLocked),
        ];
        let mut args = vec!["rev-parse"];
        for (name, _) in &marks {
            args.push("--git-path");
            args.push(name);
        }
        let out = self.git(dir, &args, None)?;
        let text = String::from_utf8_lossy(&out);
        for (line, (_, motion)) in text.lines().zip(marks) {
            if self.fs.exists(&dir.join(line)).unwrap_or(false) {
                return Ok(Some(motion));
            }
        }
        Ok(None)
    }
}

impl GitTrees {
    /// The object `rev` names in `dir`'s repository; none when it names
    /// none (`rev-parse -q --verify`).
    fn verify(&self, dir: &Path, rev: &str) -> Result<Option<String>, TreeError> {
        let args = ["rev-parse", "-q", "--verify", rev];
        match self.invoke(dir, &args, None, &[])? {
            Ok(out) => Ok(Some(String::from_utf8_lossy(&out).trim().to_string())),
            Err(failure) if is_quiet_no(&failure) => Ok(None),
            Err(failure) => Err(TreeError::Failed(Box::new(failed(&args, dir, &failure)))),
        }
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;

#[cfg(test)]
#[path = "error.test.rs"]
mod error_tests;

#[cfg(test)]
#[path = "fixtures.test.rs"]
mod fixtures;
