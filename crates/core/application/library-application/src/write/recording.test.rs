//! A use case over the recording library plans its write and writes
//! nothing; what it planned is the write the use case would have made.

use ctxloom_ports::{Change, Scope};

use super::Planned;
use crate::fakes::{plain, Setup, CONFIG, ENVELOPE};

fn world() -> (crate::Library, std::sync::Arc<crate::fakes::Mem>) {
    Setup {
        bundles: &[("demo", &[ENVELOPE, ("fragments/example.md", "EXAMPLE")])],
        layers: &[CONFIG],
        ..Setup::default()
    }
    .fixture()
}

#[test]
fn test_a_bundle_create_is_planned_as_one_new_directory_and_writes_nothing() {
    let (library, mem) = world();
    let (recording, recorder) = library.recording();
    let created = recording.bundle_create(&plain(), "notes", "my notes");
    assert!(created.result.is_ok(), "{:?}", created.result.err());
    assert_eq!(mem.writes(), 0, "{:?}", mem.log());
    assert!(!mem.has_bundle("notes"));
    let planned = recorder.take();
    let [Planned::NewBundle { bundle, files }] = &planned[..] else {
        panic!("{planned:?}")
    };
    assert_eq!(bundle, "notes");
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"bundle.yaml"), "{paths:?}");
}

#[test]
fn test_an_edit_of_one_file_is_planned_as_that_change() {
    let (library, mem) = world();
    let (recording, recorder) = library.recording();
    recording
        .item_create(&plain(), crate::ItemKind::Fragment, "demo", "rules")
        .result
        .unwrap();
    assert_eq!(mem.text("demo", "fragments/rules.md"), None);
    let planned = recorder.take();
    let [Planned::Changed { bundle, changes }] = &planned[..] else {
        panic!("{planned:?}")
    };
    assert_eq!(bundle, "demo");
    let [Change::Write(put)] = &changes[..] else {
        panic!("{changes:?}")
    };
    assert_eq!(put.path, "fragments/rules.md");
}

#[test]
fn test_a_claim_given_back_is_no_write() {
    let recorder = super::Recorder::default();
    recorder.claim("notes");
    recorder.apply("notes", &[]);
    recorder.remove_bundle("notes");
    assert!(recorder.take().is_empty());
    recorder.remove_bundle("demo");
    assert_eq!(
        recorder.take(),
        [Planned::BundleGone {
            bundle: "demo".to_string()
        }]
    );
}

#[test]
fn test_a_layer_written_twice_is_its_last_text_and_reads_back_planned() {
    let recorder = super::Recorder::default();
    recorder.layer(Scope::Home, "a: 1\n");
    recorder.layer(Scope::Home, "a: 2\n");
    assert_eq!(recorder.layer_text(Scope::Home).as_deref(), Some("a: 2\n"));
    assert_eq!(recorder.layer_text(Scope::Project), None);
    assert_eq!(recorder.take().len(), 1);
}

#[test]
fn test_a_bundles_files_are_listed_below_a_directory_of_it() {
    let (library, _) = world();
    assert_eq!(
        library.bundle_files("demo", ""),
        ["bundle.yaml", "fragments/example.md"]
    );
    assert_eq!(library.bundle_files("demo", "fragments/"), ["example.md"]);
    assert!(library.bundle_files("demo", "skills").is_empty());
    assert!(library.bundle_files("nobody", "").is_empty());
}

// Step 10b U9 (T28): a forced import over a bundle is planned as the old
// bundle gone, then a new one holding EVERY file imported (an unchanged
// file too), so the daemon makes it the sequence trash, create, purge.
#[test]
fn test_a_forced_import_over_a_bundle_is_planned_as_it_gone_then_a_new_one() {
    let (library, mem) = world();
    mem.put_user("in/demo/bundle.yaml", ENVELOPE.1.as_bytes(), false);
    mem.put_user("in/demo/fragments/example.md", b"EXAMPLE", false);
    mem.put_user("in/demo/fragments/new.md", b"NEW", false);
    let (recording, recorder) = library.recording();
    let imported = recording.bundle_import(&plain(), "in/demo", true);
    assert!(imported.result.is_ok(), "{:?}", imported.result.err());
    assert_eq!(mem.writes(), 0, "{:?}", mem.log());
    let planned = recorder.take();
    let [Planned::BundleGone { bundle: gone }, Planned::NewBundle { bundle, files }] = &planned[..]
    else {
        panic!("{planned:?}")
    };
    assert_eq!((gone.as_str(), bundle.as_str()), ("demo", "demo"));
    let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(
        paths,
        ["bundle.yaml", "fragments/example.md", "fragments/new.md"]
    );
}

// An import of a bundle the project has none of is one new directory.
#[test]
fn test_an_import_of_a_new_bundle_is_planned_as_one_new_directory() {
    let (library, mem) = world();
    mem.put_user("in/notes/bundle.yaml", ENVELOPE.1.as_bytes(), false);
    let (recording, recorder) = library.recording();
    let imported = recording.bundle_import(&plain(), "in/notes", true);
    assert!(imported.result.is_ok(), "{:?}", imported.result.err());
    let planned = recorder.take();
    let [Planned::NewBundle { bundle, .. }] = &planned[..] else {
        panic!("{planned:?}")
    };
    assert_eq!(bundle, "notes");
}
