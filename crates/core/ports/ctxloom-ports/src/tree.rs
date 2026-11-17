//! What git sees of a library tree (step 10, U12): a project checkout's
//! `.ctxloom` or the home's. git holds the library (the amendment: "this
//! should all be in git"), so ctxloom keeps no copy, manifest or digest of
//! its own: it asks git, through this port, which commit is checked out,
//! what differs from it, the object id of each file that does, what moved
//! between two commits, and whether a git operation is in the middle of
//! changing the tree.
//!
//! Every path is relative to the directory the call names (the tree's
//! root), with `/` separators, whatever repository the directory is in (a
//! project's root, a subdirectory of one, a dotfiles home): QUESTIONS.md I.
//! Nothing here writes to a repository's objects or index: status runs
//! without the optional index refresh (`--no-optional-locks`), and an object
//! id is computed without being stored.

use std::collections::BTreeMap;

use angzarr_diagnostic::Diagnostic;

use crate::Failure;

/// Why git could not be asked, in the adapter's own words. Each is a
/// failure of the environment (exit 10), never a refusal.
#[derive(Debug)]
pub enum TreeError {
    /// git is not on the `PATH` (G11).
    GitMissing(Failure),
    /// git is older than the oldest ctxloom reads a library with (G11:
    /// 2.25).
    GitTooOld(Failure),
    /// git ran and failed, or ran out its time.
    Failed(Failure),
}

impl TreeError {
    /// The adapter's diagnostic.
    pub fn diagnostic(&self) -> &dyn Diagnostic {
        match self {
            TreeError::GitMissing(cause)
            | TreeError::GitTooOld(cause)
            | TreeError::Failed(cause) => cause.as_ref(),
        }
    }

    /// The adapter's diagnostic, owned.
    pub fn into_failure(self) -> Failure {
        match self {
            TreeError::GitMissing(cause)
            | TreeError::GitTooOld(cause)
            | TreeError::Failed(cause) => cause,
        }
    }
}

/// The ignore rule that makes git ignore a path (`git check-ignore -v`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IgnoreRule {
    /// The file the rule is in, as git names it (`.gitignore`,
    /// `.git/info/exclude`, or the global excludes file's path).
    pub source: String,
    /// Its line in that file, from 1.
    pub line: u32,
    /// The rule as written (`.ctxloom/`).
    pub pattern: String,
}

/// How a path differs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathStatus {
    Added,
    Modified,
    Deleted,
    /// Moved from another path (`from_path`), as git paired them.
    Renamed,
    /// A file became a link, or a link a file.
    TypeChanged,
    /// In conflict in an operation stopped part way.
    Unmerged,
}

/// One path `git status` sees differ from the commit checked out.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StatusEntry {
    pub path: String,
    pub status: PathStatus,
    /// Where a renamed path was.
    pub from_path: Option<String>,
    /// Whether git does not track it yet (added, and not staged).
    pub untracked: bool,
}

/// One path that differs between two trees (`git diff-tree -M`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TreeChange {
    pub path: String,
    pub status: PathStatus,
    /// Where a renamed path was.
    pub from_path: Option<String>,
    /// Its object id before (none when added).
    pub from_oid: Option<String>,
    /// Its object id after (none when deleted).
    pub to_oid: Option<String>,
}

/// A git operation in progress in a tree's repository (L11).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    /// Another git process holds the index (`index.lock`).
    IndexLocked,
    Merging,
    Rebasing,
    CherryPicking,
    Reverting,
}

impl Motion {
    /// Its word in a message's selector (`index-locked`, `merging`, ...).
    pub fn word(self) -> &'static str {
        match self {
            Motion::IndexLocked => "index-locked",
            Motion::Merging => "merging",
            Motion::Rebasing => "rebasing",
            Motion::CherryPicking => "cherry-picking",
            Motion::Reverting => "reverting",
        }
    }
}

/// What git sees of a tree. Every `dir` is a directory that exists.
pub trait TreeObserver: Send + Sync {
    /// The top of the git work tree `dir` is in, as git names it; none when
    /// it is in none.
    fn work_tree(&self, dir: &str) -> Result<Option<String>, TreeError>;

    /// The rule, if any, that makes git ignore `path` (relative to `dir`),
    /// whether or not git tracks it.
    fn check_ignore(&self, dir: &str, path: &str) -> Result<Option<IgnoreRule>, TreeError>;

    /// Make `dir` a git repository of its own (`git init`), committing
    /// nothing. `ignore`, when given, is written as its `.gitignore` first,
    /// so git never sees the directory without it, and only where there is
    /// none: one there is left as it is.
    fn init_repository(&self, dir: &str, ignore: Option<&str>) -> Result<(), TreeError>;

    /// The commit checked out; none before the first commit.
    fn head(&self, dir: &str) -> Result<Option<String>, TreeError>;

    /// The object id of the tree `dir` is in `commit`; none when the commit
    /// holds nothing there.
    fn library_tree(&self, dir: &str, commit: &str) -> Result<Option<String>, TreeError>;

    /// Each path under `paths` that differs from the commit checked out
    /// (staged or not), and each git does not track, every file of an
    /// untracked directory named; sorted by path.
    fn status(&self, dir: &str, paths: &[&str]) -> Result<Vec<StatusEntry>, TreeError>;

    /// Each path under `paths` that differs between the trees (or commits)
    /// `from` and `to`, renames paired; sorted by path.
    fn diff_tree(
        &self,
        dir: &str,
        from: &str,
        to: &str,
        paths: &[&str],
    ) -> Result<Vec<TreeChange>, TreeError>;

    /// The object id git gives each file of `paths` as it is now (its
    /// filters applied, nothing stored), by path.
    fn blob_ids(&self, dir: &str, paths: &[&str]) -> Result<BTreeMap<String, String>, TreeError>;

    /// The git operation in progress in `dir`'s repository, if any.
    fn in_motion(&self, dir: &str) -> Result<Option<Motion>, TreeError>;
}

#[cfg(test)]
#[path = "tree.test.rs"]
mod tests;
