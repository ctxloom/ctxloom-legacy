//! The lean hook RPC (10c, owner budget 2026-11-18): one call, answered from
//! the projections with no round awaited, and a message delivered ONCE
//! however many hooks ask within one round.

use angzarr_embedded::v1::Cover;
use daemon_proto::v1;
use run_ports::Edge;

use crate::server::tests::{fixture, Fixture};

const HARP: &str = "amber-quiet-heron";

fn name() -> String {
    format!("projects/calm-quiet-heron/sessions/{HARP}")
}

/// `text` sent to the session as the inform `id`.
async fn send(f: &Fixture, id: &str, text: &str) {
    let root = sessions::SessionName::parse(&name()).unwrap().root();
    let cover = Cover {
        domain: sessions::SESSION_DOMAIN.to_string(),
        root: Some(root),
        edition: None,
    };
    let send = sessions::SendMessage {
        message_id: id.to_string(),
        urgency: sessions::Urgency::Inform,
        text: text.to_string(),
        requester: None,
        supported: vec![sessions::Urgency::Inform, sessions::Urgency::Wake],
    };
    let book = sessions::keyed_book(cover, &send, &format!("send:{id}"));
    f.world.handle(book).await.unwrap();
}

/// A primary `HARP` holding the inform `m1`, every projection caught up.
async fn holding_one() -> Fixture {
    let f = fixture(vec![]);
    f.mint(HARP).await;
    send(&f, "m1", "the first words").await;
    f.catch_up().await;
    f.catch_up_pending().await;
    f
}

/// A prompt's hook: its informs and wakes, the prompt the wake `nonce`
/// (empty: a human's).
fn at_a_prompt(nonce: &str) -> v1::DeliverHookContextRequest {
    v1::DeliverHookContextRequest {
        name: name(),
        carried_by: "UserPromptSubmit".to_string(),
        urgencies: vec!["inform".to_string(), "wake".to_string()],
        wake_nonce: nonce.to_string(),
        ..v1::DeliverHookContextRequest::default()
    }
}

// The race, forced: two prompts' hooks within one round (no catch-up of
// the pending projection between them, run at once), both reading `m1`
// pending. The first delivers it; the same hook again is a DUPLICATE, and
// a hook whose delivery is keyed apart (a wake line's) records nothing of
// it in the aggregate: neither gives it again.
#[tokio::test]
async fn test_two_hooks_within_one_round_deliver_a_message_once() {
    let f = holding_one().await;
    let (first, again) = tokio::join!(
        f.api.deliver_hook_context(at_a_prompt("")),
        f.api.deliver_hook_context(at_a_prompt("")),
    );
    let (first, again) = (first.unwrap(), again.unwrap());
    let (given, other) = match first.delivered.is_empty() {
        false => (first, again),
        true => (again, first),
    };
    assert_eq!(given.delivered, ["m1"]);
    assert!(given.context.contains("the first words"), "{given:?}");
    assert!(other.delivered.is_empty(), "{other:?}");
    assert!(other.context.is_empty(), "{other:?}");
    let keyed_apart = f
        .api
        .deliver_hook_context(at_a_prompt("0123456789abcdef"))
        .await
        .unwrap();
    assert!(keyed_apart.delivered.is_empty(), "{keyed_apart:?}");
    assert!(
        !keyed_apart.context.contains("the first words"),
        "{keyed_apart:?}"
    );
    assert_eq!(*f.hosting.freshened.lock().unwrap(), 0, "no round awaited");
}

// A tool call's hook delivers no message (a primary is offered no steer),
// and leaves it for the next prompt's.
#[tokio::test]
async fn test_a_tool_calls_hook_delivers_no_message_and_leaves_it_pending() {
    let f = holding_one().await;
    let tool = f
        .api
        .deliver_hook_context(v1::DeliverHookContextRequest {
            name: name(),
            carried_by: "PostToolUse".to_string(),
            ..v1::DeliverHookContextRequest::default()
        })
        .await
        .unwrap();
    assert!(
        tool.context.is_empty() && tool.delivered.is_empty(),
        "{tool:?}"
    );
    let prompt = f.api.deliver_hook_context(at_a_prompt("")).await.unwrap();
    assert_eq!(prompt.delivered, ["m1"]);
}

// No round is awaited: the index as it stands answers, so a session the
// projection does not hold yet is refused, never waited for.
#[tokio::test]
async fn test_a_session_the_index_does_not_hold_is_refused_without_a_round() {
    let f = fixture(vec![]);
    f.mint(HARP).await;
    let refused = f.api.deliver_hook_context(at_a_prompt("")).await;
    assert!(
        matches!(refused, Err(crate::ApiError::Refused(_))),
        "{refused:?}"
    );
    assert_eq!(*f.hosting.freshened.lock().unwrap(), 0, "no round awaited");
}
