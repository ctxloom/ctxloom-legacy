use std::collections::BTreeMap;

use angzarr_diagnostic::Diagnostic;

use crate::{diagnostic_samples, motion_word, LibraryTreeRejection, Motion, EN_US, LIBRARY_DOMAIN};

#[test]
fn the_domain_name_is_library() {
    assert_eq!(LIBRARY_DOMAIN, "library");
}

#[test]
fn every_diagnostic_has_a_message_and_its_code_is_prefixed() {
    let (name, catalog) = EN_US;
    assert_eq!(name, "library-tree");
    for sample in diagnostic_samples() {
        let code = sample.code();
        assert!(code.starts_with("library-tree-"), "{code} is prefixed");
        assert!(
            catalog.contains(&format!("{code} =")),
            "{code} has no message"
        );
        for (arg, _) in sample.args() {
            assert!(catalog.contains(&format!("${arg}")), "{code} uses {arg}");
        }
    }
    assert_eq!(diagnostic_samples().len(), 10);
}

#[test]
fn a_plans_rejection_speaks_as_the_plans_own_diagnostic() {
    let rejected = LibraryTreeRejection::PlanRejected {
        code: "library-application-write-unknown-kind".to_string(),
        args: BTreeMap::from([
            ("kind".to_string(), "widget".to_string()),
            ("bundle".to_string(), "demo".to_string()),
        ]),
    };
    assert_eq!(rejected.code(), "library-application-write-unknown-kind");
    let args: Vec<String> = rejected
        .args()
        .into_iter()
        .map(|(name, value)| format!("{name}={value:?}"))
        .collect();
    assert_eq!(args.len(), 2);
    assert!(args[0].starts_with("bundle="), "{args:?}");
    assert!(args[1].starts_with("kind="), "{args:?}");
}

#[test]
fn each_rejection_names_its_args() {
    let names = |r: LibraryTreeRejection| -> Vec<String> {
        r.args().into_iter().map(|(n, _)| n.to_string()).collect()
    };
    let path = || "p".to_string();
    assert_eq!(
        names(LibraryTreeRejection::PlanStale {
            path: path(),
            seen: 1,
            now: 2
        }),
        ["path", "seen", "now"]
    );
    assert_eq!(
        names(LibraryTreeRejection::ResourceExists { path: path() }),
        ["path"]
    );
    assert_eq!(
        names(LibraryTreeRejection::NoSuchResource { path: path() }),
        ["path"]
    );
    assert_eq!(
        names(LibraryTreeRejection::BuiltinReadOnly { bundle: path() }),
        ["bundle"]
    );
    assert_eq!(
        names(LibraryTreeRejection::TreeInMotion {
            motion: Motion::Merging,
            root: path()
        }),
        ["motion", "root"]
    );
    assert_eq!(
        names(LibraryTreeRejection::NotInGit { root: path() }),
        ["root"]
    );
    assert_eq!(
        names(LibraryTreeRejection::LibraryIgnored {
            source: path(),
            line: 1,
            pattern: path()
        }),
        ["source", "line", "pattern"]
    );
}

#[test]
fn rejections_display_their_code() {
    assert_eq!(
        LibraryTreeRejection::NotInGit {
            root: "/w".to_string()
        }
        .code(),
        "library-tree-not-in-git"
    );
    assert!(LibraryTreeRejection::NotInGit {
        root: "/w".to_string()
    }
    .to_string()
    .starts_with("library-tree-not-in-git"));
}

#[test]
fn each_motion_is_its_catalogs_selector() {
    let (_, catalog) = EN_US;
    for (motion, word) in [
        (Motion::IndexLocked, "index-locked"),
        (Motion::Merging, "merging"),
        (Motion::Rebasing, "rebasing"),
        (Motion::CherryPicking, "cherry-picking"),
        (Motion::Reverting, "reverting"),
    ] {
        assert_eq!(motion_word(motion), word);
        assert!(catalog.contains(&format!("[{word}]")), "{word}");
    }
}
