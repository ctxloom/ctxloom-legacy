//! A library view as its API message (OD-40; design §6): what every noun's
//! view-to-message mapping shares. Each noun's view is mapped, in its own
//! module (`noun/<noun>.rs`, design §8.4), to its `ctxloom.v1` message
//! (library.proto), named under the parent the daemon's `ResolveProject`
//! gives (OQ-2; `users/me` outside a project, OQ-3), and printed here as the
//! message's generated proto3 JSON (OQ-21): the sourced values, an agent's
//! settings (an engine's agents, and agent and `run --dry-run` once B2
//! converts them) and a free-form document. The views stay as they are; at
//! T13 the same mapping moves into the daemon.
//!
//! Converted so far: the `engine` noun (the spine's worked example,
//! `noun/engine.rs`), `agent`, `companion` (whose writes answer
//! through [`written`]) and `config`. The other nouns still print their views, until
//! their branch converts them.

use std::io::{self, Write};

use angzarr_presentation::Format;
use cli_support::{say_notices, Exit};
use library_application::{Notice, Outcome};
use serde::Serialize;

use crate::{failed, output_failure, Context, Failure, PROG};

/// Write `message`'s proto3 JSON, as every json answer is written.
pub(crate) fn print<M: serde::Serialize>(out: &mut dyn Write, message: &M) -> io::Result<()> {
    angzarr_presentation::write_json(out, message)
}

/// A converted write's answer (OD-40; OQ-8, OQ-9): the notices, said; then
/// its error, or its json, the method's response named under the parent the
/// names give (asked only here, after the write answered), or its text, with
/// what the text noted said after it; the exit `exit` gives (a preview's
/// would-change, ruling 153).
pub(crate) fn written<T, M: Serialize>(
    cx: &Context<'_>,
    outcome: Outcome<T>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    message: impl FnOnce(&str, &T) -> M,
    text: impl FnOnce(&T, &mut dyn Write, &mut Vec<String>) -> io::Result<()>,
    exit: impl FnOnce(&T) -> Exit,
) -> Result<Exit, Failure> {
    say_notices(&outcome.notices, PROG, cx.format, cx.catalog, err);
    let value = outcome.result.map_err(failed)?;
    let mut noted = Vec::new();
    if cx.format == Format::Json {
        let parent = cx.names.parent()?;
        print(out, &message(&parent, &value)).map_err(output_failure)?;
    } else {
        text(&value, out, &mut noted).map_err(output_failure)?;
    }
    let said: Vec<Notice> = noted.into_iter().map(Notice::text).collect();
    say_notices(&said, PROG, cx.format, cx.catalog, err);
    Ok(exit(&value))
}

#[cfg(test)]
#[path = "api.test.rs"]
pub(crate) mod tests;
