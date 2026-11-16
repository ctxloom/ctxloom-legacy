//! The daemon's scheduler end to end in the wiring (step 8 U5a): the core
//! the daemon starts, over a real store under a temporary home, with the
//! scheduler's test-only consumer (`scheduler-fixture`: a dummy domain whose
//! deadline arms an alarm and whose close cancels it) registered beside
//! production's. A fake clock the test moves, a wake the test forces, and
//! nothing sleeps: every wait is for what the loop has done, bounded.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use angzarr_embedded::angzarr_store_sqlite::SqliteStore;
use angzarr_embedded::v1::{CommandBook, Cover, Uuid};
use angzarr_embedded::{CommandBookExt as _, Payload as _, Registry, StreamRef, Timestamp};
use run_ports::Edge as _;
use scheduler::{stream_for, SchedulerEvent, Stream, SCHEDULER_DOMAIN};
use scheduler_fixture::{
    ArmsDeadline, Close, DeliverToDummy, DummyAggregate, DummyCommand, DummyEvent, SetDeadline,
    DUMMY_DOMAIN,
};
use scheduler_ports::FakeClock;

use super::{start, Say, Scheduler};
use crate::core_link::AlarmConsumer;
use crate::{InProcessCore, Projectors, SchedulerDues};

type Core = InProcessCore<SqliteStore>;

/// The dummy, as a domain that arms alarms registers itself.
fn dummy(registry: &mut Registry) {
    registry
        .aggregate::<DummyAggregate>(DummyAggregate::default)
        .expect("the dummy registers")
        .policy(ArmsDeadline)
        .expect("its arming registers")
        .policy(DeliverToDummy)
        .expect("its delivery registers");
}

/// The dummy without its delivery: as a process that fired an alarm and
/// died before delivering it left the store.
fn dummy_undelivered(registry: &mut Registry) {
    registry
        .aggregate::<DummyAggregate>(DummyAggregate::default)
        .expect("the dummy registers")
        .policy(ArmsDeadline)
        .expect("its arming registers");
}

const CONSUMERS: &[AlarmConsumer] = &[dummy];

fn at(seconds: i64) -> Timestamp {
    Timestamp { seconds, nanos: 0 }
}

/// A temporary home and a runtime of the daemon's kind.
fn home() -> (Arc<file_system::Rooted>, tokio::runtime::Runtime) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    (Arc::new(file_system::Rooted::temp().unwrap()), runtime)
}

/// The daemon's core over the home's store, every read model registered,
/// `consumers` the domains that arm alarms; and its due model.
async fn core_over(
    fs: &Arc<file_system::Rooted>,
    consumers: &[AlarmConsumer],
) -> (Core, SchedulerDues) {
    let store = ctxloom_store_sqlite::Stores::new(fs.clone(), "/home/u")
        .open()
        .await
        .unwrap();
    let mut kept = Projectors::default();
    let core = InProcessCore::start_consuming(
        store,
        |store| {
            kept = Projectors::filled(store);
            kept.clone()
        },
        consumers,
    )
    .await;
    (
        core,
        kept.due.expect("the daemon's core folds the due alarms"),
    )
}

/// What the scheduler said.
fn log() -> (Arc<Mutex<Vec<String>>>, Say) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&lines);
    let say: Say = Arc::new(move |line: &str| kept.lock().unwrap().push(line.to_string()));
    (lines, say)
}

/// The scheduler over `core`, on `clock`, polling so seldom that only a
/// wake moves it.
fn scheduler(core: &Core, dues: SchedulerDues, clock: &Arc<FakeClock>, say: Say) -> Scheduler {
    start(
        core.share(),
        dues,
        Arc::clone(clock) as Arc<dyn scheduler_ports::Clock>,
        Duration::from_secs(3600),
        say,
    )
}

fn dummy_cover(n: u8) -> Cover {
    Cover {
        domain: DUMMY_DOMAIN.to_string(),
        root: Some(Uuid::from_bytes([n; 16])),
        edition: None,
    }
}

async fn send(core: &Core, n: u8, command: impl Into<DummyCommand>) {
    core.edge()
        .handle(CommandBook::command_of(dummy_cover(n), &command.into()))
        .await
        .unwrap_or_else(|failure| panic!("{failure:?}"));
}

async fn set_deadline(core: &Core, n: u8, seconds: i64) {
    send(
        core,
        n,
        SetDeadline {
            deadline: at(seconds),
        },
    )
    .await;
}

async fn pages(core: &Core, cover: &Cover) -> Vec<angzarr_embedded::Any> {
    let stream = StreamRef::try_from(cover).unwrap();
    angzarr_embedded::EventStore::read(core.store(), stream, 0, None)
        .await
        .unwrap()
        .pages
        .into_iter()
        .filter_map(|page| match page.body {
            Some(angzarr_embedded::v1::event_page::Body::Event(event)) => Some(event),
            _ => None,
        })
        .collect()
}

/// The instants dummy `n` recorded its deadline passed at.
async fn passed(core: &Core, n: u8) -> Vec<i64> {
    pages(core, &dummy_cover(n))
        .await
        .iter()
        .filter_map(|event| match DummyEvent::from_any(event).unwrap() {
            DummyEvent::DeadlinePassed(passed) => Some(passed.at.seconds),
            _ => None,
        })
        .collect()
}

/// How many alarms dummy `n`'s scheduler stream recorded fired.
async fn fired(core: &Core, n: u8) -> usize {
    let target = Stream {
        domain: DUMMY_DOMAIN.to_string(),
        root: vec![n; 16],
        edition: None,
    };
    let cover = Cover {
        domain: SCHEDULER_DOMAIN.to_string(),
        root: Some(stream_for(&target)),
        edition: None,
    };
    pages(core, &cover)
        .await
        .iter()
        .filter(|event| {
            matches!(
                SchedulerEvent::from_any(event),
                Ok(SchedulerEvent::AlarmFired(_))
            )
        })
        .count()
}

// An alarm armed through the daemon's core is fired by its scheduler once
// the clock reaches it, and the target is handed `RecordAlarmReached`, all
// in the loop's one kernel call; the stopped scheduler lets the core go, so
// the store closes.
#[test]
fn an_armed_alarm_fires_at_its_instant_and_the_target_records_it() {
    let (fs, runtime) = home();
    runtime.block_on(async {
        let (core, dues) = core_over(&fs, CONSUMERS).await;
        let clock = Arc::new(FakeClock::at(1_000));
        let (said, say) = log();
        let mut scheduler = scheduler(&core, dues, &clock, say);
        set_deadline(&core, 1, 1_100).await;
        scheduler.ticked_at(at(1_000)).await;
        clock.set(at(1_099));
        scheduler.wake();
        scheduler.ticked_at(at(1_099)).await;
        assert_eq!(passed(&core, 1).await, Vec::<i64>::new(), "not yet");
        clock.set(at(1_100));
        scheduler.wake();
        scheduler.ticked_at(at(1_100)).await;
        assert_eq!(passed(&core, 1).await, vec![1_100]);
        assert_eq!(fired(&core, 1).await, 1);
        let target = Stream {
            domain: DUMMY_DOMAIN.to_string(),
            root: vec![1; 16],
            edition: None,
        };
        let line = format!("alarm fired: {} deadline", scheduler::stream_text(&target));
        assert!(
            said.lock().unwrap().contains(&line),
            "{:?}",
            said.lock().unwrap()
        );
        assert!(scheduler.stop(Duration::from_secs(10)).await, "it ended");
        core.close().await.expect("nothing else holds the store");
    });
}

// An alarm armed in this process wakes the loop as it commits: nothing else
// does here (the poll is an hour, no wake is forced), and it fires.
#[test]
fn an_alarm_armed_in_process_wakes_the_loop() {
    let (fs, runtime) = home();
    runtime.block_on(async {
        let (core, dues) = core_over(&fs, CONSUMERS).await;
        let clock = Arc::new(FakeClock::at(2_000));
        let (_said, say) = log();
        let mut scheduler = scheduler(&core, dues, &clock, say);
        scheduler.ticked_at(at(2_000)).await;
        set_deadline(&core, 1, 1_500).await;
        scheduler.fired(1).await;
        assert_eq!(passed(&core, 1).await, vec![1_500]);
        assert!(scheduler.stop(Duration::from_secs(10)).await);
        core.close().await.unwrap();
    });
}

// A cancelled alarm never fires: the dummy's close cancels it through its
// policy, and a tick past its instant fires nothing.
#[test]
fn a_cancelled_alarm_never_fires() {
    let (fs, runtime) = home();
    runtime.block_on(async {
        let (core, dues) = core_over(&fs, CONSUMERS).await;
        let clock = Arc::new(FakeClock::at(1_000));
        let (said, say) = log();
        let mut scheduler = scheduler(&core, dues, &clock, say);
        set_deadline(&core, 1, 1_100).await;
        send(&core, 1, Close {}).await;
        clock.set(at(1_200));
        scheduler.wake();
        scheduler.ticked_at(at(1_200)).await;
        assert_eq!(passed(&core, 1).await, Vec::<i64>::new());
        assert_eq!(fired(&core, 1).await, 0);
        assert!(
            !said
                .lock()
                .unwrap()
                .iter()
                .any(|l| l.starts_with("alarm fired")),
            "{:?}",
            said.lock().unwrap()
        );
        assert!(scheduler.stop(Duration::from_secs(10)).await);
        core.close().await.unwrap();
    });
}

// A daemon killed with an alarm armed (its loop cut where it waits, its
// core dropped, never closed), then a new one over the same store: the new
// scheduler rebuilds its model from the store, and the alarm, which came due
// while none ran, fires once, late, at the instant armed.
#[test]
fn an_alarm_armed_before_a_kill_fires_in_the_next_daemon() {
    let (fs, runtime) = home();
    runtime.block_on(async {
        let (core, dues) = core_over(&fs, CONSUMERS).await;
        let clock = Arc::new(FakeClock::at(1_000));
        let (_said, say) = log();
        let mut first = scheduler(&core, dues, &clock, say);
        set_deadline(&core, 1, 1_100).await;
        first.wake();
        first.ticked_at(at(1_000)).await;
        first.kill();
        drop(core);
        assert_eq!(fired_in_a_new_daemon(&fs, 1_200).await, vec![1_100]);
        // And once is all: another daemon fires nothing more.
        assert_eq!(fired_in_a_new_daemon(&fs, 1_300).await, vec![1_100]);
    });
}

/// A new daemon's core over `fs`'s store and its scheduler at `now`, ticked
/// once: what dummy 1 then holds passed.
async fn fired_in_a_new_daemon(fs: &Arc<file_system::Rooted>, now: i64) -> Vec<i64> {
    let (core, dues) = core_over(fs, CONSUMERS).await;
    let clock = Arc::new(FakeClock::at(now));
    let (_said, say) = log();
    let mut scheduler = scheduler(&core, dues, &clock, say);
    scheduler.ticked_at(at(now)).await;
    let passed = passed(&core, 1).await;
    assert!(scheduler.stop(Duration::from_secs(10)).await);
    core.close().await.unwrap();
    passed
}

// An alarm fired and not delivered (the process died between the two, or
// the delivery policy is new) is delivered by the next core's resume at its
// start (`CoreStores::at_start`), before any scheduler runs.
#[test]
fn an_alarm_fired_and_not_delivered_is_delivered_at_the_next_start() {
    let (fs, runtime) = home();
    runtime.block_on(async {
        let (core, dues) = core_over(&fs, &[dummy_undelivered]).await;
        let clock = Arc::new(FakeClock::at(1_000));
        let (_said, say) = log();
        let mut scheduler = scheduler(&core, dues, &clock, say);
        set_deadline(&core, 1, 1_000).await;
        scheduler.wake();
        scheduler.fired(1).await;
        assert_eq!(passed(&core, 1).await, Vec::<i64>::new(), "not delivered");
        assert!(scheduler.stop(Duration::from_secs(10)).await);
        core.close().await.unwrap();
        let (core, dues) = core_over(&fs, CONSUMERS).await;
        drop(dues);
        assert_eq!(
            passed(&core, 1).await,
            vec![1_000],
            "the resume delivered it"
        );
        core.close().await.unwrap();
    });
}

// A scheduler still running holds the core: the store does not close while
// it runs, so the daemon stops it first.
#[test]
fn a_running_scheduler_holds_the_store_until_it_is_stopped() {
    let (fs, runtime) = home();
    runtime.block_on(async {
        let (core, dues) = core_over(&fs, CONSUMERS).await;
        let clock = Arc::new(FakeClock::at(1_000));
        let (_said, say) = log();
        let mut scheduler = scheduler(&core, dues, &clock, say);
        scheduler.ticked_at(at(1_000)).await;
        let unclosed = core.close().await;
        assert!(
            matches!(unclosed, Err(crate::Unclosed::StillHeld { .. })),
            "{unclosed:?}"
        );
        assert!(scheduler.stop(Duration::from_secs(10)).await);
    });
}

// The production core registers the scheduler and the due model: an alarm
// armed on it by hand is folded, and the system clock reads now.
#[test]
fn the_production_core_registers_the_scheduler_and_the_system_clock_reads_now() {
    use scheduler_ports::Clock as _;
    let (fs, runtime) = home();
    runtime.block_on(async {
        let store = ctxloom_store_sqlite::Stores::new(fs.clone(), "/home/u")
            .open()
            .await
            .unwrap();
        let mut kept = Projectors::default();
        let core = InProcessCore::start_with(store, |store| {
            kept = Projectors::filled(store);
            kept.clone()
        })
        .await;
        let dues = kept.due.clone().unwrap();
        let target = Stream {
            domain: DUMMY_DOMAIN.to_string(),
            root: vec![7; 16],
            edition: None,
        };
        let cover = Cover {
            domain: SCHEDULER_DOMAIN.to_string(),
            root: Some(stream_for(&target)),
            edition: None,
        };
        let arm = scheduler::ArmAlarm {
            target,
            alarm_id: "a".to_string(),
            at: at(5),
        };
        core.edge()
            .handle(CommandBook::command_of(
                cover,
                &scheduler::SchedulerCommand::from(arm),
            ))
            .await
            .unwrap_or_else(|failure| panic!("{failure:?}"));
        let due =
            scheduler_due::ReadsDue::read_due(&dues, |due| due.due_at_or_before(&at(5)).len());
        assert_eq!(due, 1, "the kernel folded the arming inline");
        drop((dues, kept));
        core.close().await.unwrap();
    });
    let now = crate::SystemClock.now();
    let system = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert!(
        (now.seconds - system).abs() <= 1,
        "{now:?} against {system}"
    );
}
