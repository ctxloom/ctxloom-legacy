use daemon_proto::v1;

use super::*;

#[test]
fn an_item_listing_reads_back_each_item_below_its_bundle() {
    let tags = vec!["team".to_string()];
    let listed = Listed {
        kind: "fragment",
        items: vec![
            ("demo#fragments/rules", tags.as_slice()),
            ("vendor/kit#fragments/style", &[]),
        ],
        bundle_filter: "",
        total: 2,
        in_project: true,
    };
    let list = item_list(&listed);
    assert_eq!(list.kind, "fragment");
    assert_eq!(list.rows[0].bundle, "demo");
    assert_eq!(list.rows[0].bundle_label, "demo");
    assert_eq!(list.rows[0].name, "rules");
    assert_eq!(list.rows[0].tags, tags);
    assert_eq!(list.rows[1].bundle, "vendor/kit");
    assert_eq!(list.rows[1].name, "style");
    assert_eq!(list.hint, library_application::LISTING_HINT);
    let none = item_list(&Listed {
        in_project: false,
        ..listed
    });
    assert_eq!(none.hint, library_application::NO_PROJECT_HINT);
}

#[test]
fn a_shown_item_reads_back_its_body_and_distilled_state() {
    let show = item_show(
        "demo#commands/review",
        "BODY",
        true,
        v1::DistilledState::Stale as i32,
    );
    assert_eq!(show.bundle, "demo");
    assert_eq!(show.name, "review");
    assert_eq!(show.content, "BODY");
    assert!(show.distilled);
    assert_eq!(show.state, DistilledState::Stale);
    let fresh = item_show(
        "demo#fragments/x",
        "",
        false,
        v1::DistilledState::Fresh as i32,
    );
    assert_eq!(fresh.state, DistilledState::Fresh);
    assert_eq!(item_show("x", "", false, 0).state, DistilledState::None);
}

#[test]
fn a_premise_index_reads_back_each_fragment_by_its_reference() {
    let index = v1::PremiseIndex {
        name: "projects/calm-otter/premiseIndex".to_string(),
        instruction: "Pick.".to_string(),
        fragments: vec![v1::PremiseEntry {
            fragment: "projects/calm-otter/bundles/demo/fragments/rules".to_string(),
            premise: "when testing".to_string(),
        }],
    };
    let premises = premises(&index);
    assert_eq!(premises.instruction, "Pick.");
    assert_eq!(premises.fragments[0].name, "demo#fragments/rules");
    assert_eq!(premises.fragments[0].premise, "when testing");
}
