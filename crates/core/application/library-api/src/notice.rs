//! What a read says beside its answer (step 10a; texts.md §3, §4, accepted
//! verbatim): a git operation in progress in the tree it observed (L11),
//! and the home's library made a git repository (G2); and what a write
//! says (step 10b texts.md §3): a removal's path kept in the trash (G12). Notices, never
//! refusals: the read goes on.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};
use ctxloom_ports::Motion;

/// A notice of a read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryApiNotice {
    /// git is in the middle of `motion` in the tree at `root`.
    TreeInMotion { motion: Motion, root: String },
    /// The home's library at `path` was made a git repository by this read.
    HomeRepositoryMade { path: String },
    /// A removal took `path` into the trash `slot`, where it is kept 7 days
    /// (step 10b, G12; texts.md §3).
    Trashed { path: String, slot: String },
    /// A write to the home repository ctxloom made at `path` stands, but
    /// its commit (G3) failed, git saying `ext_err_txt` (step 10b, owner
    /// 2026-11-18, night: a notice, never a refusal).
    HomeNotCommitted { path: String, ext_err_txt: String },
}

impl fmt::Display for LibraryApiNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for LibraryApiNotice {
    fn code(&self) -> &str {
        match self {
            LibraryApiNotice::TreeInMotion { .. } => code!("notice-tree-in-motion"),
            LibraryApiNotice::HomeRepositoryMade { .. } => code!("notice-home-repository-made"),
            LibraryApiNotice::Trashed { .. } => code!("notice-trashed"),
            LibraryApiNotice::HomeNotCommitted { .. } => code!("notice-home-not-committed"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            LibraryApiNotice::TreeInMotion { motion, root } => vec![
                ("motion", motion.word().into()),
                ("root", root.as_str().into()),
            ],
            LibraryApiNotice::HomeRepositoryMade { path } => vec![("path", path.as_str().into())],
            LibraryApiNotice::Trashed { path, slot } => vec![
                ("path", path.as_str().into()),
                ("slot", slot.as_str().into()),
            ],
            LibraryApiNotice::HomeNotCommitted { path, ext_err_txt } => vec![
                ("path", path.as_str().into()),
                ("ext_err_txt", ext_err_txt.as_str().into()),
            ],
        }
    }
}

/// This crate's en-US messages, its name first.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!("../locales/en-US/messages.ftl"),
);

/// One value of every diagnostic with a message of this crate's.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(LibraryApiNotice::TreeInMotion {
            motion: Motion::Merging,
            root: "/work/demo/.ctxloom".to_string(),
        }),
        Box::new(LibraryApiNotice::HomeRepositoryMade {
            path: "/home/alice/.ctxloom".to_string(),
        }),
        Box::new(LibraryApiNotice::Trashed {
            path: ".ctxloom/content/bundles/v2/demo".to_string(),
            slot: ".ctxloom/state/trash/0195f1e2-0000-7000-8000-000000000000/".to_string(),
        }),
        Box::new(LibraryApiNotice::HomeNotCommitted {
            path: "/home/alice/.ctxloom".to_string(),
            ext_err_txt: "fatal: Unable to create '.git/index.lock': File exists.".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "notice.test.rs"]
mod tests;
