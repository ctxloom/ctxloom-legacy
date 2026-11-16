//! What trusting a project would allow, read from its own layer and its own
//! bundles (step 8 U1).

use crate::fakes::{Setup, CONFIG, ENVELOPE};
use crate::{HeldBack, CTXLOOM_BUNDLE_PATH};

fn held(path: &str, value: &str) -> HeldBack {
    HeldBack {
        path: path.to_string(),
        value: value.to_string(),
    }
}

#[test]
fn a_project_layers_project_only_settings_are_what_trust_allows() {
    let library = Setup {
        layers: &[
            CONFIG,
            "schema_version: 9\nagents:\n  default:\n    session_home: host\nengines:\n  claude-code:\n    command: /opt/their-claude\n    auth:\n      file: ~/their-login\n",
        ],
        ..Setup::default()
    }
    .library();
    assert_eq!(
        library.trust_settings().unwrap(),
        [
            held("agents.default.session_home", "host"),
            held("engines.claude-code.command", "/opt/their-claude"),
            held("engines.claude-code.auth.file", "~/their-login"),
        ]
    );
}

#[test]
fn the_home_layer_says_nothing_trust_allows() {
    let library = Setup {
        layers: &[
            "schema_version: 9\nengines:\n  claude-code:\n    command: /opt/mine\n",
            CONFIG,
        ],
        ..Setup::default()
    }
    .library();
    assert_eq!(library.trust_settings().unwrap(), []);
}

#[test]
fn a_project_ctxloom_bundle_is_allowed_by_trust_too() {
    let library = Setup {
        layers: &[CONFIG, CONFIG],
        bundles: &[("ctxloom", &[ENVELOPE]), ("lore", &[ENVELOPE])],
        ..Setup::default()
    }
    .library();
    let settings = library.trust_settings().unwrap();
    assert_eq!(settings.len(), 1, "{settings:?}");
    assert_eq!(settings[0].path, CTXLOOM_BUNDLE_PATH);
    assert!(settings[0].value.contains("ctxloom"), "{settings:?}");
}

#[test]
fn a_project_with_no_bundles_directory_has_no_ctxloom_bundle() {
    let library = Setup {
        layers: &[CONFIG, CONFIG],
        bundles: &[("ctxloom", &[ENVELOPE])],
        no_root: true,
        ..Setup::default()
    }
    .library();
    assert_eq!(library.trust_settings().unwrap(), []);
}

#[test]
fn outside_a_project_trust_allows_nothing() {
    let library = Setup {
        layers: &["schema_version: 9\nengines:\n  claude-code:\n    command: /opt/x\n"],
        home_only: true,
        ..Setup::default()
    }
    .library();
    assert_eq!(library.trust_settings().unwrap(), []);
}

#[test]
fn a_project_layer_that_does_not_parse_is_refused() {
    let library = Setup {
        layers: &[CONFIG, "schema_version: 9\nengines: [unclosed\n"],
        ..Setup::default()
    }
    .library();
    assert!(library.trust_settings().is_err());
}
