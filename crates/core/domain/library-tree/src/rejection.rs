//! Why a command to a library tree was refused.

use std::collections::BTreeMap;
use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};

use crate::Motion;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LibraryTreeRejection {
    /// The path's sequence moved since the token the caller sent.
    PlanStale { path: String, seen: u64, now: u64 },
    /// Something is already at the path.
    ResourceExists { path: String },
    /// Nothing is at the path.
    NoSuchResource { path: String },
    /// The path is the built-in bundle's (ruling 127).
    BuiltinReadOnly { bundle: String },
    /// The plan failed (L14): its own diagnostic's code and args, which ARE
    /// this rejection's, so the person is told that diagnostic's message
    /// from its own catalog.
    PlanRejected {
        code: String,
        args: BTreeMap<String, String>,
    },
    /// A git operation is in progress in the tree (L11).
    TreeInMotion { motion: Motion, root: String },
    /// The tree is in no git work tree (G1).
    NotInGit { root: String },
    /// git ignores the library (G1).
    LibraryIgnored {
        source: String,
        line: u32,
        pattern: String,
    },
    /// The change would change more than one file (T29, D14): each, below
    /// its bundle.
    OneChangePerCall { paths: Vec<String> },
    /// No trash slot of the request is in the state (purged already).
    NoSuchSlot { request_id: String },
    /// The slot is younger than [`crate::KEEP_DAYS`] (G12).
    SlotNotDue { request_id: String, age_days: u32 },
}

/// A motion as the catalog's selector names it.
pub fn motion_word(motion: Motion) -> &'static str {
    match motion {
        Motion::IndexLocked => "index-locked",
        Motion::Merging => "merging",
        Motion::Rebasing => "rebasing",
        Motion::CherryPicking => "cherry-picking",
        Motion::Reverting => "reverting",
    }
}

impl fmt::Display for LibraryTreeRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for LibraryTreeRejection {
    fn code(&self) -> &str {
        match self {
            LibraryTreeRejection::PlanStale { .. } => code!("plan-stale"),
            LibraryTreeRejection::ResourceExists { .. } => code!("resource-exists"),
            LibraryTreeRejection::NoSuchResource { .. } => code!("no-such-resource"),
            LibraryTreeRejection::BuiltinReadOnly { .. } => code!("builtin-read-only"),
            LibraryTreeRejection::PlanRejected { code, .. } => code,
            LibraryTreeRejection::TreeInMotion { .. } => code!("tree-in-motion"),
            LibraryTreeRejection::NotInGit { .. } => code!("not-in-git"),
            LibraryTreeRejection::LibraryIgnored { .. } => code!("library-ignored"),
            LibraryTreeRejection::OneChangePerCall { .. } => code!("one-change-per-call"),
            LibraryTreeRejection::NoSuchSlot { .. } => code!("no-such-slot"),
            LibraryTreeRejection::SlotNotDue { .. } => code!("slot-not-due"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            LibraryTreeRejection::PlanStale { path, seen, now } => vec![
                ("path", path.as_str().into()),
                ("seen", (*seen).into()),
                ("now", (*now).into()),
            ],
            LibraryTreeRejection::ResourceExists { path }
            | LibraryTreeRejection::NoSuchResource { path } => vec![("path", path.as_str().into())],
            LibraryTreeRejection::BuiltinReadOnly { bundle } => {
                vec![("bundle", bundle.as_str().into())]
            }
            LibraryTreeRejection::PlanRejected { args, .. } => args
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str().into()))
                .collect(),
            LibraryTreeRejection::TreeInMotion { motion, root } => vec![
                ("motion", motion_word(*motion).into()),
                ("root", root.as_str().into()),
            ],
            LibraryTreeRejection::NotInGit { root } => vec![("root", root.as_str().into())],
            LibraryTreeRejection::LibraryIgnored {
                source,
                line,
                pattern,
            } => vec![
                ("source", source.as_str().into()),
                ("line", (*line).into()),
                ("pattern", pattern.as_str().into()),
            ],
            LibraryTreeRejection::OneChangePerCall { paths } => vec![
                ("count", paths.len().into()),
                ("paths", paths.join(", ").into()),
            ],
            LibraryTreeRejection::NoSuchSlot { request_id } => {
                vec![("request_id", request_id.as_str().into())]
            }
            LibraryTreeRejection::SlotNotDue {
                request_id,
                age_days,
            } => vec![
                ("request_id", request_id.as_str().into()),
                ("age_days", (*age_days).into()),
            ],
        }
    }
}

/// This crate's en-US messages, its name first.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!("../locales/en-US/messages.ftl"),
);

/// One value of every rejection with a message of this crate's, for the
/// catalog test (a plan's rejection speaks in its own crate's words).
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let path = || "content/bundles/v2/demo/bundle.yaml".to_string();
    vec![
        Box::new(LibraryTreeRejection::PlanStale {
            path: path(),
            seen: 3,
            now: 5,
        }),
        Box::new(LibraryTreeRejection::ResourceExists { path: path() }),
        Box::new(LibraryTreeRejection::NoSuchResource { path: path() }),
        Box::new(LibraryTreeRejection::BuiltinReadOnly {
            bundle: "ctxloom".to_string(),
        }),
        Box::new(LibraryTreeRejection::TreeInMotion {
            motion: Motion::Merging,
            root: "/work/demo".to_string(),
        }),
        Box::new(LibraryTreeRejection::NotInGit {
            root: "/work/demo".to_string(),
        }),
        Box::new(LibraryTreeRejection::LibraryIgnored {
            source: ".gitignore".to_string(),
            line: 1,
            pattern: ".ctxloom/".to_string(),
        }),
        Box::new(LibraryTreeRejection::OneChangePerCall {
            paths: vec!["fragments/a.md".to_string(), "fragments/b.md".to_string()],
        }),
        Box::new(LibraryTreeRejection::NoSuchSlot {
            request_id: "01a1ecdd-5f31-75fa-8782-789facb7078e".to_string(),
        }),
        Box::new(LibraryTreeRejection::SlotNotDue {
            request_id: "01a1ecdd-5f31-75fa-8782-789facb7078e".to_string(),
            age_days: 6,
        }),
    ]
}
