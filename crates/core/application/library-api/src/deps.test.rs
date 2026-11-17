//! The deps reads over deps-application's fakes.

use deps_application::fakes::Fixture;

use super::{list_dependencies, list_remotes};

const PARENT: &str = "projects/calm-otter";
const URL: &str = "file:///srv/origin.git";

#[test]
fn the_remotes_are_answered_under_the_parent_the_default_marked() {
    let fx = Fixture::new();
    fx.origin("origin", URL);
    let listed = list_remotes(&fx.deps, PARENT, &Default::default())
        .result
        .unwrap();
    assert_eq!(listed.remotes.len(), 1);
    let origin = &listed.remotes[0];
    assert_eq!(origin.name, "projects/calm-otter/remotes/origin");
    assert_eq!(origin.url, URL);
    assert_eq!(origin.default, Some(false));
}

#[test]
fn a_pulled_dependency_is_named_by_its_identity_below_its_remote() {
    let fx = Fixture::new();
    let sha = fx.origin("origin", URL);
    let reference = Fixture::reference(URL, "demo");
    fx.profile("dev", &[&reference]);
    fx.deps.pull(false, true).result.unwrap();
    let listed = list_dependencies(&fx.deps, PARENT, &Default::default())
        .result
        .unwrap();
    assert_eq!(listed.dependencies.len(), 1);
    let demo = &listed.dependencies[0];
    assert!(demo.name.starts_with("projects/calm-otter/dependencies/"));
    assert_eq!(demo.reference, reference);
    assert_eq!(demo.commit, sha);
    assert_eq!(demo.remote, "projects/calm-otter/remotes/origin");
    assert_eq!(
        demo.bundle,
        "projects/calm-otter/remotes/origin/bundles/demo"
    );
    assert_eq!(demo.held, Some(false));
}

#[test]
fn a_lock_that_does_not_read_is_declined() {
    let fx = Fixture::new();
    fx.files.set_lock("not: [a lock");
    let read = list_dependencies(&fx.deps, PARENT, &Default::default()).result;
    assert!(
        matches!(read, Err(deps_application::AppError::Declined(_))),
        "{read:?}"
    );
}
