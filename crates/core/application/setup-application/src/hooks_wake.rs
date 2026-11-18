//! A primary's hooks and its pending messages (the terminal wake, owner
//! ruling 2026-11-18). The session's pending messages are the truth; a wake
//! only starts the turn whose prompt's hook delivers them:
//!
//! - `turn_start` (claude's UserPromptSubmit) delivers its informs and
//!   wakes; a prompt that IS a wake line (`run_ports::wake_nonce`) redeems
//!   its wake, and is blocked when it delivered nothing (a stale wake: no
//!   model call spent on it); a human's prompt is never blocked;
//! - `post_tool` (PostToolUse) delivers its steers alone, mid-turn.
//!
//! What is delivered is what was recorded delivered first
//! ([`HookMessages::delivered`]): what could not be recorded waits, never
//! given twice. The messages are the first part of a hook's one budget
//! (`crate::delivery`).

use crate::delivery::{messages_context, HookMessage};
use crate::Setup;

/// Why a stale wake line was blocked. DRAFT (2026-11-18).
pub(crate) const STALE_WAKE: &str =
    "ctxloom: a wake with no message waiting for it; nothing was sent to the model";

/// The urgencies a prompt delivers.
const AT_A_PROMPT: &[&str] = &["inform", "wake"];

/// The urgencies a tool call delivers.
const MID_TURN: &[&str] = &["steer"];

/// A primary's pending messages as its hooks reach them: THROUGH THE
/// DAEMON, within the hook's bound, as the library reads are.
pub trait HookMessages {
    /// The hook's session's pending messages, in the order sent, when it is
    /// a primary; none for a child (the daemon feeds its messages as
    /// turns), and when no daemon answered in time.
    fn pending(&self) -> Option<Vec<HookMessage>>;

    /// `ids` delivered, and the wake `wake` (empty: none) its prompt was,
    /// recorded (`DeliverMessages`); whether it was.
    fn delivered(&self, ids: &[String], wake: &str) -> bool;
}

impl Setup {
    /// The setup, its hooks delivering a primary's messages through
    /// `messages`.
    #[must_use]
    pub fn with_hook_messages(mut self, messages: Box<dyn HookMessages>) -> Setup {
        self.hook_messages = Some(messages);
        self
    }

    /// What the messages of `urgencies` give the session `harp`, within
    /// `budget`, once recorded with the wake `wake`: their context (empty
    /// when nothing was recorded).
    fn deliver_messages(
        &self,
        urgencies: &[&str],
        wake: &str,
        harp: &str,
        budget: usize,
    ) -> String {
        let Some(inbox) = self.hook_messages.as_deref() else {
            return String::new();
        };
        // No daemon in time: nothing read, nothing recorded.
        let Some(pending) = inbox.pending() else {
            return String::new();
        };
        let delivery = messages_context(&pending, urgencies, harp, budget);
        let something = !delivery.ids.is_empty() || !wake.is_empty();
        if !something || !inbox.delivered(&delivery.ids, wake) {
            return String::new();
        }
        delivery.context
    }

    /// A prompt's delivery: its informs and wakes' context, and whether the
    /// prompt is to be blocked (a wake line that delivered nothing).
    pub(crate) fn prompt_delivery(
        &self,
        prompt: &str,
        harp: &str,
        budget: usize,
    ) -> (String, bool) {
        let wake = run_ports::wake_nonce(prompt);
        let context = self.deliver_messages(AT_A_PROMPT, wake.unwrap_or_default(), harp, budget);
        let block = wake.is_some() && context.is_empty();
        (context, block)
    }

    /// A tool call's delivery: its steers' context.
    pub(crate) fn steer_delivery(&self, harp: &str, budget: usize) -> String {
        self.deliver_messages(MID_TURN, "", harp, budget)
    }
}

#[cfg(test)]
#[path = "hooks_wake.test.rs"]
mod tests;
