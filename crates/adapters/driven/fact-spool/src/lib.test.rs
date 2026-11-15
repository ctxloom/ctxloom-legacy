//! The hold on disk (s4-Q14, H1): append and take in order, the cut, the
//! bounds and their gap, a torn tail and corruption, the modes, and each
//! race of the design's §4 FORCED through the locks (a lock the test holds,
//! the append lock's pause seam), never waited for.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use file_system::{Fs, Open, Rooted};
use os::Mode;
use run_ports::{Bound, Fate, Held, HeldEntry, HeldFact, HeldFacts, HoldError, Settled};
use uuid::Uuid;

use super::frame::{self, Tail, MAGIC};
use super::{Pause, Spool};

fn home() -> PathBuf {
    PathBuf::from(os::host().flavor().from_top("/home/a"))
}

fn spool() -> (Arc<Rooted>, Spool) {
    let fs = Arc::new(Rooted::temp().unwrap());
    let spool = Spool::at_home(fs.clone(), &home());
    (fs, spool)
}

fn root(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn fact(id: &str) -> HeldFact {
    HeldFact {
        stamp: "v0.2.0-abcdef0".to_string(),
        session: "projects/p/sessions/brisk-copper-moth".to_string(),
        external_id: id.to_string(),
        book: id.as_bytes().repeat(4),
        held_at: None,
    }
}

fn ids(entries: &[HeldEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match entry {
            HeldEntry::Fact(fact) => fact.external_id.clone(),
            HeldEntry::Gap(gap) => format!("gap:{}:{}", gap.external_id, gap.why),
        })
        .collect()
}

/// What the session's hold holds, in order, taken and let go.
fn held(spool: &Spool, session: &Uuid) -> Vec<String> {
    let batch = spool.take(session).unwrap().expect("no other taker");
    ids(batch.entries())
}

fn hold_all(spool: &Spool, session: &Uuid, names: &[&str]) {
    for name in names {
        assert_eq!(
            spool.hold(session, fact(name)).unwrap(),
            Held::Kept,
            "{name}"
        );
    }
}

#[test]
fn a_hold_is_taken_in_the_order_held() {
    let (_fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a", "b", "c"]);
    assert_eq!(held(&spool, &s), ["a", "b", "c"]);
    // A take changes nothing until it is settled.
    assert_eq!(held(&spool, &s), ["a", "b", "c"]);
    let batch = spool.take(&s).unwrap().unwrap();
    assert_eq!(
        batch.entries()[0],
        HeldEntry::Fact(fact("a")),
        "the record whole"
    );
}

#[test]
fn a_hold_lives_under_the_homes_held_directory_named_by_its_session_root() {
    let (fs, spool) = spool();
    let s = root(7);
    hold_all(&spool, &s, &["a"]);
    let dir = home().join(".ctxloom").join("held");
    assert_eq!(spool.dir(), dir);
    assert_eq!(spool.hold_path(&s), dir.join(format!("{s}.held")));
    assert_eq!(spool.append_lock_path(&s), dir.join(format!("{s}.lock")));
    assert_eq!(
        spool.flush_lock_path(&s),
        dir.join(format!("{s}.flush.lock"))
    );
    let bytes = fs.read(&spool.hold_path(&s)).unwrap();
    assert!(bytes.starts_with(MAGIC));
}

#[test]
fn a_cut_removes_the_delivered_prefix_and_a_whole_cut_the_hold() {
    let (fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a", "b", "c"]);
    let batch = spool.take(&s).unwrap().unwrap();
    assert_eq!(batch.cut(2).unwrap(), Settled::Cut { kept: 1 });
    assert_eq!(held(&spool, &s), ["c"]);
    let batch = spool.take(&s).unwrap().unwrap();
    assert_eq!(batch.cut(1).unwrap(), Settled::Emptied);
    assert!(!fs.exists(&spool.hold_path(&s)).unwrap());
    assert!(spool.is_empty(&s).unwrap());
}

#[test]
fn a_settle_keeps_what_was_not_delivered_in_its_place() {
    let (_fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a", "b", "c", "d"]);
    let batch = spool.take(&s).unwrap().unwrap();
    assert_eq!(
        batch.settle(|i| i == 1 || i == 3).unwrap(),
        Settled::Cut { kept: 2 }
    );
    assert_eq!(held(&spool, &s), ["b", "d"]);
}

// s4-Q14, H3 (no silent drop): a record the flusher sets aside leaves the
// hold for its set-aside part, the same as corruption: its frame kept as the
// first free `<root>.held.corrupt-<n>` (0600), never taken again, and
// counted by the survey; what was kept stays in its place.
#[test]
fn a_record_set_aside_goes_to_the_holds_set_aside_part() {
    let (fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a", "b", "c", "d"]);
    let batch = spool.take(&s).unwrap().unwrap();
    let fates = [Fate::Cut, Fate::SetAside, Fate::Keep, Fate::SetAside];
    assert_eq!(
        batch.settle_fates(|i| fates[i]).unwrap(),
        Settled::Cut { kept: 1 }
    );
    assert_eq!(held(&spool, &s), ["c"]);
    let aside = spool.dir().join(format!("{s}.held.corrupt-1"));
    let bytes = fs.read(&aside).expect("the set-aside part");
    // Its frames as they were, so a person (or doctor) can read them.
    let scan = frame::scan(&[MAGIC, bytes.as_slice()].concat(), u64::MAX).unwrap();
    let entries: Vec<HeldEntry> = scan.entries.into_iter().map(|(e, _)| e).collect();
    assert_eq!(ids(&entries), ["b", "d"]);
    assert!(!fs
        .exists(&spool.dir().join(format!("{s}.held.corrupt-2")))
        .unwrap());
    if let Some(m) = mode(&fs, &aside) {
        assert_eq!(m & 0o777, 0o600, "{m:o}");
    }
    let survey = spool.survey().unwrap();
    assert_eq!(survey[0].set_aside, 1);
    assert_eq!(survey[0].facts, 1);
}

// Nothing set aside, no set-aside part is made.
#[test]
fn a_settle_that_sets_nothing_aside_makes_no_set_aside_part() {
    let (fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a", "b"]);
    let batch = spool.take(&s).unwrap().unwrap();
    batch.settle_fates(|_| Fate::Cut).unwrap();
    assert!(!names(&fs, spool.dir())
        .iter()
        .any(|n| n.contains("corrupt")));
}

#[test]
fn an_empty_hold_is_none_or_its_magic_alone() {
    let (fs, spool) = spool();
    let s = root(1);
    assert!(spool.is_empty(&s).unwrap());
    hold_all(&spool, &s, &["a"]);
    assert!(!spool.is_empty(&s).unwrap());
    fs.write_atomic(
        &spool.hold_path(&s),
        MAGIC,
        file_system::Perms::Exact(Mode::PRIVATE_FILE),
    )
    .unwrap();
    assert!(spool.is_empty(&s).unwrap());
    let batch = spool.take(&s).unwrap().unwrap();
    assert!(batch.entries().is_empty());
    assert_eq!(batch.cut(0).unwrap(), Settled::Emptied);
}

#[test]
fn taking_a_session_that_holds_nothing_takes_nothing() {
    let (_fs, spool) = spool();
    let batch = spool.take(&root(9)).unwrap().unwrap();
    assert!(batch.entries().is_empty());
    assert_eq!(batch.cut(0).unwrap(), Settled::Emptied);
}

#[test]
fn the_sessions_are_the_holds_roots_sorted() {
    let (fs, spool) = spool();
    assert!(spool.sessions().unwrap().is_empty(), "no directory yet");
    hold_all(&spool, &root(3), &["a"]);
    hold_all(&spool, &root(1), &["a"]);
    fs.open(
        &spool.dir().join("not-a-root.held"),
        Open::Create(Mode::PRIVATE_FILE),
    )
    .unwrap();
    fs.open(
        &spool.dir().join(format!("{}.held.corrupt-1", root(5))),
        Open::Create(Mode::PRIVATE_FILE),
    )
    .unwrap();
    assert_eq!(spool.sessions().unwrap(), [root(1), root(3)]);
}

// --- modes ---------------------------------------------------------------

fn mode(fs: &Rooted, path: &Path) -> Option<u32> {
    fs.metadata(path).unwrap().mode.map(|m| m.bits())
}

#[test]
fn the_directory_is_0700_and_each_hold_0600() {
    let (fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a"]);
    if let Some(dir) = mode(&fs, spool.dir()) {
        assert_eq!(dir, 0o700);
        assert_eq!(mode(&fs, &spool.hold_path(&s)), Some(0o600));
        let batch = spool.take(&s).unwrap().unwrap();
        hold_all(&spool, &s, &["b"]);
        batch.cut(1).unwrap();
        assert_eq!(mode(&fs, &spool.hold_path(&s)), Some(0o600), "rewritten");
    }
}

#[test]
fn an_open_hold_is_tightened_at_every_use() {
    let (fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a"]);
    if mode(&fs, spool.dir()).is_none() {
        return; // Windows: the profile's ACL holds them (M8).
    }
    fs.set_permissions(spool.dir(), Mode::DIR).unwrap();
    fs.set_permissions(&spool.hold_path(&s), Mode::FILE)
        .unwrap();
    hold_all(&spool, &s, &["b"]);
    assert_eq!(mode(&fs, spool.dir()), Some(0o700));
    assert_eq!(mode(&fs, &spool.hold_path(&s)), Some(0o600));
    fs.set_permissions(spool.dir(), Mode::DIR).unwrap();
    fs.set_permissions(&spool.hold_path(&s), Mode::FILE)
        .unwrap();
    spool.take(&s).unwrap().unwrap();
    assert_eq!(mode(&fs, spool.dir()), Some(0o700));
    assert_eq!(mode(&fs, &spool.hold_path(&s)), Some(0o600));
}

// --- the bound and its gap -----------------------------------------------

fn framed(id: &str) -> u64 {
    frame::frame(&HeldEntry::Fact(fact(id))).len() as u64
}

#[test]
fn past_the_sessions_bound_the_newest_is_dropped_and_a_gap_kept() {
    let (_fs, spool) = spool();
    let one = framed("f0");
    let spool = spool.bounded(MAGIC.len() as u64 + 3 * one, u64::MAX);
    let s = root(1);
    hold_all(&spool, &s, &["f0", "f1", "f2"]);
    assert_eq!(
        spool.hold(&s, fact("f3")).unwrap(),
        Held::Full {
            bound: Bound::Session(MAGIC.len() as u64 + 3 * one),
            gap: true
        }
    );
    assert_eq!(held(&spool, &s), ["f0", "f1", "f2", "gap:f3:bound"]);
}

#[test]
fn a_fact_larger_than_the_bound_is_never_held() {
    let (_fs, spool) = spool();
    let spool = spool.bounded(framed("x"), u64::MAX);
    let s = root(1);
    assert!(matches!(
        spool.hold(&s, fact("a-long-external-id")).unwrap(),
        Held::Full { gap: true, .. }
    ));
    assert_eq!(held(&spool, &s), ["gap:a-long-external-id:bound"]);
}

#[test]
fn gaps_pass_the_bound_by_their_slack_and_no_more() {
    let (_fs, spool) = spool();
    let spool = spool.bounded(MAGIC.len() as u64, u64::MAX);
    let s = root(1);
    let mut gaps = 0;
    loop {
        match spool.hold(&s, fact("f")).unwrap() {
            Held::Full { gap: true, .. } => gaps += 1,
            Held::Full { gap: false, .. } => break,
            Held::Kept => panic!("nothing fits"),
        }
        assert!(gaps < 10_000, "the slack bounds the gaps");
    }
    let size = spool.take(&s).unwrap().unwrap().entries().len();
    assert_eq!(size, gaps);
    assert!(gaps > 100, "64 KiB of gaps: {gaps}");
    let bytes: u64 = MAGIC.len() as u64 + gaps as u64 * frame::frame(&gap_entry()).len() as u64;
    assert!(
        bytes <= MAGIC.len() as u64 + run_ports::GAP_SLACK,
        "{bytes}"
    );
}

fn gap_entry() -> HeldEntry {
    HeldEntry::Gap(run_ports::HeldGap {
        stamp: "v0.2.0-abcdef0".to_string(),
        external_id: "f".to_string(),
        held_at: None,
        why: "bound".to_string(),
    })
}

#[test]
fn past_the_homes_bound_every_session_drops_its_newest() {
    let (_fs, spool) = spool();
    let one = framed("f0");
    let file = MAGIC.len() as u64 + one;
    let spool = spool.bounded(u64::MAX / 4, 2 * file);
    hold_all(&spool, &root(1), &["f0"]);
    hold_all(&spool, &root(2), &["f0"]);
    assert_eq!(
        spool.hold(&root(3), fact("f0")).unwrap(),
        Held::Full {
            bound: Bound::Home(2 * file),
            gap: true
        }
    );
    assert_eq!(
        spool.hold(&root(1), fact("f1")).unwrap(),
        Held::Full {
            bound: Bound::Home(2 * file),
            gap: true
        }
    );
    assert_eq!(held(&spool, &root(1)), ["f0", "gap:f1:bound"]);
    assert_eq!(held(&spool, &root(2)), ["f0"]);
    assert_eq!(held(&spool, &root(3)), ["gap:f0:bound"]);
}

#[test]
fn the_rulings_bounds_are_the_default() {
    let (_fs, spool) = spool();
    assert_eq!(spool.session_bound, run_ports::SESSION_BOUND);
    assert_eq!(spool.home_bound, run_ports::HOME_BOUND);
}

// --- a torn tail and corruption ------------------------------------------

fn append_raw(fs: &Rooted, path: &Path, bytes: &[u8]) {
    use std::io::Write;
    let mut file = fs.open(path, Open::Append(Mode::PRIVATE_FILE)).unwrap();
    file.write_all(bytes).unwrap();
}

fn names(fs: &Rooted, dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs
        .read_dir(dir)
        .unwrap()
        .into_iter()
        .map(|e| e.name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// (f) A torn append: the valid records are taken, the torn tail cut, and
// nothing is set aside.
#[test]
fn a_torn_tail_is_cut_and_nothing_set_aside() {
    let (fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a", "b"]);
    let whole = frame::frame(&HeldEntry::Fact(fact("c")));
    append_raw(&fs, &spool.hold_path(&s), &whole[..whole.len() / 2]);
    assert_eq!(held(&spool, &s), ["a", "b"]);
    let bytes = fs.read(&spool.hold_path(&s)).unwrap();
    assert_eq!(
        frame::scan(&bytes, u64::MAX).unwrap().tail,
        Tail::Clean,
        "cut"
    );
    assert!(!names(&fs, spool.dir())
        .iter()
        .any(|n| n.contains("corrupt")));
}

#[test]
fn an_append_after_a_torn_tail_starts_clean() {
    let (fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a"]);
    append_raw(&fs, &spool.hold_path(&s), &[9, 0, 0]);
    hold_all(&spool, &s, &["b"]);
    assert_eq!(held(&spool, &s), ["a", "b"]);
}

#[test]
fn a_hold_whose_creation_tore_is_empty_and_held_anew() {
    let (fs, spool) = spool();
    let s = root(1);
    fs.create_dir_all(spool.dir(), super::PRIVATE_DIR).unwrap();
    append_raw(&fs, &spool.hold_path(&s), &MAGIC[..3]);
    hold_all(&spool, &s, &["a"]);
    assert_eq!(held(&spool, &s), ["a"]);
}

fn corrupt_after(fs: &Rooted, spool: &Spool, s: &Uuid, good: &[&str]) -> Vec<u8> {
    hold_all(spool, s, good);
    let mut bad = frame::frame(&HeldEntry::Fact(fact("bad")));
    let last = bad.len() - 1;
    bad[last] ^= 0xff;
    bad.extend(frame::frame(&HeldEntry::Fact(fact("after"))));
    append_raw(fs, &spool.hold_path(s), &bad);
    bad
}

#[test]
fn a_corrupt_hold_gives_its_valid_prefix_and_sets_the_rest_aside() {
    let (fs, spool) = spool();
    let s = root(1);
    let rest = corrupt_after(&fs, &spool, &s, &["a", "b"]);
    assert_eq!(held(&spool, &s), ["a", "b"]);
    let aside = spool.dir().join(format!("{s}.held.corrupt-1"));
    assert_eq!(fs.read(&aside).unwrap(), rest, "the rest, kept as it was");
    if let Some(bits) = mode(&fs, &aside) {
        assert_eq!(bits, 0o600);
    }
    // Later appends start a clean hold behind the prefix.
    hold_all(&spool, &s, &["c"]);
    assert_eq!(held(&spool, &s), ["a", "b", "c"]);
    // A second corruption is set aside beside the first.
    corrupt_after(&fs, &spool, &s, &[]);
    assert_eq!(held(&spool, &s), ["a", "b", "c"]);
    assert!(fs
        .exists(&spool.dir().join(format!("{s}.held.corrupt-2")))
        .unwrap());
}

#[test]
fn an_append_to_a_corrupt_hold_sets_the_rest_aside_first() {
    let (fs, spool) = spool();
    let s = root(1);
    corrupt_after(&fs, &spool, &s, &["a"]);
    hold_all(&spool, &s, &["b"]);
    assert_eq!(held(&spool, &s), ["a", "b"]);
    assert!(fs
        .exists(&spool.dir().join(format!("{s}.held.corrupt-1")))
        .unwrap());
}

#[test]
fn a_hold_of_another_format_is_never_touched() {
    let (fs, spool) = spool();
    let s = root(1);
    fs.create_dir_all(spool.dir(), super::PRIVATE_DIR).unwrap();
    let mut future = MAGIC.to_vec();
    future[7] = 2;
    future.extend_from_slice(b"whatever a later ctxloom writes");
    append_raw(&fs, &spool.hold_path(&s), &future);
    let refused = |e: HoldError| assert!(matches!(e, HoldError::Format { .. }), "{e:?}");
    refused(spool.hold(&s, fact("a")).unwrap_err());
    refused(spool.take(&s).unwrap_err());
    refused(spool.is_empty(&s).unwrap_err());
    assert_eq!(fs.read(&spool.hold_path(&s)).unwrap(), future);
    assert_eq!(
        names(&fs, spool.dir())
            .iter()
            .filter(|n| n.contains("corrupt"))
            .count(),
        0
    );
}

// --- discard and survey ----------------------------------------------------

#[test]
fn a_discard_removes_the_hold_and_its_set_asides_and_counts_its_facts() {
    let (fs, spool) = spool();
    let s = root(1);
    corrupt_after(&fs, &spool, &s, &["a", "b"]);
    spool.take(&s).unwrap().unwrap();
    hold_all(&spool, &s, &["c"]);
    hold_all(&spool, &root(2), &["other"]);
    assert_eq!(spool.discard(&s).unwrap(), 3);
    let left = names(&fs, spool.dir());
    assert!(
        !left
            .iter()
            .any(|n| n.starts_with(&s.to_string()) && n.contains("held")),
        "{left:?}"
    );
    assert_eq!(
        held(&spool, &root(2)),
        ["other"],
        "another session's is kept"
    );
    assert_eq!(spool.discard(&s).unwrap(), 0, "nothing left");
}

#[test]
fn a_survey_says_what_each_hold_holds() {
    use daemon_proto::google::protobuf::Timestamp;
    let (fs, spool) = spool();
    // Room for two facts with their times, not for a third.
    let one = framed("f0");
    let spool = spool.bounded(MAGIC.len() as u64 + 2 * one + 20, u64::MAX);
    let s = root(1);
    let at = |seconds| Some(Timestamp { seconds, nanos: 0 });
    for (n, (stamp, seconds)) in [("v0.3.0-abcdef0", 20), ("v0.2.0-abcdef0", 10)]
        .into_iter()
        .enumerate()
    {
        let f = HeldFact {
            stamp: stamp.to_string(),
            held_at: at(seconds),
            ..fact(&format!("f{n}"))
        };
        spool.hold(&s, f).unwrap();
    }
    spool.hold(&s, fact("f2")).unwrap(); // past the bound: a gap
    let other = root(2);
    corrupt_after(
        &fs,
        &spool.clone().bounded(u64::MAX / 4, u64::MAX),
        &other,
        &["a"],
    );
    let survey = spool.survey().unwrap();
    assert_eq!(survey.len(), 2);
    let first = &survey[0];
    assert_eq!(first.root, s);
    assert_eq!(
        first.session.as_deref(),
        Some("projects/p/sessions/brisk-copper-moth")
    );
    assert_eq!((first.facts, first.gaps), (2, 1));
    assert_eq!(first.stamps, ["v0.2.0-abcdef0", "v0.3.0-abcdef0"]);
    assert_eq!(first.oldest, at(10));
    assert_eq!(first.bytes, fs.metadata(&spool.hold_path(&s)).unwrap().len);
    assert!(!first.bad_tail && !first.unreadable && first.set_aside == 0);
    let second = &survey[1];
    assert!(second.bad_tail, "read, not repaired");
    spool.take(&other).unwrap().unwrap();
    let second = spool.survey().unwrap().remove(1);
    assert_eq!(
        (second.facts, second.set_aside, second.bad_tail),
        (1, 1, false)
    );
}

// --- the races, forced -----------------------------------------------------

// (a) Two appenders at once, each on its own handle (flock excludes
// across handles): every record is kept, each appender's in its own order.
#[test]
fn race_two_appenders_keep_every_record_each_in_its_order() {
    let (_fs, spool) = spool();
    let spool = spool.waiting(file_lock::LOCK_WAIT);
    let s = root(1);
    let appender = |name: &'static str| {
        let spool = spool.clone();
        thread::spawn(move || {
            for n in 0..1000 {
                assert_eq!(
                    spool.hold(&s, fact(&format!("{name}{n}"))).unwrap(),
                    Held::Kept
                );
            }
        })
    };
    let (a, b) = (appender("a"), appender("b"));
    a.join().unwrap();
    b.join().unwrap();
    let all = held(&spool, &s);
    assert_eq!(all.len(), 2000);
    for name in ["a", "b"] {
        let mine: Vec<&String> = all.iter().filter(|id| id.starts_with(name)).collect();
        let want: Vec<String> = (0..1000).map(|n| format!("{name}{n}")).collect();
        assert_eq!(mine, want.iter().collect::<Vec<_>>(), "{name}'s order");
    }
}

/// A pause that says, on its first call, that the lock was found held, and
/// then waits to be told to try again.
fn signalling() -> (
    impl Fn(&Path) -> Pause + Send + Sync,
    mpsc::Receiver<()>,
    mpsc::Sender<()>,
) {
    let (blocked_tx, blocked_rx) = mpsc::channel::<()>();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let go_rx = Arc::new(std::sync::Mutex::new(go_rx));
    let pauses = move |_: &Path| -> Pause {
        let blocked = blocked_tx.clone();
        let go = go_rx.clone();
        Box::new(move || {
            blocked.send(()).ok();
            go.lock().unwrap().recv().ok();
            Ok(())
        })
    };
    (pauses, blocked_rx, go_tx)
}

// (b) An append that finds the append lock held waits through its pause,
// appends nothing meanwhile, and appends once the lock is let go.
#[test]
fn race_an_append_waits_out_a_held_append_lock() {
    let (fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a"]);
    let (pauses, blocked, go) = signalling();
    let spool = spool.pausing(pauses);
    let held_lock = file_lock::lock(&*fs, &spool.append_lock_path(&s), &mut || Ok(())).unwrap();
    let appending = {
        let spool = spool.clone();
        thread::spawn(move || spool.hold(&s, fact("b")))
    };
    blocked.recv().unwrap();
    let before = fs.read(&spool.hold_path(&s)).unwrap();
    assert_eq!(
        frame::scan(&before, u64::MAX).unwrap().entries.len(),
        1,
        "nothing yet"
    );
    drop(held_lock);
    go.send(()).unwrap();
    assert_eq!(appending.join().unwrap().unwrap(), Held::Kept);
    assert_eq!(held(&spool, &s), ["a", "b"]);
}

// An append whose pause gives up fails, said, and appends nothing.
#[test]
fn race_an_append_that_cannot_have_the_lock_fails_and_appends_nothing() {
    let (fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a"]);
    let spool = spool.pausing(|path| {
        let path = path.display().to_string();
        Box::new(move || {
            Err(file_lock::LockError::Timeout {
                path: path.clone(),
                waited_ms: 1000,
            })
        })
    });
    let _held = file_lock::lock(&*fs, &spool.append_lock_path(&s), &mut || Ok(())).unwrap();
    assert!(matches!(
        spool.hold(&s, fact("b")).unwrap_err(),
        HoldError::Lock { .. }
    ));
    assert!(matches!(
        spool.is_empty(&s).unwrap_err(),
        HoldError::Lock { .. }
    ));
    drop(_held);
    assert_eq!(held(&spool, &s), ["a"]);
}

// (c) An append between a take and its cut is kept, after what was kept of
// the snapshot: nothing lost, nothing of the delivered prefix again.
#[test]
fn race_an_append_during_a_flush_is_kept_and_the_prefix_not_redelivered() {
    let (_fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a", "b", "c"]);
    let batch = spool.take(&s).unwrap().unwrap();
    hold_all(&spool, &s, &["d"]);
    assert_eq!(batch.cut(2).unwrap(), Settled::Cut { kept: 1 });
    assert_eq!(held(&spool, &s), ["c", "d"]);
    let batch = spool.take(&s).unwrap().unwrap();
    hold_all(&spool, &s, &["e"]);
    batch.cut(2).unwrap();
    assert_eq!(held(&spool, &s), ["e"]);
}

// (d) Of several processes taking one hold at once exactly one takes it;
// every other skips, at once, and changes nothing.
#[test]
fn race_one_taker_takes_a_hold_and_every_other_skips() {
    let (fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a", "b"]);
    let first = spool.take(&s).unwrap().expect("the first taker");
    let others: Vec<_> = (0..4)
        .map(|_| {
            let spool = spool.clone();
            thread::spawn(move || spool.take(&s).unwrap().is_none())
        })
        .collect();
    for other in others {
        assert!(other.join().unwrap(), "skipped");
    }
    let before = fs.read(&spool.hold_path(&s)).unwrap();
    assert_eq!(first.cut(1).unwrap(), Settled::Cut { kept: 1 });
    let after = spool.take(&s).unwrap().expect("free once settled");
    assert_eq!(ids(after.entries()), ["b"], "cut once");
    assert_ne!(before, fs.read(&spool.hold_path(&s)).unwrap());
}

// (d') A flush lock held by another process (here, by the test itself on
// its own handle) is never waited for: the take skips.
#[test]
fn race_a_held_flush_lock_is_skipped_not_waited_for() {
    let (fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a"]);
    let other = file_lock::lock(&*fs, &spool.flush_lock_path(&s), &mut || Ok(())).unwrap();
    let spool = spool.pausing(|_| Box::new(|| panic!("the flush lock is never waited for")));
    assert!(spool.take(&s).unwrap().is_none());
    drop(other);
    assert_eq!(held(&spool, &s), ["a"]);
}

// (e) A taker that delivered and crashed before its cut (its batch dropped)
// leaves the hold whole: the next taker delivers the same records again
// (each a DUPLICATE by its external id), and cuts them once.
#[test]
fn race_a_crash_between_delivering_and_cutting_leaves_the_prefix() {
    let (_fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a", "b"]);
    let crashed = spool.take(&s).unwrap().unwrap();
    let delivered = ids(crashed.entries());
    drop(crashed);
    let again = spool
        .take(&s)
        .unwrap()
        .expect("the lock went with the crash");
    assert_eq!(ids(again.entries()), delivered);
    assert_eq!(again.cut(2).unwrap(), Settled::Emptied);
    assert!(spool.is_empty(&s).unwrap());
}

// (g) A discard between a take and its cut: the cut finds another hold
// (held anew since) and leaves it as it is.
#[test]
fn race_a_discard_during_a_flush_leaves_the_new_hold_alone() {
    let (_fs, spool) = spool();
    let s = root(1);
    hold_all(&spool, &s, &["a", "b"]);
    let batch = spool.take(&s).unwrap().unwrap();
    assert_eq!(spool.discard(&s).unwrap(), 2);
    hold_all(&spool, &s, &["c"]);
    assert_eq!(batch.cut(2).unwrap(), Settled::Replaced);
    assert_eq!(held(&spool, &s), ["c"]);
    // And with nothing held anew, the cut finds nothing to cut.
    let batch = spool.take(&s).unwrap().unwrap();
    spool.discard(&s).unwrap();
    assert_eq!(batch.cut(1).unwrap(), Settled::Replaced);
    assert!(spool.is_empty(&s).unwrap());
}

// (g') Appenders racing discards on real locks: whatever is left is a
// whole hold, never a torn or corrupt one.
#[test]
fn race_appenders_and_discards_leave_a_whole_hold() {
    let (fs, spool) = spool();
    let spool = spool.waiting(file_lock::LOCK_WAIT);
    let s = root(1);
    let appenders: Vec<_> = (0..2)
        .map(|t| {
            let spool = spool.clone();
            thread::spawn(move || {
                for n in 0..300 {
                    spool.hold(&s, fact(&format!("t{t}-{n}"))).unwrap();
                }
            })
        })
        .collect();
    for _ in 0..50 {
        spool.discard(&s).unwrap();
    }
    for appender in appenders {
        appender.join().unwrap();
    }
    if let Ok(bytes) = fs.read(&spool.hold_path(&s)) {
        assert_eq!(frame::scan(&bytes, u64::MAX).unwrap().tail, Tail::Clean);
    }
    assert!(!names(&fs, spool.dir())
        .iter()
        .any(|n| n.contains("corrupt")));
}
