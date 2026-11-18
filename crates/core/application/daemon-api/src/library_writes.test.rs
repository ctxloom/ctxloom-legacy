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
            keep: true,
        }),
        r#move: None,
        purge: None,
    };
    assert_eq!(step_path(&step), "content/bundles/v2/demo");
    let purge = library_tree::Step {
        trash: None,
        purge: Some(library_tree::PurgeSlot {
            request_id: "r-0".to_string(),
        }),
        ..step
    };
    assert_eq!(step_path(&purge), "state/trash/r-0");
}

// E1, extended to the writes: a write's directory is the one it carries,
// set and read back through the request it is, as its environment is.
#[test]
fn test_a_writes_directory_is_its_requests_own() {
    use super::LibraryWriteRequest;
    let mut request = daemon_proto::v1::UpdateConfigRequest::default();
    assert_eq!(request.cwd(), "");
    request.set_cwd("/work/demo".to_string());
    assert_eq!(request.cwd(), "/work/demo");
    assert_eq!(request.cwd, "/work/demo");
}

// Step 10b U3 (G4 superseded): a write is planned against the etag its
// caller's preview answered, the path's sequence then, so `decide` judges
// it stale when the path has moved since; one call that plans and applies
// (`--yes`) is planned against the path as it is now.
#[test]
fn test_a_writes_seen_sequence_is_its_callers_etag_else_the_paths_now() {
    use super::seen_sequence;
    assert_eq!(seen_sequence("", 9).unwrap(), 9);
    assert_eq!(seen_sequence("4", 9).unwrap(), 4);
    assert_eq!(seen_sequence("0", 9).unwrap(), 0);
    assert!(seen_sequence("abc", 9).is_err());
}

// Every write that names an existing resource carries the etag its
// caller sends back; a create carries none.
#[test]
fn test_an_update_or_a_removal_carries_its_etag_and_a_create_none() {
    use super::LibraryWriteRequest;
    let update = daemon_proto::v1::UpdateFragmentRequest {
        etag: "7".to_string(),
        ..Default::default()
    };
    assert_eq!(update.etag(), "7");
    let removal = daemon_proto::v1::DeleteBundleRequest {
        etag: "3".to_string(),
        ..Default::default()
    };
    assert_eq!(removal.etag(), "3");
    assert_eq!(daemon_proto::v1::CreateBundleRequest::default().etag(), "");
}

// An operation that made nothing is the domain's refusal: something at the
// path it creates is `ResourceExists` (D4, NOREPLACE), nothing at the path
// it takes `NoSuchResource`; one that made its change is no refusal.
#[test]
fn test_an_operation_that_made_nothing_is_the_domain_s_refusal() {
    use ctxloom_ports::Operated;
    assert!(super::not_made(Operated::Done).is_none());
    let code = |operated| match super::not_made(operated) {
        Some(crate::refusal::ApiError::Refused(refusal)) => refusal
            .refusal
            .type_url
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_string(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        code(Operated::Exists {
            path: "content/bundles/v2/notes".to_string()
        }),
        "ResourceExists"
    );
    assert_eq!(
        code(Operated::Missing {
            path: "content/bundles/v2/demo".to_string()
        }),
        "NoSuchResource"
    );
}

// An etag that is not a number (none a preview answered) is the request's
// own bad argument (protos.md §1.1): the typed `EtagInvalid`, naming the
// etag as sent, never a failure (exit 10).
#[test]
fn test_an_etag_that_is_not_a_number_is_refused_as_an_invalid_argument() {
    use super::seen_sequence;
    use daemon_proto::Refusal as _;
    let Err(crate::refusal::ApiError::Refused(refusal)) = seen_sequence("abc", 9) else {
        panic!("not refused")
    };
    assert_eq!(refusal.code, daemon_proto::v1::EtagInvalid::REFUSAL_STATUS);
    let invalid: daemon_proto::v1::EtagInvalid = refusal.refusal.to_msg().unwrap();
    assert_eq!(invalid.etag, "abc");
    assert!(matches!(
        seen_sequence("-1", 9),
        Err(crate::refusal::ApiError::Refused(_))
    ));
}
