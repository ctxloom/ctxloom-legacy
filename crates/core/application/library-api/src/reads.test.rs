use library_application::fakes::{Setup, ENVELOPE};
use library_application::Flags;

use super::{list_agents, list_profiles, list_skills};

const PARENT: &str = "projects/calm-otter";

const DEMO: &[(&str, &str)] = &[
    ENVELOPE,
    (
        "skills/reviewer/SKILL.md",
        "---\nname: reviewer\ndescription: Reviews code\n---\nSKILL-BODY\n",
    ),
    (
        "skills/.reviewer.meta.yaml",
        "tags: [\"ctxloom:link_id=pair\"]\n",
    ),
];

const PROJECT: &[(&str, &str)] = &[
    ENVELOPE,
    ("profiles/dev.yaml", "description: dev\nbundles: [demo]\n"),
];

const AGENTS: &str = "schema_version: 9\nagents:\n  dev:\n    engine: claude-code\n    profiles: [dev]\ndefault_agent: dev\n";

fn library(layers: &[&str]) -> library_application::Library {
    Setup {
        bundles: &[("demo", DEMO), ("project", PROJECT)],
        layers,
        ..Setup::default()
    }
    .library()
}

#[test]
fn the_agents_are_answered_under_the_parent_the_default_marked() {
    let listed = list_agents(
        &library(&[AGENTS]),
        &Flags::default(),
        PARENT,
        &Default::default(),
    )
    .result
    .unwrap();
    assert_eq!(listed.agents.len(), 1);
    let dev = &listed.agents[0];
    assert_eq!(dev.name, "projects/calm-otter/agents/dev");
    assert_eq!(dev.engine, "projects/calm-otter/engines/claude-code");
    assert_eq!(dev.default, Some(true));
}

#[test]
fn no_agent_is_an_empty_listing() {
    let listed = list_agents(
        &library(&["schema_version: 9\n"]),
        &Flags::default(),
        PARENT,
        &Default::default(),
    )
    .result
    .unwrap();
    assert!(listed.agents.is_empty());
}

#[test]
fn the_profiles_are_answered_below_the_bundle_that_ships_them() {
    let listed = list_profiles(
        &library(&[AGENTS]),
        &Flags::default(),
        PARENT,
        &Default::default(),
    )
    .result
    .unwrap();
    let names: Vec<&str> = listed.profiles.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"projects/calm-otter/bundles/project/profiles/dev"),
        "{names:?}"
    );
}

#[test]
fn the_skills_are_answered_with_their_tags_below_their_bundle() {
    let listed = list_skills(
        &library(&[AGENTS]),
        &Flags::default(),
        PARENT,
        &Default::default(),
    )
    .result
    .unwrap();
    assert_eq!(listed.skills.len(), 1, "{listed:?}");
    let reviewer = &listed.skills[0];
    assert_eq!(
        reviewer.name,
        "projects/calm-otter/bundles/demo/skills/reviewer"
    );
    assert!(
        reviewer.tags.iter().any(|t| t == "ctxloom:link_id=pair"),
        "{reviewer:?}"
    );
}
