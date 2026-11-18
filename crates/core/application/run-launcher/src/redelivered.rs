//! A live session's re-assembled context, as its engine is handed it (step
//! 10c U13, G6/G7; texts.md §1): one framing for every engine, carried by
//! each engine's own `run_ports::Engine::deliver_context`.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};

use crate::EN_US;

/// The framing line of a re-assembled context: what changed (its bundles,
/// and `config` for a config change; never a path or a value).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextRedelivered {
    pub changed: Vec<String>,
}

impl Diagnostic for ContextRedelivered {
    fn code(&self) -> &str {
        code!("context-redelivered")
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        vec![("changed", self.changed.join(", ").into())]
    }
}

impl fmt::Display for ContextRedelivered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::catalog::render(EN_US.1, self, f)
    }
}

/// `context` (as `context assemble` renders it, its premise index included)
/// re-delivered for what `changed` names: the framing line, a blank line,
/// then the context whole.
pub fn redelivered_context(changed: &[String], context: &str) -> String {
    let framing = ContextRedelivered {
        changed: changed.to_vec(),
    };
    format!("{framing}\n\n{context}")
}

#[cfg(test)]
#[path = "redelivered.test.rs"]
mod tests;
