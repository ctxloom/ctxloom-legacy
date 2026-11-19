//! A primary's hooks deliver its pending messages (the terminal wake, owner
//! ruling 2026-11-18), in ONE daemon call (the lean hook RPC): the prompt's
//! informs and wakes; a tool call's delivers none (a primary takes no
//! steer); a stale wake line is blocked, a human's prompt never.

use std::sync::{Arc, Mutex};

use crate::fakes::{world, Asked, Deliveries, Inbox, CONFIG};
use crate::HookMessage;

const HARP: &str = "amber-quiet-heron";
const NONCE: &str = "0123456789abcdef";

fn message(id: &str, urgency: &str, text: &str) -> HookMessage {
    HookMessage {
        id: id.to_string(),
        urgency: urgency.to_string(),
        text: text.to_string(),
        sender: String::new(),
    }
}

/// The setup, its hooks reaching `pending` (`None`: no daemon answered),
/// each delivery recorded unless `refusing`.
fn hooked(pending: Option<Vec<HookMessage>>, refusing: bool) -> (crate::Setup, Deliveries) {
    let (setup, delivered, _) = asking(pending, refusing);
    (setup, delivered)
}

/// [`hooked`], with each call a hook made.
fn asking(pending: Option<Vec<HookMessage>>, refusing: bool) -> (crate::Setup, Deliveries, Asked) {
    let inbox = Inbox {
        pending: Mutex::new(pending),
        refusing,
        ..Inbox::default()
    };
    let delivered = Arc::clone(&inbox.delivered);
    let asked = Arc::clone(&inbox.asked);
    (
        world(CONFIG).setup.with_hook_contexts(Box::new(inbox)),
        delivered,
        asked,
    )
}

fn prompt(setup: &crate::Setup, text: &str) -> String {
    let payload = serde_json::json!({ "prompt": text }).to_string();
    setup
        .execute_hook("claude-code", "UserPromptSubmit", &payload, HARP)
        .unwrap()
        .stdout
}

fn mixed() -> Vec<HookMessage> {
    vec![
        message("m1", "inform", "the tests pass"),
        message("m2", "steer", "stop and rebase"),
        message("m3", "wake", "child kid finished its turn"),
    ]
}

// A human's prompt: its informs and wakes delivered, recorded with no wake;
// never blocked.
#[test]
fn a_prompt_delivers_the_informs_and_wakes_and_records_them() {
    let (setup, delivered) = hooked(Some(mixed()), false);
    let out = prompt(&setup, "go on");
    assert!(
        out.contains("- from the user (inform): the tests pass"),
        "{out}"
    );
    assert!(
        out.contains("- from the user (wake): child kid finished its turn"),
        "{out}"
    );
    assert!(
        !out.contains("stop and rebase"),
        "a steer is a tool call's: {out}"
    );
    assert!(!out.contains(";block="), "{out}");
    assert_eq!(
        *delivered.lock().unwrap(),
        [(vec!["m1".to_string(), "m3".to_string()], String::new())]
    );
}

// The wake line: what it woke the session for delivered, its nonce
// redeemed.
#[test]
fn a_wake_line_delivers_and_redeems_its_wake() {
    let (setup, delivered) = hooked(Some(mixed()), false);
    let out = prompt(&setup, &run_ports::wake_line(NONCE));
    assert!(out.contains("child kid finished its turn"), "{out}");
    assert!(!out.contains(";block="), "{out}");
    assert_eq!(
        *delivered.lock().unwrap(),
        [(vec!["m1".to_string(), "m3".to_string()], NONCE.to_string())]
    );
}

// A stale wake line (nothing waits for it): blocked, so no model call is
// spent on it; its nonce still redeemed (recorded with no message).
#[test]
fn a_stale_wake_line_is_blocked() {
    let (setup, delivered) = hooked(Some(vec![message("m2", "steer", "x")]), false);
    let out = prompt(&setup, &run_ports::wake_line(NONCE));
    assert!(out.contains(";block="), "{out}");
    assert!(out.starts_with("context=;"), "{out}");
    assert_eq!(
        *delivered.lock().unwrap(),
        [(Vec::new(), NONCE.to_string())]
    );
}

// A human's prompt is never blocked: with nothing waiting it is answered
// silence, and nothing is recorded.
#[test]
fn a_human_prompt_with_nothing_waiting_is_never_blocked() {
    let (setup, delivered) = hooked(Some(Vec::new()), false);
    let quoting = format!("why did I see {}?", run_ports::wake_line(NONCE));
    for text in ["go on", quoting.as_str()] {
        let out = prompt(&setup, text);
        assert_eq!(out, "context=;notice=\n", "{text}");
    }
    assert!(delivered.lock().unwrap().is_empty());
}

// What could not be recorded is not delivered (it waits, never given
// twice); a wake line then delivers nothing and is blocked.
#[test]
fn a_delivery_not_recorded_gives_nothing() {
    let (setup, _) = hooked(Some(mixed()), true);
    assert_eq!(prompt(&setup, "go on"), "context=;notice=\n");
    assert!(prompt(&setup, &run_ports::wake_line(NONCE)).contains(";block="));
}

// No daemon answered: a human's prompt goes on as ever; a wake line, which
// carries nothing for the model, is blocked.
#[test]
fn with_no_daemon_a_prompt_goes_on_and_a_wake_line_is_blocked() {
    let (setup, delivered) = hooked(None, false);
    assert_eq!(prompt(&setup, "go on"), "context=;notice=\n");
    assert!(prompt(&setup, &run_ports::wake_line(NONCE)).contains(";block="));
    assert!(delivered.lock().unwrap().is_empty());
}

// A tool call delivers no message: it would deliver a steer alone, and a
// primary is offered none (owner ruling 2026-11-18; a child's are fed as
// turns). Its one call asks for no urgency (it carries the owed context
// alone); the skill mates line is answered as ever. A prompt asks for its
// informs and wakes, in one call too.
#[test]
fn a_tool_call_asks_for_no_messages_and_delivers_none() {
    let (setup, delivered, asked) = asking(Some(mixed()), false);
    let out = setup
        .execute_hook("claude-code", "PostToolUse", r#"{"skill":"admit"}"#, HARP)
        .unwrap()
        .stdout;
    assert!(
        out.contains("Skill admit completed"),
        "the mates line: {out}"
    );
    assert!(!out.contains("stop and rebase"), "{out}");
    assert!(!out.contains("the tests pass"), "{out}");
    assert!(delivered.lock().unwrap().is_empty());
    prompt(&setup, "go on");
    let asked = asked.lock().unwrap();
    let calls: Vec<(&str, Vec<String>)> = asked
        .iter()
        .map(|(hook, urgencies, _)| (hook.as_str(), urgencies.clone()))
        .collect();
    assert_eq!(
        calls,
        [
            ("PostToolUse", Vec::new()),
            (
                "UserPromptSubmit",
                vec!["inform".to_string(), "wake".to_string()]
            ),
        ],
        "one call per hook"
    );
}
