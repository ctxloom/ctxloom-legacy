use std::collections::BTreeMap;

use ctxloom_ports::{Motion, PathStatus};
use library_tree::{ChangeStatus, Library, RecordTreeObserved, TreeKind, TreeName};
use sessions::ProjectName;

use super::{sight, PATHSPEC};
use crate::fakes::{changed, Git};

const DIR: &str = "/work/demo/.ctxloom";
const BUNDLE: &str = "content/bundles/v2/demo/bundle.yaml";
const FRAGMENT: &str = "content/bundles/v2/demo/fragments/example.md";
const RENAMED: &str = "content/bundles/v2/demo/fragments/renamed.md";

fn tree() -> TreeName {
    TreeName::checkout(ProjectName::new("calm-otter").unwrap(), "brisk-copper-moth").unwrap()
}

/// The record's state after a baseline at `head` (library tree `tree`),
/// with `dirty` seen then.
fn held(head: &str, tree: &str, dirty: &[(&str, &str)]) -> Library {
    Library {
        name: super::tests::tree().to_string(),
        kind: Some(TreeKind::Checkout),
        head: head.to_string(),
        tree: tree.to_string(),
        dirty: dirty
            .iter()
            .map(|(p, o)| (p.to_string(), o.to_string()))
            .collect(),
        last_cause: "obs:earlier".to_string(),
        baselined: true,
        baseline_sequence: 0,
        ..Library::default()
    }
}

fn fact(git: &Git, held: &Library) -> Option<RecordTreeObserved> {
    sight(git, DIR, &tree(), held).unwrap().fact
}

fn changes(fact: &RecordTreeObserved) -> Vec<(String, ChangeStatus, String)> {
    fact.changes
        .iter()
        .map(|c| (c.path.clone(), c.status, c.from_path.clone()))
        .collect()
}

fn one(path: &str, status: ChangeStatus) -> Vec<(String, ChangeStatus, String)> {
    vec![(path.to_string(), status, String::new())]
}

#[test]
fn the_library_is_ctxloom_s_content_and_its_three_files() {
    assert_eq!(
        PATHSPEC,
        [
            "content/bundles/v2",
            "config.yaml",
            "remotes.yaml",
            "lock.yaml"
        ]
    );
}

#[test]
fn a_trees_first_sight_is_its_baseline_listing_what_git_sees_uncommitted() {
    let git = Git::at("c1", "t1")
        .file(BUNDLE, "b1")
        .says(BUNDLE, PathStatus::Added);
    let fact = fact(&git, &Library::default()).expect("a baseline");
    assert!(fact.baseline);
    assert_eq!(fact.name, tree().to_string());
    assert_eq!(fact.kind, TreeKind::Checkout);
    assert_eq!(fact.head_before, "");
    assert_eq!(fact.head_after, "c1");
    assert_eq!(fact.tree_after, "t1");
    assert_eq!(changes(&fact), one(BUNDLE, ChangeStatus::Added));
    assert_eq!(fact.changes[0].to_oid, "b1");
    assert_eq!(fact.dirty_after.len(), 1);
    assert_eq!(fact.dirty_after[0].oid, "b1");
    assert_eq!(fact.in_motion, None);
}

#[test]
fn a_clean_trees_baseline_lists_nothing_of_what_the_commit_holds() {
    let git = Git::at("c1", "t1").file(BUNDLE, "b1");
    let fact = fact(&git, &Library::default()).expect("a baseline");
    assert!(fact.baseline);
    assert!(fact.changes.is_empty());
    assert!(fact.dirty_after.is_empty());
}

#[test]
fn a_repository_with_no_commit_is_baselined_on_its_untracked_files() {
    let git = Git {
        head: None,
        work_tree: Some("/work/demo".to_string()),
        ..Git::default()
    }
    .file("config.yaml", "k1")
    .says("config.yaml", PathStatus::Added);
    let fact = fact(&git, &Library::default()).expect("a baseline");
    assert_eq!(fact.head_after, "");
    assert_eq!(fact.tree_after, "");
    assert_eq!(changes(&fact), one("config.yaml", ChangeStatus::Added));
}

#[test]
fn a_sight_of_nothing_new_records_nothing_and_asks_no_diff() {
    let git = Git::at("c1", "t1").file(BUNDLE, "b1");
    assert_eq!(fact(&git, &held("c1", "t1", &[])), None);
    assert!(!git.calls().iter().any(|c| c.starts_with("diff_tree")));
}

#[test]
fn a_hand_edit_of_a_committed_file_is_modified() {
    let git = Git::at("c1", "t1")
        .file(FRAGMENT, "f2")
        .says(FRAGMENT, PathStatus::Modified);
    let fact = fact(&git, &held("c1", "t1", &[])).expect("a change");
    assert!(!fact.baseline);
    assert_eq!(fact.base_cause, "obs:earlier");
    assert_eq!(changes(&fact), one(FRAGMENT, ChangeStatus::Modified));
    assert_eq!(fact.changes[0].to_oid, "f2");
    assert_eq!(fact.head_before, "c1");
    assert_eq!(fact.head_after, "c1");
}

#[test]
fn a_second_edit_of_a_file_git_already_sees_modified_is_seen_by_its_id() {
    let git = Git::at("c1", "t1")
        .file(FRAGMENT, "f3")
        .says(FRAGMENT, PathStatus::Modified);
    let fact = fact(&git, &held("c1", "t1", &[(FRAGMENT, "f2")])).expect("a change");
    assert_eq!(changes(&fact), one(FRAGMENT, ChangeStatus::Modified));
    assert_eq!(fact.changes[0].from_oid, "f2");
    assert_eq!(fact.changes[0].to_oid, "f3");
    // The same id again is nothing new.
    let same = Git::at("c1", "t1")
        .file(FRAGMENT, "f2")
        .says(FRAGMENT, PathStatus::Modified);
    assert_eq!(
        super::tests::fact(&same, &held("c1", "t1", &[(FRAGMENT, "f2")])),
        None
    );
}

#[test]
fn a_file_put_back_as_the_commit_has_it_is_a_change_back() {
    // Clean again: git says nothing of it, and its id is the commit's.
    let git = Git::at("c1", "t1").file(BUNDLE, "b1");
    let fact = fact(&git, &held("c1", "t1", &[(BUNDLE, "b2")])).expect("a change");
    assert_eq!(changes(&fact), one(BUNDLE, ChangeStatus::Modified));
    assert_eq!(fact.changes[0].to_oid, "b1");
    assert!(fact.dirty_after.is_empty());
}

#[test]
fn an_untracked_file_taken_away_is_deleted_and_one_put_back_added() {
    let gone = Git::at("c1", "t1");
    let fact = fact(&gone, &held("c1", "t1", &[(BUNDLE, "b1")])).expect("a change");
    assert_eq!(changes(&fact), one(BUNDLE, ChangeStatus::Deleted));
    assert_eq!(fact.changes[0].to_oid, "");
    let back = Git::at("c1", "t1").file(BUNDLE, "b1");
    let fact = super::tests::fact(&back, &held("c1", "t1", &[(BUNDLE, "")])).expect("a change");
    assert_eq!(changes(&fact), one(BUNDLE, ChangeStatus::Added));
}

#[test]
fn a_commit_made_outside_is_diffed_between_the_commits_it_moved() {
    let mut git = Git::at("c2", "t2").file(FRAGMENT, "f2");
    git.trees.insert("c1".to_string(), "t1".to_string());
    git.diffs.insert(
        ("c1".to_string(), "c2".to_string()),
        vec![changed(
            FRAGMENT,
            PathStatus::Modified,
            None,
            (Some("f1"), Some("f2")),
        )],
    );
    let fact = fact(&git, &held("c1", "t1", &[])).expect("a change");
    assert_eq!(fact.head_before, "c1");
    assert_eq!(fact.head_after, "c2");
    assert_eq!(fact.tree_after, "t2");
    assert_eq!(changes(&fact), one(FRAGMENT, ChangeStatus::Modified));
    assert_eq!(fact.changes[0].from_oid, "f1");
}

#[test]
fn an_edit_committed_after_it_was_seen_dirty_records_nothing_more_than_the_commit() {
    // Seen dirty at f2, then committed as f2: the tree moved, nothing changed.
    let mut git = Git::at("c2", "t2").file(FRAGMENT, "f2");
    git.trees.insert("c1".to_string(), "t1".to_string());
    git.diffs.insert(
        ("c1".to_string(), "c2".to_string()),
        vec![changed(
            FRAGMENT,
            PathStatus::Modified,
            None,
            (Some("f1"), Some("f2")),
        )],
    );
    let fact = fact(&git, &held("c1", "t1", &[(FRAGMENT, "f2")])).expect("the dirty set moved");
    assert!(fact.changes.is_empty());
    assert!(fact.dirty_after.is_empty());
}

#[test]
fn a_commit_that_touches_nothing_of_the_library_records_nothing() {
    let mut git = Git::at("c2", "t1");
    git.trees.insert("c1".to_string(), "t1".to_string());
    assert_eq!(fact(&git, &held("c1", "t1", &[])), None);
    assert!(!git.calls().iter().any(|c| c.starts_with("diff_tree")));
}

#[test]
fn a_staged_rename_is_one_change_from_its_source() {
    let git = Git::at("c1", "t1")
        .file(RENAMED, "f1")
        .renamed(RENAMED, FRAGMENT);
    let fact = fact(&git, &held("c1", "t1", &[])).expect("a change");
    assert_eq!(
        changes(&fact),
        vec![(
            RENAMED.to_string(),
            ChangeStatus::Renamed,
            FRAGMENT.to_string()
        )]
    );
    // Its source is gone from the dirty set's view of the commit.
    let dirty: BTreeMap<_, _> = fact
        .dirty_after
        .iter()
        .map(|d| (d.path.as_str(), d.oid.as_str()))
        .collect();
    assert_eq!(dirty, BTreeMap::from([(FRAGMENT, ""), (RENAMED, "f1")]));
}

#[test]
fn a_committed_rename_is_one_change_from_its_source() {
    let mut git = Git::at("c2", "t2").file(RENAMED, "f1");
    git.trees.insert("c1".to_string(), "t1".to_string());
    git.diffs.insert(
        ("c1".to_string(), "c2".to_string()),
        vec![changed(
            RENAMED,
            PathStatus::Renamed,
            Some(FRAGMENT),
            (Some("f1"), Some("f1")),
        )],
    );
    let fact = fact(&git, &held("c1", "t1", &[])).expect("a change");
    assert_eq!(
        changes(&fact),
        vec![(
            RENAMED.to_string(),
            ChangeStatus::Renamed,
            FRAGMENT.to_string()
        )]
    );
    assert_eq!(fact.changes[0].from_oid, "f1");
}

#[test]
fn a_file_moved_without_git_is_a_delete_and_an_add() {
    let git = Git::at("c1", "t1")
        .file(RENAMED, "f1")
        .says(FRAGMENT, PathStatus::Deleted)
        .says(RENAMED, PathStatus::Added);
    let fact = fact(&git, &held("c1", "t1", &[])).expect("a change");
    assert_eq!(
        changes(&fact),
        vec![
            (FRAGMENT.to_string(), ChangeStatus::Deleted, String::new()),
            (RENAMED.to_string(), ChangeStatus::Added, String::new()),
        ]
    );
}

#[test]
fn a_type_change_is_said_as_one() {
    let git = Git::at("c1", "t1")
        .file(FRAGMENT, "l1")
        .says(FRAGMENT, PathStatus::TypeChanged);
    let fact = fact(&git, &held("c1", "t1", &[])).expect("a change");
    assert_eq!(changes(&fact), one(FRAGMENT, ChangeStatus::TypeChanged));
}

#[test]
fn a_conflicted_path_is_modified_and_the_operation_said() {
    let mut git = Git::at("c1", "t1")
        .file(FRAGMENT, "f9")
        .says(FRAGMENT, PathStatus::Unmerged);
    git.motion = Some(Motion::Merging);
    let seen = sight(&git, DIR, &tree(), &held("c1", "t1", &[])).unwrap();
    assert_eq!(seen.motion, Some(Motion::Merging));
    let fact = seen.fact.expect("a change");
    assert_eq!(fact.in_motion, Some(library_tree::Motion::Merging));
    assert_eq!(changes(&fact), one(FRAGMENT, ChangeStatus::Modified));
}

#[test]
fn every_motion_is_carried_into_the_fact() {
    for (motion, said) in [
        (Motion::IndexLocked, library_tree::Motion::IndexLocked),
        (Motion::Merging, library_tree::Motion::Merging),
        (Motion::Rebasing, library_tree::Motion::Rebasing),
        (Motion::CherryPicking, library_tree::Motion::CherryPicking),
        (Motion::Reverting, library_tree::Motion::Reverting),
    ] {
        let mut git = Git::at("c1", "t1");
        git.motion = Some(motion);
        let fact = fact(&git, &Library::default()).expect("a baseline");
        assert_eq!(fact.in_motion, Some(said));
    }
}

#[test]
fn a_motion_alone_with_nothing_new_records_nothing_and_is_still_said() {
    let mut git = Git::at("c1", "t1");
    git.motion = Some(Motion::IndexLocked);
    let seen = sight(&git, DIR, &tree(), &held("c1", "t1", &[])).unwrap();
    assert_eq!(seen.fact, None);
    assert_eq!(seen.motion, Some(Motion::IndexLocked));
}

#[test]
fn a_last_seen_commit_git_no_longer_has_is_baselined_again() {
    // c1 was gc'd: git knows no tree of it.
    let git = Git::at("c2", "t2")
        .file(BUNDLE, "b1")
        .says(BUNDLE, PathStatus::Modified);
    let fact = fact(&git, &held("c1", "t1", &[])).expect("a baseline");
    assert!(fact.baseline);
    assert_eq!(changes(&fact), one(BUNDLE, ChangeStatus::Modified));
    assert!(!git.calls().iter().any(|c| c.starts_with("diff_tree")));
}

#[test]
fn a_tree_whose_commit_is_gone_from_its_branch_is_baselined_again() {
    let git = Git {
        head: None,
        work_tree: Some("/work/demo".to_string()),
        ..Git::default()
    };
    let fact = fact(&git, &held("c1", "t1", &[])).expect("a baseline");
    assert!(fact.baseline);
    assert_eq!(fact.head_after, "");
}

#[test]
fn a_library_first_committed_after_its_baseline_is_diffed_from_the_commit_before() {
    // At c1 the library had no tree; c2 holds it.
    let mut git = Git::at("c2", "t2").file(BUNDLE, "b1");
    git.diffs.insert(
        ("c1".to_string(), "c2".to_string()),
        vec![changed(BUNDLE, PathStatus::Added, None, (None, Some("b1")))],
    );
    let fact = fact(&git, &held("c1", "", &[])).expect("a change");
    assert!(!fact.baseline);
    assert_eq!(changes(&fact), one(BUNDLE, ChangeStatus::Added));
}

#[test]
fn a_first_commit_of_files_seen_untracked_records_only_the_moved_tree() {
    let git = Git::at("c1", "t1").file("config.yaml", "k1");
    let mut before = held("", "", &[("config.yaml", "k1")]);
    before.head = String::new();
    let fact = fact(&git, &before).expect("the tree and dirty set moved");
    assert!(fact.changes.is_empty());
    assert_eq!(fact.head_before, "");
    assert_eq!(fact.head_after, "c1");
    assert!(!git.calls().iter().any(|c| c.starts_with("diff_tree")));
}

#[test]
fn the_status_is_asked_over_the_library_alone() {
    let git = Git::at("c1", "t1");
    let _ = fact(&git, &held("c1", "t1", &[]));
    assert!(git
        .calls()
        .contains(&"status content/bundles/v2 config.yaml remotes.yaml lock.yaml".to_string()));
}

// A path git says is deleted has no id, whatever is still on disk (a
// `git rm --cached` keeps the file): in a baseline its id is never asked.
#[test]
fn a_path_git_says_deleted_has_no_id_whatever_is_on_disk() {
    let git = Git::at("c1", "t1")
        .file(FRAGMENT, "f1")
        .says(FRAGMENT, PathStatus::Deleted);
    let fact = fact(&git, &Library::default()).expect("a baseline");
    assert_eq!(changes(&fact), one(FRAGMENT, ChangeStatus::Deleted));
    assert_eq!(fact.changes[0].to_oid, "");
    assert_eq!(fact.dirty_after.len(), 1);
    assert_eq!(fact.dirty_after[0].oid, "");
}

// A rename's source the record held dirty is said by its target's change
// alone, never as a deletion of its own.
#[test]
fn a_staged_rename_of_a_path_seen_dirty_is_one_change_from_its_source() {
    let git = Git::at("c1", "t1")
        .file(RENAMED, "f2")
        .renamed(RENAMED, FRAGMENT);
    let fact = fact(&git, &held("c1", "t1", &[(FRAGMENT, "f2")])).expect("a change");
    assert_eq!(
        changes(&fact),
        vec![(
            RENAMED.to_string(),
            ChangeStatus::Renamed,
            FRAGMENT.to_string()
        )]
    );
}

#[test]
fn a_committed_type_change_is_said_as_one() {
    let mut git = Git::at("c2", "t2").file(FRAGMENT, "l2");
    git.trees.insert("c1".to_string(), "t1".to_string());
    git.diffs.insert(
        ("c1".to_string(), "c2".to_string()),
        vec![changed(
            FRAGMENT,
            PathStatus::TypeChanged,
            None,
            (Some("f1"), Some("l2")),
        )],
    );
    let fact = fact(&git, &held("c1", "t1", &[])).expect("a change");
    assert_eq!(changes(&fact), one(FRAGMENT, ChangeStatus::TypeChanged));
}

// A deletion already seen, still deleted, is nothing new.
#[test]
fn a_deletion_seen_before_and_still_there_records_nothing() {
    let git = Git::at("c1", "t1").says(FRAGMENT, PathStatus::Deleted);
    assert_eq!(fact(&git, &held("c1", "t1", &[(FRAGMENT, "")])), None);
}

// Past a baseline a path the record held dirty is asked its id: one git now
// says is deleted (staged, the file left on disk) is dirty with none.
#[test]
fn a_path_seen_dirty_then_deleted_is_dirty_with_no_id_whatever_is_on_disk() {
    let git = Git::at("c1", "t1")
        .file(FRAGMENT, "f1")
        .says(FRAGMENT, PathStatus::Deleted);
    let fact = fact(&git, &held("c1", "t1", &[(FRAGMENT, "f2")])).expect("a change");
    assert_eq!(changes(&fact), one(FRAGMENT, ChangeStatus::Deleted));
    assert_eq!(fact.dirty_after.len(), 1);
    assert_eq!(fact.dirty_after[0].oid, "");
}
