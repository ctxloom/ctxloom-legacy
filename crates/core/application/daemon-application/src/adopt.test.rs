//! A daemon dying under a live TUI (owner, 2026-11-14): a new daemon adopts,
//! at its start, the runs its sweep leaves, and takes over their end path,
//! over the real aggregates (run-launcher's `TestEdge`) and locks a test
//! turns from held to free.

use std::sync::{Arc, Mutex};

use angzarr_embedded::v1::Cover;
use angzarr_embedded::Payload;
use run_launcher::fakes::{uuid, TestEdge};
use run_ports::{LockProbe, RunLocks};
use runs::RunEvent;

use crate::tests::{ended, events, mint, session_events, Deaf, NoHandOffs, NoHomes, Plans};
use crate::{Daemon, Folds, Place, Ports};

const TUI: &str = "5f0c7a52-3333-5333-8333-000000000003";
/// The run lock the frontend took before it asked (U5's race).
const FRONTEND_LOCK: &str = "/home/u/.ctxloom/runs/0193a8f0-request.lock";
/// The session's checkout, under ctxloom's own worktrees.
const CHECKOUT: &str = "/home/u/.ctxloom/worktrees/calm-otter/tui/dev";

/// The frontend's locks, as a test turns them: the run's own (taken before
/// it asked) and the session's.
#[derive(Clone)]
struct Frontend(Arc<Mutex<(LockProbe, LockProbe)>>);

impl Frontend {
    fn holding(run: LockProbe, session: LockProbe) -> Frontend {
        Frontend(Arc::new(Mutex::new((run, session))))
    }

    fn set(&self, run: LockProbe, session: LockProbe) {
        *self.0.lock().unwrap() = (run, session);
    }
}

impl RunLocks for Frontend {
    fn hold_run(&self, run: &str) -> Result<run_ports::HeldLock, run_ports::LockError> {
        Ok(run_ports::HeldLock::new(format!("/locks/{run}.lock"), ()))
    }

    fn probe(&self, path: &str) -> LockProbe {
        match path == FRONTEND_LOCK {
            true => self.0.lock().unwrap().0,
            false => LockProbe::Unproven,
        }
    }

    fn session(&self, project_dir: &str, harp: &str) -> LockProbe {
        match (project_dir, harp) {
            ("/work", "tui") => self.0.lock().unwrap().1,
            _ => LockProbe::Unproven,
        }
    }
}

/// The project's workspaces: what each was asked to remove.
#[derive(Default)]
struct Checkouts(Mutex<Vec<String>>);

impl isolation_ports::Workspaces for Checkouts {
    fn prepare(
        &self,
        _: &isolation_ports::Request,
    ) -> Result<isolation_ports::Prepared, isolation_ports::Failure> {
        unreachable!("an adopted run is placed by its first daemon")
    }

    fn session_known(&self, _: &str) -> Result<bool, isolation_ports::Failure> {
        Ok(true)
    }

    fn scratch(
        &self,
        _: Option<&str>,
    ) -> Result<Vec<isolation_ports::Scratch>, isolation_ports::Failure> {
        Ok(Vec::new())
    }

    fn place_files(
        &self,
        _: &str,
        _: &[isolation_ports::PlacedFile],
    ) -> Result<(), isolation_ports::Failure> {
        Ok(())
    }

    fn remove_files(&self, _: &str, _: &[String]) {}

    fn inspect(&self, path: &str) -> isolation_ports::Standing {
        isolation_ports::Standing::Unresolved(path.to_string())
    }

    fn remove(&self, repo_dir: &str, path: &str) -> isolation_ports::Removal {
        self.0.lock().unwrap().push(format!("{repo_dir} {path}"));
        isolation_ports::Removal::Removed
    }
}

struct AllCheckouts(Arc<Checkouts>);

impl crate::ChildWorkspaces for AllCheckouts {
    fn of(&self, _: &str, _: &str) -> Arc<dyn isolation_ports::Workspaces + Send + Sync> {
        Arc::clone(&self.0) as Arc<dyn isolation_ports::Workspaces + Send + Sync>
    }
}

struct Rig {
    edge: Arc<TestEdge>,
    locks: Frontend,
    checkouts: Arc<Checkouts>,
    said: Arc<Mutex<Vec<String>>>,
}

impl Rig {
    fn new(run: LockProbe, session: LockProbe) -> Rig {
        Rig {
            edge: Arc::new(TestEdge::new()),
            locks: Frontend::holding(run, session),
            checkouts: Arc::new(Checkouts::default()),
            said: Arc::default(),
        }
    }

    /// A daemon started over the rig's store, after the one that prepared
    /// the session was killed.
    fn daemon(&self) -> Daemon<TestEdge> {
        let said = Arc::clone(&self.said);
        Daemon::new(Ports {
            edge: Arc::clone(&self.edge),
            engines: Vec::new(),
            plans: Box::new(Plans::default()),
            hand_offs: Box::new(NoHandOffs),
            homes: Box::new(NoHomes),
            ears: Box::new(Deaf),
            run_locks: Arc::new(self.locks.clone()),
            place: Place {
                home: "/home/u".to_string(),
                program_dir: None,
                os: "linux".to_string(),
                user_home: None,
            },
            folds: Folds::default(),
            say: Box::new(move |line| said.lock().unwrap().push(line.to_string())),
        })
        .placing(Arc::new(AllCheckouts(Arc::clone(&self.checkouts))))
    }

    fn who(&self) -> runs_application::Identity {
        runs_application::Identity {
            run: runs::run_root(TUI, 1),
            session: uuid(TUI),
            incarnation: 1,
            harp: "tui".to_string(),
            depth: 0,
        }
    }

    /// The prepared session's run, enqueued with the frontend's run lock,
    /// its start recorded when `started`.
    async fn prepared(&self, started: bool) {
        mint(&self.edge, TUI, "tui", "claude-code", None).await;
        runs_application::Enqueuer::new(Arc::clone(&self.edge))
            .enqueue(runs_application::EnqueueRequest {
                session: TUI.to_string(),
                incarnation: 1,
                mode: runs::RunMode::Interactive,
                evidence: runs_application::Evidence::passing("claude-code"),
                launcher_lock: FRONTEND_LOCK.to_string(),
            })
            .await
            .unwrap();
        if started {
            self.report_started().await;
        }
    }

    /// The frontend's start report.
    async fn report_started(&self) {
        runs_application::Facts::new(Arc::clone(&self.edge))
            .record(
                &self.who(),
                runs_application::RunnerFact::RunnerStarted(runs::RunnerStarted {
                    runtime: "claude-code".to_string(),
                    workspace: CHECKOUT.to_string(),
                    pid: 4242,
                }),
            )
            .await
            .unwrap();
    }

    /// The frontend's exit report.
    async fn report_exited(&self) {
        runs_application::Facts::new(Arc::clone(&self.edge))
            .record(
                &self.who(),
                runs_application::RunnerFact::RunExited(runs::RunExited { code: 0, signal: 0 }),
            )
            .await
            .unwrap();
    }

    /// The run's ends, as its stream holds them.
    async fn run_ends(&self) -> Vec<RunEvent> {
        let run = Cover {
            domain: runs::RUN_DOMAIN.to_string(),
            root: Some(runs::run_root(TUI, 1)),
            edition: None,
        };
        events(&self.edge, &run)
            .await
            .into_iter()
            .map(|any| RunEvent::from_any(&any).unwrap())
            .filter(|event| matches!(event, RunEvent::RunEnded(_)))
            .collect()
    }

    async fn incarnations_ended(&self) -> Vec<(u32, String)> {
        ended(&session_events(&self.edge, TUI).await)
    }

    fn removed(&self) -> Vec<String> {
        self.checkouts.0.lock().unwrap().clone()
    }
}

fn causes(ends: &[RunEvent]) -> Vec<runs::EndCause> {
    ends.iter()
        .filter_map(|event| match event {
            RunEvent::RunEnded(ended) => Some(ended.cause),
            _ => None,
        })
        .collect()
}

// (1) A daemon killed under a live TUI: the next daemon's start finds the
// session's lock held (its frontend alive), sweeps nothing and adopts the
// run; the frontend's exit report then ends the incarnation `runner-exit`
// and lets the checkout go WIP-safe, as the first daemon would have.
#[tokio::test]
async fn a_new_daemon_adopts_a_live_tuis_run_and_its_exit_ends_the_incarnation_and_the_checkout() {
    let rig = Rig::new(LockProbe::Unproven, LockProbe::Held);
    rig.prepared(true).await;
    let mut daemon = rig.daemon();
    let swept = daemon
        .start_up(&rig.edge.store, rig.edge.catalog())
        .await
        .unwrap();
    assert!(swept.is_empty(), "its owner is alive: {swept:?}");
    assert_eq!(daemon.adopted_runs().len(), 1, "adopted");
    // While the TUI runs, nothing is ended.
    assert!(
        !daemon
            .end_the_adopted(&rig.edge.store, rig.edge.catalog())
            .await
    );
    assert!(rig.run_ends().await.is_empty());
    assert!(rig.incarnations_ended().await.is_empty());
    // The frontend reports the exit, then lets the session's lock go.
    rig.report_exited().await;
    rig.locks.set(LockProbe::Unproven, LockProbe::Free);
    assert!(
        daemon
            .end_the_adopted(&rig.edge.store, rig.edge.catalog())
            .await
    );
    assert_eq!(causes(&rig.run_ends().await), [runs::EndCause::RunnerExit]);
    assert_eq!(
        rig.incarnations_ended().await,
        [(1, "runner-exit".to_string())]
    );
    assert_eq!(rig.removed(), [format!("/work {CHECKOUT}")]);
    assert!(daemon.adopted_runs().is_empty(), "its end path is done");
    // Nothing more, round after round.
    assert!(
        !daemon
            .end_the_adopted(&rig.edge.store, rig.edge.catalog())
            .await
    );
    assert_eq!(rig.removed().len(), 1);
    let said = rig.said.lock().unwrap().join("\n");
    assert!(said.contains("adopted"), "{said}");
}

// U5's start-report race, forced: the daemon is killed between the
// frontend's plan request and its start report. The frontend took the run's
// own lock BEFORE it asked, so the next daemon's start reads it held: the
// run is not orphaned but adopted, and once the frontend reports its start
// and its exit the incarnation ends `runner-exit`.
#[tokio::test]
async fn a_daemon_started_between_the_plan_and_the_start_report_reads_the_frontend_alive() {
    let rig = Rig::new(LockProbe::Held, LockProbe::Unproven);
    rig.prepared(false).await;
    let mut daemon = rig.daemon();
    let swept = daemon
        .start_up(&rig.edge.store, rig.edge.catalog())
        .await
        .unwrap();
    assert!(
        swept.is_empty(),
        "its frontend holds the run's lock: {swept:?}"
    );
    assert!(
        !daemon
            .end_the_adopted(&rig.edge.store, rig.edge.catalog())
            .await
    );
    assert!(rig.run_ends().await.is_empty(), "not orphaned");
    // Its frontend starts the TUI (the session's lock taken), reports the
    // start and lets the run's lock go; then the TUI exits.
    rig.locks.set(LockProbe::Held, LockProbe::Held);
    rig.report_started().await;
    rig.locks.set(LockProbe::Unproven, LockProbe::Held);
    assert!(
        !daemon
            .end_the_adopted(&rig.edge.store, rig.edge.catalog())
            .await
    );
    rig.report_exited().await;
    rig.locks.set(LockProbe::Unproven, LockProbe::Free);
    assert!(
        daemon
            .end_the_adopted(&rig.edge.store, rig.edge.catalog())
            .await
    );
    assert_eq!(causes(&rig.run_ends().await), [runs::EndCause::RunnerExit]);
    assert_eq!(
        rig.incarnations_ended().await,
        [(1, "runner-exit".to_string())]
    );
    assert_eq!(rig.removed(), [format!("/work {CHECKOUT}")]);
}

// A frontend gone without reporting (the session's lock free, no exit in
// the run's stream) is a kill, as the first daemon's `Frontended` read it:
// its run's exit is recorded (signal 9), keyed as the frontend's own exit,
// and then its incarnation ends `runner-exit`.
#[tokio::test]
async fn an_adopted_frontend_gone_without_reporting_ends_as_a_kill() {
    let rig = Rig::new(LockProbe::Unproven, LockProbe::Held);
    rig.prepared(true).await;
    let mut daemon = rig.daemon();
    daemon
        .start_up(&rig.edge.store, rig.edge.catalog())
        .await
        .unwrap();
    rig.locks.set(LockProbe::Unproven, LockProbe::Free);
    assert!(
        daemon
            .end_the_adopted(&rig.edge.store, rig.edge.catalog())
            .await
    );
    assert_eq!(causes(&rig.run_ends().await), [runs::EndCause::RunnerExit]);
    assert!(
        daemon
            .end_the_adopted(&rig.edge.store, rig.edge.catalog())
            .await
    );
    assert_eq!(
        rig.incarnations_ended().await,
        [(1, "runner-exit".to_string())]
    );
    // Its late exit report is the same fact: a duplicate, nothing more.
    rig.report_exited().await;
    assert_eq!(causes(&rig.run_ends().await), [runs::EndCause::RunnerExit]);
}

// A frontend that died before its start (the run's own lock left free, no
// start recorded) leaves its run to the sweep: `launch-orphaned`, its
// incarnation `orphaned`.
#[tokio::test]
async fn an_adopted_run_whose_frontend_died_before_its_start_is_orphaned() {
    let rig = Rig::new(LockProbe::Held, LockProbe::Unproven);
    rig.prepared(false).await;
    let mut daemon = rig.daemon();
    daemon
        .start_up(&rig.edge.store, rig.edge.catalog())
        .await
        .unwrap();
    rig.locks.set(LockProbe::Free, LockProbe::Unproven);
    assert!(
        daemon
            .end_the_adopted(&rig.edge.store, rig.edge.catalog())
            .await
    );
    assert_eq!(
        causes(&rig.run_ends().await),
        [runs::EndCause::LaunchOrphaned]
    );
    assert_eq!(
        rig.incarnations_ended().await,
        [(1, "orphaned".to_string())]
    );
    assert!(daemon.adopted_runs().is_empty());
    assert!(rig.removed().is_empty(), "it never had a checkout");
}

// The idle count's sweep never takes an adopted run: its end path is the
// adoption's (a kill, its exit, then `runner-exit`), never `runner-lost`.
#[tokio::test]
async fn the_idle_sweep_leaves_an_adopted_run_to_its_end_path() {
    let rig = Rig::new(LockProbe::Unproven, LockProbe::Held);
    rig.prepared(true).await;
    let mut daemon = rig.daemon();
    daemon
        .start_up(&rig.edge.store, rig.edge.catalog())
        .await
        .unwrap();
    rig.locks.set(LockProbe::Unproven, LockProbe::Free);
    assert_eq!(
        daemon.live_runs(&rig.edge.store, rig.edge.catalog()).await,
        Ok(1)
    );
    assert!(rig.run_ends().await.is_empty(), "not swept as lost");
}

// Adoption is the start's: a start tried again adopts nothing twice.
#[tokio::test]
async fn a_start_tried_again_adopts_a_run_once() {
    let rig = Rig::new(LockProbe::Unproven, LockProbe::Held);
    rig.prepared(true).await;
    let mut daemon = rig.daemon();
    for _ in 0..2 {
        daemon
            .start_up(&rig.edge.store, rig.edge.catalog())
            .await
            .unwrap();
    }
    assert_eq!(daemon.adopted_runs().len(), 1);
    let said = rig.said.lock().unwrap();
    assert_eq!(said.iter().filter(|l| l.contains("adopted")).count(), 1);
}
