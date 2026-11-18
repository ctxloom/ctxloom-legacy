//! A project is in git (step 10, G1): init (and `config create`, which is
//! init's scaffold without the interview, QUESTIONS.md E) makes the
//! directory a git repository when it is in none, unasked, and says so. It
//! commits nothing: what to commit, and when, stays hers. A directory
//! already in a work tree, its root or below it, is left as it is.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};
use ctxloom_ports::{TreeError, TreeObserver};

/// What init says when it made the directory a git repository.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GitInitialized;

impl fmt::Display for GitInitialized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for GitInitialized {
    fn code(&self) -> &str {
        code!("notice-git-initialized")
    }

    // Equivalent: its only mutant (`vec![]`) is the same empty list.
    #[cfg_attr(test, mutants::skip)]
    fn args(&self) -> Vec<(&str, Arg)> {
        Vec::new()
    }
}

/// Make `dir` a git repository when it is in no work tree (`git init`,
/// committing nothing): the notice when it did, none when `dir` was in one
/// already. git missing, too old or failing is the observer's error, before
/// anything is made.
pub fn ensure_repository(
    observer: &dyn TreeObserver,
    dir: &str,
) -> Result<Option<GitInitialized>, TreeError> {
    if observer.work_tree(dir)?.is_some() {
        return Ok(None);
    }
    observer.init_repository(dir, None)?;
    Ok(Some(GitInitialized))
}

/// This crate's en-US messages, its name first.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!("../locales/en-US/messages.ftl"),
);

/// One value of every diagnostic this crate defines, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![Box::new(GitInitialized)]
}

#[cfg(test)]
#[path = "git.test.rs"]
pub(crate) mod tests;
