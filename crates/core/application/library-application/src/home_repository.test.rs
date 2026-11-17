//! The home's repository: made where there is none, or where a repository
//! ignores it; used as it is otherwise.

use std::collections::BTreeMap;
use std::sync::Mutex;

use ctxloom_ports::{IgnoreRule, Motion, StatusEntry, TreeChange, TreeError, TreeObserver};

use super::{ensure_home_repository, same_dir, HomeRepository, HOME_GITIGNORE};

/// An observer whose work tree is `top` and whose ignore rule is `ignored`,
/// recording what it is asked.
#[derive(Default)]
struct Fake {
    top: Option<String>,
    ignored: bool,
    asked_ignore: Mutex<Vec<(String, String)>>,
    made: Mutex<Vec<(String, Option<String>)>>,
}

impl TreeObserver for Fake {
    fn work_tree(&self, _: &str) -> Result<Option<String>, TreeError> {
        Ok(self.top.clone())
    }

    fn check_ignore(&self, dir: &str, path: &str) -> Result<Option<IgnoreRule>, TreeError> {
        self.asked_ignore
            .lock()
            .unwrap()
            .push((dir.to_string(), path.to_string()));
        Ok(self.ignored.then(|| IgnoreRule {
            source: ".gitignore".to_string(),
            line: 1,
            pattern: ".ctxloom/".to_string(),
        }))
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

    fn in_motion(&self, _: &str) -> Result<Option<Motion>, TreeError> {
        unreachable!("not asked")
    }
}

const HOME: &str = "/home/alice/.ctxloom";

fn made_with_allowlist(fake: &Fake) -> bool {
    *fake.made.lock().unwrap() == vec![(HOME.to_string(), Some(HOME_GITIGNORE.to_string()))]
}

// G2(a): a home in no work tree is made a repository, its allowlist first.
#[test]
fn a_home_in_no_work_tree_is_made_a_repository_with_the_allowlist() {
    let fake = Fake::default();
    assert_eq!(
        ensure_home_repository(&fake, HOME).unwrap(),
        HomeRepository::Made
    );
    assert!(made_with_allowlist(&fake));
    assert!(fake.asked_ignore.lock().unwrap().is_empty());
}

// G2: a home that is a repository already is never made again.
#[test]
fn a_home_that_is_a_repository_of_its_own_is_used_as_it_is() {
    for top in [HOME, "/home/alice/.ctxloom/", "\\home\\alice\\.ctxloom"] {
        let fake = Fake {
            top: Some(top.to_string()),
            ignored: true,
            ..Fake::default()
        };
        assert_eq!(
            ensure_home_repository(&fake, HOME).unwrap(),
            HomeRepository::UsedAsItIs,
            "{top}"
        );
        assert!(fake.made.lock().unwrap().is_empty());
        assert!(fake.asked_ignore.lock().unwrap().is_empty(), "{top}");
    }
}

// G2(a): a dotfiles home is used as it is, its config judged by its rules.
#[test]
fn a_home_in_a_dotfiles_repository_that_keeps_it_is_used_as_it_is() {
    let fake = Fake {
        top: Some("/home/alice".to_string()),
        ..Fake::default()
    };
    assert_eq!(
        ensure_home_repository(&fake, HOME).unwrap(),
        HomeRepository::UsedAsItIs
    );
    assert_eq!(
        *fake.asked_ignore.lock().unwrap(),
        vec![(HOME.to_string(), "config.yaml".to_string())]
    );
    assert!(fake.made.lock().unwrap().is_empty());
}

// QUESTIONS.md C: a dotfiles repository that ignores the home's library
// gets a nested repository of the home's own.
#[test]
fn a_home_a_dotfiles_repository_ignores_is_made_a_repository_of_its_own() {
    let fake = Fake {
        top: Some("/home/alice".to_string()),
        ignored: true,
        ..Fake::default()
    };
    assert_eq!(
        ensure_home_repository(&fake, HOME).unwrap(),
        HomeRepository::Made
    );
    assert!(made_with_allowlist(&fake));
}

#[test]
fn the_allowlist_ignores_everything_but_ctxlooms_own_files() {
    assert_eq!(
        HOME_GITIGNORE,
        "# ctxloom: only what is listed here is ctxloom's own, and seen by git.\n/*\n!/.gitignore\n!/config.yaml\n!/remotes.yaml\n!/lock.yaml\n!/content/\n"
    );
    assert!(same_dir("/a/b", "/a/b/"));
    assert!(same_dir("C:\\a\\b", "C:/a/b"));
    assert!(!same_dir("/a/b", "/a/bc"));
}
