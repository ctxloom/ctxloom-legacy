//! The tests' own files and repositories: real paths on the host, reached
//! through the file-system seam (the adapter runs a real git, which names
//! real paths), under a root the test owns and removes.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use file_system::{Fs, Perms, Rooted};
use os::Mode;
use process_env::EnvSnapshot;

/// The host's own filesystem (no root), for real paths.
pub(crate) fn fs() -> Arc<dyn Fs> {
    let os = os::host();
    let top = os.flavor().top();
    Arc::new(file_system::Os::new(os, Path::new(top)).unwrap())
}

/// Whether this host runs the tests' fake programs: unix, the one flavor
/// `fixture_program::install_program` does not refuse as `Unsupported`.
/// Elsewhere the asking test is skipped and says so on stderr, `SKIP <test>:
/// <why>`, never silently; the same test runs unchanged on unix.
pub(crate) fn runs_scripts() -> bool {
    if os::host().flavor() == os::Flavor::Unix {
        return true;
    }
    let test = std::thread::current();
    eprintln!(
        "SKIP {}: its fake programs are sh scripts (unix only)",
        test.name().unwrap_or("a test")
    );
    false
}

pub(crate) fn mkdirs(path: impl AsRef<Path>) {
    fs().create_dir_all(path.as_ref(), Mode::from_bits(0o777))
        .unwrap();
}

pub(crate) fn put(path: impl AsRef<Path>, text: &str) {
    if let Some(parent) = path.as_ref().parent() {
        mkdirs(parent);
    }
    fs().write_atomic(path.as_ref(), text.as_bytes(), Perms::Exact(Mode::FILE))
        .unwrap();
}

pub(crate) fn slurp(path: impl AsRef<Path>) -> String {
    fs().read_to_string(path.as_ref()).unwrap()
}

pub(crate) fn exists(path: impl AsRef<Path>) -> bool {
    fs().exists(path.as_ref()).unwrap()
}

/// The process's environment, as the program reads it once.
pub(crate) fn env() -> Arc<EnvSnapshot> {
    Arc::new(EnvSnapshot::from_process())
}

/// A temporary directory with a git repository at `repo` inside it (the
/// directory itself when `repo` is empty), its `HOME` the directory, so no
/// configuration of the host's is read.
pub(crate) struct Repo {
    _root: Rooted,
    /// The temporary directory, resolved.
    pub(crate) top: PathBuf,
    /// The repository's root.
    pub(crate) repo: PathBuf,
}

impl Repo {
    /// A repository at `rel` below a fresh temporary directory.
    pub(crate) fn at(rel: &str) -> Repo {
        let root = Rooted::temp().unwrap();
        let top = fs().canonicalize(root.root()).unwrap();
        let repo = if rel.is_empty() {
            top.clone()
        } else {
            top.join(rel)
        };
        mkdirs(&repo);
        let made = Repo {
            _root: root,
            top,
            repo,
        };
        made.git(&["init", "-q"]);
        made
    }

    /// A temporary directory in no repository.
    pub(crate) fn none() -> (Rooted, PathBuf) {
        let root = Rooted::temp().unwrap();
        let top = fs().canonicalize(root.root()).unwrap();
        (root, top)
    }

    /// `git args` in the repository, with a fixed identity; its output.
    pub(crate) fn git(&self, args: &[&str]) -> String {
        git_in(&self.repo, &self.top, args)
    }

    /// `rel` below the repository.
    pub(crate) fn path(&self, rel: &str) -> PathBuf {
        self.repo.join(rel)
    }

    /// `rel` below the repository, as text.
    pub(crate) fn dir(&self, rel: &str) -> String {
        self.path(rel).display().to_string()
    }

    /// Everything staged and committed as `message`.
    pub(crate) fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", message]);
    }
}

/// `git args` in `dir` with a fixed identity, `HOME` at `home` and none of
/// the caller's environment but its `PATH`; its output.
pub(crate) fn git_in(dir: &Path, home: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", env().var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// An environment for the observer: the host's `PATH` alone, `HOME` at
/// `home`, no system configuration.
pub(crate) fn env_at(home: &Path) -> Arc<EnvSnapshot> {
    let path = env().var("PATH").unwrap_or_default().to_os_string();
    let base = EnvSnapshot::from_process();
    Arc::new(EnvSnapshot::from_parts(
        base.flavor(),
        base.cwd(),
        base.temp_dir().to_path_buf(),
        [
            ("PATH".into(), path),
            ("HOME".into(), home.as_os_str().to_os_string()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ],
    ))
}
