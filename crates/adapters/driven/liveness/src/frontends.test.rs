//! `ctxloom shutdown` finds every live frontend by its lock, and asks it to
//! stop by a file it watches (owner, 2026-11-15).

use std::sync::Arc;

use file_system::{Fs, Open, Rooted};
use os::Mode;

use super::FrontendLocks;

fn home() -> String {
    os::host().flavor().from_top("/home/u")
}

fn frontends() -> (Arc<Rooted>, FrontendLocks) {
    let fs = Arc::new(Rooted::temp().unwrap());
    (fs.clone(), FrontendLocks::new(fs, home()))
}

// A frontend's lock is under ctxloom's home, by its process id.
#[test]
fn a_frontends_lock_is_under_the_home_by_its_pid() {
    let (_, locks) = frontends();
    let at = |path: &str| std::path::PathBuf::from(os::host().flavor().from_top(path));
    assert_eq!(
        locks.lock_path(42),
        at("/home/u/.ctxloom/frontends/42.lock")
    );
    assert_eq!(
        locks.stop_path(42),
        at("/home/u/.ctxloom/frontends/42.stop")
    );
}

// A held lock is a live frontend; let go, it is none, and its files go.
#[test]
fn a_held_lock_is_a_live_frontend_and_a_let_go_one_leaves_nothing() {
    let (fs, locks) = frontends();
    assert!(locks.live().is_empty(), "no directory: none");
    let first = locks.hold(7).unwrap();
    let second = locks.hold(31).unwrap();
    assert_eq!(locks.live(), [7, 31]);
    drop(first);
    assert_eq!(locks.live(), [31]);
    assert!(!fs.exists(&locks.lock_path(7)).unwrap(), "its file went");
    drop(second);
    assert!(locks.live().is_empty());
}

// A dead frontend's lock file (left by a kill) reads free: it is no live
// frontend, and the look takes its files away.
#[test]
fn a_dead_frontends_files_are_taken_away() {
    let (fs, locks) = frontends();
    let path = locks.lock_path(9);
    fs.create_dir_all(path.parent().unwrap(), Mode::from_bits(0o700))
        .unwrap();
    fs.open(&path, Open::Create(Mode::from_bits(0o600)))
        .unwrap();
    locks.ask_to_stop(9).unwrap();
    fs.open(
        &path.with_file_name("not-a-pid.lock"),
        Open::Create(Mode::from_bits(0o600)),
    )
    .unwrap();
    assert!(locks.live().is_empty());
    assert!(!fs.exists(&path).unwrap());
    assert!(!locks.stop_asked(9));
}

// A stop asked is seen by the frontend; its hold takes an earlier one away
// first, and takes its own away when it is let go.
#[test]
fn a_stop_asked_is_seen_and_let_go_with_the_hold() {
    let (_, locks) = frontends();
    let held = locks.hold(5).unwrap();
    assert!(!locks.stop_asked(5));
    locks.ask_to_stop(5).unwrap();
    assert!(locks.stop_asked(5));
    drop(held);
    assert!(!locks.stop_asked(5), "let go with the hold");
    locks.ask_to_stop(5).unwrap();
    let again = locks.hold(5).unwrap();
    assert!(!locks.stop_asked(5), "an earlier process's ask is not its");
    drop(again);
}

// A lock another holds cannot be held again.
#[test]
fn a_held_frontend_lock_is_not_taken_twice() {
    let (_, locks) = frontends();
    let _held = locks.hold(3).unwrap();
    assert!(locks.hold(3).is_err());
}
