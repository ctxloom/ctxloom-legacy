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

#[test]
fn a_skill_listing_reads_back_each_skill_below_its_bundle() {
    let listed = v1::ListSkillsResponse {
        skills: vec![v1::Skill {
            name: "users/me/remotes/acme/bundles/kit/skills/review".to_string(),
            display_name: "reviewer".to_string(),
            description: "Reviews".to_string(),
            tags: vec!["t".to_string()],
            file_count: 2,
            ..v1::Skill::default()
        }],
        next_page_token: String::new(),
    };
    let list = skill_list(&listed, "acme/kit", 5);
    assert_eq!(list.rows[0].name, "acme/kit/review");
    assert_eq!(list.rows[0].source, "acme/kit");
    assert_eq!(list.rows[0].display_name, "reviewer");
    assert_eq!(list.rows[0].file_count, 2);
    assert_eq!(list.bundle_filter, "acme/kit");
    assert_eq!(list.total, 5);
}

#[test]
fn a_shown_skill_reads_back_each_files_mode_from_its_executability() {
    let skill = v1::Skill {
        name: "projects/p/bundles/demo/skills/review".to_string(),
        body: "BODY".to_string(),
        files: vec![
            v1::SkillFile {
                path: "SKILL.md".to_string(),
                sha256: "ab".to_string(),
                executable: false,
            },
            v1::SkillFile {
                path: "run.sh".to_string(),
                sha256: "cd".to_string(),
                executable: true,
            },
        ],
        ..v1::Skill::default()
    };
    let show = skill_show(&skill);
    assert_eq!(
        (show.bundle.as_str(), show.name.as_str()),
        ("demo", "review")
    );
    assert_eq!(show.body, "BODY");
    assert_eq!(show.files[0].mode, "0644");
    assert_eq!(show.files[1].mode, "0755");
}

#[test]
fn a_companion_listing_reads_back_each_by_its_name_program_and_state() {
    let listed = v1::ListCompanionsResponse {
        companions: vec![
            v1::Companion {
                name: "projects/p/companions/acme".to_string(),
                bin: "ctxloom-acme".to_string(),
                resolves: Some(true),
                state: v1::companion::State::Disabled as i32,
            },
            v1::Companion {
                name: "users/me/companions/gone".to_string(),
                bin: "ctxloom-gone".to_string(),
                resolves: Some(false),
                state: v1::companion::State::Enabled as i32,
            },
        ],
        next_page_token: String::new(),
    };
    let rows = companions(&listed);
    assert_eq!(rows[0].name, "acme");
    assert_eq!(rows[0].program, "ctxloom-acme");
    assert!(rows[0].exists);
    assert_eq!(rows[0].state, library_application::CompanionState::Disabled);
    assert_eq!(rows[1].name, "gone");
    assert!(!rows[1].exists);
    assert_eq!(rows[1].state, library_application::CompanionState::Enabled);
}

#[test]
fn a_profile_reads_back_by_the_name_typed_and_its_references_short() {
    // Owner ruling 2026-11-17: text prints the short reference.
    assert_eq!(
        profile_ask("projects/p/bundles/project/profiles/dev"),
        "dev"
    );
    assert_eq!(
        profile_ask("projects/p/bundles/demo/profiles/x"),
        "ctxloom+local:demo#profiles/x"
    );
    assert_eq!(
        profile_ask("projects/p/remotes/acme/bundles/kit/profiles/x"),
        "acme/kit#profiles/x"
    );
    let profile = v1::Profile {
        name: "projects/p/bundles/project/profiles/dev".to_string(),
        description: "dev profile".to_string(),
        parents: vec!["projects/p/bundles/project/profiles/base".to_string()],
        bundles: vec!["projects/p/bundles/demo".to_string()],
        exclude_mcp_servers: vec!["tree-sitter".to_string()],
        default: Some(true),
        ..<v1::Profile as Default>::default()
    };
    let show = profile_show(&profile);
    assert_eq!(show.name, "dev");
    assert_eq!(show.bundle, "project");
    assert_eq!(show.parents, ["project#profiles/base"]);
    assert_eq!(show.bundles, ["demo"]);
    assert_eq!(show.exclude_mcp, profile.exclude_mcp_servers);
    assert!(show.default);
    let listed = v1::ListProfilesResponse {
        profiles: vec![profile],
        next_page_token: String::new(),
    };
    let list = profile_list(&listed, true);
    assert_eq!(list.rows[0].name, "dev");
    assert_eq!(list.rows[0].bundle, "project");
    assert_eq!(list.rows[0].parents, ["project#profiles/base"]);
    assert_eq!(list.rows[0].bundles, ["demo"]);
    assert!(list.rows[0].default);
    assert_eq!(list.hint, library_application::PROFILE_HINT);
    assert_eq!(
        profile_list(&listed, false).hint,
        library_application::NO_PROJECT_HINT
    );
}

// step 10a U2 (owner ruling 2026-11-17): a profile's selecting tags,
// fragments (by their short references), hooks and denied tools read back
// from its message, and its one-item view prints them in the document's
// shape, keys sorted
#[test]
fn a_profile_s_fragments_hooks_and_denied_tools_read_back_and_view() {
    let hook = v1::ProfileHook {
        event: "pre_tool".to_string(),
        name: "0".to_string(),
        command: "lint".to_string(),
        kind: "command".to_string(),
        args: vec!["--fix".to_string()],
        ..v1::ProfileHook::default()
    };
    let profile = v1::Profile {
        name: "projects/p/bundles/demo/profiles/full".to_string(),
        select_tags: vec!["go".to_string()],
        fragments: vec![
            v1::ProfileFragment {
                fragment: "plain".to_string(),
                priority: 0,
            },
            v1::ProfileFragment {
                fragment: "projects/p/bundles/demo/fragments/pri".to_string(),
                priority: 3,
            },
        ],
        hooks: Some(v1::ProfileHooks {
            unified: [(
                "pre_tool".to_string(),
                v1::ProfileHookList { hooks: vec![hook] },
            )]
            .into_iter()
            .collect(),
            ext: Default::default(),
        }),
        deny_tools: vec!["Bash".to_string()],
        ..<v1::Profile as Default>::default()
    };
    let show = profile_show(&profile);
    assert_eq!(show.select_tags, ["go"]);
    let fragments: Vec<(&str, i64)> = show
        .fragments
        .iter()
        .map(|f| (f.name.as_str(), f.priority))
        .collect();
    assert_eq!(fragments, [("plain", 0), ("demo#fragments/pri", 3)]);
    let pre = &show.hooks.unified["pre_tool"][0];
    assert_eq!(
        (pre.command.as_str(), pre.kind.as_str()),
        ("lint", "command")
    );
    assert_eq!(pre.args, ["--fix"]);
    assert_eq!(show.deny_tools, ["Bash"]);
    let view = super::profile_view("demo#profiles/full", &profile);
    assert_eq!(
        view.content,
        "# Profile: full\ndeny_tools:\n  - Bash\nfragments:\n  - plain\n  - name: demo#fragments/pri\n    priority: 3\nhooks:\n  unified:\n    pre_tool:\n      - args:\n          - --fix\n        command: lint\n        type: command\nselect_tags:\n  - go\n"
    );
}
