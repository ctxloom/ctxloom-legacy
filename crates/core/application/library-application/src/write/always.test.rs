//! session_approvals.feature's allow-always at the application port.

use std::sync::Arc;

use ctxloom_ports::Scope;

use crate::fakes::{plain, Mem, Setup, ENVELOPE};
use crate::Library;

const PROJECT: &str =
    "schema_version: 9\nagents:\n  default:\n    engine: claude-code\ndefault_agent: default\n";

fn world(layers: &'static [&'static str]) -> (Library, Arc<Mem>) {
    Setup {
        bundles: &[("project", &[ENVELOPE])],
        layers,
        ..Setup::default()
    }
    .fixture()
}

const RULE: &str = "Bash(rm -rf build/)";

#[test]
fn without_yes_it_previews_the_effective_document_with_the_rule_and_writes_nothing() {
    let (library, mem) = world(&[PROJECT]);
    let before = mem.config(Scope::Project);
    let shown = library
        .allow_always(&plain(), "default", "claude-code", RULE, false)
        .result
        .unwrap();
    assert_eq!(shown.key, "agents.default.engines.claude-code.permissions");
    assert_eq!(shown.rule, RULE);
    assert_eq!(
        shown.document,
        r#"{"defaultMode":"acceptEdits","allow":["Bash(rm -rf build/)"]}"#
    );
    assert!(!shown.applied);
    assert!(
        shown.path.ends_with(".ctxloom/config.yaml"),
        "{}",
        shown.path
    );
    assert_eq!(mem.config(Scope::Project), before);
}

#[test]
fn with_yes_it_writes_the_rule_over_the_engines_default_document_once() {
    let (library, mem) = world(&[PROJECT]);
    let written = library
        .allow_always(&plain(), "default", "claude-code", RULE, true)
        .result
        .unwrap();
    assert!(written.applied);
    let text = mem.config(Scope::Project).unwrap();
    assert!(text.contains("Bash(rm -rf build/)"), "{text}");
    assert!(text.contains("defaultMode: acceptEdits"), "{text}");
    let again = library
        .allow_always(&plain(), "default", "claude-code", RULE, true)
        .result
        .unwrap();
    assert!(!again.applied, "the rule is there already");
    assert_eq!(mem.config(Scope::Project).unwrap(), text);
}

const HOME: &str = "schema_version: 9\nagents:\n  default:\n    engines:\n      claude-code:\n        permissions:\n          defaultMode: default\n          allow:\n            - Read(a)\n";

#[test]
fn it_writes_into_the_layer_that_holds_the_agents_block() {
    let (library, mem) = world(&[HOME, PROJECT]);
    let written = library
        .allow_always(&plain(), "default", "claude-code", RULE, true)
        .result
        .unwrap();
    assert_eq!(
        written.document,
        r#"{"defaultMode":"default","allow":["Read(a)","Bash(rm -rf build/)"]}"#
    );
    let home = mem.config(Scope::Home).unwrap();
    assert!(home.contains("Bash(rm -rf build/)"), "{home}");
    assert!(home.contains("defaultMode: default"), "{home}");
    assert!(home.contains("Read(a)"), "{home}");
    assert!(!mem.config(Scope::Project).unwrap().contains("Bash("));
    assert_eq!(written.path, library_home_path(&mem));
}

/// The home layer's path, as the fake names it.
fn library_home_path(mem: &Mem) -> String {
    use ctxloom_ports::ConfigFiles as _;
    mem.layer_path(Scope::Home)
}

const BOTH: &str = "schema_version: 9\nagents:\n  default:\n    engine: claude-code\n    engines:\n      claude-code:\n        model: haiku\ndefault_agent: default\n";

#[test]
fn the_projects_block_wins_when_both_layers_hold_one() {
    let (library, mem) = world(&[HOME, BOTH]);
    library
        .allow_always(&plain(), "default", "claude-code", RULE, true)
        .result
        .unwrap();
    assert!(mem
        .config(Scope::Project)
        .unwrap()
        .contains("Bash(rm -rf build/)"));
    assert!(!mem.config(Scope::Home).unwrap().contains("Bash("));
}

#[test]
fn an_agent_nobody_defined_is_refused() {
    let (library, _) = world(&[PROJECT]);
    let refused = library
        .allow_always(&plain(), "nobody", "claude-code", RULE, true)
        .result;
    assert!(refused.is_err(), "{refused:?}");
}
