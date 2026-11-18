//! Step 10c U13 (texts.md §1): a re-assembled context, as an engine is
//! handed it.

use super::{redelivered_context, ContextRedelivered};

// The framing line names what changed (bundles, and `config`, comma
// separated: never a path or a value), says the context changed and that
// what follows replaces the first; then a blank line, then the context
// exactly as assembled.
#[test]
fn a_redelivered_context_is_framed_then_given_whole() {
    let changed = vec!["config".to_string(), "demo".to_string()];
    let text = redelivered_context(&changed, "# Changed while it ran\n\nmore");
    let (framing, rest) = text
        .split_once("\n\n")
        .expect("a blank line after the framing");
    assert!(
        framing.starts_with("ctxloom: your context changed while this session ran (config, demo)."),
        "{framing}"
    );
    assert!(
        framing.contains("replaces the context you were given"),
        "{framing}"
    );
    assert_eq!(rest, "# Changed while it ran\n\nmore");
}

// Its message is the catalog's, by its code, with its one arg.
#[test]
fn the_framing_is_the_catalogs_message() {
    use angzarr_diagnostic::Diagnostic;
    let framing = ContextRedelivered {
        changed: vec!["demo".to_string()],
    };
    assert_eq!(framing.code(), "run-launcher-context-redelivered");
    assert_eq!(framing.args().len(), 1);
}
