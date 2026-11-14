//! The interactive session's FRONTEND (U5: T9, S3, S17): what `ctxloom run`
//! does once the user's daemon has planned and prepared the session and
//! answered its one `Launch` (T10). The daemon decided everything about the
//! session; the frontend only puts the engine on the user's terminal, exactly
//! as the launch says, and tells the daemon what became of it:
//!
//! 1. it takes the session's lock (at once or refused, `sessions-session-
//!    running`, exit 20) and holds it for the TUI's whole life (rulings 85,
//!    92), so the session reads live;
//! 2. it starts the launch on the terminal (its program, arguments, WHOLE
//!    environment and directory, nothing inherited);
//! 3. it reports the start (`RunnerStarted`, with the pid the spawn gave),
//!    waits for the TUI's exit, and reports it (`RunExited`), or reports a
//!    launch that never started (`LaunchFailed`);
//! 4. it lets the lock go, only after the exit was reported.
//!
//! Each report is a runs fact keyed by its run (`started:<run>`,
//! `exit:<run>`, `launch-failed:<run>`), so a report delivered twice, or one
//! the daemon also records, is a DUPLICATE. Each is RETRIED for up to
//! [`REPORT_FOR`] (S17): the adapter behind [`Reports`] finds the daemon
//! serving, or starts one, on every attempt, so a report outlives the daemon
//! it was meant for. A report that is never delivered within the bound is
//! said, and changes nothing of the TUI.
//!
//! The concurrency (the TUI waited for while the daemon's watch is read for
//! a stop) is the composition root's: this is the order of the steps, over
//! ports, so a test forces each race it is about.

use std::time::Duration;

use library_application::Notice;
use run_ports::{BoxFuture, Delivery, Launch, LaunchFailure, Limits, Running, TurnOutput};
use session_ports::{Held, SessionLock};

use crate::error::RunError;
use crate::one_shot::{Done, Ended};

/// How long a report is retried for, on whichever daemon serves, before it
/// is given up and said (S17: 60 s).
pub const REPORT_FOR: Duration = Duration::from_secs(60);

/// How long the frontend waits between two attempts at a report.
pub const REPORT_PAUSE: Duration = Duration::from_millis(250);

/// What the frontend tells the daemon of its run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    /// The engine was spawned, with this pid.
    Started { pid: u32 },
    /// The engine exited: its code, or the signal that ended it.
    Exited {
        code: Option<i32>,
        signal: Option<i32>,
    },
    /// The engine was never started: `deterministic` says a retry would fail
    /// the same way.
    NotStarted { deterministic: bool, detail: String },
}

/// What the daemon prepared: the session's run, its resource names, and the
/// one launch the frontend starts.
#[derive(Clone, Debug)]
pub struct Prepared {
    /// The run's resource name (`projects/{project}/runs/{run}`).
    pub run: String,
    /// The session's resource name (`projects/{project}/sessions/{harp}`).
    pub session: String,
    pub launch: Launch,
}

/// Where the frontend's reports go: ONE attempt at delivering `report` for
/// the run `launch` starts, to the daemon serving now (found, or started
/// when none serves). The answer is the delivery's: settled (applied, a
/// duplicate, parked or refused) or not (nobody answered, in time).
pub trait Reports: Send + Sync {
    fn deliver<'a>(&'a self, launch: &'a Launch, report: &'a Report) -> BoxFuture<'a, Delivery>;
}

/// The frontend's time: how long it has been since it began, and a pause
/// between two attempts. The root's is the clock; a test's is a counter.
pub trait Pause: Send + Sync {
    fn elapsed(&self) -> Duration;
    fn pause(&self, pause: Duration) -> BoxFuture<'_, ()>;
}

/// The terminal the launch is started on: spawned (an `Err` started
/// nothing), then waited for to its exit. The root's is `run_terminal`'s,
/// its wait on a blocking thread; a stop it was made with asks the TUI to
/// stop, then kills it after the grace (s4-Q10).
pub trait Terminal: Send + Sync {
    fn start(&self, launch: &Launch, limits: &Limits) -> Result<Box<dyn Running>, LaunchFailure>;
    fn wait(&self, running: Box<dyn Running>) -> BoxFuture<'_, Result<TurnOutput, LaunchFailure>>;
}

/// What the frontend runs over.
pub struct Frontend<'a> {
    pub locks: &'a mut dyn SessionLock,
    pub terminal: &'a dyn Terminal,
    pub reports: &'a dyn Reports,
    pub pause: &'a dyn Pause,
    pub limits: Limits,
}

/// What the TUI came to, before the daemon's last word: its end, and what the
/// frontend itself noticed (a report it could not deliver).
pub type Started = Done<Ended>;

impl Frontend<'_> {
    /// Run `prepared` on the terminal: the lock taken, the engine started and
    /// waited for, each step reported, the lock let go.
    pub async fn run(&mut self, prepared: &Prepared) -> Started {
        let mut notices = Vec::new();
        let result = self.session(prepared, &mut notices).await;
        Done { result, notices }
    }

    async fn session(
        &mut self,
        prepared: &Prepared,
        notices: &mut Vec<Notice>,
    ) -> Result<Ended, RunError> {
        let launch = &prepared.launch;
        let lock = match self.locks.hold(&launch.at.harp) {
            Ok(lock) => lock,
            Err(error) => {
                let report = Report::NotStarted {
                    deterministic: true,
                    detail: error.report().to_string(),
                };
                self.report(launch, &report, notices).await;
                return Err(RunError::Lock(error));
            }
        };
        let running = match self.terminal.start(launch, &self.limits) {
            Ok(running) => running,
            Err(failure) => return Err(self.not_started(launch, failure, lock, notices).await),
        };
        let started = Report::Started { pid: running.pid() };
        self.report(launch, &started, notices).await;
        let output = match self.terminal.wait(running).await {
            Ok(output) => output,
            Err(failure) => return Err(self.not_started(launch, failure, lock, notices).await),
        };
        let exited = Report::Exited {
            code: output.exit.code,
            signal: output.exit.signal,
        };
        self.report(launch, &exited, notices).await;
        // Let go only now: the session reads live until its exit is known.
        drop(lock);
        Ok(Ended {
            harp: launch.at.harp.clone(),
            session: prepared.session.clone(),
            engine: launch.engine.clone(),
            code: output.exit.code,
        })
    }

    /// The launch that did not start (or whose wait broke): reported, the
    /// lock let go after.
    async fn not_started(
        &self,
        launch: &Launch,
        failure: LaunchFailure,
        lock: Held,
        notices: &mut Vec<Notice>,
    ) -> RunError {
        let report = Report::NotStarted {
            deterministic: failure.deterministic,
            detail: failure.detail.clone(),
        };
        self.report(launch, &report, notices).await;
        drop(lock);
        RunError::NotStarted {
            detail: failure.detail,
        }
    }

    /// Deliver `report`, retried for up to [`REPORT_FOR`] from its first
    /// attempt while nobody answers; one not delivered by then (or refused)
    /// is said.
    async fn report(&self, launch: &Launch, report: &Report, notices: &mut Vec<Notice>) {
        let since = self.pause.elapsed();
        loop {
            let delivery = self.reports.deliver(launch, report).await;
            match delivery {
                Delivery::Applied | Delivery::Duplicate => return,
                Delivery::Parked | Delivery::Refused { .. } => {
                    notices.push(undelivered(report, &delivery));
                    return;
                }
                Delivery::Unavailable { .. } | Delivery::TimedOut => {}
            }
            if self.pause.elapsed().saturating_sub(since) >= REPORT_FOR {
                notices.push(undelivered(report, &delivery));
                return;
            }
            self.pause.pause(REPORT_PAUSE).await;
        }
    }
}

/// What a report that did not reach the daemon is said as.
fn undelivered(report: &Report, delivery: &Delivery) -> Notice {
    let what = match report {
        Report::Started { .. } => "the session's start",
        Report::Exited { .. } => "the session's exit",
        Report::NotStarted { .. } => "that the session never started",
    };
    let why = match delivery {
        Delivery::Refused { detail } | Delivery::Unavailable { detail } => detail.clone(),
        Delivery::Parked => "the daemon parked it".to_string(),
        Delivery::TimedOut => "no daemon answered in time".to_string(),
        Delivery::Applied | Delivery::Duplicate => String::new(),
    };
    Notice::Finding {
        text: format!("could not tell the daemon {what}: {why}"),
        remedy: "the next daemon's start ends the session's run once its lock is free".to_string(),
    }
}

#[cfg(test)]
#[path = "frontend.test.rs"]
mod tests;
