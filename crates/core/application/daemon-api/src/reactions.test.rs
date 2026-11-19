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

/// An engine that carries a context at every hook, as claude's
/// additionalContext does; it drives nothing.
struct Carries;

impl run_ports::Engine for Carries {
    fn name(&self) -> &str {
        "claude-code"
    }
    fn prepare(
        &self,
        _: &run_ports::PrepareRequest,
    ) -> Result<run_ports::Prepared, run_ports::EngineError> {
        unreachable!("a delivery prepares nothing")
    }
    fn drive_turn(
        &self,
        _: &run_ports::TurnRequest,
    ) -> Result<run_ports::TurnCommand, run_ports::EngineError> {
        unreachable!("a delivery drives nothing")
    }
    fn finish_turn(&self, _: &run_ports::TurnOutput) -> run_ports::TurnOutcome {
        unreachable!("a delivery drives nothing")
    }
    fn drive_interactive(
        &self,
        _: &run_ports::TurnRequest,
    ) -> Result<run_ports::TurnCommand, run_ports::EngineError> {
        unreachable!("a delivery drives nothing")
    }
    fn deliver_context(&self, _: &str, context: &str) -> run_ports::ContextDelivery {
        run_ports::ContextDelivery::HookContext(context.to_string())
    }
}

const TREE: &str = "projects/calm-quiet-heron/checkouts/c/library";

/// `fact` recorded on the session `harp`'s stream, keyed `key`.
async fn on_session<F: angzarr_embedded::markup::DomainMessage>(
    f: &crate::server::tests::Fixture,
    harp: &str,
    fact: &F,
    key: &str,
) {
    use run_ports::Edge as _;
    let name = format!("projects/calm-quiet-heron/sessions/{harp}");
    let cover = angzarr_embedded::v1::Cover {
        domain: sessions::SESSION_DOMAIN.to_string(),
        root: Some(sessions::SessionName::parse(&name).unwrap().root()),
        edition: None,
    };
    f.world
        .handle(sessions::fact_book(cover, fact, key))
        .await
        .unwrap();
}

fn owed_at(sequence: u64) -> sessions::RecordContextOwed {
    sessions::RecordContextOwed {
        tree: TREE.to_string(),
        sequence,
        bundles: vec!["demo".to_string()],
    }
}

// Step 10c U13 rework, the race forced: a change recorded while a context
// is being re-assembled and handed is past the positions the delivery
// covers (read before it was assembled), so it stays owed and the next
// hook collects it. The old wall-clock mark (`context_since`, the
// delivery's own instant) lost it. A batch handed again is a DUPLICATE,
// and with nothing owed nothing is collected.
#[tokio::test]
async fn test_a_change_recorded_while_a_context_is_handed_stays_owed() {
    let f = crate::server::tests::fixture(vec![]);
    f.mint("amber-swift-owl").await;
    let loadout = sessions::RecordLoadout {
        loadout: sessions::SessionLoadout {
            agent: "dev".to_string(),
            profiles: Vec::new(),
            bundles: vec!["demo".to_string()],
        },
        incarnation: 1,
        covered: vec![sessions::TreePosition {
            tree: TREE.to_string(),
            sequence: 3,
        }],
    };
    on_session(&f, "amber-swift-owl", &loadout, "loadout:1").await;
    on_session(&f, "amber-swift-owl", &owed_at(4), "owed:4").await;
    f.catch_up().await;
    let record = || f.index.index().find("amber-swift-owl").unwrap();
    let before = record();
    let collected = f
        .api
        .collect(&before, &Carries, "PostToolUse", || async {
            // Mid-delivery: the next change lands, owed to the session.
            on_session(&f, "amber-swift-owl", &owed_at(5), "owed:5").await;
            f.catch_up().await;
            Ok("the context".to_string())
        })
        .await
        .unwrap();
    let (text, changed) = collected.expect("delivered");
    assert!(text.contains("the context"), "{text}");
    assert_eq!(changed, ["demo"]);
    f.catch_up().await;
    let handed = record().context.unwrap();
    assert_eq!(handed.covered.get(TREE), Some(&4));
    assert_eq!(
        handed.owed_positions(),
        [sessions::TreePosition {
            tree: TREE.to_string(),
            sequence: 5,
        }],
        "the change recorded mid-delivery stays owed"
    );
    // The same batch handed again (a second hook that read it first): a
    // DUPLICATE, nothing handed.
    let again = f
        .api
        .collect(&before, &Carries, "PostToolUse", || async {
            Ok("the context".to_string())
        })
        .await
        .unwrap();
    assert_eq!(again, None);
    // The next hook collects the change left owed, and then nothing is.
    let next = f
        .api
        .collect(&record(), &Carries, "UserPromptSubmit", || async {
            Ok("the context, again".to_string())
        })
        .await
        .unwrap();
    assert!(next.is_some());
    f.catch_up().await;
    assert!(record().context.unwrap().owed.is_empty());
    let none = f
        .api
        .collect(&record(), &Carries, "UserPromptSubmit", || async {
            panic!("nothing owed is never assembled")
        })
        .await
        .unwrap();
    assert_eq!(none, None);
}
