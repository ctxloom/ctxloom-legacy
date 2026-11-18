use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ctxloom_ports::{Operated, Operation, Operations, Point, Put};
use file_system::{Fs, Perms, Rooted};
use os::Mode;

use super::TreeOperations;

const TREE: &str = "/proj/.ctxloom";
const REQUEST: &str = "01a1ecdd-5f31-75fa-8782-789facb7078e";

fn tree() -> String {
    os::host().flavor().from_top(TREE)
}

fn at(rel: &str) -> PathBuf {
    rel.split('/')
        .fold(PathBuf::from(tree()), |path, step| path.join(step))
}

fn setup() -> (Arc<Rooted>, TreeOperations) {
    let fs = Arc::new(Rooted::temp().expect("a temporary root"));
    fs.create_dir_all(&at(""), Mode::from_bits(0o755)).unwrap();
    let ops = TreeOperations::new(fs.clone());
    (fs, ops)
}

fn put(fs: &Rooted, rel: &str, bytes: &[u8]) {
    let path = at(rel);
    fs.create_dir_all(path.parent().unwrap(), Mode::from_bits(0o755))
        .unwrap();
    fs.write_atomic(&path, bytes, Perms::Exact(Mode::from_bits(0o644)))
        .unwrap();
}

fn read(fs: &Rooted, rel: &str) -> Option<Vec<u8>> {
    fs.read(&at(rel)).ok()
}

/// Every file below `rel`, relative to it, sorted.
fn files(fs: &Rooted, rel: &str) -> Vec<String> {
    fn walk(fs: &Rooted, dir: &Path, prefix: &str, out: &mut Vec<String>) {
        let Ok(entries) = fs.read_dir(dir) else {
            return;
        };
        for entry in entries {
            let name = entry.name.to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            match entry.kind {
                file_system::Kind::Dir => walk(fs, &dir.join(&entry.name), &rel, out),
                _ => out.push(rel),
            }
        }
    }
    let mut out = Vec::new();
    walk(fs, &at(rel), "", &mut out);
    out.sort();
    out
}

/// Run `operation`, recording the points it reached, and at each the
/// files then under the staging.
fn operate(
    fs: &Rooted,
    ops: &TreeOperations,
    operation: &Operation,
) -> (Operated, Vec<(Point, Vec<String>)>) {
    let seen = RefCell::new(Vec::new());
    let done = ops
        .operate(&tree(), REQUEST, operation, &|point| {
            seen.borrow_mut().push((point, files(fs, "state/staging")));
        })
        .map_err(|e| e.to_string())
        .expect("the operation");
    (done, seen.into_inner())
}

fn new_bundle() -> Operation {
    Operation::CreateDir {
        path: "content/bundles/v2/notes".to_string(),
        files: vec![
            Put::file("bundle.yaml", "description: my notes\n"),
            Put::file("fragments/a.md", "a\n"),
        ],
    }
}

// A new directory is built whole under the request's staging, and only
// then renamed in: at its `staged` point the target is not there yet.
#[test]
fn test_a_new_directory_is_staged_whole_under_state_and_then_renamed_in() {
    let (fs, ops) = setup();
    let (done, seen) = operate(&fs, &ops, &new_bundle());
    assert_eq!(done, Operated::Done);
    assert_eq!(
        seen,
        vec![(
            Point::Staged,
            vec![
                format!("{REQUEST}/notes/bundle.yaml"),
                format!("{REQUEST}/notes/fragments/a.md"),
            ]
        )]
    );
    assert_eq!(
        files(&fs, "content/bundles/v2/notes"),
        vec!["bundle.yaml", "fragments/a.md"]
    );
    assert_eq!(
        read(&fs, "content/bundles/v2/notes/bundle.yaml").as_deref(),
        Some(b"description: my notes\n".as_slice())
    );
    assert!(
        files(&fs, "state/staging").is_empty(),
        "nothing staged left"
    );
}

// NOREPLACE (D4): a directory that appeared meanwhile is never renamed
// over; it is left as it was, and the staging is dropped.
#[test]
fn test_a_new_directory_is_never_renamed_over_one_that_appeared_meanwhile() {
    let (fs, ops) = setup();
    let other = RefCell::new(false);
    let done = ops
        .operate(&tree(), REQUEST, &new_bundle(), &|point| {
            if point == Point::Staged {
                put(&fs, "content/bundles/v2/notes/bundle.yaml", b"other\n");
                *other.borrow_mut() = true;
            }
        })
        .map_err(|e| e.to_string())
        .unwrap();
    assert!(other.into_inner());
    assert_eq!(
        done,
        Operated::Exists {
            path: "content/bundles/v2/notes".to_string()
        }
    );
    assert_eq!(files(&fs, "content/bundles/v2/notes"), vec!["bundle.yaml"]);
    assert_eq!(
        read(&fs, "content/bundles/v2/notes/bundle.yaml").as_deref(),
        Some(b"other\n".as_slice())
    );
    assert!(
        files(&fs, "state/staging").is_empty(),
        "nothing staged left"
    );
}

// A file is written under the staging and renamed over its target; no
// temporary is ever beside the target.
#[test]
fn test_a_file_is_replaced_from_the_staging_never_from_beside_it() {
    let (fs, ops) = setup();
    put(&fs, "content/bundles/v2/demo/fragments/a.md", b"old\n");
    let beside = RefCell::new(Vec::new());
    let done = ops
        .operate(
            &tree(),
            REQUEST,
            &Operation::Replace(Put::file("content/bundles/v2/demo/fragments/a.md", "new\n")),
            &|point| {
                assert_eq!(point, Point::Staged);
                *beside.borrow_mut() = files(&fs, "content/bundles/v2/demo/fragments");
                assert_eq!(files(&fs, "state/staging"), vec![format!("{REQUEST}/a.md")]);
            },
        )
        .map_err(|e| e.to_string())
        .unwrap();
    assert_eq!(done, Operated::Done);
    assert_eq!(
        beside.into_inner(),
        vec!["a.md"],
        "only the target beside it"
    );
    assert_eq!(
        read(&fs, "content/bundles/v2/demo/fragments/a.md").as_deref(),
        Some(b"new\n".as_slice())
    );
    assert!(files(&fs, "state/staging").is_empty());
}

// A file in a directory not yet there (a first fragment) has its
// directory made.
#[test]
fn test_a_file_is_replaced_into_a_directory_not_yet_there() {
    let (fs, ops) = setup();
    let (done, _) = operate(
        &fs,
        &ops,
        &Operation::Replace(Put::file("config.yaml", "x: 1\n")),
    );
    assert_eq!(done, Operated::Done);
    assert_eq!(
        read(&fs, "config.yaml").as_deref(),
        Some(b"x: 1\n".as_slice())
    );
}

// A removal renames the path into its request's trash slot, whole, at
// the path it had below the tree; then its `trashed` point.
#[test]
fn test_a_removal_renames_the_path_into_its_request_s_trash_slot_whole() {
    let (fs, ops) = setup();
    put(&fs, "content/bundles/v2/demo/bundle.yaml", b"d\n");
    put(&fs, "content/bundles/v2/demo/fragments/a.md", b"a\n");
    let (done, seen) = operate(
        &fs,
        &ops,
        &Operation::Trash {
            path: "content/bundles/v2/demo".to_string(),
        },
    );
    assert_eq!(done, Operated::Done);
    assert_eq!(seen, vec![(Point::Trashed, Vec::new())]);
    assert!(files(&fs, "content/bundles/v2/demo").is_empty());
    assert_eq!(
        files(&fs, &format!("state/trash/{REQUEST}")),
        vec![
            "content/bundles/v2/demo/bundle.yaml",
            "content/bundles/v2/demo/fragments/a.md"
        ]
    );
}

// A removal retried after its rename (the path in the slot, gone from the
// tree) is done; one of nothing is missing.
#[test]
fn test_a_removal_already_in_its_slot_is_done_and_one_of_nothing_is_missing() {
    let (fs, ops) = setup();
    put(&fs, "content/bundles/v2/demo/bundle.yaml", b"d\n");
    let trash = Operation::Trash {
        path: "content/bundles/v2/demo".to_string(),
    };
    assert_eq!(operate(&fs, &ops, &trash).0, Operated::Done);
    assert_eq!(operate(&fs, &ops, &trash).0, Operated::Done);
    let other = ops
        .operate(
            &tree(),
            "0199aaaa-0000-7000-8000-000000000000",
            &trash,
            &|_| {},
        )
        .map_err(|e| e.to_string())
        .unwrap();
    assert_eq!(
        other,
        Operated::Missing {
            path: "content/bundles/v2/demo".to_string()
        }
    );
}

// A move is one rename, never over what is at its target.
#[test]
fn test_a_move_is_one_rename_never_over_its_target() {
    let (fs, ops) = setup();
    put(&fs, "content/bundles/v2/a/bundle.yaml", b"a\n");
    put(&fs, "content/bundles/v2/b/bundle.yaml", b"b\n");
    let over = Operation::Move {
        from: "content/bundles/v2/a".to_string(),
        to: "content/bundles/v2/b".to_string(),
        into: None,
    };
    assert_eq!(
        operate(&fs, &ops, &over).0,
        Operated::Exists {
            path: "content/bundles/v2/b".to_string()
        }
    );
    assert_eq!(
        read(&fs, "content/bundles/v2/b/bundle.yaml").as_deref(),
        Some(b"b\n".as_slice())
    );
    let moved = Operation::Move {
        from: "content/bundles/v2/a".to_string(),
        to: "content/bundles/v2/team/c".to_string(),
        into: None,
    };
    assert_eq!(operate(&fs, &ops, &moved).0, Operated::Done);
    assert_eq!(
        read(&fs, "content/bundles/v2/team/c/bundle.yaml").as_deref(),
        Some(b"a\n".as_slice())
    );
    assert!(files(&fs, "content/bundles/v2/a").is_empty());
}

// A request's earlier staging (a try killed after staging, then retried
// by the same daemon) is dropped before it builds afresh.
#[test]
fn test_a_retry_drops_its_request_s_earlier_staging_first() {
    let (fs, ops) = setup();
    put(
        &fs,
        &format!("state/staging/{REQUEST}/notes/stale.md"),
        b"x\n",
    );
    let (done, seen) = operate(&fs, &ops, &new_bundle());
    assert_eq!(done, Operated::Done);
    assert!(
        !seen[0].1.iter().any(|f| f.ends_with("stale.md")),
        "{seen:?}"
    );
    assert_eq!(
        files(&fs, "content/bundles/v2/notes"),
        vec!["bundle.yaml", "fragments/a.md"]
    );
}

// A daemon's start sweeps the staging whole; the trash is left.
#[test]
fn test_the_sweep_drops_the_staging_and_leaves_the_trash() {
    let (fs, ops) = setup();
    put(&fs, "state/staging/a/notes/bundle.yaml", b"x\n");
    put(
        &fs,
        "state/trash/b/content/bundles/v2/demo/bundle.yaml",
        b"d\n",
    );
    ops.sweep_staging(&tree())
        .map_err(|e| e.to_string())
        .unwrap();
    assert!(files(&fs, "state/staging").is_empty());
    assert_eq!(
        files(&fs, "state/trash"),
        vec!["b/content/bundles/v2/demo/bundle.yaml"]
    );
    ops.sweep_staging(&tree())
        .map_err(|e| e.to_string())
        .expect("a sweep of nothing is done");
}

// A slot is purged whole, and only its own.
#[test]
fn test_a_purge_drops_its_slot_alone() {
    let (fs, ops) = setup();
    put(&fs, "state/trash/a/content/x.md", b"x\n");
    put(&fs, "state/trash/b/content/y.md", b"y\n");
    ops.purge_slot(&tree(), "a")
        .map_err(|e| e.to_string())
        .unwrap();
    assert_eq!(files(&fs, "state/trash"), vec!["b/content/y.md"]);
    ops.purge_slot(&tree(), "a")
        .map_err(|e| e.to_string())
        .expect("a purge of nothing is done");
}

// Step 10b U9 (T28): a sequence's last step purges its OWN slot, whole,
// and a retry of it, the slot gone, is done.
#[test]
fn test_a_purge_operation_drops_its_requests_own_slot() {
    let (fs, ops) = setup();
    put(&fs, &format!("state/trash/{REQUEST}/content/x.md"), b"x\n");
    put(&fs, "state/trash/other/content/y.md", b"y\n");
    assert_eq!(operate(&fs, &ops, &Operation::Purge).0, Operated::Done);
    assert_eq!(files(&fs, "state/trash"), vec!["other/content/y.md"]);
    assert_eq!(operate(&fs, &ops, &Operation::Purge).0, Operated::Done);
}

// A path that climbs out of the tree, or a request id that is no plain
// name, is refused before anything is touched.
#[test]
fn test_a_path_or_request_that_climbs_is_refused_and_touches_nothing() {
    let (fs, ops) = setup();
    put(&fs, "content/x.md", b"x\n");
    for (request, operation) in [
        (REQUEST, Operation::Replace(Put::file("../outside.md", "x"))),
        (
            REQUEST,
            Operation::Trash {
                path: "content/../../x".to_string(),
            },
        ),
        (
            "../escape",
            Operation::Trash {
                path: "content/x.md".to_string(),
            },
        ),
    ] {
        let refused = ops.operate(&tree(), request, &operation, &|_| {});
        assert!(refused.is_err(), "{operation:?} under {request}");
    }
    assert_eq!(
        read(&fs, "content/x.md").as_deref(),
        Some(b"x\n".as_slice())
    );
    assert!(files(&fs, "state").is_empty());
}

// A path through a link inside the tree is refused, so a rename never
// lands outside it.
#[test]
fn test_a_path_through_a_link_is_refused() {
    if os::host().flavor() != os::Flavor::Unix {
        let test = std::thread::current();
        eprintln!(
            "SKIP {}: a directory link is a junction there, made with a privilege",
            test.name().unwrap_or("a test")
        );
        return;
    }
    let (fs, ops) = setup();
    fs.create_dir_all(
        &PathBuf::from(os::host().flavor().from_top("/elsewhere")),
        Mode::from_bits(0o755),
    )
    .unwrap();
    fs.create_dir_all(&at("content"), Mode::from_bits(0o755))
        .unwrap();
    fs.symlink_dir(
        &PathBuf::from(os::host().flavor().from_top("/elsewhere")),
        &at("content/link"),
    )
    .unwrap();
    let refused = ops.operate(
        &tree(),
        REQUEST,
        &Operation::Replace(Put::file("content/link/x.md", "x")),
        &|_| {},
    );
    assert!(refused.is_err());
    assert!(fs
        .read(&PathBuf::from(
            os::host().flavor().from_top("/elsewhere/x.md")
        ))
        .is_err());
}

// Step 10b (QUESTIONS 1(a)): a move into ANOTHER tree is one rename, its
// target below that tree, never over what is there; a retry after it is
// done.
#[test]
fn test_a_move_into_another_tree_is_one_rename_below_it_never_over_its_target() {
    let (fs, ops) = setup();
    let other = os::host().flavor().from_top("/other/.ctxloom");
    fs.create_dir_all(Path::new(&other), Mode::from_bits(0o755))
        .unwrap();
    put(&fs, "content/bundles/v2/demo/bundle.yaml", b"d\n");
    let moved = Operation::Move {
        from: "content/bundles/v2/demo".to_string(),
        to: "content/bundles/v2/demo".to_string(),
        into: Some(other.clone()),
    };
    assert_eq!(operate(&fs, &ops, &moved).0, Operated::Done);
    let landed = Path::new(&other).join("content/bundles/v2/demo/bundle.yaml");
    assert_eq!(fs.read(&landed).unwrap(), b"d\n");
    assert!(files(&fs, "content/bundles/v2/demo").is_empty());
    // Retried after its rename: done, the bundle where it went.
    assert_eq!(operate(&fs, &ops, &moved).0, Operated::Done);
    put(&fs, "content/bundles/v2/demo/bundle.yaml", b"again\n");
    assert_eq!(
        operate(&fs, &ops, &moved).0,
        Operated::Exists {
            path: "content/bundles/v2/demo".to_string()
        }
    );
    assert_eq!(fs.read(&landed).unwrap(), b"d\n");
}
