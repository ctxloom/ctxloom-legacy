use std::sync::{Arc, Mutex};

use daemon_proto::v1;
use library_application::fakes::{Setup, ENVELOPE};
use library_application::Library;

use super::{bundle_of, flags_of, parent_of, Libraries};
use crate::server::tests::{fixture, observing, resolve, Gits, COMMIT, ROOT};
use crate::{ApiError, Calls};

#[test]
fn a_read_s_parent_is_its_project_or_the_home() {
    let (parent, project) = parent_of("projects/calm-otter/bundles/demo").unwrap();
    assert_eq!(parent, "projects/calm-otter");
    assert_eq!(project.unwrap().harp(), "calm-otter");
    let (parent, project) = parent_of("users/me").unwrap();
    assert_eq!(parent, "users/me");
    assert!(project.is_none());
    assert!(parent_of("bundles/demo").is_none());
    assert!(parent_of("projects/").is_none());
}

#[test]
fn a_listing_s_bundle_is_the_one_its_parent_names() {
    assert_eq!(bundle_of("projects/p/bundles/demo"), "demo");
    assert_eq!(
        bundle_of("projects/p/remotes/acme/bundles/rules"),
        "acme/rules"
    );
    assert_eq!(bundle_of("projects/p"), "");
    assert_eq!(bundle_of("users/me"), "");
}

#[test]
fn a_read_s_globals_are_its_flags_never_trusted() {
    let flags = flags_of(Some(&v1::Globals {
        degraded: true,
        no_companions: true,
        config_set: vec!["a=b".to_string()],
    }));
    assert!(flags.degraded && flags.no_companions && !flags.trusted);
    assert_eq!(flags.config_set, vec!["a=b".to_string()]);
    assert!(!flags_of(None).degraded);
}

const SKILLS: &[(&str, &str)] = &[
    ENVELOPE,
    (
        "skills/reviewer/SKILL.md",
        "---\nname: reviewer\ndescription: Reviews\n---\nBODY\n",
    ),
];

/// The library a test reads, and where each read was asked to open it.
#[derive(Default)]
struct Opened(Mutex<Vec<Asked>>);

/// A directory a read opened the library at, and its environment.
type Asked = (String, Vec<(String, String)>);

impl Libraries for Opened {
    fn root(&self, dir: &str, _: &[(String, String)]) -> Option<String> {
        // A worktree below the project is a checkout of its own (L2).
        Some(dir.to_string())
    }

    fn read(
        &self,
        dir: &str,
        env: &[(String, String)],
        read: &mut dyn FnMut(&Library),
    ) -> Result<(), ApiError> {
        self.0.lock().unwrap().push((dir.to_string(), env.to_vec()));
        let library = Setup {
            bundles: &[("demo", SKILLS)],
            layers: &["schema_version: 9\n"],
            ..Setup::default()
        }
        .library();
        read(&library);
        Ok(())
    }
}

fn path(value: &str) -> Option<v1::EnvVars> {
    Some(v1::EnvVars {
        vars: vec![v1::EnvVar {
            name: "PATH".to_string(),
            value: value.to_string(),
        }],
    })
}

// Step 10a U2 (E1): a library read is answered at the caller's directory,
// in its environment, after its tree is observed; what the observation
// said rides beside the answer (NOTICE_METADATA's notices).
#[tokio::test]
async fn a_library_read_is_observed_then_answered_at_the_callers_directory() {
    let f = fixture(vec!["calm-quiet-heron"]);
    f.api.resolve_project(resolve(COMMIT, "")).await.unwrap();
    f.catch_up().await;
    let gits = Arc::new(Gits {
        work_tree: Some(ROOT.to_string()),
        motion: Some(ctxloom_ports::Motion::Rebasing),
        ..Gits::default()
    });
    let opened = Arc::new(Opened::default());
    let api = f
        .restarted()
        .await
        .reading(Arc::clone(&opened) as Arc<dyn Libraries>)
        .observing(observing(&f.world, &gits));
    let read = api
        .list_skills(v1::ListSkillsRequest {
            parent: "projects/calm-quiet-heron".to_string(),
            cwd: "/work/calm-wt".to_string(),
            env: path("/opt/git/bin"),
            ..v1::ListSkillsRequest::default()
        })
        .await
        .unwrap();
    assert_eq!(
        read.answer.skills[0].name,
        "projects/calm-quiet-heron/bundles/demo/skills/reviewer"
    );
    assert_eq!(read.notices[0].code, "library-api-notice-tree-in-motion");
    let opened = opened.0.lock().unwrap();
    assert_eq!(opened[0].0, "/work/calm-wt");
    assert_eq!(
        opened[0].1,
        vec![("PATH".to_string(), "/opt/git/bin".to_string())]
    );
    assert_eq!(
        *gits.envs.lock().unwrap(),
        vec![vec![("PATH".to_string(), "/opt/git/bin".to_string())]]
    );
}

// With no directory of its own, a read is answered at the project's
// registered root.
#[tokio::test]
async fn a_read_with_no_directory_is_answered_at_the_registered_root() {
    let f = fixture(vec!["calm-quiet-heron"]);
    f.api.resolve_project(resolve(COMMIT, "")).await.unwrap();
    f.catch_up().await;
    let gits = Arc::new(Gits {
        work_tree: Some(ROOT.to_string()),
        ..Gits::default()
    });
    let opened = Arc::new(Opened::default());
    let api = f
        .restarted()
        .await
        .reading(Arc::clone(&opened) as Arc<dyn Libraries>)
        .observing(observing(&f.world, &gits));
    let read = api
        .list_agents(v1::ListAgentsRequest {
            parent: "projects/calm-quiet-heron".to_string(),
            ..v1::ListAgentsRequest::default()
        })
        .await
        .unwrap();
    assert!(read.answer.agents.is_empty());
    assert!(read.notices.is_empty(), "{:?}", read.notices);
    assert_eq!(opened.0.lock().unwrap()[0].0, ROOT);
}

// A library read outside git is refused by the library domain, and nothing
// is opened (G1).
#[tokio::test]
async fn a_library_read_outside_git_opens_nothing() {
    let f = fixture(vec!["calm-quiet-heron"]);
    f.api.resolve_project(resolve(COMMIT, "")).await.unwrap();
    f.catch_up().await;
    let gits = Arc::new(Gits::default());
    let opened = Arc::new(Opened::default());
    let api = f
        .restarted()
        .await
        .reading(Arc::clone(&opened) as Arc<dyn Libraries>)
        .observing(observing(&f.world, &gits));
    let refused = api
        .list_profiles(v1::ListProfilesRequest {
            parent: "projects/calm-quiet-heron".to_string(),
            ..v1::ListProfilesRequest::default()
        })
        .await;
    assert!(matches!(refused, Err(ApiError::Refused(_))), "{refused:?}");
    assert!(opened.0.lock().unwrap().is_empty());
}

// E5: a decline of the read's use case is its typed refusal.
#[tokio::test]
async fn a_read_a_name_does_not_shape_is_refused_name_invalid() {
    let f = fixture(vec!["calm-quiet-heron"]);
    let refused = f
        .api
        .reading(Arc::new(Opened::default()))
        .list_agents(v1::ListAgentsRequest {
            parent: "agents".to_string(),
            ..v1::ListAgentsRequest::default()
        })
        .await;
    let Err(ApiError::Refused(refusal)) = refused else {
        panic!("{refused:?}");
    };
    assert!(refusal.refusal.type_url.ends_with("NameInvalid"));
}
