//! A write's landing among the trees observed, and its command id.

use library_api::writes::{Lands, Scope};
use library_api::{Observed, Site};
use library_tree::TreeName;
use sessions::ProjectName;

use super::{landing_of, request_bytes, step_path};

fn observed(tree: TreeName) -> Observed {
    Observed {
        tree,
        notices: Vec::new(),
        motion: None,
    }
}

fn seen(with_project: bool) -> Vec<(Site, Observed)> {
    let project = ProjectName::parse("projects/amber-quiet-heron").unwrap();
    let mut seen = Vec::new();
    if with_project {
        seen.push((
            Site::checkout(project.clone(), "/work/demo"),
            observed(TreeName::checkout(project, "brisk-copper-moth").unwrap()),
        ));
    }
    seen.push((
        Site::Home {
            tree: "/home/alice/.ctxloom".to_string(),
        },
        observed(TreeName::home("calm-otter-run").unwrap()),
    ));
    seen
}

#[test]
fn test_content_lands_in_the_checkout_else_the_home_and_a_layer_in_its_scope() {
    let both = seen(true);
    let at = |lands| landing_of(&both, lands).map(|l| l.site.tree().to_string());
    assert_eq!(at(Lands::Content).as_deref(), Some("/work/demo/.ctxloom"));
    assert_eq!(
        at(Lands::Layer(Scope::Project)).as_deref(),
        Some("/work/demo/.ctxloom")
    );
    assert_eq!(
        at(Lands::Layer(Scope::Home)).as_deref(),
        Some("/home/alice/.ctxloom")
    );
    let home = seen(false);
    assert_eq!(
        landing_of(&home, Lands::Content).map(|l| l.root()),
        Some("/home/alice/.ctxloom".to_string())
    );
    assert!(landing_of(&home, Lands::Layer(Scope::Project)).is_none());
    assert_eq!(
        landing_of(&both, Lands::Content).map(|l| l.root()),
        Some("/work/demo".to_string())
    );
}

#[test]
fn test_the_command_id_is_the_requests_own_uuid() {
    let id = "0190a4c2-7d1e-7c3a-9b5f-0123456789ab";
    assert_eq!(
        uuid::Uuid::from_bytes(request_bytes(id)).to_string(),
        id,
        "a retry under the same id is a duplicate"
    );
    assert_ne!(request_bytes("not a uuid"), request_bytes("not a uuid"));
}

#[test]
fn test_a_steps_path_is_its_kinds_own() {
    let step = library_tree::Step {
        replace: None,
        create: None,
        trash: Some(library_tree::TrashPath {
            name: String::new(),
            verb: String::new(),
            path: "content/bundles/v2/demo".to_string(),
            seen_sequence: 0,
            evidence: library_tree::Evidence {
                tree_reads: true,
                plan_rejection: None,
                builtin: false,
                in_motion: None,
                in_git: true,
                ignored: None,
                root: String::new(),
            },
        }),
        r#move: None,
    };
    assert_eq!(step_path(&step), "content/bundles/v2/demo");
}
