//! `ctxloom shutdown [<when>]` (owner, 2026-11-15: one ctxloom version runs
//! at a time, and an installer asks every running ctxloom of this user to
//! stop before it replaces the binary): the daemon and the live
//! interactive frontends, which outlive their daemon (U5's adoption).
//!
//! `<when>` is a span of the one grammar (`library::config::span`: `15m`,
//! `1h30m`, `now`); none given, the home config's `daemon.drain_timeout`,
//! else [`daemon_application::HAND_OVER_PERIOD`]. A span that is not `now`
//! DRAINS the daemon within it (`daemon stop --drain`'s request, its
//! timeout this one), and waits for the daemon to exit and the frontends to
//! end, at most that long (the daemon then a grace longer, as its period's
//! end stops what it still hosts). Then, `now` at once: each live frontend
//! is asked to stop (its stop file, [`liveness::FrontendLocks`]; it stops
//! its engine as a SIGTERM would, ruling 114, and reports its end), waited
//! for, and the daemon serving then is asked the plain stop
//! (`--even-if-live`: its engines stopped, ruling 114), waited for. Nothing
//! is killed. What still runs is said, and the command fails (10).

use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use angzarr_diagnostic::{code, Arg, Diagnostic};
use angzarr_presentation::{render, Format, Render};
use clap::{Arg as ClapArg, Command};
use cli_support::Exit;
use ctxloom_cli::{Call, Failure};
use file_system::Fs;
use liveness::FrontendLocks;
use serde::Serialize;

use super::{block_on, called, client_reaching, files_of, output, own_stamp, serving, Reach};
use crate::Wiring;

/// The noun.
pub(crate) const NOUN: &str = "shutdown";

/// How much longer than its drain's timeout the daemon is waited for: its
/// period's end stops each one-shot (a five-second grace, then its end
/// recorded) and cuts each busy child before it exits.
const DRAIN_GRACE: Duration = Duration::from_secs(30);

/// How long the frontends asked to stop are waited for: the TUI's stop
/// grace (`Terminal::STOP_GRACE`), then its exit reported.
const FRONTENDS_WAIT: Duration = Duration::from_secs(20);

/// How long the daemon asked the plain stop is waited for.
const STOP_WAIT: Duration = Duration::from_secs(30);

/// How often what still runs is looked at.
const LOOK: Duration = Duration::from_millis(100);

/// The noun as the command line has it.
pub(crate) fn noun() -> Command {
    Command::new(NOUN)
        .about("Stop every running ctxloom of this user, gracefully within a time, then what remains")
        .arg(
            ClapArg::new("when")
                .value_name("DURATION")
                .help("How long the drain waits (15m, 1h30m, now); default the config's daemon.drain_timeout, else 5m"),
        )
}

/// Why the shutdown is refused, or did not stop everything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShutdownError {
    /// `<when>` is no span.
    BadWhen { value: String, cause: String },
    /// What still runs once everything was asked to stop and waited for.
    Incomplete { daemon: String, frontends: String },
}

impl std::fmt::Display for ShutdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for ShutdownError {}

impl Diagnostic for ShutdownError {
    fn code(&self) -> &str {
        match self {
            ShutdownError::BadWhen { .. } => code!("shutdown-bad-time"),
            ShutdownError::Incomplete { .. } => code!("shutdown-incomplete"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            ShutdownError::BadWhen { value, cause } => vec![
                ("value", value.as_str().into()),
                ("ext_err_txt", cause.as_str().into()),
            ],
            ShutdownError::Incomplete { daemon, frontends } => vec![
                ("daemon", daemon.as_str().into()),
                ("frontends", frontends.as_str().into()),
            ],
        }
    }
}

/// One value of each, for the catalog's test.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(ShutdownError::BadWhen {
            value: "soon".to_string(),
            cause: "not a span".to_string(),
        }),
        Box::new(ShutdownError::Incomplete {
            daemon: "pid 4242".to_string(),
            frontends: "pid 77".to_string(),
        }),
    ]
}

/// The home config's `daemon.drain_timeout` (owner, 2026-11-15), read
/// afresh, as a command reads the home layer: none when it is not said, or
/// the config does not read.
pub(crate) fn configured_drain_timeout(fs: &Arc<dyn Fs>, home: &Path) -> Option<Duration> {
    let app = config_files::AppDir {
        path: home.join(".ctxloom"),
        source: ctxloom_ports::Source::Home,
        ignored_root: None,
    };
    let files = config_files::ConfigFilesFs::new(Arc::clone(fs), app, home);
    library_application::load_config(
        &files,
        &[],
        &library_application::Flags::default(),
        &mut Vec::new(),
    )
    .ok()?
    .drain_timeout()
}

/// What the shutdown did, step by step, and what it left running.
#[derive(Debug, Default, Serialize)]
struct Report {
    /// The drain's timeout in seconds; 0 for `now`.
    drain_seconds: u64,
    /// Each step, as it was said.
    steps: Vec<String>,
}

impl Render for Report {
    fn json(&self) -> impl Serialize + '_ {
        self
    }

    fn text(&self, out: &mut dyn Write) -> std::io::Result<()> {
        self.steps
            .iter()
            .try_for_each(|step| writeln!(out, "{step}"))
    }
}

/// What runs now: the daemon serving (its pid, what it hosts) and the
/// live frontends' process ids.
struct Running {
    daemon: Option<u32>,
    frontends: Vec<u32>,
}

impl Running {
    fn nothing(&self) -> bool {
        self.daemon.is_none() && self.frontends.is_empty()
    }

    fn said(&self) -> String {
        let daemon = self.daemon.map_or("no daemon".to_string(), |pid| {
            format!("the daemon (pid {pid})")
        });
        match self.frontends.as_slice() {
            [] => format!("{daemon}, no frontend"),
            pids => format!("{daemon}, frontends {}", pids_of(pids)),
        }
    }
}

fn pids_of(pids: &[u32]) -> String {
    pids.iter()
        .map(|pid| format!("pid {pid}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Run `ctxloom shutdown`.
pub(crate) fn run(
    wiring: &Wiring,
    stamp: &str,
    call: &Call<'_>,
    out: &mut dyn Write,
) -> Result<Exit, Failure> {
    let when = call
        .matches
        .subcommand_matches(NOUN)
        .and_then(|matches| matches.get_one::<String>("when").cloned());
    let period = match &when {
        Some(text) => library::config::span::parse(text).map_err(|cause| {
            Failure::new(
                ShutdownError::BadWhen {
                    value: text.clone(),
                    cause: cause.to_string(),
                },
                Exit::REFUSED,
            )
        })?,
        None => configured_drain_timeout(&wiring.fs(), wiring.home())
            .unwrap_or(daemon_application::HAND_OVER_PERIOD),
    };
    let format = call.format();
    let mut report = Report {
        drain_seconds: period.as_secs(),
        steps: Vec::new(),
    };
    let mut say = |line: String| {
        if format == Format::Text {
            let _ = writeln!(out, "{line}");
        }
        report.steps.push(line);
    };
    let shut = Shutdown { wiring, stamp };
    let left = shut.down(period, &mut say)?;
    if format == Format::Json {
        output(render(out, format, &report))?;
    }
    if left.nothing() {
        return Ok(Exit::OK);
    }
    Err(Failure::new(
        ShutdownError::Incomplete {
            daemon: left
                .daemon
                .map_or("none".to_string(), |pid| format!("pid {pid}")),
            frontends: match left.frontends.as_slice() {
                [] => "none".to_string(),
                pids => pids_of(pids),
            },
        },
        Exit::FAILED,
    ))
}

/// The shutdown of `wiring`'s home, as the client of stamp `stamp`.
struct Shutdown<'a> {
    wiring: &'a Wiring,
    stamp: &'a str,
}

impl Shutdown<'_> {
    fn frontends(&self) -> FrontendLocks {
        FrontendLocks::new(self.wiring.fs(), self.wiring.home())
    }

    /// What runs now.
    fn running(&self) -> Result<Running, Failure> {
        let files = files_of(self.wiring);
        Ok(Running {
            daemon: serving(self.wiring, &files)?.map(|found| found.pid),
            frontends: self.frontends().live(),
        })
    }

    /// Look at what runs until `done` says it is enough, or `within` has
    /// passed: what runs then.
    fn wait(&self, within: Duration, done: impl Fn(&Running) -> bool) -> Result<Running, Failure> {
        let deadline = Instant::now() + within;
        loop {
            let running = self.running()?;
            if done(&running) || Instant::now() >= deadline {
                return Ok(running);
            }
            std::thread::sleep(LOOK);
        }
    }

    /// A client of the daemon serving, starting none.
    fn client(&self) -> daemon_api::Client<Reach<'_>> {
        client_reaching(self.wiring, Reach::new(self.wiring, self.stamp).observing())
    }

    /// Drain within `period` (unless it is none), then stop what remains;
    /// what still runs after.
    fn down(&self, period: Duration, say: &mut dyn FnMut(String)) -> Result<Running, Failure> {
        let at_first = self.running()?;
        if at_first.nothing() {
            say("nothing of ctxloom is running".to_string());
            return Ok(at_first);
        }
        say(format!("running: {}", at_first.said()));
        if !period.is_zero() {
            if at_first.daemon.is_some() {
                let own = own_stamp(self.wiring.env(), self.stamp);
                let drained = block_on(self.client().drain(own, Some(period)))?.map_err(called)?;
                let hosts: Vec<String> = drained
                    .iter()
                    .flat_map(|daemon| daemon.runs.iter().map(|run| run.harp.clone()))
                    .collect();
                say(format!(
                    "asked the daemon to drain within {}s; it hosts: {}",
                    period.as_secs(),
                    if hosts.is_empty() {
                        "nothing".to_string()
                    } else {
                        hosts.join(", ")
                    }
                ));
            }
            let after = self.wait(period, Running::nothing)?;
            if after.nothing() {
                say("drained: everything ended".to_string());
                return Ok(after);
            }
            // The daemon's own period ends with it: it stops what it still
            // hosts, then exits, a grace later.
            let after = match after.daemon {
                Some(_) => self.wait(DRAIN_GRACE, |running| running.daemon.is_none())?,
                None => after,
            };
            say(format!(
                "after the drain's {}s, still running: {}",
                period.as_secs(),
                after.said()
            ));
        }
        self.stop_what_remains(say)
    }

    /// Ask each live frontend to stop, then the daemon serving the plain
    /// stop, waiting for each; what still runs after.
    fn stop_what_remains(&self, say: &mut dyn FnMut(String)) -> Result<Running, Failure> {
        let frontends = self.frontends();
        let live = frontends.live();
        for pid in &live {
            match frontends.ask_to_stop(*pid) {
                Ok(()) => say(format!("asked the frontend pid {pid} to stop")),
                Err(e) => say(format!("cannot ask the frontend pid {pid} to stop: {e}")),
            }
        }
        if !live.is_empty() {
            let after = self.wait(FRONTENDS_WAIT, |running| running.frontends.is_empty())?;
            if !after.frontends.is_empty() {
                say(format!(
                    "after {}s, frontends still running: {}",
                    FRONTENDS_WAIT.as_secs(),
                    pids_of(&after.frontends)
                ));
            }
        }
        if let Some(stopped) = block_on(self.client().stop(true))?.map_err(called)? {
            say(format!(
                "asked the daemon (pid {}) to stop even if live",
                stopped.pid
            ));
            let after = self.wait(STOP_WAIT, |running| running.daemon.is_none())?;
            if after.daemon.is_some() {
                say(format!(
                    "after {}s, the daemon still serves",
                    STOP_WAIT.as_secs()
                ));
            }
        }
        let left = self.running()?;
        if left.nothing() {
            say("stopped: nothing of ctxloom is running".to_string());
        } else {
            say(format!("still running: {}", left.said()));
        }
        Ok(left)
    }
}

#[cfg(test)]
#[path = "shutdown.test.rs"]
mod tests;
