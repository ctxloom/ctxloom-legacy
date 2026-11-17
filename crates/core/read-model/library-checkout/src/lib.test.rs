//! The checkouts: each tree folded from its stream with the domain's own
//! fold, each path's token its last event's sequence, answered as the
//! domain's state message.

use angzarr_embedded::angzarr_proto::v1::{
    event_meta::Causation, EventMeta, FactCause, FactOrigin,
};
use angzarr_embedded::angzarr_router::hpb::ProjectRequest;
use angzarr_embedded::testing::block_on_ready as ready;
use angzarr_embedded::v1::{event_page, Cover, EventBook, EventPage};
use angzarr_embedded::{Payload, ProjectorHandler};
use library_tree::{
    tree_root, Change, ChangeStatus, DirtyPath, External, LibraryEvent, StepApplied, TreeKind,
    TreeObserved, LIBRARY_DOMAIN,
};

use crate::{caught_up, LibraryCheckout, LibraryCheckouts};

const TREE: &str = "projects/amber-quiet-heron/checkouts/brisk-copper-moth/library";
const FILE: &str = "content/bundles/v2/demo/fragments/example.md";

fn observed(baseline: bool, changes: &[(&str, &str)]) -> LibraryEvent {
    LibraryEvent::TreeObserved(TreeObserved {
        name: TREE.to_string(),
        external: Some(External {
            head_before: "h-0".to_string(),
            head_after: "h-1".to_string(),
        }),
        intended: None,
        changes: changes
            .iter()
            .map(|(path, oid)| Change {
                path: path.to_string(),
                status: ChangeStatus::Modified,
                from_oid: String::new(),
                to_oid: oid.to_string(),
                from_path: String::new(),
            })
            .collect(),
        dirty_after: changes
            .iter()
            .map(|(path, oid)| DirtyPath {
                path: path.to_string(),
                oid: oid.to_string(),
            })
            .collect(),
        tree_after: "t-1".to_string(),
        baseline,
        in_motion: None,
    })
}

/// The tree's stream: `events` from `first` on, each caused by the fact
/// `obs:<sequence>`.
fn on(domain: &str, first: u64, events: Vec<LibraryEvent>) -> EventBook {
    let pages: Vec<EventPage> = (first..)
        .zip(events)
        .map(|(sequence, event)| EventPage {
            sequence,
            body: Some(event_page::Body::Event(event.to_any())),
            meta: Some(EventMeta {
                causation: Some(Causation::Fact(FactCause {
                    command_id: None,
                    origin: Some(FactOrigin {
                        external_id: format!("obs:{sequence}"),
                        description: String::new(),
                    }),
                })),
                ..EventMeta::default()
            }),
        })
        .collect();
    let next = pages.last().map_or(0, |p| p.sequence + 1);
    EventBook {
        cover: Some(Cover {
            domain: domain.to_string(),
            root: Some(tree_root(TREE)),
            edition: None,
        }),
        pages,
        next_sequence: next,
        snapshot: None,
    }
}

#[test]
fn a_tree_is_folded_from_its_stream_each_path_at_its_last_events_sequence() {
    let mut model = LibraryCheckout::new();
    model
        .project(&on(LIBRARY_DOMAIN, 0, vec![observed(true, &[])]))
        .unwrap();
    model
        .project(&on(
            LIBRARY_DOMAIN,
            1,
            vec![observed(false, &[(FILE, "f1")])],
        ))
        .unwrap();
    let root = tree_root(TREE);
    let tree = model.tree(&root).expect("held");
    assert_eq!(tree.name, TREE);
    assert_eq!(tree.kind, Some(TreeKind::Checkout));
    assert_eq!(tree.head, "h-1");
    assert_eq!(tree.last_cause, "obs:1");
    assert_eq!(model.token(&root, FILE), Some(1));
    assert_eq!(model.token(&root, "config.yaml"), Some(0));
    assert_eq!(model.token(&tree_root("users/me/library"), FILE), None);
}

#[test]
fn a_step_applied_moves_its_paths_tokens_as_the_aggregates_fold_does() {
    let mut model = LibraryCheckout::new();
    model
        .project(&on(
            LIBRARY_DOMAIN,
            0,
            vec![
                observed(true, &[]),
                LibraryEvent::StepApplied(StepApplied {
                    request_id: "r-1".to_string(),
                    step: 0,
                    observed: vec![DirtyPath {
                        path: FILE.to_string(),
                        oid: "n".to_string(),
                    }],
                }),
            ],
        ))
        .unwrap();
    assert_eq!(model.token(&tree_root(TREE), FILE), Some(1));
}

#[test]
fn a_book_from_sequence_zero_starts_the_stream_afresh() {
    let mut model = LibraryCheckout::new();
    model
        .project(&on(
            LIBRARY_DOMAIN,
            0,
            vec![observed(true, &[(FILE, "f1")])],
        ))
        .unwrap();
    let mut gone = on(LIBRARY_DOMAIN, 0, vec![]);
    gone.next_sequence = 0;
    model.project(&gone).unwrap();
    assert!(model.tree(&tree_root(TREE)).is_none());
    model
        .project(&on(LIBRARY_DOMAIN, 0, vec![observed(true, &[])]))
        .unwrap();
    assert!(model.tree(&tree_root(TREE)).unwrap().resources.is_empty());
}

#[test]
fn another_domains_book_is_none_of_its() {
    let mut model = LibraryCheckout::new();
    model
        .project(&on("trust", 0, vec![observed(true, &[])]))
        .unwrap();
    assert!(model.tree(&tree_root(TREE)).is_none());
}

#[test]
fn the_projection_is_the_trees_state_message() {
    use prost::Message as _;
    let model = LibraryCheckouts::default();
    let view = ready(model.dispatch(&ProjectRequest {
        events: Some(on(LIBRARY_DOMAIN, 0, vec![observed(true, &[(FILE, "f1")])])),
    }))
    .unwrap()
    .projection
    .expect("a view");
    assert_eq!(
        view.type_url,
        <library_domain_proto::v1::LibraryState as prost::Name>::type_url()
    );
    let state = library_domain_proto::v1::LibraryState::decode(view.value.as_slice()).unwrap();
    assert_eq!(state.name, TREE);
    assert_eq!(state.baseline_sequence, 0);
    assert_eq!(
        state
            .resources
            .get(FILE)
            .map(|r| (r.sequence, r.oid.as_str())),
        Some((0, "f1"))
    );
    assert_eq!(model.name(), "library-checkout");
    assert!(model.checkouts().tree(&tree_root(TREE)).is_some());
    let _ = caught_up(model);
}

#[test]
fn a_stream_it_holds_nothing_of_has_no_view() {
    use held_projection::Model as _;
    let model = LibraryCheckout::new();
    let cover = Cover {
        domain: LIBRARY_DOMAIN.to_string(),
        root: Some(tree_root(TREE)),
        edition: None,
    };
    assert!(model.view(&cover).is_none());
}
