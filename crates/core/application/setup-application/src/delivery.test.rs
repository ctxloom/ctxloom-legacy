//! A primary's hooks' delivery of its pending messages (the terminal wake).

use super::*;

fn message(id: &str, urgency: &str, text: &str, sender: &str) -> HookMessage {
    HookMessage {
        id: id.to_string(),
        urgency: urgency.to_string(),
        text: text.to_string(),
        sender: sender.to_string(),
    }
}

#[test]
fn the_messages_of_the_urgencies_asked_are_delivered_in_order_with_their_sender() {
    let pending = [
        message("m1", "inform", "the tests pass", ""),
        message("m2", "steer", "stop and rebase", ""),
        message(
            "m3",
            "wake",
            "child kid finished its turn",
            "projects/p/sessions/kid",
        ),
    ];
    let delivery = messages_context(
        &pending,
        &["inform", "wake"],
        "amber-quiet-heron",
        HOOK_CONTEXT_BUDGET,
    );
    assert_eq!(delivery.ids, ["m1", "m3"]);
    assert_eq!(
        delivery.context,
        "ctxloom: messages sent to this session (each delivered once):\n\
         - from the user (inform): the tests pass\n\
         - from projects/p/sessions/kid (wake): child kid finished its turn"
    );
    let steers = messages_context(
        &pending,
        &["steer"],
        "amber-quiet-heron",
        HOOK_CONTEXT_BUDGET,
    );
    assert_eq!(steers.ids, ["m2"]);
    assert!(steers
        .context
        .ends_with("- from the user (steer): stop and rebase"));
}

#[test]
fn nothing_of_the_urgencies_asked_delivers_nothing() {
    let pending = [message("m1", "steer", "x", "")];
    assert_eq!(
        messages_context(&pending, &["inform", "wake"], "h", HOOK_CONTEXT_BUDGET),
        Delivery::default()
    );
}

// The budget left: what fits whole is delivered, the rest waits, and the
// pointer says how many and where.
#[test]
fn the_messages_past_the_cap_wait_behind_a_pointer_to_session_show() {
    let long = "x".repeat(3000);
    let pending: Vec<HookMessage> = (1..=4)
        .map(|n| message(&format!("m{n}"), "inform", &long, ""))
        .collect();
    let delivery = messages_context(&pending, &["inform"], "amber-quiet-heron", 7000);
    assert_eq!(delivery.ids, ["m1", "m2"]);
    assert!(delivery.context.chars().count() <= 7000);
    assert!(
        delivery.context.ends_with(
            "ctxloom: 2 more waiting; `ctxloom session show amber-quiet-heron` lists them"
        ),
        "{}",
        delivery.context
    );
}

// Exactly the budget fits; one character over does not.
#[test]
fn a_delivery_of_exactly_the_budget_fits_whole() {
    let header = "ctxloom: messages sent to this session (each delivered once):\n";
    let framing = header.chars().count() + "- from the user (inform): ".chars().count();
    let budget = 5000;
    let fits = message("m1", "inform", &"y".repeat(budget - framing), "");
    let delivery = messages_context(std::slice::from_ref(&fits), &["inform"], "h", budget);
    assert_eq!(delivery.context.chars().count(), budget);
    assert!(!delivery.context.contains("(cut:"));
    let over = message("m1", "inform", &"y".repeat(budget - framing + 1), "");
    let cut = messages_context(&[over], &["inform"], "h", budget);
    assert!(cut
        .context
        .contains("(cut: a hook gives at most 10000 characters)"));
    assert!(cut.context.chars().count() <= budget);
}

// A first message too long alone is delivered cut, never left to be
// delivered (and cut) at every prompt; the rest wait.
#[test]
fn a_first_message_too_long_alone_is_delivered_cut() {
    let pending = [
        message("m1", "wake", &"z".repeat(12_000), ""),
        message("m2", "wake", "short", ""),
    ];
    let delivery = messages_context(
        &pending,
        &["wake"],
        "amber-quiet-heron",
        HOOK_CONTEXT_BUDGET,
    );
    assert_eq!(delivery.ids, ["m1"]);
    assert!(delivery.context.chars().count() <= HOOK_CONTEXT_BUDGET);
    assert!(delivery
        .context
        .contains("zzz ... (cut: a hook gives at most 10000 characters)"));
    assert!(delivery
        .context
        .ends_with("ctxloom: 1 more waiting; `ctxloom session show amber-quiet-heron` lists them"));
}

// The one join point: each part that says something, in order.
#[test]
fn a_hooks_context_joins_the_parts_that_say_something() {
    assert_eq!(compose_context(&["a", "", "b"]), "a\n\nb");
    assert_eq!(compose_context(&["", ""]), "");
    assert_eq!(compose_context(&["only"]), "only");
}

// ONE budget per hook answer: the messages first, then the re-delivered
// context (U13), which is cut to what is left and marked.
#[test]
fn a_hooks_parts_share_one_budget_the_later_cut_to_what_is_left() {
    let messages = "m".repeat(6000);
    let context = "c".repeat(6000);
    let joined = compose_context(&[&messages, &context]);
    assert_eq!(joined.chars().count(), HOOK_CONTEXT_BUDGET);
    assert!(joined.starts_with(&messages), "the messages whole");
    assert!(joined.ends_with("(cut: a hook gives at most 10000 characters)"));
    assert_eq!(remaining(&[&messages]), HOOK_CONTEXT_BUDGET - 6000 - 2);
    assert_eq!(remaining(&[]), HOOK_CONTEXT_BUDGET);
    let small = "s".repeat(100);
    assert_eq!(
        compose_context(&[&small, &small]).chars().count(),
        202,
        "nothing cut"
    );
}
