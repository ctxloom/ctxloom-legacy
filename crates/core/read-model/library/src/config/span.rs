//! A span of time as a person writes it in the config or on the command
//! line: ctxloom-lib's one grammar (`duration-text`: `5m`, `1h30m`, `1.5h`,
//! `0s`; units `s m h d w`, whole seconds), and the word `now`, which is no
//! time at all (owner, 2026-11-15). `daemon.drain_timeout` and `ctxloom
//! shutdown <duration>` read it.

use std::time::{Duration, UNIX_EPOCH};

pub use duration_text::DurationError;

/// The word that is a span of no time.
pub const NOW: &str = "now";

/// `text` as a span: `now` is zero, anything else is read by
/// `duration-text`'s grammar (whitespace around it ignored, as there).
pub fn parse(text: &str) -> Result<Duration, DurationError> {
    if text.trim() == NOW {
        return Ok(Duration::ZERO);
    }
    // A span reads no clock: the instant is never consulted by `parse`.
    duration_text::DurationText::new(UNIX_EPOCH).parse(text)
}

#[cfg(test)]
#[path = "span.test.rs"]
mod tests;
