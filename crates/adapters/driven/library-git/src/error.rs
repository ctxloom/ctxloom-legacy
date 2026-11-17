//! Why git could not be asked: missing, too old, or failing (G11). Each is a
//! failure of the environment, never a refusal.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LibraryGitError {
    /// git is not on the `PATH`.
    GitMissing,
    /// git is older than 2.25: `version` as it said it.
    GitTooOld { version: String },
    /// git ran and failed: its subcommand and flags (never a path's
    /// content), where, and its own words.
    Failed {
        args: String,
        dir: String,
        ext_err_txt: String,
    },
}

impl fmt::Display for LibraryGitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for LibraryGitError {
    fn code(&self) -> &str {
        match self {
            LibraryGitError::GitMissing => code!("git-missing"),
            LibraryGitError::GitTooOld { .. } => code!("git-too-old"),
            LibraryGitError::Failed { .. } => code!("failed"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            LibraryGitError::GitMissing => Vec::new(),
            LibraryGitError::GitTooOld { version } => vec![("version", version.as_str().into())],
            LibraryGitError::Failed {
                args,
                dir,
                ext_err_txt,
            } => vec![
                ("args", args.as_str().into()),
                ("dir", dir.as_str().into()),
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

/// One value of every error this crate defines, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(LibraryGitError::GitMissing),
        Box::new(LibraryGitError::GitTooOld {
            version: "2.20.1".to_string(),
        }),
        Box::new(LibraryGitError::Failed {
            args: "status --porcelain=v2".to_string(),
            dir: "/work/.ctxloom".to_string(),
            ext_err_txt: "fatal: bad object".to_string(),
        }),
    ]
}
