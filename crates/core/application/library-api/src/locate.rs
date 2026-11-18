//! Whether a library tree is git's (G1): a project's library is refused
//! outside a git work tree (`library-tree-not-in-git`) and where git
//! ignores it (`library-tree-library-ignored`), each the library domain's
//! own refusal, for a read as for a write (exit 20).

use ctxloom_ports::{TreeError, TreeObserver};
use library_tree::LibraryTreeRejection;

/// The file a library always has, whose ignore rule is the library's.
const PROBE: &str = ".ctxloom/config.yaml";

/// Why the library of the project at `root` (its `.ctxloom` there) is not
/// read, if it is not: none when it is in a work tree git does not ignore
/// it in.
pub fn refusal(
    observer: &dyn TreeObserver,
    root: &str,
) -> Result<Option<LibraryTreeRejection>, TreeError> {
    if observer.work_tree(root)?.is_none() {
        return Ok(Some(LibraryTreeRejection::NotInGit {
            root: root.to_string(),
        }));
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
