use daemon_proto::google::protobuf::value::Kind;

use super::{document, engine_ref, profile_id, profile_name, skill_name};

#[test]
fn a_document_keeps_its_shape() {
    let value = document(&serde_json::json!({"allow": ["Read"], "n": 2, "on": true, "x": null}));
    let Some(Kind::StructValue(fields)) = value.kind else {
        panic!("an object");
    };
    assert!(matches!(
        fields.fields["allow"].kind,
        Some(Kind::ListValue(_))
    ));
    assert!(matches!(fields.fields["n"].kind, Some(Kind::NumberValue(n)) if n == 2.0));
    assert!(matches!(
        fields.fields["on"].kind,
        Some(Kind::BoolValue(true))
    ));
    assert!(matches!(fields.fields["x"].kind, Some(Kind::NullValue(_))));
}

#[test]
fn an_agent_that_names_no_engine_references_none() {
    assert_eq!(engine_ref("users/me", ""), "");
    assert_eq!(
        engine_ref("users/me", "opencode"),
        "users/me/engines/opencode"
    );
}

#[test]
fn a_profile_is_named_below_its_bundle_the_project_s_own_for_none() {
    assert_eq!(profile_id("demo#profiles/dev"), "dev");
    assert_eq!(profile_id("dev"), "dev");
    assert_eq!(
        profile_name("projects/p", "", "dev"),
        "projects/p/bundles/project/profiles/dev"
    );
    assert_eq!(
        profile_name("projects/p", "acme/rules", "dev"),
        "projects/p/remotes/acme/bundles/rules/profiles/dev"
    );
}

#[test]
fn a_skill_is_named_below_its_bundle() {
    assert_eq!(
        skill_name("projects/p", "demo", "reviewer"),
        "projects/p/bundles/demo/skills/reviewer"
    );
}

fn item_list(kind: &str) -> library_application::ItemList {
    library_application::ItemList {
        rows: vec![library_application::ItemRow {
            name: "one".to_string(),
            tags: vec!["go".to_string()],
            bundle: "demo".to_string(),
            reference: "demo#x/one".to_string(),
            remote: String::new(),
            bundle_label: String::new(),
        }],
        kind: kind.to_string(),
        bundle_filter: String::new(),
        total: 1,
        hint: String::new(),
    }
}

#[test]
fn a_listed_item_is_named_below_its_bundle_and_keeps_its_tags() {
    let commands = super::commands("projects/p", &item_list("command"));
    assert_eq!(
        commands.commands[0].name,
        "projects/p/bundles/demo/commands/one"
    );
    assert_eq!(commands.commands[0].tags, ["go"]);
    let fragments = super::fragments("projects/p", &item_list("fragment"));
    assert_eq!(
        fragments.fragments[0].name,
        "projects/p/bundles/demo/fragments/one"
    );
    assert_eq!(fragments.fragments[0].reference, "demo#x/one");
    assert_eq!(fragments.fragments[0].tags, ["go"]);
}

#[test]
fn a_shown_item_is_its_whole_resource() {
    let show = library_application::ItemShow {
        name: "one".to_string(),
        bundle: "demo".to_string(),
        reference: "demo#x/one".to_string(),
        content: "BODY".to_string(),
        distilled: true,
        state: library_application::DistilledState::Stale,
    };
    let command = super::command("projects/p", &show);
    assert_eq!(command.name, "projects/p/bundles/demo/commands/one");
    assert_eq!(command.reference, "demo#x/one");
    assert_eq!(command.body, "BODY");
    assert!(command.body_is_distilled);
    assert_eq!(
        command.distilled,
        daemon_proto::v1::DistilledState::Stale as i32
    );
    let fragment = super::fragment("projects/p", &show);
    assert_eq!(fragment.name, "projects/p/bundles/demo/fragments/one");
    assert_eq!(
        fragment.distilled,
        daemon_proto::v1::DistilledState::Stale as i32
    );
}

#[test]
fn a_premise_index_names_each_fragment() {
    let premises = library_application::Premises {
        instruction: "Pick.".to_string(),
        fragments: vec![
            library_application::PremiseRow {
                name: "demo#fragments/rules".to_string(),
                premise: "when testing".to_string(),
            },
            library_application::PremiseRow {
                name: "no reference".to_string(),
                premise: "p".to_string(),
            },
        ],
    };
    let index = super::premise_index("users/me", &premises);
    assert_eq!(index.name, "users/me/premiseIndex");
    assert_eq!(index.instruction, "Pick.");
    assert_eq!(
        index.fragments[0].fragment,
        "users/me/bundles/demo/fragments/rules"
    );
    assert_eq!(index.fragments[1].fragment, "no reference");
}
