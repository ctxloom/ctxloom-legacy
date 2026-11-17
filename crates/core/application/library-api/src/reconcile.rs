//! The reconcile (step 10a U2, L5, L6, L8): what git sees of a tree now,
//! compared with what the record last held of it, as the one observation a
//! read records, or none when nothing differs.
//!
//! git is asked, in order: the operation in progress (L11), the commit
//! checked out and the library's tree id in it, `status` over the library's
//! pathspec (L13), and the object id of every path that differs or differed
//! (a second edit of a file git already sees modified is seen by its id, L8).
//! When the library's tree id moved since the record's, the commits are
//! diffed (`diff-tree -M`, so a committed rename is one, L7); a commit that
//! changes nothing of the library diffs nothing (QUESTIONS A). A tree whose
//! last-seen commit git no longer has (gc'd) is baselined again.
//!
//! A path's change is judged against what the record knew of it: its id when
//! it was last dirty, else the id the commit diff gives it, else "present at
//! HEAD". It is added when it was absent and is there now, deleted when it
//! was there and is gone, renamed where git paired it, otherwise modified.

use std::collections::{BTreeMap, BTreeSet};

use ctxloom_ports::{Motion, PathStatus, StatusEntry, TreeChange, TreeError, TreeObserver};
use library_tree::{Change, ChangeStatus, DirtyPath, Library, RecordTreeObserved, TreeName};

/// The library's paths in a tree (L13): ctxloom's own content and its three
/// files. Nothing under `sessions/`, `state/` or `cache/` is the library.
pub const PATHSPEC: [&str; 4] = [
    "content/bundles/v2",
    "config.yaml",
    "remotes.yaml",
    "lock.yaml",
];

/// What a read saw of its tree: the observation to record (none when
/// nothing differs from the record), and the git operation in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct Sight {
    pub fact: Option<RecordTreeObserved>,
    pub motion: Option<Motion>,
}

/// What the record knew of a path before this sight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Before<'a> {
    /// Its id (empty: absent).
    Known(&'a str),
    /// Present at the commit, its id not asked.
    AtHead,
}

impl Before<'_> {
    fn absent(self) -> bool {
        self == Before::Known("")
    }
}

/// How git paired or classed a path now, where it said.
#[derive(Clone, Debug, Default)]
struct Said {
    renamed_from: Option<String>,
    type_changed: bool,
    /// Absent at the commit checked out: git says it was added.
    added: bool,
}

/// What `observer` sees of the tree `dir` (named `name`) against `held`,
/// the record's state of it (a default one for a tree never observed).
pub fn sight(
    observer: &dyn TreeObserver,
    dir: &str,
    name: &TreeName,
    held: &Library,
) -> Result<Sight, TreeError> {
    let motion = observer.in_motion(dir)?;
    let head = observer.head(dir)?.unwrap_or_default();
    let tree_after = match head.as_str() {
        "" => String::new(),
        commit => observer.library_tree(dir, commit)?.unwrap_or_default(),
    };
    let status = observer.status(dir, &PATHSPEC)?;
    let mut baseline = !held.baselined;
    let committed = if baseline || tree_after == held.tree || held.head.is_empty() {
        Vec::new()
    } else if head.is_empty()
        || (!held.tree.is_empty() && observer.library_tree(dir, &held.head)?.is_none())
    {
        // The commit last seen is gone (gc'd, or the branch with it): the
        // tree is baselined again.
        baseline = true;
        Vec::new()
    } else {
        observer.diff_tree(dir, &held.head, &head, &PATHSPEC)?
    };
    let renamed_away = renamed_sources(&status, &committed);
    // Every path whose id is asked: the dirty ones there now, and (past
    // the baseline) the ones the record held dirty or the commits moved. A
    // path with no file is left out of the answer: absent.
    let mut asked: BTreeSet<&str> = status
        .iter()
        .filter(|entry| entry.status != PathStatus::Deleted)
        .map(|entry| entry.path.as_str())
        .collect();
    if !baseline {
        asked.extend(held.dirty.keys().map(String::as_str));
        asked.extend(committed.iter().map(|change| change.path.as_str()));
    }
    let ids = observer.blob_ids(dir, &asked.iter().copied().collect::<Vec<_>>())?;
    let dirty_after = dirty_of(&status, &ids);
    let changes = if baseline {
        baseline_changes(&status, &ids)
    } else {
        changes_since(held, &status, &committed, &ids, &renamed_away)
    };
    let same = changes.is_empty()
        && !baseline
        && dirty_after
            .iter()
            .map(|dirty| (dirty.path.as_str(), dirty.oid.as_str()))
            .eq(held.dirty.iter().map(|(p, o)| (p.as_str(), o.as_str())))
        && tree_after == held.tree;
    let fact = (!same).then(|| RecordTreeObserved {
        name: name.to_string(),
        kind: name.kind(),
        base_cause: held.last_cause.clone(),
        head_before: held.head.clone(),
        head_after: head,
        tree_after,
        changes,
        dirty_after,
        baseline,
        in_motion: motion.map(motion_of),
    });
    Ok(Sight { fact, motion })
}

/// The paths a rename (staged or committed) moved away from.
fn renamed_sources(status: &[StatusEntry], committed: &[TreeChange]) -> BTreeSet<String> {
    status
        .iter()
        .filter_map(|entry| entry.from_path.clone())
        .chain(
            committed
                .iter()
                .filter_map(|change| change.from_path.clone()),
        )
        .collect()
}

/// The dirty set git sees now, by path, sorted: each path's id, empty when
/// deleted (a rename's source among them).
fn dirty_of(status: &[StatusEntry], ids: &BTreeMap<String, String>) -> Vec<DirtyPath> {
    let mut dirty: BTreeMap<&str, String> = BTreeMap::new();
    for entry in status {
        let oid = match entry.status {
            PathStatus::Deleted => String::new(),
            _ => ids.get(&entry.path).cloned().unwrap_or_default(),
        };
        dirty.insert(&entry.path, oid);
        if let Some(from) = &entry.from_path {
            dirty.entry(from).or_default();
        }
    }
    dirty
        .into_iter()
        .map(|(path, oid)| DirtyPath {
            path: path.to_string(),
            oid,
        })
        .collect()
}

/// A baseline's changes: what git sees uncommitted, as git classes it.
fn baseline_changes(status: &[StatusEntry], ids: &BTreeMap<String, String>) -> Vec<Change> {
    status
        .iter()
        .map(|entry| Change {
            path: entry.path.clone(),
            status: status_of(entry.status),
            from_oid: String::new(),
            // A deleted path's id is never asked in a baseline: none.
            to_oid: ids.get(&entry.path).cloned().unwrap_or_default(),
            from_path: entry.from_path.clone().unwrap_or_default(),
        })
        .collect()
}

/// git's class of a path in a baseline.
fn status_of(status: PathStatus) -> ChangeStatus {
    match status {
        PathStatus::Added => ChangeStatus::Added,
        PathStatus::Deleted => ChangeStatus::Deleted,
        PathStatus::Renamed => ChangeStatus::Renamed,
        PathStatus::TypeChanged => ChangeStatus::TypeChanged,
        PathStatus::Modified | PathStatus::Unmerged => ChangeStatus::Modified,
    }
}

fn motion_of(motion: Motion) -> library_tree::Motion {
    match motion {
        Motion::IndexLocked => library_tree::Motion::IndexLocked,
        Motion::Merging => library_tree::Motion::Merging,
        Motion::Rebasing => library_tree::Motion::Rebasing,
        Motion::CherryPicking => library_tree::Motion::CherryPicking,
        Motion::Reverting => library_tree::Motion::Reverting,
    }
}

/// The changes since the record's state: each path git sees dirty now,
/// each the record held dirty, and each the commits moved, judged against
/// what the record knew of it.
fn changes_since(
    held: &Library,
    status: &[StatusEntry],
    committed: &[TreeChange],
    ids: &BTreeMap<String, String>,
    renamed_away: &BTreeSet<String>,
) -> Vec<Change> {
    let mut said: BTreeMap<&str, Said> = BTreeMap::new();
    let mut gone: BTreeSet<&str> = BTreeSet::new();
    for entry in status {
        let at = said.entry(&entry.path).or_default();
        at.renamed_from.clone_from(&entry.from_path);
        at.type_changed = entry.status == PathStatus::TypeChanged;
        at.added = entry.status == PathStatus::Added;
        if entry.status == PathStatus::Deleted {
            gone.insert(&entry.path);
        }
    }
    let mut diffed: BTreeMap<&str, &TreeChange> = BTreeMap::new();
    for change in committed {
        diffed.insert(&change.path, change);
        let at = said.entry(&change.path).or_default();
        if at.renamed_from.is_none() {
            at.renamed_from.clone_from(&change.from_path);
        }
        at.type_changed |= change.status == PathStatus::TypeChanged;
    }
    let mut paths: BTreeSet<&str> = said.keys().copied().collect();
    paths.extend(held.dirty.keys().map(String::as_str));
    let mut changes = Vec::new();
    for path in paths {
        let now: &str = match gone.contains(path) {
            true => "",
            false => ids.get(path).map_or("", String::as_str),
        };
        let at = said.get(path).cloned().unwrap_or_default();
        if renamed_away.contains(path) && now.is_empty() && at.renamed_from.is_none() {
            // A rename's source: said by its target's change.
            continue;
        }
        let diff = diffed.get(path);
        let before = match (held.dirty.get(path), &at.renamed_from, diff) {
            (Some(oid), _, _) => Before::Known(oid),
            // A renamed path was absent where it is now.
            (None, Some(_), _) => Before::Known(""),
            (None, None, Some(change)) => Before::Known(change.from_oid.as_deref().unwrap_or("")),
            // Absent at the commit, and clean at the last sight.
            (None, None, None) if at.added => Before::Known(""),
            (None, None, None) => Before::AtHead,
        };
        let status = match (before, now) {
            (before, "") if before.absent() => continue,
            (_, "") => ChangeStatus::Deleted,
            (Before::Known(oid), now) if oid == now => continue,
            _ if at.renamed_from.is_some() => ChangeStatus::Renamed,
            (before, _) if before.absent() => ChangeStatus::Added,
            _ if at.type_changed => ChangeStatus::TypeChanged,
            _ => ChangeStatus::Modified,
        };
        let from_oid = match (status, before, diff) {
            (ChangeStatus::Renamed, _, Some(change)) => change.from_oid.clone().unwrap_or_default(),
            (_, Before::Known(oid), _) => oid.to_string(),
            _ => String::new(),
        };
        changes.push(Change {
            path: path.to_string(),
            status,
            from_oid,
            to_oid: now.to_string(),
            from_path: match status {
                ChangeStatus::Renamed => at.renamed_from.clone().unwrap_or_default(),
                _ => String::new(),
            },
        });
    }
    changes
}

#[cfg(test)]
#[path = "reconcile.test.rs"]
mod tests;
