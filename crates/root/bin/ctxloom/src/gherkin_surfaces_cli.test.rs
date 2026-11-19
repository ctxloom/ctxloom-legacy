//! The steps of the surfaces World's CLI contract features (`features/cli/`)
//! that no journey's module has, written as step 11 un-pended them (each
//! step names its feature).

use cucumber::{given, then, when};

use super::World;
use crate::tests::files;

/// remote.feature: a command run from a subdirectory of the project (made
/// if it is not there), with no `CTXLOOM_ROOT`, so the project is found by
/// walking up.
#[when(expr = "I run {string} from the project subdirectory {string}")]
fn run_from_subdirectory(w: &mut World, line: String, sub: String) {
    let dir = w.common.project.dir().join(&sub);
    files::mkdirs(&dir);
    w.common.run_lines_at(Some(&dir), &line, "");
}

/// remote.feature: ctxloom has never run in this home.
#[given(expr = "the home has no {string} directory yet")]
fn home_lacks_dir(w: &mut World, rel: String) {
    assert!(
        !files::exists(w.common.project.home().join(&rel)),
        "the home already has {rel}"
    );
}

/// remote.feature: the directory's owner may read, write and search it,
/// where the host has permission bits at all.
#[then(expr = "the home directory {string} is readable, writable and searchable by its owner")]
fn home_dir_usable(w: &mut World, rel: String) {
    let dir = w.common.project.home().join(&rel);
    assert!(files::exists(&dir), "{rel} does not exist");
    if let Some(mode) = files::mode_of(&dir) {
        assert_eq!(mode & 0o700, 0o700, "{rel} is {mode:o}");
    }
}

#[then(expr = "the home file {string} contains {string}")]
fn home_holds(w: &mut World, rel: String, text: String) {
    let held = w.common.project.read_home(&rel).expect("a home file");
    assert!(held.contains(&text), "{rel}: {held}");
}
