//! A ctxloom command's flush of its home's HOLDS on the way in (s4-Q14,
//! unit H3; owner, H-Q5 revised: the client side pushes a hold over the
//! daemon's API, and the daemon reads no hold file).
//!
//! A command of an up-to-date version (not older than the serving daemon,
//! [`runs_application::command_flushes`]) that reaches the daemon delivers
//! what EVERY session's hold under its home holds, through the daemon's
//! listener with the user bearer, exactly as a hook delivers (the same
//! [`runs_application::Flush`]): each hold taken at once or skipped (its
//! flush lock), in held order, its delivered prefix cut, a gap record
//! delivered as the gap fact in its place. Every session's, not only the
//! command's project's: the daemon and its one store serve the whole home,
//! and a hold names its session only inside its records. The whole flush is
//! bounded by [`FLUSH_BOUND`], so a command never stalls on it; what the
//! bound leaves is the next command's (or the session's next hook's).
//!
//! It says nothing: what it could not deliver stays held, and a final
//! answer (refused, parked) or a foreign record leaves the hold unsaid.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use run_ports::{FactSink, HeldFacts, HoldError};
use runs_application::{command_flushes, Flush, HookClock, SessionFlush};

use super::{files_of, own_stamp, serving_here};
use crate::capture::{daemon_sink, Deadline};
use crate::Wiring;

/// The most a command's flush of its home's holds may take, in all.
pub(crate) const FLUSH_BOUND: Duration = Duration::from_secs(2);

/// What a command's flush did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Flushed {
    /// No live daemon serves this home: nothing was delivered (none is
    /// started for a flush).
    NotServing,
    /// The serving daemon, of `daemon`, is newer than this command: it is
    /// not this command's to deliver to.
    Older { daemon: String },
    /// Each hold it found, and what became of it.
    Done(Vec<SessionFlush>),
    /// The holds could not be listed.
    Failed(HoldError),
}

/// Deliver every session's hold under `wiring`'s home to the daemon now
/// serving it (none is started), as a command of stamp `stamp` (the
/// binary's; a test's own under test-support), within [`FLUSH_BOUND`].
/// The frontend calls it at its start and its exit report (slice 4 U5);
/// a command on its way in calls it through [`on_the_way_in`].
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "slice 4 U5's frontend calls it at its start and its exit report"
    )
)]
pub(crate) fn flush_holds(wiring: &Wiring, stamp: &str) -> Flushed {
    let own = own_stamp(wiring.env(), stamp);
    match serving_here(wiring, &files_of(wiring)) {
        Ok(Some(found)) => flush_to(wiring, own, &found),
        Ok(None) | Err(_) => Flushed::NotServing,
    }
}

/// [`flush_holds`] to the daemon `found` names, live and this home's.
pub(crate) fn flush_to(wiring: &Wiring, own: &str, found: &run_ports::Discovery) -> Flushed {
    let held = fact_spool::Spool::at_home(wiring.fs(), wiring.home());
    let sink = daemon_sink(found.clone());
    let clock = Deadline(Instant::now() + FLUSH_BOUND);
    flush_with(&held, &clock, own, &found.stamp, &sink)
}

/// A command of stamp `own`'s flush of `held` to a daemon of stamp
/// `daemon`, through `sink`, within `clock`'s time.
pub(crate) fn flush_with<H, C, S>(held: &H, clock: &C, own: &str, daemon: &str, sink: &S) -> Flushed
where
    H: HeldFacts + ?Sized,
    C: HookClock + ?Sized,
    S: FactSink + ?Sized,
{
    if !command_flushes(own, daemon) {
        return Flushed::Older {
            daemon: daemon.to_string(),
        };
    }
    match Flush::new(held, clock).every(daemon, sink) {
        Ok(done) => Flushed::Done(done),
        Err(error) => Flushed::Failed(error),
    }
}

/// Whether this process has flushed on its way in: once a process, however
/// many times it reaches the daemon.
static FLUSHED_ON_THE_WAY_IN: WayIn = WayIn::new();

/// A once-a-process latch.
pub(crate) struct WayIn(AtomicBool);

impl WayIn {
    pub(crate) const fn new() -> WayIn {
        WayIn(AtomicBool::new(false))
    }

    /// Whether this is the first time it is asked.
    pub(crate) fn first(&self) -> bool {
        !self.0.swap(true, Ordering::SeqCst)
    }
}

/// A command reaching the daemon `found` names, of stamp `own`: its flush
/// of the home's holds, the first time this process reaches it.
pub(crate) fn on_the_way_in(wiring: &Wiring, own: &str, found: &run_ports::Discovery) {
    if FLUSHED_ON_THE_WAY_IN.first() {
        let _ = flush_to(wiring, own, found);
    }
}

#[cfg(test)]
#[path = "flush.test.rs"]
mod tests;
