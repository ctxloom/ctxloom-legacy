//! The hook's hold policy (s4-Q14, H2): each row of the design's table
//! (hook-hold.design.md §1, H-Q1, H-Q2 and H-Q5 revised), over a fake sink,
//! a fake clock and an in-memory hold. A race is forced, never waited for:
//! the second hook runs INSIDE the first one's delivery.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use angzarr_embedded::v1::{command_page, CommandBook, Uuid};
use daemon_proto::google::protobuf::Timestamp;
use run_ports::{
    Batch, Bound, Delivery, FactSink, Held, HeldEntry, HeldFact, HeldFacts, HeldGap, HoldError,
    HoldSummary, Settle, Settled,
};
use surface_ports::{HookEvent, MessageDisplay};

use super::*;
use crate::fakes::{uuid, SESSION_ME, SESSION_OTHER};
use crate::observe;

const OWN: &str = "v0.2.0-aaaaaaa";
const OLDER: &str = "v0.1.0-bbbbbbb";
const NEWER: &str = "v0.3.0-ccccccc";
const NAME: &str = "projects/amber-quiet-heron/sessions/brisk-copper-moth";

fn session() -> Uuid {
    uuid(SESSION_ME)
}

fn root() -> uuid::Uuid {
    uuid::Uuid::parse_str(SESSION_ME).unwrap()
}

fn prompt(id: &str) -> HookEvent {
    HookEvent {
        event: "turn_start".to_string(),
        prompt_id: id.to_string(),
        prompt: format!("prompt {id}"),
        ..HookEvent::default()
    }
}

/// A MessageDisplay piece: one observation (`n` is 1, its only piece).
fn pieces(n: u32) -> HookEvent {
    HookEvent {
        event: "message_display".to_string(),
        prompt_id: "p1".to_string(),
        message: Some(MessageDisplay {
            turn_id: "t1".to_string(),
            message_id: "m1".to_string(),
            index: 0,
            is_final: n == 1,
            delta: "words".to_string(),
        }),
        ..HookEvent::default()
    }
}

fn turn_end(id: &str) -> HookEvent {
    HookEvent {
        event: "turn_end".to_string(),
        prompt_id: id.to_string(),
        ..HookEvent::default()
    }
}

/// The external ids `event`'s observations have, in order.
fn ids_of(event: &HookEvent) -> Vec<String> {
    observe(event)
        .iter()
        .map(|o| o.external_id(&session()))
        .collect()
}

/// A held record of `event`'s first observation, held by a hook of `stamp`.
fn held_of(event: &HookEvent, stamp: &str) -> HeldEntry {
    let observation = &observe(event)[0];
    HeldEntry::Fact(HeldFact::of_book(
        stamp,
        NAME,
        observation.external_id(&session()),
        &observation.book(&session()),
        None,
    ))
}

fn ids_held(memory: &Memory) -> Vec<String> {
    memory
        .entries()
        .iter()
        .map(|e| match e {
            HeldEntry::Fact(f) => f.external_id.clone(),
            HeldEntry::Gap(g) => format!("gap:{}", g.external_id),
        })
        .collect()
}

// ------------------------------------------------------------------ fakes

#[derive(Default)]
struct Inner {
    entries: Vec<HeldEntry>,
    /// A take's flush lock is held.
    taking: bool,
    /// What `hold` answers instead of keeping.
    refuse: Option<Result<Held, HoldError>>,
    /// Calls of the port, by name.
    calls: Vec<&'static str>,
}

/// One session's hold in memory, with the adapter's take and settle
/// semantics: a take holds the flush lock until settled or dropped, and a
/// settle keeps what was appended since the snapshot.
#[derive(Default, Clone)]
struct Memory(Arc<Mutex<Inner>>);

impl Memory {
    fn holding(entries: Vec<HeldEntry>) -> Memory {
        let memory = Memory::default();
        memory.0.lock().unwrap().entries = entries;
        memory
    }

    fn entries(&self) -> Vec<HeldEntry> {
        self.0.lock().unwrap().entries.clone()
    }

    fn calls(&self) -> Vec<&'static str> {
        self.0.lock().unwrap().calls.clone()
    }

    fn refusing(&self, answer: Result<Held, HoldError>) {
        self.0.lock().unwrap().refuse = Some(answer);
    }
}

impl HeldFacts for Memory {
    fn hold(&self, session: &uuid::Uuid, fact: HeldFact) -> Result<Held, HoldError> {
        assert_eq!(*session, root(), "the hold is the session's");
        let mut inner = self.0.lock().unwrap();
        inner.calls.push("hold");
        if let Some(answer) = inner.refuse.clone() {
            return answer;
        }
        inner.entries.push(HeldEntry::Fact(fact));
        Ok(Held::Kept)
    }

    fn is_empty(&self, _: &uuid::Uuid) -> Result<bool, HoldError> {
        let mut inner = self.0.lock().unwrap();
        inner.calls.push("is_empty");
        Ok(inner.entries.is_empty())
    }

    fn sessions(&self) -> Result<Vec<uuid::Uuid>, HoldError> {
        Ok(vec![root()])
    }

    fn take(&self, _: &uuid::Uuid) -> Result<Option<Batch>, HoldError> {
        let mut inner = self.0.lock().unwrap();
        inner.calls.push("take");
        if inner.taking {
            return Ok(None);
        }
        inner.taking = true;
        let snapshot = inner.entries.clone();
        let len = snapshot.len();
        Ok(Some(Batch::new(
            snapshot,
            Box::new(Settling {
                memory: self.clone(),
                len,
            }),
        )))
    }

    fn discard(&self, _: &uuid::Uuid) -> Result<u64, HoldError> {
        Ok(0)
    }

    fn survey(&self) -> Result<Vec<HoldSummary>, HoldError> {
        Ok(Vec::new())
    }
}

struct Settling {
    memory: Memory,
    len: usize,
}

impl Settle for Settling {
    fn settle(self: Box<Self>, keep: &[bool]) -> Result<Settled, HoldError> {
        let mut inner = self.memory.0.lock().unwrap();
        let since = inner.entries.split_off(self.len);
        let kept: Vec<HeldEntry> = std::mem::take(&mut inner.entries)
            .into_iter()
            .zip(keep)
            .filter(|(_, k)| **k)
            .map(|(e, _)| e)
            .collect();
        let count = kept.len();
        inner.entries = kept;
        inner.entries.extend(since);
        Ok(if inner.entries.is_empty() {
            Settled::Emptied
        } else {
            Settled::Cut { kept: count }
        })
    }
}

impl Drop for Settling {
    fn drop(&mut self) {
        self.memory.0.lock().unwrap().taking = false;
    }
}

/// The hook's time: what is left, spent by each delivery the sink takes.
struct Clock(RefCell<Duration>);

impl Clock {
    fn with(left: Duration) -> Clock {
        Clock(RefCell::new(left))
    }

    fn spend(&self, spent: Duration) {
        let left = *self.0.borrow();
        *self.0.borrow_mut() = left.saturating_sub(spent);
    }
}

impl HookClock for Clock {
    fn remaining(&self) -> Duration {
        *self.0.borrow()
    }

    fn now(&self) -> Option<Timestamp> {
        Some(Timestamp {
            seconds: 1_700_000_000,
            nanos: 0,
        })
    }
}

/// A sink that records each book's external id and answers from a script
/// (`Applied` once it runs out), spending `cost` of the clock each time,
/// and runs `during` (once) inside its first delivery.
struct Sink<'c> {
    clock: Option<&'c Clock>,
    cost: Duration,
    answers: RefCell<VecDeque<Delivery>>,
    delivered: RefCell<Vec<String>>,
    within: RefCell<Vec<Duration>>,
    during: RefCell<Option<Box<dyn FnOnce() + 'c>>>,
}

impl<'c> Sink<'c> {
    fn new() -> Sink<'c> {
        Sink {
            clock: None,
            cost: Duration::ZERO,
            answers: RefCell::new(VecDeque::new()),
            delivered: RefCell::new(Vec::new()),
            within: RefCell::new(Vec::new()),
            during: RefCell::new(None),
        }
    }

    fn spending(clock: &'c Clock, cost: Duration) -> Sink<'c> {
        Sink {
            clock: Some(clock),
            cost,
            ..Sink::new()
        }
    }

    fn answering(self, answers: impl IntoIterator<Item = Delivery>) -> Sink<'c> {
        *self.answers.borrow_mut() = answers.into_iter().collect();
        self
    }

    fn delivered(&self) -> Vec<String> {
        self.delivered.borrow().clone()
    }
}

fn external_id(book: &CommandBook) -> String {
    match book.page.as_ref().and_then(|p| p.body.as_ref()) {
        Some(command_page::Body::Fact(fact)) => fact.origin.clone().unwrap().external_id,
        _ => panic!("a fact book"),
    }
}

impl FactSink for Sink<'_> {
    fn deliver(&self, book: CommandBook, within: Duration) -> Delivery {
        self.delivered.borrow_mut().push(external_id(&book));
        self.within.borrow_mut().push(within);
        if let Some(during) = self.during.borrow_mut().take() {
            during();
        }
        if let Some(clock) = self.clock {
            clock.spend(self.cost);
        }
        self.answers
            .borrow_mut()
            .pop_front()
            .unwrap_or(Delivery::Applied)
    }
}

fn daemon<'a, 'c>(stamp: &'a str, sink: &'a Sink<'c>) -> Serving<'a, Sink<'c>> {
    Serving::Daemon { stamp, sink }
}

/// A PostToolBatch of `n` calls: one observation each, in call order.
fn batch_of(n: usize) -> HookEvent {
    HookEvent {
        event: "post_tool_batch".to_string(),
        prompt_id: "p1".to_string(),
        tool_calls: (0..n)
            .map(|i| surface_ports::ToolCall {
                tool: "Bash".to_string(),
                tool_use_id: format!("t{i}"),
                input: "{}".to_string(),
                response: "ok".to_string(),
                ..surface_ports::ToolCall::default()
            })
            .collect(),
        ..HookEvent::default()
    }
}

fn outcomes(kept: &[Kept]) -> Vec<Outcome> {
    kept.iter().map(|k| k.outcome.clone()).collect()
}

fn long() -> Clock {
    Clock::with(Duration::from_secs(10))
}

// ------------------------------------------------------------- the table

// No daemon could be reached or started (H-Q2 (b)): every observation is
// held, in order, by the hook's stamp and the session's name, and nothing
// is delivered.
#[test]
fn a_hook_that_reaches_no_daemon_holds_every_fact_in_order() {
    let memory = Memory::default();
    let clock = long();
    let holding = Holding::new(&memory, &clock, OWN, NAME);
    let report = holding.record::<Sink>(
        &session(),
        &pieces(1),
        Serving::Unreached {
            why: "no daemon".to_string(),
        },
    );
    assert_eq!(
        outcomes(&report.own),
        vec![Outcome::Held(Why::Unreached {
            why: "no daemon".to_string()
        })]
    );
    assert_eq!(ids_held(&memory), ids_of(&pieces(1)));
    let HeldEntry::Fact(fact) = &memory.entries()[0] else {
        panic!("a fact")
    };
    assert_eq!(fact.stamp, OWN);
    assert_eq!(fact.session, NAME);
    assert_eq!(fact.held_at, clock.now());
    assert_eq!(
        fact.decode_book().unwrap(),
        observe(&pieces(1))[0].book(&session()),
        "the book exactly as the observation made it"
    );
    assert!(report.flushed.is_empty());
}

// A daemon of an older version (H-Q1 (b)): held, nothing delivered, the
// hold not even taken.
#[test]
fn a_hook_newer_than_its_daemon_holds_and_delivers_nothing() {
    let memory = Memory::holding(vec![held_of(&prompt("p0"), OWN)]);
    let clock = long();
    let sink = Sink::new();
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &prompt("p1"),
        daemon(OLDER, &sink),
    );
    assert_eq!(
        outcomes(&report.own),
        vec![Outcome::Held(Why::Older {
            daemon: OLDER.to_string(),
            own: OWN.to_string()
        })]
    );
    assert!(sink.delivered().is_empty());
    assert_eq!(
        ids_held(&memory),
        [ids_of(&prompt("p0")), ids_of(&prompt("p1"))].concat()
    );
    assert!(!memory.calls().contains(&"take"));
}

// A daemon up to date for the hook, its session holding nothing: the facts
// are delivered as before, and the hold is only looked at.
#[test]
fn a_hook_of_the_daemons_version_with_nothing_held_delivers_directly() {
    let memory = Memory::default();
    let clock = long();
    let sink = Sink::new();
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &prompt("p1"),
        daemon(OWN, &sink),
    );
    assert_eq!(
        outcomes(&report.own),
        vec![Outcome::Delivered(Delivery::Applied)]
    );
    assert_eq!(sink.delivered(), ids_of(&prompt("p1")));
    assert!(memory.entries().is_empty());
    assert_eq!(memory.calls(), vec!["is_empty"]);
    assert!(report.said().is_empty());
}

// A hook older than its daemon delivers (up to date is "not older").
#[test]
fn a_hook_older_than_its_daemon_delivers() {
    let memory = Memory::default();
    let clock = long();
    let sink = Sink::new();
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &prompt("p1"),
        daemon(NEWER, &sink),
    );
    assert_eq!(
        outcomes(&report.own),
        vec![Outcome::Delivered(Delivery::Applied)]
    );
    assert_eq!(sink.delivered(), ids_of(&prompt("p1")));
}

// H-Q5 revised: an up-to-date hook that finds its session's hold non-empty
// delivers it in held order, cuts it, then delivers its own facts.
#[test]
fn an_up_to_date_hook_flushes_its_sessions_hold_in_order_then_delivers_its_own() {
    let memory = Memory::holding(vec![
        held_of(&prompt("p1"), OLDER),
        held_of(&pieces(1), OWN),
        HeldEntry::Gap(HeldGap {
            stamp: OWN.to_string(),
            external_id: "dropped".to_string(),
            held_at: None,
            why: "bound".to_string(),
        }),
    ]);
    let clock = long();
    let sink = Sink::new();
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &turn_end("p1"),
        daemon(OWN, &sink),
    );
    assert_eq!(
        sink.delivered(),
        [
            ids_of(&prompt("p1")),
            ids_of(&pieces(1)),
            ids_of(&turn_end("p1"))
        ]
        .concat(),
        "held first, in order; the gap is not a fact"
    );
    assert!(memory.entries().is_empty(), "the hold is cut");
    assert_eq!(
        outcomes(&report.flushed),
        vec![
            Outcome::Delivered(Delivery::Applied),
            Outcome::Delivered(Delivery::Applied)
        ]
    );
    assert_eq!(
        outcomes(&report.own),
        vec![Outcome::Delivered(Delivery::Applied)]
    );
    assert!(report.said().is_empty());
}

// The hook's time runs out mid-flush: what was delivered is cut, the rest
// stays, and the hook's own fact is held behind it.
#[test]
fn a_budget_spent_mid_flush_cuts_what_was_delivered_and_holds_the_rest() {
    let memory = Memory::holding(vec![
        held_of(&prompt("p1"), OWN),
        held_of(&prompt("p2"), OWN),
        held_of(&prompt("p3"), OWN),
    ]);
    let clock = Clock::with(Duration::from_secs(2));
    let sink = Sink::spending(&clock, Duration::from_secs(1));
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &turn_end("p3"),
        daemon(OWN, &sink),
    );
    assert_eq!(
        sink.delivered(),
        [ids_of(&prompt("p1")), ids_of(&prompt("p2"))].concat()
    );
    assert_eq!(
        *sink.within.borrow(),
        vec![Duration::from_secs(2), Duration::from_secs(1)],
        "each delivery waits only what is left"
    );
    assert_eq!(
        ids_held(&memory),
        [ids_of(&prompt("p3")), ids_of(&turn_end("p3"))].concat(),
        "the undelivered one first, then the hook's own"
    );
    assert_eq!(outcomes(&report.own), vec![Outcome::HeldBehind]);
}

// A held fact's delivery left unsettled stops the flush: it and everything
// after it stay, and the hook's own fact goes behind.
#[test]
fn an_unsettled_held_delivery_keeps_it_and_holds_the_hook_behind() {
    let memory = Memory::holding(vec![
        held_of(&prompt("p1"), OWN),
        held_of(&prompt("p2"), OWN),
    ]);
    let clock = long();
    let sink = Sink::new().answering([Delivery::Applied, Delivery::TimedOut]);
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &turn_end("p2"),
        daemon(OWN, &sink),
    );
    assert_eq!(
        ids_held(&memory),
        [ids_of(&prompt("p2")), ids_of(&turn_end("p2"))].concat()
    );
    assert_eq!(outcomes(&report.own), vec![Outcome::HeldBehind]);
    assert_eq!(report.said().len(), 1, "the own fact held behind is said");
}

// A final answer to a held fact (refused, parked) is settled: it leaves the
// hold and is said, never retried.
#[test]
fn a_refused_held_fact_leaves_the_hold_and_is_said() {
    let memory = Memory::holding(vec![held_of(&prompt("p1"), OWN)]);
    let clock = long();
    let refused = Delivery::Refused {
        detail: "no".to_string(),
    };
    let sink = Sink::new().answering([refused.clone()]);
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &turn_end("p1"),
        daemon(OWN, &sink),
    );
    assert!(memory.entries().is_empty());
    assert_eq!(outcomes(&report.flushed), vec![Outcome::Delivered(refused)]);
    assert_eq!(
        outcomes(&report.own),
        vec![Outcome::Delivered(Delivery::Applied)]
    );
    assert_eq!(report.said().len(), 1);
}

// An own delivery left unsettled (the daemon died under the hook) is held,
// and every later one of the hook behind it (H-Q2).
#[test]
fn an_unsettled_own_delivery_is_held_and_the_rest_behind_it() {
    let event = batch_of(3);
    let memory = Memory::default();
    let clock = long();
    let unavailable = Delivery::Unavailable {
        detail: "gone".to_string(),
    };
    let sink = Sink::new().answering([Delivery::Applied, unavailable.clone()]);
    let report =
        Holding::new(&memory, &clock, OWN, NAME).record(&session(), &event, daemon(OWN, &sink));
    assert_eq!(
        outcomes(&report.own),
        vec![
            Outcome::Delivered(Delivery::Applied),
            Outcome::Held(Why::Unsettled(unavailable)),
            Outcome::HeldBehind
        ]
    );
    assert_eq!(sink.delivered().len(), 2, "nothing tried after it");
    assert_eq!(ids_held(&memory), ids_of(&event)[1..].to_vec());
}

// The hook's time already spent: its own fact is held, never sent.
#[test]
fn an_own_fact_with_no_time_left_is_held() {
    let memory = Memory::default();
    let clock = Clock::with(Duration::ZERO);
    let sink = Sink::new();
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &prompt("p1"),
        daemon(OWN, &sink),
    );
    assert!(sink.delivered().is_empty());
    assert_eq!(outcomes(&report.own), vec![Outcome::Held(Why::OutOfTime)]);
}

// Another process is taking the hold (its flush lock is held): the hook
// skips it, never waits, and holds its own fact behind.
#[test]
fn a_hold_another_process_is_taking_holds_the_hook_behind() {
    let memory = Memory::holding(vec![held_of(&prompt("p1"), OWN)]);
    let other = memory.take(&root()).unwrap().expect("the first take");
    let clock = long();
    let sink = Sink::new();
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &turn_end("p1"),
        daemon(OWN, &sink),
    );
    assert!(sink.delivered().is_empty());
    assert_eq!(outcomes(&report.own), vec![Outcome::HeldBehind]);
    drop(other);
    assert_eq!(
        ids_held(&memory),
        [ids_of(&prompt("p1")), ids_of(&turn_end("p1"))].concat()
    );
}

// Two hooks of one session at once, forced: the second fires INSIDE the
// first one's delivery of the held fact. It finds the hold being taken and
// holds behind; the first, once its cut is made, takes the hold again,
// delivers the second's fact, then its own. Nothing is lost or delivered
// twice, and the session's facts reach the stream in held order.
#[test]
fn two_hooks_of_one_session_at_once_keep_its_facts_first_in_first_out() {
    let memory = Memory::holding(vec![held_of(&prompt("p1"), OWN)]);
    let clock = long();
    let second_report = RefCell::new(None);
    let sink = Sink::new();
    {
        let memory = memory.clone();
        let second_report = &second_report;
        *sink.during.borrow_mut() = Some(Box::new(move || {
            let clock = long();
            let other = Sink::new();
            let report = Holding::new(&memory, &clock, OWN, NAME).record(
                &session(),
                &pieces(1),
                daemon(OWN, &other),
            );
            assert!(other.delivered().is_empty(), "the second delivers nothing");
            *second_report.borrow_mut() = Some(report);
        }));
    }
    let first = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &turn_end("p1"),
        daemon(OWN, &sink),
    );
    let second = second_report
        .borrow_mut()
        .take()
        .expect("the second hook ran");
    assert_eq!(outcomes(&second.own), vec![Outcome::HeldBehind]);
    assert_eq!(
        sink.delivered(),
        [
            ids_of(&prompt("p1")),
            ids_of(&pieces(1)),
            ids_of(&turn_end("p1"))
        ]
        .concat()
    );
    assert!(memory.entries().is_empty());
    assert_eq!(first.flushed.len(), 2);
}

// A held record the daemon is not up to date for (held by a newer hook, a
// downgrade) is kept and passed over: the records after it are delivered,
// and so is the hook's own fact.
#[test]
fn a_record_held_for_a_newer_daemon_is_kept_and_blocks_nothing() {
    let memory = Memory::holding(vec![
        held_of(&prompt("p1"), NEWER),
        held_of(&prompt("p2"), OWN),
    ]);
    let clock = long();
    let sink = Sink::new();
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &turn_end("p2"),
        daemon(OWN, &sink),
    );
    assert_eq!(
        sink.delivered(),
        [ids_of(&prompt("p2")), ids_of(&turn_end("p2"))].concat()
    );
    assert_eq!(ids_held(&memory), ids_of(&prompt("p1")));
    assert_eq!(
        outcomes(&report.own),
        vec![Outcome::Delivered(Delivery::Applied)]
    );
}

// A held record that is not a fact of this session (another stream's book,
// or an external id not its book's) is dropped, said, and never sent.
#[test]
fn a_held_record_of_another_stream_is_dropped_and_said() {
    let observation = &observe(&prompt("p1"))[0];
    let elsewhere = HeldEntry::Fact(HeldFact::of_book(
        OWN,
        NAME,
        observation.external_id(&uuid(SESSION_OTHER)),
        &observation.book(&uuid(SESSION_OTHER)),
        None,
    ));
    let relabelled = HeldEntry::Fact(HeldFact::of_book(
        OWN,
        NAME,
        "hook:prompt:not-its-book",
        &observation.book(&session()),
        None,
    ));
    let memory = Memory::holding(vec![elsewhere, relabelled]);
    let clock = long();
    let sink = Sink::new();
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &turn_end("p1"),
        daemon(OWN, &sink),
    );
    assert_eq!(sink.delivered(), ids_of(&turn_end("p1")));
    assert!(memory.entries().is_empty());
    assert_eq!(
        outcomes(&report.flushed),
        vec![
            Outcome::NotHeld(NotHeld::Foreign),
            Outcome::NotHeld(NotHeld::Foreign)
        ]
    );
    assert_eq!(report.said().len(), 2);
}

// A hold at its bound keeps nothing more: the fact is said as dropped.
#[test]
fn a_hold_at_its_bound_drops_the_fact_and_says_so() {
    let memory = Memory::default();
    memory.refusing(Ok(Held::Full {
        bound: Bound::Session(32 << 20),
        gap: true,
    }));
    let clock = long();
    let report = Holding::new(&memory, &clock, OWN, NAME).record::<Sink>(
        &session(),
        &prompt("p1"),
        Serving::Unreached {
            why: "no daemon".to_string(),
        },
    );
    assert_eq!(
        outcomes(&report.own),
        vec![Outcome::NotHeld(NotHeld::Full(Bound::Session(32 << 20)))]
    );
}

// A hold that cannot be used fails open: the fact is said, with the error.
#[test]
fn a_hold_that_cannot_be_used_says_why() {
    let memory = Memory::default();
    let error = HoldError::Lock {
        path: "/h/.ctxloom/held/x.lock".to_string(),
        ext_err_txt: "held".to_string(),
    };
    memory.refusing(Err(error.clone()));
    let clock = long();
    let sink = Sink::new();
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &prompt("p1"),
        daemon(OLDER, &sink),
    );
    assert_eq!(
        outcomes(&report.own),
        vec![Outcome::NotHeld(NotHeld::Failed(error))]
    );
}

// An event that observes nothing touches nothing.
#[test]
fn an_event_of_no_observation_touches_no_hold() {
    let memory = Memory::holding(vec![held_of(&prompt("p1"), OWN)]);
    let clock = long();
    let sink = Sink::new();
    let report = Holding::new(&memory, &clock, OWN, NAME).record(
        &session(),
        &HookEvent::default(),
        daemon(OWN, &sink),
    );
    assert_eq!(report, HoldReport::default());
    assert!(memory.calls().is_empty());
    assert!(sink.delivered().is_empty());
}
