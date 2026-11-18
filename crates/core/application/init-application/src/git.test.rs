//! The git step: a directory in no work tree is made a repository, one in a
//! work tree is left alone, and git's failure is the observer's.

use std::collections::BTreeMap;
use std::sync::Mutex;

use angzarr_diagnostic::{Arg, Diagnostic};
use ctxloom_ports::{IgnoreRule, Motion, StatusEntry, TreeChange, TreeError, TreeObserver};

use super::{diagnostic_samples, ensure_repository, GitInitialized, EN_US};

/// An observer that answers `top` for every work tree (or fails, `missing`),
/// and records each repository it is asked to make.
#[derive(Default)]
pub(crate) struct Fake {
    pub(crate) top: Option<String>,
    pub(crate) missing: bool,
    pub(crate) made: Mutex<Vec<(String, Option<String>)>>,
    pub(crate) asked: Mutex<Vec<String>>,
}

#[derive(Debug)]
pub(crate) struct Missing;

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("library-git-git-missing")
    }
}

impl Diagnostic for Missing {
    fn code(&self) -> &str {
        "library-git-git-missing"
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        Vec::new()
    }
}

impl TreeObserver for Fake {
    fn work_tree(&self, dir: &str) -> Result<Option<String>, TreeError> {
        self.asked.lock().unwrap().push(dir.to_string());
        if self.missing {
            return Err(TreeError::GitMissing(Box::new(Missing)));
        }
        Ok(self.top.clone())
    }

    fn check_ignore(&self, _: &str, _: &str) -> Result<Option<IgnoreRule>, TreeError> {
        unreachable!("not asked")
    }

    fn init_repository(&self, dir: &str, ignore: Option<&str>) -> Result<(), TreeError> {
        self.made
            .lock()
            .unwrap()
            .push((dir.to_string(), ignore.map(str::to_string)));
        Ok(())
    }

    fn head(&self, _: &str) -> Result<Option<String>, TreeError> {
        unreachable!("not asked")
    }

    fn library_tree(&self, _: &str, _: &str) -> Result<Option<String>, TreeError> {
        unreachable!("not asked")
    }

    fn status(&self, _: &str, _: &[&str]) -> Result<Vec<StatusEntry>, TreeError> {
        unreachable!("not asked")
    }

    fn diff_tree(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &[&str],
    ) -> Result<Vec<TreeChange>, TreeError> {
        unreachable!("not asked")
    }

    fn blob_ids(&self, _: &str, _: &[&str]) -> Result<BTreeMap<String, String>, TreeError> {
        unreachable!("not asked")
    }

    fn blob_id_of(&self, _: &str, _: &str, _: &[u8]) -> Result<String, TreeError> {
        unreachable!("not asked")
    }

    fn in_motion(&self, _: &str) -> Result<Option<Motion>, TreeError> {
        unreachable!("not asked")
    }

    fn commit(&self, _: &str, _: &str, _: &[&str]) -> Result<(), TreeError> {
        unreachable!("not asked")
    }
}

#[test]
fn a_directory_in_no_work_tree_is_made_a_repository_with_no_ignore_file_of_ctxlooms() {
    let fake = Fake::default();
    assert_eq!(
        ensure_repository(&fake, "/work/p").unwrap(),
        Some(GitInitialized)
    );
    assert_eq!(
        *fake.made.lock().unwrap(),
        vec![("/work/p".to_string(), None)]
    );
}

#[test]
fn a_directory_in_a_work_tree_is_left_as_it_is() {
    let fake = Fake {
        top: Some("/work".to_string()),
        ..Fake::default()
    };
    assert_eq!(ensure_repository(&fake, "/work/p").unwrap(), None);
    assert!(fake.made.lock().unwrap().is_empty());
}

#[test]
fn git_missing_is_the_observers_error_and_nothing_is_made() {
    let fake = Fake {
        missing: true,
        ..Fake::default()
    };
    let error = ensure_repository(&fake, "/work/p").unwrap_err();
    assert_eq!(error.diagnostic().code(), "library-git-git-missing");
    assert!(fake.made.lock().unwrap().is_empty());
}

#[test]
fn the_notice_has_its_message() {
    let samples = diagnostic_samples();
    assert_eq!(samples.len(), 1);
    for sample in samples {
        let id = format!("{} =", sample.code());
        assert!(
            EN_US.1.lines().any(|l| l.starts_with(&id)),
            "{} has no message",
            sample.code()
        );
        assert!(sample.args().is_empty());
        assert_eq!(format!("{sample}"), sample.code());
    }
    assert_eq!(EN_US.0, "init-application");
    assert_eq!(
        GitInitialized.code(),
        "init-application-notice-git-initialized"
    );
}
