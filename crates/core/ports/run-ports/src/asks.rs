//! An engine's permission asks, ROUTED to the human (step 8 U7): an engine
//! that asks over its own protocol (opencode's ACP
//! `session/request_permission`, a hosted child's or a one-shot's) hands
//! each ask to a [`PermissionAsks`], which records it as an approval ask
//! and waits for the user's answer, then answers with the option she chose.
//!
//! The options are the ENGINE'S OWN (owner, 2026-11-16): each is relayed
//! back by its own id ([`AskOption::option_id`], ACP's `optionId`), so an
//! `allow-always` is opencode's to keep for its session, and nothing is
//! written (Q4). An ask nobody answers ends [`PermissionAnswer::Cancelled`]
//! (its expiry, or no answer at all), and an ask its turn no longer waits
//! for (ctxloom cancelled the turn: a steer, an interrupt, the child's end)
//! is WITHDRAWN (Q10): answered cancelled by the engine's adapter, and
//! [`PermissionAsks::withdraw`] told so.
//!
//! Synchronous, as the rest of the run ports: [`PermissionAsks::ask`] blocks
//! its caller until the ask is settled, so an adapter calls it off its
//! protocol's own loop.

use std::sync::Arc;
use std::time::Duration;

/// What an option of an ask does, as the engine's protocol kinds it (ACP's
/// `PermissionOptionKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

/// One option an engine offers with its ask, as the engine names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskOption {
    /// The engine's own id of it (ACP's `optionId`), answered back verbatim.
    pub option_id: String,
    /// What the engine calls it (ACP's `name`), as the human is shown it.
    pub name: String,
    pub kind: AskKind,
}

/// An engine's ask: the request it is known by, the tool it asks to run,
/// with what, and the options it offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionAsk {
    /// The engine's own id of the ask (ACP's tool call id): one ask however
    /// often it is delivered.
    pub request_id: String,
    pub tool_name: String,
    /// What the tool is to run with, as JSON text (`{}` when the engine
    /// said nothing).
    pub input_json: String,
    pub options: Vec<AskOption>,
}

/// What became of an ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionAnswer {
    /// The human chose the engine's option of this id.
    Selected(String),
    /// No option was chosen: the ask expired, was withdrawn, or could not
    /// be asked at all.
    Cancelled,
}

/// Where an engine's asks go: recorded, then waited on until the human
/// answers, the ask expires or it is withdrawn.
pub trait PermissionAsks: Send + Sync {
    /// Record `ask` and wait until it is settled: the option the human
    /// chose, else cancelled. Blocks its caller.
    fn ask(&self, ask: &PermissionAsk) -> PermissionAnswer;

    /// The ask `request_id` is no longer waited for (its turn was cancelled,
    /// `reason` says how): it is withdrawn, so it is listed no more and an
    /// answer to it is refused. Blocks its caller until it is recorded.
    fn withdraw(&self, request_id: &str, reason: &str);
}

/// The session a routed ask is asked from, and what it is recorded with:
/// its run, its engine, the agent it was planned for and its engine's
/// `approval_timeout` (read with its plan at its launch, T21).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Asker {
    /// The session's stable id.
    pub session: String,
    /// Its run's root, as text.
    pub run: String,
    pub engine: String,
    pub agent: String,
    pub approval_timeout: Duration,
}

/// What routes the asks of a launch's engine: the [`PermissionAsks`] of the
/// session `asker` describes.
pub trait PermissionRouter: Send + Sync {
    fn asks(&self, asker: &Asker) -> Arc<dyn PermissionAsks>;
}

#[cfg(test)]
#[path = "asks.test.rs"]
mod tests;
