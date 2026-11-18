//! Why `init` did not complete.

use std::fmt;

use angzarr_diagnostic::{Arg, Diagnostic};
use ctxloom_ports::Failure;

#[derive(Debug)]
pub enum InitError {
    /// The new project could not be scaffolded: the scaffold's own reason.
    Create(Failure),
    /// The directory could not be inspected: the adapter's own words.
    Files(Failure),
    /// git could not make or read the directory's repository (missing, too
    /// old, failing): the observer's own words (G1, G11).
    Git(Failure),
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for InitError {
    fn code(&self) -> &str {
        match self {
            InitError::Create(cause) | InitError::Files(cause) | InitError::Git(cause) => {
                cause.code()
            }
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            InitError::Create(cause) | InitError::Files(cause) | InitError::Git(cause) => {
                cause.args()
            }
        }
    }
}
