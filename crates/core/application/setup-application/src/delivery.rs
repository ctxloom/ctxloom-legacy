//! What a primary's hooks deliver of its pending messages (the terminal
//! wake, owner ruling 2026-11-18): the prompt's hook (`turn_start`) its
//! informs and wakes, a tool call's (`post_tool`) its steers, each as the
//! engine's model-visible context. One hook answer has ONE budget,
//! [`HOOK_CONTEXT_BUDGET`] characters (claude caps a hook's
//! `additionalContext` at 10,000, 2.1.325; owner ruling 2026-11-18,
//! replacing Go's 7500), shared by every part: the messages first, then
//! the re-delivered context (step 10c's U13). The messages are given what
//! budget is left ([`messages_context`]'s `budget`): those that fit whole
//! are delivered, the rest wait, with a pointer to `session show`; a first
//! message too long alone is delivered cut. A hook's context is joined in
//! ONE place, [`compose_context`], over the one budget, each part one
//! argument of it, in order.
//!
//! The words are DRAFT text (2026-11-18), for the pre-release review.

/// The most characters one hook answer gives the engine, every part of it.
pub const HOOK_CONTEXT_BUDGET: usize = 10_000;

/// One message a primary's hook may deliver.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct HookMessage {
    pub id: String,
    /// Its urgency's word: `inform`, `wake` or `steer`.
    pub urgency: String,
    pub text: String,
    /// The sender's resource name; empty: the user.
    pub sender: String,
}

/// What a hook delivers: the context it gives, and the ids it gave.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Delivery {
    pub context: String,
    pub ids: Vec<String>,
}

/// The first line of a delivery. DRAFT.
const HEADER: &str = "ctxloom: messages sent to this session (each delivered once):";

/// One message's line. DRAFT.
fn line(message: &HookMessage) -> String {
    let from = match message.sender.as_str() {
        "" => "the user",
        sender => sender,
    };
    format!("- from {from} ({}): {}", message.urgency, message.text)
}

/// The pointer to the messages that wait, `left` of them. DRAFT.
fn pointer(left: usize, harp: &str) -> String {
    format!("ctxloom: {left} more waiting; `ctxloom session show {harp}` lists them")
}

/// What marks a part cut to the hook's budget. DRAFT.
const CUT: &str = " ... (cut: a hook gives at most 10000 characters)";

fn chars(text: &str) -> usize {
    text.chars().count()
}

/// The delivery of the messages of `urgencies` among `pending` (in the
/// order sent) for the session `harp`, within `budget` characters (what
/// is left of [`HOOK_CONTEXT_BUDGET`]).
pub fn messages_context(
    pending: &[HookMessage],
    urgencies: &[&str],
    harp: &str,
    budget: usize,
) -> Delivery {
    let chosen: Vec<&HookMessage> = pending
        .iter()
        .filter(|message| urgencies.contains(&message.urgency.as_str()))
        .collect();
    if chosen.is_empty() {
        return Delivery::default();
    }
    let mut lines = vec![HEADER.to_string()];
    let mut ids = Vec::new();
    for (n, message) in chosen.iter().enumerate() {
        let left = chosen.len() - n - 1;
        let room = |lines: &[String], extra: &str, more: usize| {
            let tail = if more > 0 {
                chars(&pointer(more, harp)) + 1
            } else {
                0
            };
            lines.iter().map(|l| chars(l) + 1).sum::<usize>() + chars(extra) + tail
        };
        let whole = line(message);
        if room(&lines, &whole, left) <= budget {
            lines.push(whole);
            ids.push(message.id.clone());
            continue;
        }
        if ids.is_empty() {
            // The first alone is too long: delivered cut.
            let fits = budget
                .saturating_sub(room(&lines, CUT, left))
                .saturating_sub(1);
            let cut: String = whole.chars().take(fits).collect();
            lines.push(cut + CUT);
            ids.push(message.id.clone());
            if left > 0 {
                lines.push(pointer(left, harp));
            }
            break;
        }
        lines.push(pointer(chosen.len() - n, harp));
        break;
    }
    Delivery {
        context: lines.join("\n"),
        ids,
    }
}

/// The separator between two parts.
const BETWEEN: &str = "\n\n";

/// What is left of [`HOOK_CONTEXT_BUDGET`] after `parts` are joined: what
/// the next part may take (the messages, given the skill mates line).
pub fn remaining(parts: &[&str]) -> usize {
    let used = chars(&compose_context(parts));
    let between = if used > 0 { chars(BETWEEN) } else { 0 };
    HOOK_CONTEXT_BUDGET.saturating_sub(used + between)
}

/// A hook's model-visible context: each part that says something, in
/// order, a blank line between, within [`HOOK_CONTEXT_BUDGET`]: a part past
/// it is cut to what is left (marked), and the parts after it are left out.
/// THE join point of a hook's context: the skill mates line, the messages,
/// then the re-delivered context (U13), each one argument.
pub fn compose_context(parts: &[&str]) -> String {
    let mut joined = String::new();
    for part in parts.iter().filter(|part| !part.is_empty()) {
        let between = if joined.is_empty() { "" } else { BETWEEN };
        let used = chars(&joined) + chars(between);
        if used + chars(part) <= HOOK_CONTEXT_BUDGET {
            joined.push_str(between);
            joined.push_str(part);
            continue;
        }
        let fits = HOOK_CONTEXT_BUDGET.saturating_sub(used + chars(CUT));
        if fits > 0 {
            joined.push_str(between);
            joined.extend(part.chars().take(fits));
            joined.push_str(CUT);
        }
        break;
    }
    joined
}

#[cfg(test)]
#[path = "delivery.test.rs"]
mod tests;
