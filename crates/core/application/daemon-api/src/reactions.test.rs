//! Step 10c U13: what a daemon round observes (G5(b)).

use session_index::{SessionRecord, State};

use super::live_dirs;

fn record(harp: &str, project: &str, dir: &str, ended: bool) -> SessionRecord {
    SessionRecord {
        harp: harp.to_string(),
        project_id: project.to_string(),
        project_dir: dir.to_string(),
        ended_at: ended.then(Default::default),
        ..SessionRecord::default()
    }
}

// A round observes each directory a live session runs in, once, and no
// other: a session whose lock does not read live is not one, and one with
// an end stamp is never probed (a round over a home of ended sessions
// probes nothing).
#[test]
fn a_round_observes_each_directory_a_live_session_runs_in_once() {
    let records = [
        record("brisk-copper-moth", "p-1", "/work/a", false),
        record("amber-quiet-heron", "p-1", "/work/a", false),
        record("calm-otter-run", "p-2", "/work/b", false),
        record("olive-quiet-finch", "p-3", "/work/c", true),
    ];
    let state = |_: &str, harp: &str| match harp {
        "calm-otter-run" => State::Ended,
        "olive-quiet-finch" => panic!("an ended session is never probed"),
        _ => State::Live,
    };
    assert_eq!(
        live_dirs(&records, state),
        [("p-1".to_string(), "/work/a".to_string())]
    );
    assert!(live_dirs(&[], |_: &str, _: &str| State::Live).is_empty());
}

// Step 10c G7 (protos.md §4.1): a recorded change is a thin notice to a
// watcher of its tree: its tree, outside or intended (with its request
// and verb), its sequence, its paths and how each changed, when it was
// recorded. A watcher of a project is told its checkouts' changes, one of
// the home the home's; of nothing else.
#[test]
fn a_recorded_change_is_a_thin_notice_to_a_watcher_of_its_tree() {
    use super::{notice_of, watches};
    let recorded = library_checkout::Recorded {
        at: 3,
        tree: "projects/p/checkouts/c/library".to_string(),
        sequence: 7,
        cause: "cmd".to_string(),
        intended: Some(("r-1".to_string(), "fragment create".to_string())),
        changes: vec![library_checkout::Changed {
            path: "content/bundles/v2/demo/fragments/rules.md".to_string(),
            status: "added".to_string(),
            from_path: String::new(),
        }],
        recorded_at: (100, 5),
    };
    let notice = notice_of(&recorded);
    assert_eq!(notice.tree, recorded.tree);
    assert_eq!(
        notice.cause,
        daemon_proto::v1::library_change::Cause::Intended as i32
    );
    assert_eq!(
        (notice.request_id.as_str(), notice.verb.as_str()),
        ("r-1", "fragment create")
    );
    assert_eq!(notice.sequence, 7);
    assert_eq!(notice.changes[0].path, recorded.changes[0].path);
    assert_eq!(notice.changes[0].status, "added");
    assert_eq!(
        notice.observed_at.map(|at| (at.seconds, at.nanos)),
        Some((100, 5))
    );
    let outside = notice_of(&library_checkout::Recorded {
        intended: None,
        ..recorded.clone()
    });
    assert_eq!(
        outside.cause,
        daemon_proto::v1::library_change::Cause::External as i32
    );
    assert!(outside.request_id.is_empty() && outside.verb.is_empty());
    assert!(watches("projects/p", &recorded.tree));
    assert!(!watches("projects/q", &recorded.tree));
    assert!(!watches("projects/p", "projects/pq/checkouts/c/library"));
    assert!(watches("users/me", "users/me/library"));
    assert!(!watches("users/me", &recorded.tree));
}
