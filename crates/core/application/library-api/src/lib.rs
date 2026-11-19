//! The library's reads in the daemon (step 10a U2; plan `t13-step10.plan.md`
//! §2 U2): every read observes its tree first, records what changed as one
//! fact of the library domain, and only then opens the library. The tree is
//! seen by the daemon's scanner (step 10c U13 rework: every file with its
//! content's SHA-256, no git), and the aggregate compares its listing. A
//! tree outside git, or one git ignores, is refused (G1); a git operation
//! in progress is said (L11).
//!
//! - [`locate::refusal`]: a project's library outside git or ignored.
//! - [`Observations`]: the two, recorded through the daemon's edge.
//! - [`names`]: the library's resource names, the one naming every answer
//!   gives;
//! - [`resources`]: a library view as its API message, the one mapping;
//! - [`reads`]: the reads the daemon serves, each over the library opened
//!   for it;
//! - [`premise`]: the premise index an edge renders after an assembly.

pub mod deps;
pub mod distill;
mod locate;
pub mod names;
mod notice;
mod observe;
pub mod premise;
pub mod reads;
pub mod reconcile;
pub mod resources;
pub mod served_writes;
pub mod writes;

pub use locate::refusal;
#[cfg(any(test, feature = "test-support"))]
pub use notice::diagnostic_samples;
pub use notice::{LibraryApiNotice, EN_US};
pub use observe::{
    listed_as_held, CheckoutHarps, Clock, Observations, Observed, Observers, ReadError, Site, Trees,
};

#[cfg(test)]
#[path = "fakes.test.rs"]
mod fakes;
