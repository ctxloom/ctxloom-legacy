//! The frontend's wiring, its pure parts: what a report is on the wire,
//! what its holding came to, what the daemon reads back of the reports, and
//! what a prepared session's end says on its watch.

use angzarr_embedded::v1::{command_page, event_page, EventPage};
use angzarr_embedded::Any;
use daemon_proto::v1;
use prost::Message;
use run_application::frontend::Report;
use run_application::{Done, Ended, RunError};
use run_ports::{Delivery, Launch, LaunchKind, Stdio, TurnCommand};

use super::{events_of, external_id_of, project_of, report_book, reported_of, seen_in};

const RUN: &str = "0195f0a4-7b1e-7000-8000-000000000001";
const SESSION: &str = "0195f0a4-7b1e-7000-8000-000000000002";

fn launch() -> Launch {
    let mut command = TurnCommand::new("/bin/claude");
    command.cwd = Some("/work/checkout".to_string());
    let mut launch = Launch::new(LaunchKind::Terminal, "claude-code", Stdio::Inherit, command);
    launch.at.run = RUN.to_string();
    launch.at.session = SESSION.to_string();
    launch.at.harp = "amber-quiet-heron".to_string();
    launch.at.incarnation = 1;
    launch.session_lock = "/work/.ctxloom/sessions/amber-quiet-heron.lock".to_string();
    launch
}

/// The fact a book carries: its type, its key and its payload.
fn fact_of(book: &angzarr_embedded::v1::CommandBook) -> (String, String, Vec<u8>) {
    let Some(command_page::Body::Fact(fact)) = book.page.clone().and_then(|page| page.body) else {
        panic!("a fact body")
    };
    let payload = fact.payload.expect("a payload");
    (
        payload.type_url,
        fact.origin.expect("an origin").external_id,
        payload.value,
    )
}

#[test]
fn each_report_is_its_run_s_fact_keyed_as_the_launcher_keys_its_own() {
    let started = report_book(&launch(), &Report::Started { pid: 4242 });
    let (url, key, bytes) = fact_of(&started);
    assert!(url.ends_with(".RunnerStarted"), "{url}");
    assert_eq!(key, format!("started:{RUN}"));
    let wire = <runs::RunnerStarted as angzarr_embedded::markup::DomainMessage>::Wire::decode(
        bytes.as_slice(),
    )
    .unwrap();
    assert_eq!(wire.pid, 4242);
    assert_eq!(wire.runtime, "claude-code");
    assert_eq!(wire.workspace, "/work/checkout");
    let exited = report_book(
        &launch(),
        &Report::Exited {
            code: None,
            signal: Some(15),
        },
    );
    let (url, key, bytes) = fact_of(&exited);
    assert!(url.ends_with(".RunExited"), "{url}");
    assert_eq!(key, format!("exit:{RUN}"));
    let wire = <runs::RunExited as angzarr_embedded::markup::DomainMessage>::Wire::decode(
        bytes.as_slice(),
    )
    .unwrap();
    assert_eq!((wire.code, wire.signal), (0, 15));
    let failed = report_book(
        &launch(),
        &Report::NotStarted {
            deterministic: true,
            detail: "held".to_string(),
        },
    );
    let (url, key, _) = fact_of(&failed);
    assert!(url.ends_with(".LaunchFailed"), "{url}");
    assert_eq!(key, format!("launch-failed:{RUN}"));
    for book in [started, exited, failed] {
        assert_eq!(book.cover.expect("a cover").domain, runs::RUN_DOMAIN);
    }
}

#[test]
fn the_lock_s_project_is_three_up_from_it() {
    let wiring = crate::Wiring::new(std::sync::Arc::new(process_env::EnvSnapshot::from_parts(
        os::host().flavor(),
        Ok(std::path::PathBuf::from("/elsewhere")),
        std::path::PathBuf::from("/tmp"),
        Vec::new(),
    )));
    let Ok(wiring) = wiring else { return };
    assert_eq!(
        project_of(&launch(), &wiring),
        std::path::PathBuf::from("/work")
    );
}

fn page(any: Any) -> EventPage {
    EventPage {
        body: Some(event_page::Body::Event(any)),
        ..EventPage::default()
    }
}

fn any_of<M: angzarr_embedded::markup::DomainMessage>(message: &M) -> Any {
    Any::from_msg(&message.to_wire()).unwrap()
}

#[test]
fn the_daemon_reads_the_start_then_the_end_as_the_exit() {
    let started = page(any_of(&runs::RunnerStartedRecorded {
        runtime: "claude-code".to_string(),
        workspace: String::new(),
        pid: 7,
    }));
    let ended = page(any_of(&runs::RunEnded {
        cause: runs::EndCause::RunnerExit,
        exit_code: Some(3),
        detail: String::new(),
    }));
    let seen = seen_in(std::slice::from_ref(&started), &[]);
    assert_eq!(seen.started, Some(7));
    assert_eq!(seen.exited, None);
    let seen = seen_in(&[started, ended.clone()], &[]);
    assert_eq!(seen.exited.map(|e| e.code), Some(Some(3)));
    assert!(seen.failed.is_none());
    // An end before any start: it never started.
    let seen = seen_in(&[ended], &[]);
    assert!(seen.failed.is_some());
    assert!(!seen.closed);
}

#[test]
fn the_daemon_reads_the_session_closed() {
    let closed = page(any_of(&sessions::SessionClosed {
        cause: "user".to_string(),
    }));
    assert!(seen_in(&[], &[closed]).closed);
}

#[test]
fn a_prepared_session_s_end_says_its_notices_then_its_end() {
    let ended = Done::<Ended> {
        result: Ok(Ended {
            harp: "h".to_string(),
            session: "s".to_string(),
            engine: "claude-code".to_string(),
            code: Some(0),
        }),
        notices: vec![library_application::Notice::text("left")],
    };
    let events = events_of(&ended);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0].event,
        Some(v1::run_event::Event::Notice(_))
    ));
    assert_eq!(
        events[1].event,
        Some(v1::run_event::Event::End(v1::RunEnd::default()))
    );
    let failed = Done::<Ended> {
        result: Err(RunError::NotEnded {
            run: "r".to_string(),
        }),
        notices: Vec::new(),
    };
    let events = events_of(&failed);
    assert!(matches!(
        &events[0].event,
        Some(v1::run_event::Event::End(end)) if end.code == "run-application-not-ended"
    ));
}

// Slice 4: what a report's one attempt came to, as the frontend is told it.
#[test]
fn a_reports_holding_is_what_the_frontend_is_told() {
    use run_application::frontend::Reported;
    use runs_application::{Kept, NotHeld, Outcome, Why};
    let kept = |outcome| Kept {
        external_id: "started:r".to_string(),
        outcome,
    };
    assert_eq!(
        reported_of(Some(&kept(Outcome::Delivered(Delivery::Duplicate)))),
        Reported::Delivered(Delivery::Duplicate)
    );
    assert_eq!(
        reported_of(Some(&kept(Outcome::Held(Why::Unreached {
            why: "no daemon".to_string()
        })))),
        Reported::Held {
            why: "no daemon".to_string()
        }
    );
    let Reported::Held { why } = reported_of(Some(&kept(Outcome::Held(Why::Older {
        daemon: "v0.1.0-a".to_string(),
        own: "v0.2.0-b".to_string(),
    })))) else {
        panic!("held");
    };
    assert!(
        why.contains("v0.1.0-a") && why.contains("v0.2.0-b"),
        "{why}"
    );
    assert!(matches!(
        reported_of(Some(&kept(Outcome::HeldBehind))),
        Reported::Held { .. }
    ));
    assert!(matches!(
        reported_of(Some(&kept(Outcome::NotHeld(NotHeld::Foreign)))),
        Reported::Lost { .. }
    ));
    assert!(matches!(reported_of(None), Reported::Lost { .. }));
}

#[test]
fn a_reports_external_id_is_its_books() {
    let book = report_book(&launch(), &Report::Started { pid: 7 });
    assert_eq!(external_id_of(&book), format!("started:{RUN}"));
    assert_eq!(
        external_id_of(&angzarr_embedded::v1::CommandBook::default()),
        ""
    );
}
