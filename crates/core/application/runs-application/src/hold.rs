//! The hook's HOLD policy (s4-Q14, unit H2): whether a hook delivers what
//! it observed to the user's daemon or keeps it in its session's hold, and,
//! when the daemon is up to date for it, delivering what the session
//! already holds FIRST (owner, H-Q5 revised: the client side flushes, never
//! the daemon).
//!
//! [`Holding::record`] judges one hook invocation's observations together,
//! in [`observe`] order:
//!
//! - no daemon reached or started ([`Serving::Unreached`], H-Q2): every
//!   observation is HELD;
//! - a daemon of an older version than the hook's ([`up_to_date`] false,
//!   H-Q1): every observation is HELD;
//! - a daemon up to date for the hook: the session's hold, if it holds
//!   anything, is taken ([`HeldFacts::take`]) and delivered in held order,
//!   each record the daemon is up to date for, then its delivered records
//!   cut, again until nothing it can deliver is left; then the hook's own
//!   observations are delivered. A hold another process is taking, a
//!   delivery left unsettled or the hook's time spent leaves the rest of the
//!   hold where it is and holds the hook's own facts BEHIND it, so a
//!   session's facts reach its stream first-in, first-out. An own delivery
//!   left unsettled (the daemon died under it) is held, and every later one
//!   behind it.
//!
//! A final answer (`Refused`, `Parked`) is never held: a retry would be
//! answered the same. A held record the daemon is not up to date for (a
//! downgrade) is kept and passed over, so it blocks nothing after it; a gap
//! record goes (it was said when the fact was dropped); a record that is not
//! a fact of this session (its external id is not its book's, or its book is
//! another stream's) is dropped and said. The hook's time is the
//! [`HookClock`]'s: the core reads no clock. Nothing here fails: whatever
//! went wrong is a [`Kept`] in the [`HoldReport`], for the hook to say.

use std::time::Duration;

use angzarr_embedded::v1::Uuid;
use daemon_proto::google::protobuf::Timestamp;
use run_ports::{Bound, Delivery, FactSink, HeldFacts, HoldError};
use surface_ports::HookEvent;

/// How many times one hook takes its session's hold before it gives up and
/// holds its own facts behind what is left: facts held by other hooks while
/// it delivers are taken again, a bounded number of times.
pub const TAKE_ROUNDS: usize = 8;

/// The hook's time, as its driving adapter reads the clock.
pub trait HookClock {
    /// What is left of the hook's bound for delivering.
    fn remaining(&self) -> Duration;
    /// The time now, a held fact's `held_at` (an age; it orders nothing).
    fn now(&self) -> Option<Timestamp>;
}

/// Whom the hook would deliver to.
pub enum Serving<'a, S: FactSink + ?Sized> {
    /// The user's daemon, of the versionator stamp `stamp`, through `sink`.
    Daemon { stamp: &'a str, sink: &'a S },
    /// No daemon could be reached or started within the hook's bound:
    /// `why`, as the hook says it.
    Unreached { why: String },
}

/// Why a fact was held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// The serving daemon, of `daemon`, is older than the hook's `own`.
    Older { daemon: String, own: String },
    /// No daemon could be reached or started.
    Unreached { why: String },
    /// Its delivery was not settled (the daemon died under it, or did not
    /// answer in time).
    Unsettled(Delivery),
    /// The hook's time was spent before it could be delivered.
    OutOfTime,
}

/// Why a fact was neither recorded nor held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotHeld {
    /// The hold is at its bound: this fact was dropped (a gap marks it).
    Full(Bound),
    /// The hold could not be used.
    Failed(HoldError),
    /// A held record that is not a fact of this session: dropped.
    Foreign,
}

/// What became of one fact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Delivered, and finally answered.
    Delivered(Delivery),
    /// Held, for `Why`.
    Held(Why),
    /// Held behind what the session's hold still holds.
    HeldBehind,
    /// Neither recorded nor held.
    NotHeld(NotHeld),
}

/// One fact, and what became of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    pub external_id: String,
    pub outcome: Outcome,
}

impl Kept {
    /// Whether the hook has something to say of it: anything but a fact
    /// the daemon has (applied, or a duplicate).
    pub fn said(&self) -> bool {
        !matches!(
            self.outcome,
            Outcome::Delivered(Delivery::Applied | Delivery::Duplicate)
        )
    }
}

/// What a hook's holding did: the session's held facts it delivered (or
/// dropped), then its own facts.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct HoldReport {
    pub flushed: Vec<Kept>,
    pub own: Vec<Kept>,
}

impl HoldReport {
    /// Every fact the hook has something to say of, the held ones first.
    pub fn said(&self) -> Vec<&Kept> {
        self.flushed
            .iter()
            .chain(&self.own)
            .filter(|kept| kept.said())
            .collect()
    }
}

/// The hook-side hold policy over a session's hold.
pub struct Holding<'a, H: HeldFacts + ?Sized, C: HookClock + ?Sized> {
    held: &'a H,
    clock: &'a C,
    own: &'a str,
    session: &'a str,
}

impl<'a, H: HeldFacts + ?Sized, C: HookClock + ?Sized> Holding<'a, H, C> {
    /// The policy of a hook of stamp `own` for the session named `session`
    /// (`CTXLOOM_SESSION`'s resource name), over `held` and `clock`.
    pub fn new(held: &'a H, clock: &'a C, own: &'a str, session: &'a str) -> Holding<'a, H, C> {
        Holding {
            held,
            clock,
            own,
            session,
        }
    }

    /// Deliver or hold `event`'s observations for `session` (the stream
    /// root), as `serving` allows. Infallible by type. RED: not built.
    pub fn record<S: FactSink + ?Sized>(
        &self,
        session: &Uuid,
        event: &HookEvent,
        serving: Serving<'_, S>,
    ) -> HoldReport {
        let _ = (
            self.held,
            self.clock,
            self.own,
            self.session,
            session,
            event,
            serving,
        );
        HoldReport::default()
    }
}

#[cfg(test)]
#[path = "hold.test.rs"]
mod tests;
