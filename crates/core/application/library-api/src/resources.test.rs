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

// step 10a U2 (owner ruling 2026-11-17): a profile's message carries its
// select_tags, fragments (an item reference by its resource name, a bare
// name as written), hooks and deny_tools
#[test]
fn a_profile_s_message_carries_its_selecting_tags_fragments_hooks_and_denied_tools() {
    use library_application::{FragmentRef, Hook, ProfileHooks, ProfileShow};
    let hook = Hook {
        event: "pre_tool".to_string(),
        name: "0".to_string(),
        order: Some(2),
        command: "lint".to_string(),
        kind: "command".to_string(),
        args: vec!["--fix".to_string()],
        timeout: 5,
        is_async: true,
        ..Hook::default()
    };
    let mut hooks = ProfileHooks::default();
    hooks.unified.insert("pre_tool".to_string(), vec![hook]);
    hooks
        .ext
        .entry("claude-code".to_string())
        .or_default()
        .insert("Notification".to_string(), vec![Hook::default()]);
    let show = ProfileShow {
        name: "dev".to_string(),
        select_tags: vec!["go".to_string()],
        fragments: vec![
            FragmentRef {
                name: "plain".to_string(),
                priority: 0,
            },
            FragmentRef {
                name: "demo#fragments/pri".to_string(),
                priority: 3,
            },
        ],
        hooks,
        deny_tools: vec!["Bash".to_string()],
        ..ProfileShow::default()
    };
    let profile = super::profile_shown("projects/p", &show);
    assert_eq!(profile.select_tags, ["go"]);
    let fragments: Vec<(&str, i64)> = profile
        .fragments
        .iter()
        .map(|f| (f.fragment.as_str(), f.priority))
        .collect();
    assert_eq!(
        fragments,
        [("plain", 0), ("projects/p/bundles/demo/fragments/pri", 3)]
    );
    assert_eq!(profile.deny_tools, ["Bash"]);
    let hooks = profile.hooks.expect("hooks");
    let pre = &hooks.unified["pre_tool"].hooks[0];
    assert_eq!(
        (pre.event.as_str(), pre.name.as_str(), pre.order),
        ("pre_tool", "0", Some(2))
    );
    assert_eq!(
        (pre.command.as_str(), pre.kind.as_str(), pre.timeout),
        ("lint", "command", 5)
    );
    assert_eq!(pre.args, ["--fix"]);
    assert!(pre.is_async);
    assert_eq!(
        hooks.ext["claude-code"].events["Notification"].hooks.len(),
        1
    );
    let none = super::profile_shown("projects/p", &ProfileShow::default());
    assert_eq!(none.hooks, None, "a profile with no hooks carries none");
}
