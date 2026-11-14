//! The daemon's ONE launcher (owner T18), in the application ring: what the
//! user's daemon does with the one store (T14), over ports, so the
//! composition root only composes it.
//!
//! - Its START is the orphan sweep ([`Daemon::start_up`]): `run-launcher`'s
//!   [`Sweeper`], the one every launcher's step runs (rulings 86, 92, 93,
//!   95, 100), over every run the store holds, before anything is hosted. A
//!   killed daemon's children (their run locks left free) end `RunnerLost`.
//! - Each [`Daemon::tick`]: the read models catch up (the same folds the
//!   kernel runs inline, S32), the policy plan is made for each project on
//!   the board (`session-policies`), and its steps are taken: a launch
//!   claimed and minted, a child started (planned by its project's dry
//!   run, enqueued judged on its launcher's environment, its session home
//!   prepared and its engine started), a turn fed, a clear handed, an end
//!   asked; what a child said is recorded as its run's facts. A child's
//!   hand-off is read at each of its starts, deleted at its end, and a
//!   hand-off no open session holds is swept (T6, slice 4 U3).
//! - The one-shots it hosts ([`OneShots`], U4, T7): each is the run use
//!   case's one-shot run in the daemon over its one store, in its client's
//!   environment (the engine's TURN command, owner s4-Q8), its engine
//!   stopped by the tick's stop step once its run's stream asks.
//! - Engines are resolved by NAME through the registry the root composes
//!   ([`HostedEngine`]: an `Engine` beside its `ChildEngine`), never one
//!   adapter named in code.
//!
//! The clock, the runtime, the push and the poll stay the root's: it calls
//! [`Daemon::tick`] when a commit lands or the poll passes, and decides the
//! idle exit from [`Daemon::hosting`].

mod daemon;
mod one_shot;
mod ports;

pub use daemon::{Daemon, Folds, Place, Ports, Taken};
pub use one_shot::{
    Created, HostProject, HostRequest, Hosted, OneShotControl, OneShotHost, OneShots, Told, Watch,
    STOP_GRACE_MS,
};
pub use ports::{
    ChildHomes, ChildPlans, Ears, ForLaunch, HandOff, HandOffs, HostedEngine, Kept, Transcripts,
};
pub use run_launcher::{Sweeper, Swept};

#[cfg(test)]
#[path = "daemon.test.rs"]
mod tests;

#[cfg(test)]
#[path = "hosted.test.rs"]
mod hosted;
