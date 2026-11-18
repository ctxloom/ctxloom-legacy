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
