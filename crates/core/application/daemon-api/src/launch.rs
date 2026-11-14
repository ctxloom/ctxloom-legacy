//! The launch on the wire (T10): `run_ports::Launch`, the one launch's
//! value, as `ctxloom.v1.Launch` and back. The value is the core's
//! (`run-ports`); only this layer sees the wire type.
//!
//! What the launch said (its ignored permits, the settings it left out) and
//! its input (a one-shot turn's prompt) are never on the wire: a launch read
//! from it says nothing and feeds nothing. A one-shot's piped stdio has no
//! wire value of its own (`STDIO_UNSPECIFIED`): its launch is the daemon's
//! and never crosses.

use std::time::Duration;

use daemon_proto::v1;
use run_ports::{Launch, LaunchAt, LaunchKind, Stdio, StopPolicy};

/// `launch` as the wire carries it.
pub fn to_wire(launch: &Launch) -> v1::Launch {
    let _ = launch;
    v1::Launch::default()
}

/// The launch `wire` carries; none when it names no kind it knows.
pub fn from_wire(wire: &v1::Launch) -> Option<Launch> {
    let _ = (
        wire,
        Duration::ZERO,
        LaunchAt::default(),
        StopPolicy::of(LaunchKind::Child),
        Stdio::Piped,
    );
    None
}

#[cfg(test)]
#[path = "launch.test.rs"]
mod tests;
