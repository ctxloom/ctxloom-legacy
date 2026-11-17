//! The library's reads in the daemon (step 10a U2; plan `t13-step10.plan.md`
//! §2 U2): every read observes its tree through git first (L5, L6), records
//! what changed as one fact of the library domain, and only then opens the
//! library. A tree outside git, or one git ignores, is refused (G1); a git
//! operation in progress is said (L11).
//!
//! - [`reconcile::sight`]: what git sees now against the record's state,
//!   as the one observation to record, or none.
//! - [`locate::refusal`]: a project's library outside git or ignored.
//! - [`Observations`]: the two, recorded through the daemon's edge.
//! - [`names`]: the library's resource names, the one naming every answer
//!   gives.

mod locate;
pub mod names;
mod notice;
mod observe;
pub mod reconcile;

pub use locate::refusal;
#[cfg(any(test, feature = "test-support"))]
pub use notice::diagnostic_samples;
pub use notice::{LibraryApiNotice, EN_US};
pub use observe::{CheckoutHarps, Observations, Observed, Observers, ReadError, Site};
pub use reconcile::{sight, Sight, PATHSPEC};

#[cfg(test)]
#[path = "fakes.test.rs"]
mod fakes;
