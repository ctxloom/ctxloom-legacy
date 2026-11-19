//! git's operation in progress, as the library domain names it (L11). The
//! reconcile against git (step 10a U2) is gone: the scanner sees a tree
//! (step 10c U13 rework) and the aggregate compares its listing.

use ctxloom_ports::Motion;

/// git's operation as the library domain names it.
pub fn motion_of(motion: Motion) -> library_tree::Motion {
    match motion {
        Motion::IndexLocked => library_tree::Motion::IndexLocked,
        Motion::Merging => library_tree::Motion::Merging,
        Motion::Rebasing => library_tree::Motion::Rebasing,
        Motion::CherryPicking => library_tree::Motion::CherryPicking,
        Motion::Reverting => library_tree::Motion::Reverting,
    }
}
