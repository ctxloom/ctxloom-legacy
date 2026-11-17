//! The home is a git repository (step 10, G2): when the home's `.ctxloom`
//! is in no git work tree, it is made one, once, unasked, committing
//! nothing. Its `.gitignore` is an ALLOWLIST ([`HOME_GITIGNORE`]), so only
//! ctxloom's own files are ever seen, and nothing else in the home (an older
//! ctxloom's litter, the store, the runs) is tracked by accident; once
//! written it is hers. A home already in a repository (a dotfiles one) is
//! used as it is, unless that repository ignores the home's library
//! (QUESTIONS.md C): then the home gets a repository of its own, nested.
//!
//! Each home a command names is made a repository at its first read
//! (QUESTIONS.md B). ctxloom commits there only in a repository it made
//! (QUESTIONS.md D; the commits are 10b's).

use ctxloom_ports::{TreeError, TreeObserver};

/// The home repository's `.gitignore`, written whole when ctxloom makes it.
pub const HOME_GITIGNORE: &str =
    "# ctxloom: only what is listed here is ctxloom's own, and seen by git.\n\
/*\n\
!/.gitignore\n\
!/config.yaml\n\
!/remotes.yaml\n\
!/lock.yaml\n\
!/content/\n";

/// What the home's library is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HomeRepository {
    /// ctxloom made it a repository, just now.
    Made,
    /// It was in a work tree that does not ignore it, which is used as it is.
    UsedAsItIs,
}

/// The file whose ignoring is the library's: the config, which every home
/// library has first.
const PROBE: &str = "config.yaml";

/// Make the home's library `app_dir` (a directory that exists) a git
/// repository when it is in none, or in one that ignores it.
pub fn ensure_home_repository(
    observer: &dyn TreeObserver,
    app_dir: &str,
) -> Result<HomeRepository, TreeError> {
    let used = match observer.work_tree(app_dir)? {
        None => false,
        // A repository of its own: nothing more to ask.
        Some(top) if same_dir(&top, app_dir) => true,
        Some(_) => observer.check_ignore(app_dir, PROBE)?.is_none(),
    };
    if used {
        return Ok(HomeRepository::UsedAsItIs);
    }
    observer.init_repository(app_dir, Some(HOME_GITIGNORE))?;
    Ok(HomeRepository::Made)
}

/// Whether two names of a directory are the same text, as git and the host
/// spell it (`\` or `/`, a trailing separator or none).
fn same_dir(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_string();
    norm(a) == norm(b)
}

#[cfg(test)]
#[path = "home_repository.test.rs"]
mod tests;
