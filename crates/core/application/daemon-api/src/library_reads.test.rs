use std::sync::{Arc, Mutex};

use daemon_proto::v1;
use library_application::fakes::{Setup, ENVELOPE};
use library_application::Library;

use super::{flags_of, parent_of, Libraries};
use crate::server::tests::{fixture, observing, resolve, Gits, COMMIT, ROOT};
use crate::{ApiError, Calls};
use run_ports::Edge as _;

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
    ("fragments/go.md", "GO-BODY\n"),
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

// Slice 3 step 6, step 10a U2: AssembleContext is a library read: made at
// the project's REGISTERED root when the caller gives no directory; a
// project nothing registered is not found; a name of no shape is invalid.
#[tokio::test]
async fn assemble_context_is_made_at_the_projects_registered_root() {
    let f = fixture(vec!["calm-quiet-heron"]);
    let opened = Arc::new(Opened::default());
    let none = f
        .restarted()
        .await
        .reading(Arc::clone(&opened) as Arc<dyn Libraries>)
        .assemble_context(v1::AssembleContextRequest {
            name: "projects/calm-quiet-heron".to_string(),
            ..v1::AssembleContextRequest::default()
        })
        .await;
    assert!(
        matches!(&none, Err(ApiError::Refused(r)) if r.refusal.type_url.ends_with("ProjectNotFound")),
        "{none:?}"
    );
    f.api.resolve_project(resolve(COMMIT, "")).await.unwrap();
    f.catch_up().await;
    let api = f
        .restarted()
        .await
        .reading(Arc::clone(&opened) as Arc<dyn Libraries>);
    let made = api
        .assemble_context(v1::AssembleContextRequest {
            name: "projects/calm-quiet-heron".to_string(),
            fragments: vec!["demo#fragments/go".to_string()],
            ..v1::AssembleContextRequest::default()
        })
        .await
        .unwrap();
    assert!(made.context.contains("GO-BODY"), "{made:?}");
    assert_eq!(
        made.fragments[0].fragment,
        "projects/calm-quiet-heron/bundles/demo/fragments/go"
    );
    assert_eq!(opened.0.lock().unwrap()[0].0, ROOT);
    let invalid = api
        .assemble_context(v1::AssembleContextRequest {
            name: "calm".to_string(),
            ..v1::AssembleContextRequest::default()
        })
        .await;
    assert!(
        matches!(&invalid, Err(ApiError::Refused(r)) if r.refusal.type_url.ends_with("NameInvalid")),
        "{invalid:?}"
    );
}

// Step 10a U2: AssembleContext observes the project's tree first, in the
// caller's environment, and records its baseline once; a git operation in
// progress is said in its answer's notices (the MCP tool's message).
#[tokio::test]
async fn assemble_context_observes_the_projects_library_first() {
    let f = fixture(vec!["calm-quiet-heron"]);
    f.api.resolve_project(resolve(COMMIT, "")).await.unwrap();
    f.catch_up().await;
    let gits = Arc::new(Gits {
        work_tree: Some(ROOT.to_string()),
        motion: Some(ctxloom_ports::Motion::Merging),
        ..Gits::default()
    });
    let api = f
        .restarted()
        .await
        .reading(Arc::new(Opened::default()))
        .observing(observing(&f.world, &gits));
    let request = v1::AssembleContextRequest {
        name: "projects/calm-quiet-heron".to_string(),
        env: path("/usr/bin"),
        ..v1::AssembleContextRequest::default()
    };
    let made = api.assemble_context(request.clone()).await.unwrap();
    assert_eq!(made.notices.len(), 1, "{:?}", made.notices);
    assert_eq!(made.notices[0].code, "library-api-notice-tree-in-motion");
    assert!(
        made.notices[0].text.contains("merging"),
        "{:?}",
        made.notices
    );
    api.assemble_context(request).await.unwrap();
    assert_eq!(
        *gits.envs.lock().unwrap(),
        vec![vec![("PATH".to_string(), "/usr/bin".to_string())]; 2]
    );
    let tree = library_tree::TreeName::checkout(
        sessions::ProjectName::new("calm-quiet-heron").unwrap(),
        "brisk-copper-moth",
    )
    .unwrap();
    let cover = angzarr_embedded::v1::Cover {
        domain: library_tree::LIBRARY_DOMAIN.to_string(),
        root: Some(tree.root()),
        edition: None,
    };
    assert_eq!(f.world.events(&cover, 0).await.unwrap().len(), 1);
}

// Step 10a U2 (G1): an assembly outside git is refused with the library
// domain's own refusal, before anything is opened.
#[tokio::test]
async fn assemble_context_outside_git_is_refused_before_it_assembles() {
    let f = fixture(vec!["calm-quiet-heron"]);
    f.api.resolve_project(resolve(COMMIT, "")).await.unwrap();
    f.catch_up().await;
    let opened = Arc::new(Opened::default());
    let api = f
        .restarted()
        .await
        .reading(Arc::clone(&opened) as Arc<dyn Libraries>)
        .observing(observing(&f.world, &Arc::new(Gits::default())));
    let refused = api
        .assemble_context(v1::AssembleContextRequest {
            name: "projects/calm-quiet-heron".to_string(),
            ..v1::AssembleContextRequest::default()
        })
        .await;
    let Err(ApiError::Refused(refusal)) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(
        crate::rejection_of(&refusal.refusal),
        Some(crate::Rejection::Library(
            library_tree::LibraryTreeRejection::NotInGit {
                root: ROOT.to_string()
            }
        ))
    );
    assert!(opened.0.lock().unwrap().is_empty());
}
