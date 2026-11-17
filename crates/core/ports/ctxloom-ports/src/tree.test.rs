//! The tree port's data: a motion's word, and an error's diagnostic.

use angzarr_diagnostic::{Arg, Diagnostic};

use super::{Motion, TreeError};

#[derive(Debug)]
struct Said(&'static str);

impl std::fmt::Display for Said {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl Diagnostic for Said {
    fn code(&self) -> &str {
        self.0
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        Vec::new()
    }
}

#[test]
fn each_motion_has_the_word_its_message_selects_on() {
    let words: Vec<&str> = [
        Motion::IndexLocked,
        Motion::Merging,
        Motion::Rebasing,
        Motion::CherryPicking,
        Motion::Reverting,
    ]
    .into_iter()
    .map(Motion::word)
    .collect();
    assert_eq!(
        words,
        [
            "index-locked",
            "merging",
            "rebasing",
            "cherry-picking",
            "reverting"
        ]
    );
}

#[test]
fn every_error_is_its_adapters_diagnostic() {
    for (error, code) in [
        (
            TreeError::GitMissing(Box::new(Said("a-missing"))),
            "a-missing",
        ),
        (
            TreeError::GitTooOld(Box::new(Said("a-too-old"))),
            "a-too-old",
        ),
        (TreeError::Failed(Box::new(Said("a-failed"))), "a-failed"),
    ] {
        assert_eq!(error.diagnostic().code(), code);
        assert_eq!(error.into_failure().code(), code);
    }
}
