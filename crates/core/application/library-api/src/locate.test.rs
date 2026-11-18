use ctxloom_ports::IgnoreRule;
use library_tree::LibraryTreeRejection;

use super::refusal;
use crate::fakes::Git;

#[test]
fn a_library_in_a_work_tree_git_does_not_ignore_is_read() {
    let git = Git::at("c1", "t1");
    assert_eq!(refusal(&git, "/work/demo").unwrap(), None);
    assert_eq!(
        git.calls(),
        vec!["work_tree", "check_ignore .ctxloom/config.yaml"]
    );
}

#[test]
fn a_library_in_no_work_tree_is_refused_naming_its_root_and_asks_nothing_more() {
    let git = Git::default();
    assert_eq!(
        refusal(&git, "/work/demo").unwrap(),
        Some(LibraryTreeRejection::NotInGit {
            root: "/work/demo".to_string()
        })
    );
    assert_eq!(git.calls(), vec!["work_tree"]);
}

#[test]
fn a_library_git_ignores_is_refused_naming_the_rule() {
    let mut git = Git::at("c1", "t1");
    git.ignored = Some(IgnoreRule {
        source: ".gitignore".to_string(),
        line: 3,
        pattern: ".ctxloom/*".to_string(),
    });
    assert_eq!(
        refusal(&git, "/work/demo").unwrap(),
        Some(LibraryTreeRejection::LibraryIgnored {
            source: ".gitignore".to_string(),
            line: 3,
            pattern: ".ctxloom/*".to_string(),
        })
    );
}
