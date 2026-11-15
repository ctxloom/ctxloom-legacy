//! The frontend's order of steps over scripted ports: the lock around the
//! TUI, each report after the step it reports, and the reports retried
//! across a daemon that is gone (S17), each race forced by the script.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use run_ports::{
    BoxFuture, Delivery, Exit, Launch, LaunchFailure, LaunchKind, Limits, Running, Stdio,
    TurnCommand, TurnOutput,
};
use session_ports::{Held, LockError, SessionLock};

use super::{Frontend, Pause, Prepared, Report, Reports, Terminal, REPORT_FOR, REPORT_PAUSE};
use crate::RunError;

/// Everything that happened, in order.
type Timeline = Arc<Mutex<Vec<String>>>;

fn said(timeline: &Timeline, what: impl Into<String>) {
    timeline.lock().unwrap().push(what.into());
}

/// A lock that says when it is taken and let go, or is held by another.
struct Locks {
    timeline: Timeline,
    held_by_another: bool,
}

struct Guard(Timeline, String);

impl Drop for Guard {
    fn drop(&mut self) {
        said(&self.0, format!("unlock {}", self.1));
    }
}

impl SessionLock for Locks {
    fn hold(&mut self, harp: &str) -> Result<Held, LockError> {
        if self.held_by_another {
            return Err(LockError::Held(angzarr_diagnostic::Report::new(
                "file-lock-held",
            )));
        }
        said(&self.timeline, format!("lock {harp}"));
        Ok(Held::new(Guard(
            Arc::clone(&self.timeline),
            harp.to_string(),
        )))
    }
}

struct Spawned(u32);

impl Running for Spawned {
    fn pid(&self) -> u32 {
        self.0
    }

    fn finish(self: Box<Self>) -> Result<TurnOutput, LaunchFailure> {
        unreachable!("the frontend waits through its terminal")
    }
}

/// A terminal that spawns (or refuses to) and exits as told.
struct Tty {
    timeline: Timeline,
    refuse: bool,
    exit: Exit,
    /// What it was started with.
    started: Mutex<Option<Launch>>,
}

impl Terminal for Tty {
    fn start(&self, launch: &Launch, _: &Limits) -> Result<Box<dyn Running>, LaunchFailure> {
        if self.refuse {
            return Err(LaunchFailure {
                deterministic: true,
                detail: "no such program".to_string(),
            });
        }
        said(&self.timeline, "spawn");
        *self.started.lock().unwrap() = Some(launch.clone());
        Ok(Box::new(Spawned(4242)))
    }

    fn wait(&self, _: Box<dyn Running>) -> BoxFuture<'_, Result<TurnOutput, LaunchFailure>> {
        said(&self.timeline, "exited");
        let exit = self.exit;
        Box::pin(async move {
            Ok(TurnOutput {
                exit,
                stdout: Vec::new(),
                stderr: Vec::new(),
                timed_out: false,
                truncated: false,
                drain_incomplete: false,
            })
        })
    }
}

/// A daemon that answers each attempt as scripted (then applies), and the
/// time each attempt takes on the fake clock.
struct Daemon {
    timeline: Timeline,
    script: Mutex<VecDeque<Delivery>>,
    clock: Arc<Clock>,
}

impl Reports for Daemon {
    fn deliver<'a>(&'a self, _: &'a Launch, report: &'a Report) -> BoxFuture<'a, Delivery> {
        let delivery = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Delivery::Applied);
        said(
            &self.timeline,
            format!("report {} {}", name(report), outcome(&delivery)),
        );
        // A delivery that found nobody took a second of the clock.
        self.clock.advance(Duration::from_secs(1));
        Box::pin(async move { delivery })
    }
}

fn name(report: &Report) -> &'static str {
    match report {
        Report::Started { .. } => "started",
        Report::Exited { .. } => "exited",
        Report::NotStarted { .. } => "not-started",
    }
}

fn outcome(delivery: &Delivery) -> &'static str {
    match delivery {
        Delivery::Applied => "applied",
        Delivery::Duplicate => "duplicate",
        Delivery::Parked => "parked",
        Delivery::Refused { .. } => "refused",
        Delivery::NoSuchSession => "no-such-session",
        Delivery::Unavailable { .. } | Delivery::TimedOut => "unreached",
    }
}

#[derive(Default)]
struct Clock {
    now: Mutex<Duration>,
}

impl Clock {
    fn advance(&self, by: Duration) {
        *self.now.lock().unwrap() += by;
    }
}

impl Pause for Clock {
    fn elapsed(&self) -> Duration {
        *self.now.lock().unwrap()
    }

    fn pause(&self, pause: Duration) -> BoxFuture<'_, ()> {
        self.advance(pause);
        Box::pin(async {})
    }
}

fn unreached() -> Delivery {
    Delivery::Unavailable {
        detail: "no ctxloom daemon could be started".to_string(),
    }
}

fn prepared() -> Prepared {
    let mut command = TurnCommand::new("/bin/claude");
    command.args = vec!["--settings".to_string(), "hooks.json".to_string()];
    command
        .env
        .insert("HOME".to_string(), "/s/home".to_string());
    command.cwd = Some("/work".to_string());
    let mut launch = Launch::new(LaunchKind::Terminal, "claude-code", Stdio::Inherit, command);
    launch.at.harp = "amber-quiet-heron".to_string();
    launch.at.run = "00000000-0000-0000-0000-000000000001".to_string();
    launch.at.incarnation = 1;
    Prepared {
        run: "projects/p/runs/00000000-0000-0000-0000-000000000001".to_string(),
        session: "projects/p/sessions/amber-quiet-heron".to_string(),
        launch,
    }
}

struct Rig {
    timeline: Timeline,
    locks: Locks,
    tty: Tty,
    daemon: Daemon,
    clock: Arc<Clock>,
}

fn rig(script: Vec<Delivery>) -> Rig {
    let timeline = Timeline::default();
    let clock = Arc::new(Clock::default());
    Rig {
        locks: Locks {
            timeline: Arc::clone(&timeline),
            held_by_another: false,
        },
        tty: Tty {
            timeline: Arc::clone(&timeline),
            refuse: false,
            exit: Exit {
                code: Some(0),
                signal: None,
            },
            started: Mutex::new(None),
        },
        daemon: Daemon {
            timeline: Arc::clone(&timeline),
            script: Mutex::new(script.into()),
            clock: Arc::clone(&clock),
        },
        clock,
        timeline,
    }
}

async fn run(rig: &mut Rig) -> super::Started {
    let clock = Arc::clone(&rig.clock);
    let mut frontend = Frontend {
        locks: &mut rig.locks,
        terminal: &rig.tty,
        reports: &rig.daemon,
        pause: &*clock,
        limits: Limits::new(Duration::from_secs(1)),
    };
    frontend.run(&prepared()).await
}

fn timeline(rig: &Rig) -> Vec<String> {
    rig.timeline.lock().unwrap().clone()
}

#[tokio::test]
async fn test_the_lock_is_taken_before_the_start_and_let_go_after_the_exit_is_reported() {
    let mut rig = rig(Vec::new());
    let done = run(&mut rig).await;
    let ended = done.result.unwrap();
    assert_eq!(ended.harp, "amber-quiet-heron");
    assert_eq!(ended.session, "projects/p/sessions/amber-quiet-heron");
    assert_eq!(ended.engine, "claude-code");
    assert_eq!(ended.code, Some(0));
    assert!(done.notices.is_empty());
    assert_eq!(
        timeline(&rig),
        [
            "lock amber-quiet-heron",
            "spawn",
            "report started applied",
            "exited",
            "report exited applied",
            "unlock amber-quiet-heron",
        ]
    );
}

#[tokio::test]
async fn test_the_launch_is_started_exactly_as_the_daemon_said() {
    let mut rig = rig(Vec::new());
    let _ = run(&mut rig).await;
    let started = rig.tty.started.lock().unwrap().clone().unwrap();
    assert_eq!(started, prepared().launch);
}

#[tokio::test]
async fn test_a_signal_s_end_is_reported_as_the_signal() {
    let mut rig = rig(Vec::new());
    rig.tty.exit = Exit {
        code: None,
        signal: Some(15),
    };
    let done = run(&mut rig).await;
    assert_eq!(done.result.unwrap().code, None);
}

#[tokio::test]
async fn test_a_lock_another_holds_starts_nothing_and_reports_the_launch_failed() {
    let mut rig = rig(Vec::new());
    rig.locks.held_by_another = true;
    let done = run(&mut rig).await;
    assert!(matches!(
        done.result,
        Err(RunError::Lock(LockError::Held(_)))
    ));
    assert_eq!(timeline(&rig), ["report not-started applied"]);
}

#[tokio::test]
async fn test_a_launch_that_does_not_start_is_reported_and_its_lock_let_go() {
    let mut rig = rig(Vec::new());
    rig.tty.refuse = true;
    let done = run(&mut rig).await;
    assert!(
        matches!(&done.result, Err(RunError::NotStarted { detail }) if detail == "no such program")
    );
    assert_eq!(
        timeline(&rig),
        [
            "lock amber-quiet-heron",
            "report not-started applied",
            "unlock amber-quiet-heron",
        ]
    );
}

#[tokio::test]
async fn test_a_daemon_gone_between_the_plan_and_the_start_gets_the_start_from_the_next() {
    // The daemon that prepared the session is gone when the start is
    // reported: the next attempts find (or start) the next one.
    let mut rig = rig(vec![unreached(), unreached()]);
    let done = run(&mut rig).await;
    assert!(done.result.is_ok());
    assert!(done.notices.is_empty());
    assert_eq!(
        timeline(&rig),
        [
            "lock amber-quiet-heron",
            "spawn",
            "report started unreached",
            "report started unreached",
            "report started applied",
            "exited",
            "report exited applied",
            "unlock amber-quiet-heron",
        ]
    );
}

#[tokio::test]
async fn test_a_daemon_gone_between_the_start_and_the_exit_gets_the_exit_from_the_next() {
    let mut rig = rig(vec![Delivery::Applied, unreached(), Delivery::Duplicate]);
    let done = run(&mut rig).await;
    assert!(done.result.is_ok());
    assert!(done.notices.is_empty());
    assert_eq!(
        timeline(&rig),
        [
            "lock amber-quiet-heron",
            "spawn",
            "report started applied",
            "exited",
            "report exited unreached",
            "report exited duplicate",
            "unlock amber-quiet-heron",
        ]
    );
}

#[tokio::test]
async fn test_a_report_is_retried_for_its_bound_then_said_and_the_lock_let_go() {
    // Nobody answers the exit at all: it is tried until REPORT_FOR has
    // passed since its first attempt, then said; the lock is let go after.
    let mut rig = rig(Vec::new());
    let attempts = 1 + (REPORT_FOR.as_millis() / (1000 + REPORT_PAUSE.as_millis())) as usize;
    let mut script = vec![Delivery::Applied];
    script.extend((0..attempts + 1).map(|_| unreached()));
    rig.daemon.script = Mutex::new(script.into());
    let done = run(&mut rig).await;
    assert!(done.result.is_ok());
    assert_eq!(done.notices.len(), 1, "{:?}", done.notices);
    let said = format!("{:?}", done.notices[0]);
    assert!(said.contains("the session's exit"), "{said}");
    let tries = timeline(&rig)
        .iter()
        .filter(|t| *t == "report exited unreached")
        .count();
    assert_eq!(tries, attempts);
    assert_eq!(
        timeline(&rig).last().map(String::as_str),
        Some("unlock amber-quiet-heron")
    );
    // The clock passed the bound, and not by more than one attempt.
    let elapsed = rig.clock.elapsed();
    assert!(elapsed >= REPORT_FOR, "{elapsed:?}");
    assert!(elapsed < REPORT_FOR + Duration::from_secs(3), "{elapsed:?}");
}

#[tokio::test]
async fn test_a_refused_report_is_said_at_once_and_not_retried() {
    let mut rig = rig(vec![Delivery::Refused {
        detail: "the run has ended".to_string(),
    }]);
    let done = run(&mut rig).await;
    assert!(done.result.is_ok());
    assert_eq!(done.notices.len(), 1);
    let said = format!("{:?}", done.notices[0]);
    assert!(said.contains("the session's start"), "{said}");
    assert!(said.contains("the run has ended"), "{said}");
    assert_eq!(
        timeline(&rig)
            .iter()
            .filter(|t| t.starts_with("report started"))
            .count(),
        1
    );
}

#[tokio::test]
async fn test_a_parked_report_is_said() {
    let mut rig = rig(vec![Delivery::Applied, Delivery::Parked]);
    let done = run(&mut rig).await;
    assert_eq!(done.notices.len(), 1);
    assert!(format!("{:?}", done.notices[0]).contains("parked"));
}

// s4-Q14, H3 follow-up: a report the daemon answers no such session is
// final, and said as such.
#[test]
fn a_report_to_a_session_with_no_record_is_said_so() {
    let library_application::Notice::Finding { text, .. } =
        super::undelivered(&Report::Started { pid: 1 }, &Delivery::NoSuchSession)
    else {
        panic!("a finding");
    };
    assert_eq!(
        text,
        "could not tell the daemon the session's start: the daemon has no record of this session"
    );
}
