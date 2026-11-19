//! Whether a library tree git holds is one it ignores: such a library is
//! refused (`library-tree-library-ignored`, the library domain's own
//! refusal, for a read as for a write, exit 20). The library is git-free
//! (owner, 2026-11-18; step 10c U6 took out G1's refusal outside a work
//! tree, `library-tree-not-in-git`, which init no longer avoids by running
//! `git init`): a library in no work tree is read as any other.

use ctxloom_ports::{TreeError, TreeObserver};
use library_tree::LibraryTreeRejection;

/// The file a library always has, whose ignore rule is the library's.
const PROBE: &str = ".ctxloom/config.yaml";

/// Why the library of the project at `root` (its `.ctxloom` there) is not
/// read, if it is not: none when it is in no work tree, or in one git does
/// not ignore it in.
pub fn refusal(
    observer: &dyn TreeObserver,
    root: &str,
) -> Result<Option<LibraryTreeRejection>, TreeError> {
    // The library is git-free (owner, 2026-11-18): a library in no work
    // tree is read as any other; only one git holds and ignores is refused.
    if observer.work_tree(root)?.is_none() {
        return Ok(None);
    }
    Ok(observer
        .check_ignore(root, PROBE)?
        .map(|rule| LibraryTreeRejection::LibraryIgnored {
            source: rule.source,
            line: rule.line,
            pattern: rule.pattern,
        }))
}

#[cfg(test)]
#[path = "locate.test.rs"]
mod tests;
