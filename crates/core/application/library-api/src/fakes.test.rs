//! A git that answers from a table, and counts what it is asked.

use std::collections::BTreeMap;
use std::sync::Mutex;

use ctxloom_ports::{
    IgnoreRule, Motion, PathStatus, StatusEntry, TreeChange, TreeError, TreeObserver,
};

/// What the fake git holds: the commit checked out, each commit's library
/// tree id, the files on disk by id, what status says, the diffs between
/// commits, the operation in progress, the work tree and the ignore rule.
#[derive(Default)]
pub(crate) struct Git {
    pub(crate) head: Option<String>,
    pub(crate) trees: BTreeMap<String, String>,
    pub(crate) files: BTreeMap<String, String>,
    pub(crate) status: Vec<StatusEntry>,
    pub(crate) diffs: BTreeMap<(String, String), Vec<TreeChange>>,
    pub(crate) motion: Option<Motion>,
    pub(crate) work_tree: Option<String>,
    pub(crate) ignored: Option<IgnoreRule>,
    pub(crate) asked: Mutex<Vec<String>>,
}

impl Git {
    /// A repository at `head`, its library tree `tree`.
    pub(crate) fn at(head: &str, tree: &str) -> Git {
        Git {
            head: Some(head.to_string()),
            trees: BTreeMap::from([(head.to_string(), tree.to_string())]),
            work_tree: Some("/work/demo".to_string()),
            ..Git::default()
        }
    }

    /// The file `path` on disk, its id `oid`.
    pub(crate) fn file(mut self, path: &str, oid: &str) -> Git {
        self.files.insert(path.to_string(), oid.to_string());
        self
    }

    /// `git status` saying `path` is `status`.
    pub(crate) fn says(mut self, path: &str, status: PathStatus) -> Git {
        self.status.push(entry(path, status, None));
        self.status.sort_by(|a, b| a.path.cmp(&b.path));
        self
    }

    /// `git status` pairing `path` with `from` as a rename.
    pub(crate) fn renamed(mut self, path: &str, from: &str) -> Git {
        self.status
            .push(entry(path, PathStatus::Renamed, Some(from)));
        self.status.sort_by(|a, b| a.path.cmp(&b.path));
        self
    }

    /// Every call asked so far, by its method's name.
    pub(crate) fn calls(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }

    fn ask(&self, call: &str) {
        self.asked.lock().unwrap().push(call.to_string());
    }
}

pub(crate) fn entry(path: &str, status: PathStatus, from: Option<&str>) -> StatusEntry {
    StatusEntry {
        path: path.to_string(),
        status,
        from_path: from.map(str::to_string),
        untracked: status == PathStatus::Added,
    }
}

pub(crate) fn changed(
    path: &str,
    status: PathStatus,
    from: Option<&str>,
    ids: (Option<&str>, Option<&str>),
) -> TreeChange {
    TreeChange {
        path: path.to_string(),
        status,
        from_path: from.map(str::to_string),
        from_oid: ids.0.map(str::to_string),
        to_oid: ids.1.map(str::to_string),
    }
}

impl TreeObserver for Git {
    fn work_tree(&self, _dir: &str) -> Result<Option<String>, TreeError> {
        self.ask("work_tree");
        Ok(self.work_tree.clone())
    }

    fn check_ignore(&self, _dir: &str, path: &str) -> Result<Option<IgnoreRule>, TreeError> {
        self.ask(&format!("check_ignore {path}"));
        Ok(self.ignored.clone())
    }

    fn init_repository(&self, _dir: &str, _ignore: Option<&str>) -> Result<(), TreeError> {
        self.ask("init_repository");
        Ok(())
    }

    fn head(&self, _dir: &str) -> Result<Option<String>, TreeError> {
        self.ask("head");
        Ok(self.head.clone())
    }

    fn library_tree(&self, _dir: &str, commit: &str) -> Result<Option<String>, TreeError> {
        self.ask(&format!("library_tree {commit}"));
        Ok(self.trees.get(commit).cloned())
    }

    fn status(&self, _dir: &str, paths: &[&str]) -> Result<Vec<StatusEntry>, TreeError> {
        self.ask(&format!("status {}", paths.join(" ")));
        Ok(self.status.clone())
    }

    fn diff_tree(
        &self,
        _dir: &str,
        from: &str,
        to: &str,
        _paths: &[&str],
    ) -> Result<Vec<TreeChange>, TreeError> {
        self.ask(&format!("diff_tree {from} {to}"));
        Ok(self
            .diffs
            .get(&(from.to_string(), to.to_string()))
            .cloned()
            .unwrap_or_default())
    }

    fn blob_ids(&self, _dir: &str, paths: &[&str]) -> Result<BTreeMap<String, String>, TreeError> {
        self.ask(&format!("blob_ids {}", paths.join(" ")));
        Ok(paths
            .iter()
            .filter_map(|p| self.files.get(*p).map(|oid| (p.to_string(), oid.clone())))
            .collect())
    }

    fn in_motion(&self, _dir: &str) -> Result<Option<Motion>, TreeError> {
        self.ask("in_motion");
        Ok(self.motion)
    }
}
