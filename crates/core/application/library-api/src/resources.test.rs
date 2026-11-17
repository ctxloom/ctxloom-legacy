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
