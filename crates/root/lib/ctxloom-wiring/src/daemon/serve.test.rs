//! The ports the root gives the daemon's launcher (T18), each over a
//! temporary home.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use daemon_application::{ChildHomes, Ears, HandOffs};
use run_ports::{ChildObservation, Exit};

use super::{advertised, said, step_of, Advertised, HandOffFiles, Homes, Pumps, Say};
use crate::daemon::files::DaemonFiles;
use crate::fixtures;

const SESSION: &str = "5f0c7a52-2222-5222-8222-000000000002";

// T6: a child's hand-off is read at each start and left for its life,
// listed while it is there, and deleted.
#[test]
fn a_hand_off_is_read_as_it_was_left_listed_and_deleted() {
    let root = fixtures::root();
    let home = root.root().join("home");
    let files = DaemonFiles::new(fixtures::fs(), &home);
    let handed = daemon_application::HandOff {
        env: BTreeMap::from([("KEY".to_string(), "v".to_string())]),
        launcher: BTreeMap::from([("XDG_CONFIG_HOME".to_string(), "/c".to_string())]),
        agent: "reviewer".to_string(),
    };
    files.hand_off(SESSION, &handed).unwrap();
    let hand_offs = HandOffFiles(DaemonFiles::new(fixtures::fs(), &home));
    assert_eq!(hand_offs.read(SESSION), Some(handed.clone()));
    assert_eq!(hand_offs.read(SESSION), Some(handed.clone()), "read again");
    let listed = hand_offs.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session, SESSION);
    assert!(!listed[0].marked, "no launch's");
    hand_offs.claim(SESSION);
    assert_eq!(hand_offs.read(SESSION), Some(handed), "a claim keeps it");
    hand_offs.delete(SESSION);
    assert_eq!(hand_offs.read(SESSION), None, "deleted");
    assert!(hand_offs.list().is_empty());
}

// A child's session home and its files are in its project's session
// directory.
#[test]
fn a_childs_home_and_its_files_are_in_its_projects_session_directory() {
    let root = fixtures::root();
    let project = root.root().join("work");
    fixtures::mkdirs(&project);
    let homes = Homes(fixtures::fs());
    let dir = project.display().to_string();
    let mut records = homes.records(&dir);
    assert_eq!(records.dir("kid"), format!("{dir}/.ctxloom/sessions/kid"));
    records.write("kid", "home/settings.json", b"{}").unwrap();
    assert!(fixtures::exists(
        project.join(".ctxloom/sessions/kid/home/settings.json")
    ));
}

// What a child says is pumped off its own thread and heard, by run, in the
// order said.
#[test]
fn a_childs_observations_are_heard_by_run_in_order() {
    let pumps = Pumps::new();
    let said = vec![
        ChildObservation::Started {
            native_id: "n1".to_string(),
        },
        ChildObservation::Exited {
            exit: Exit {
                code: Some(0),
                signal: None,
            },
            detail: String::new(),
        },
    ];
    pumps.listen("run-1".to_string(), Box::new(said.clone().into_iter()));
    let mut heard = Vec::new();
    for _ in 0..200 {
        heard.extend(pumps.heard());
        if heard.len() == said.len() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let expected: Vec<(String, ChildObservation)> =
        said.into_iter().map(|o| ("run-1".to_string(), o)).collect();
    assert_eq!(heard, expected);
}

// The log is the one writer the loop and the launcher share.
#[test]
fn a_line_said_reaches_the_log() {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&lines);
    let say: Say = Arc::new(Mutex::new(Box::new(move |line: &str| {
        kept.lock().unwrap().push(line.to_string());
    })));
    said(&say, "serving");
    assert_eq!(*lines.lock().unwrap(), ["serving"]);
}

// M11: the lock is the single-instance lock alone; the discovery file is how a daemon
// is found. Its own is left be, one gone is published again (it still holds
// the single-instance lock), and another daemon's means this one lost it.
#[test]
fn a_daemon_keeps_its_advertisement_republishes_a_gone_one_and_yields_to_anothers() {
    let own = run_ports::Discovery {
        stamp: "v0.1.0-abc1234".to_string(),
        pid: 7,
        endpoint: "https://127.0.0.1:4242".to_string(),
        cert: run_ports::PublicCert::from_der(vec![0x30, 0x00]).unwrap(),
        bearer: run_ports::Token::from_bytes([7; 32]),
        home: "/home/u".to_string(),
    };
    assert_eq!(advertised(&own, Some(&own)), Advertised::Ours);
    assert_eq!(advertised(&own, None), Advertised::Gone);
    let another = run_ports::Discovery {
        bearer: run_ports::Token::from_bytes([8; 32]),
        ..own.clone()
    };
    assert_eq!(advertised(&own, Some(&another)), Advertised::Replaced);
    let moved = run_ports::Discovery {
        endpoint: "https://127.0.0.1:4343".to_string(),
        ..own.clone()
    };
    assert_eq!(advertised(&own, Some(&moved)), Advertised::Replaced);
}

// U6: a hosted child's session lock is the platform's file lock at
// `<project>/.ctxloom/sessions/<harp>.lock`, its directory made: held, a
// second hold is refused; let go, it is free again.
#[test]
fn a_childs_session_lock_is_held_once_and_let_go_when_dropped() {
    let root = fixtures::root();
    let project = root.root().join("work");
    fixtures::mkdirs(&project);
    let homes = Homes(fixtures::fs());
    let dir = project.display().to_string();
    let held = homes.hold_session(&dir, "kid").unwrap();
    assert_eq!(
        held.path(),
        project
            .join(".ctxloom/sessions/kid.lock")
            .display()
            .to_string()
    );
    assert!(homes.hold_session(&dir, "kid").is_err(), "held already");
    drop(held);
    assert!(homes.hold_session(&dir, "kid").is_ok(), "free again");
}

// U6: the kill switch's steps, by the type of the book applied.
#[test]
fn each_kill_step_is_the_book_it_names() {
    use angzarr_embedded::v1::{command_page, Command, CommandBook, CommandPage, Fact};
    let book = |name: &str, fact: bool| {
        let payload = Some(angzarr_embedded::Any {
            type_url: format!("type.googleapis.com/ctxloom.x.v1.{name}"),
            value: Vec::new(),
        });
        CommandBook {
            cover: None,
            page: Some(CommandPage {
                body: Some(match fact {
                    true => command_page::Body::Fact(Fact {
                        payload,
                        origin: None,
                    }),
                    false => command_page::Body::Command(Command {
                        payload,
                        handling: None,
                    }),
                }),
                ..CommandPage::default()
            }),
        }
    };
    for (name, fact, step) in [
        ("EnqueueRun", false, Some("enqueued")),
        ("MintSession", false, Some("minted")),
        ("RecordRunnerStarted", true, Some("started")),
        ("RecordTurnStarted", true, Some("turn")),
        ("RecordPromptSubmitted", true, Some("hook")),
        ("RecordTurnEnded", true, Some("hook")),
        ("SendMessage", false, None),
    ] {
        assert_eq!(step_of(&book(name, fact)), step, "{name}");
    }
    assert_eq!(step_of(&CommandBook::default()), None);
}

// U6: the switch names one step, and is taken only by it.
#[test]
fn the_kill_switch_is_taken_by_the_step_it_names_alone() {
    let root = fixtures::root();
    let home = root.root().join("home");
    let files = DaemonFiles::new(fixtures::fs(), &home);
    assert!(!files.take_die_at("turn"), "none there");
    fixtures::mkdirs(home.join(".ctxloom/daemon"));
    fixtures::put_mode(home.join(".ctxloom/daemon/test-die-at"), "turn\n", 0o600);
    assert!(!files.take_die_at("hook"), "another step's");
    assert!(files.take_die_at("turn"));
    assert!(!files.take_die_at("turn"), "taken once");
}

// ---------------------- a read catches up first (owner, H3's condition)

/// Whether `future` is ready on one poll (a waker that wakes nothing: the
/// test drives the loop's rounds itself, never a timer).
fn ready<F: std::future::Future + ?Sized>(future: &mut std::pin::Pin<Box<F>>) -> bool {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    future.as_mut().poll(&mut cx).is_ready()
}

// A read of the API (`Hosting::fresh`, which every projection query the
// daemon serves awaits first: GetSession, ListSessions, SearchSessions,
// GetTranscript, GetProject, AssembleContext) answers only once a round
// that STARTED after it was asked has finished, so its catch-up read what
// was committed before the read; a round already running when it was asked
// does not do. Forced: the rounds are the test's.
#[tokio::test]
async fn a_read_waits_for_a_round_started_after_it_was_asked() {
    use daemon_api::Hosting;
    let one_shots = Arc::new(daemon_application::OneShots::new(Box::new(Kept(
        Arc::default(),
    ))));
    let shared = super::Shared::new(
        session_index::SessionIndexes::default(),
        one_shots,
        unsaid(),
    );
    let running = shared.round_starts();
    let mut read = shared.fresh();
    assert!(!ready(&mut read), "no round since it was asked");
    shared.round_done(running);
    assert!(
        !ready(&mut read),
        "the round running when it was asked is not enough"
    );
    let next = shared.round_starts();
    assert!(!ready(&mut read), "a round started, not finished");
    shared.round_done(next);
    assert!(ready(&mut read), "the round after it finished: caught up");
}

/// A host whose one-shot is placed at once and ended when the test says,
/// through the `Told` it keeps.
struct Kept(Arc<Mutex<Vec<daemon_application::Told>>>);

struct NoStop;

impl daemon_application::OneShotControl for NoStop {
    fn stop(&self) {}
}

impl daemon_application::OneShotHost for Kept {
    fn host(
        &self,
        _: daemon_application::HostRequest,
        told: daemon_application::Told,
    ) -> Box<dyn daemon_application::OneShotControl> {
        told.placed(&run_application::Placement {
            run: "0195f0a4-7b1e-7000-8000-000000000001".to_string(),
            session: SESSION.to_string(),
            name: "projects/calm-otter/sessions/harp-1".to_string(),
            harp: "harp-1".to_string(),
            engine: "claude-code".to_string(),
        });
        self.0.lock().unwrap().push(told);
        Box::new(NoStop)
    }
}

// askew-glitter: a one-shot's client hears its reply the moment it ends
// (`Told::done`), and may ask the daemon to stop at once. What the API
// says it hosts is the one-shots as they are NOW, never the loop's last
// round's copy: the round below saw it live, it then ended before the
// next, and a stop between them is not refused for it.
#[test]
fn a_one_shot_that_ended_is_not_hosted_before_the_loops_next_round() {
    use daemon_api::Hosting;
    use daemon_proto::v1::hosted_run::Kind;
    let told = Arc::new(Mutex::new(Vec::new()));
    let one_shots = Arc::new(daemon_application::OneShots::new(Box::new(Kept(
        Arc::clone(&told),
    ))));
    let shared = super::Shared::new(
        session_index::SessionIndexes::default(),
        Arc::clone(&one_shots),
        unsaid(),
    );
    one_shots.create(daemon_application::HostRequest::default());
    // The round, as the loop tells it: its children.
    let child = (
        SESSION.to_string(),
        "0195f0a4-7b1e-7000-8000-000000000009".to_string(),
    );
    shared.tell(vec![child], Vec::new(), std::time::Instant::now());
    assert_eq!(
        kinds(&shared.hosted()),
        [
            ("000000000009", Kind::Child),
            ("000000000001", Kind::OneShot)
        ],
        "the round's child, and the one-shot live"
    );
    told.lock().unwrap()[0].done(run_application::Done {
        result: Ok(run_application::Answer {
            harp: "harp-1".to_string(),
            session: SESSION.to_string(),
            engine: "claude-code".to_string(),
            answer: "HELLO".to_string(),
        }),
        notices: Vec::new(),
    });
    assert_eq!(
        kinds(&shared.hosted()),
        [("000000000009", Kind::Child)],
        "the one-shot ended, with no round since"
    );
}

// Step 8 U4: an ask is recorded with what the round says of the session
// that asks (its run, engine, agent and timeout); one it does not host has
// none.
#[test]
fn an_asker_is_the_rounds_hosted_child_of_that_session() {
    use daemon_api::Hosting;
    let one_shots = Arc::new(daemon_application::OneShots::new(Box::new(Kept(Arc::new(
        Mutex::new(Vec::new()),
    )))));
    let shared = super::Shared::new(
        session_index::SessionIndexes::default(),
        one_shots,
        unsaid(),
    );
    let asker = |session: &str| daemon_application::Asker {
        session: session.to_string(),
        run: format!("run-of-{session}"),
        engine: "claude-code".to_string(),
        agent: "default".to_string(),
        approval_timeout: std::time::Duration::from_secs(300),
    };
    assert_eq!(shared.asker("s-1"), None);
    shared.tell(
        Vec::new(),
        vec![asker("s-1"), asker("s-2")],
        std::time::Instant::now(),
    );
    assert_eq!(shared.asker("s-2"), Some(asker("s-2")));
    assert_eq!(shared.asker("s-3"), None);
}

/// Each hosted run's root's last twelve digits, and its kind.
fn kinds(
    hosted: &[daemon_proto::v1::HostedRun],
) -> Vec<(&str, daemon_proto::v1::hosted_run::Kind)> {
    hosted
        .iter()
        .map(|run| (&run.run[run.run.len() - 12..], run.kind()))
        .collect()
}

/// A drain's period with no config saying one: the default, five minutes.
fn unsaid() -> super::Period {
    super::Period {
        default: daemon_application::HAND_OVER_PERIOD,
        configured: Box::new(|| None),
    }
}

// Owner, 2026-11-15: a drain's period is the one its request asked
// (`ctxloom shutdown <duration>`), else the home config's (read again each
// time, so a change while it drains is the drain's), else the default; the
// hand-over's warning says the same.
#[test]
fn a_drains_period_is_the_requests_else_the_configs_else_the_default() {
    use daemon_api::Hosting;
    use std::sync::atomic::{AtomicU64, Ordering};
    let one_shots = || {
        Arc::new(daemon_application::OneShots::new(Box::new(Kept(
            Arc::default(),
        ))))
    };
    let said = Arc::new(AtomicU64::new(0));
    let config = Arc::clone(&said);
    let shared = super::Shared::new(
        session_index::SessionIndexes::default(),
        one_shots(),
        super::Period {
            default: Duration::from_secs(300),
            configured: Box::new(move || match config.load(Ordering::SeqCst) {
                0 => None,
                seconds => Some(Duration::from_secs(seconds)),
            }),
        },
    );
    assert_eq!(shared.drain_period(), Duration::from_secs(300));
    said.store(60, Ordering::SeqCst);
    assert_eq!(
        shared.drain_period(),
        Duration::from_secs(60),
        "the config's"
    );
    shared.drain(None);
    said.store(5, Ordering::SeqCst);
    assert_eq!(shared.drain_period(), Duration::from_secs(5), "read again");
    assert_eq!(shared.hand_over_seconds(), 5);
    let asked = super::Shared::new(
        session_index::SessionIndexes::default(),
        one_shots(),
        super::Period {
            default: Duration::from_secs(300),
            configured: Box::new(|| Some(Duration::from_secs(60))),
        },
    );
    assert!(!asked.draining());
    asked.drain(Some(Duration::from_secs(900)));
    asked.drain(Some(Duration::from_secs(1)));
    assert_eq!(
        asked.drain_period(),
        Duration::from_secs(900),
        "the first ask's"
    );
    assert!(asked.draining());
}
