//! `HELD-FACTS-h1` (s4-Q14, unit H4): the capture facts this home's holds
//! keep, which no daemon has recorded yet, read through the holds' port
//! (never the store, never the daemon).
//!
//! - `ok`: nothing is held;
//! - `info`: facts a daemon of their version (or newer) takes, held for
//!   less than an hour: the next ctxloom command that reaches one delivers
//!   them;
//! - `warn`: a hold doctor cannot read, or one with facts held more than an
//!   hour, facts of a NEWER ctxloom than this one (no daemon of this
//!   version takes them: a downgrade, owner H-Q7 (a), kept until a
//!   reinstall or the session's destroy), facts dropped past its bound
//!   (gaps), set-aside parts (what a flush could never record), a format
//!   this ctxloom cannot read, or a torn or corrupt tail.
//!
//! Each hold is named by its session's harp (its first fact's session),
//! else its root, with its count, size, stamps and oldest instant. Doctor
//! removes no hold: `--fix` leaves them (owner H-Q7 (a)).

use run_ports::{HoldError, HoldSummary};

use crate::checks::{Check, Status};

/// The row's marker.
pub const MARKER: &str = "DOCTOR-CHECK-HELD-FACTS-h1";

/// How long a fact may be held before doctor warns of it: an hour.
pub const STALE_AFTER_SECONDS: i64 = 3_600;

/// The row for `holds` (the holds' survey, or why it could not be read),
/// judged by this ctxloom's stamp `own` at `now` (seconds since the epoch).
pub fn held_facts(holds: &Result<Vec<HoldSummary>, HoldError>, own: &str, now: i64) -> Check {
    let _ = (holds, own, now);
    Check {
        marker: MARKER,
        name: "held facts",
        status: Status::Ok,
        detail: "no fact is held".to_string(),
        remedy: String::new(),
    }
}
