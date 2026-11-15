//! `ctxloom daemon serve`: the user's daemon (D18), one per user, over the
//! ONE store that holds every project's streams (T14). Only the holder of
//! the single-instance lock serves (S-e: the one policy runner).
//!
//! This is composition alone (T18): the launcher is `daemon-application`'s
//! [`Daemon`], given its ports here (the engine registry, the child's plan by
//! its project's dry run, the hand-off files, the session directories, the
//! pump that hears each child, the run locks and the tokens). What stays
//! here is the root's: the single-instance lock, the runtime, the store and its core
//! (the read models registered in its kernel, S32), the listener and its
//! certificate, the discovery file, and the pace: the launcher's start
//! (the orphan sweep) first, then a round at the first commit through this
//! process's store handles (`CommitNotify`, the push) or after the poll
//! interval, whichever is first (another process's commit reaches no
//! in-process signal, so the poll stays, S-N4). The daemon exits when asked
//! (`daemon stop`), when another daemon's discovery replaced its own (it
//! lost the single-instance lock), or once it has hosted nothing, had nothing to do and
//! known no run started and not ended (M9: any process's) for its idle
//! time.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use angzarr_embedded::angzarr_ports::CommitNotify;
use angzarr_embedded::angzarr_store_sqlite::SqliteStore;
use credentials::OsTokenSource;
use ctxloom_store_sqlite::Stores;
use daemon_application::{
    ChildHomes, ChildPlans, Daemon, Ears, Folds, HandOffs, OneShots, Place as Told, Ports,
};
use file_system::Fs;
use hook_listener::{HookListener, TlsIdentity};
use run_ports::{CertMint, CertNames, ChildObservation, Discovery, SessionRecords, TokenSource};
use runs_application::UserGate;
use session_files::SessionDirs;

use super::files::{DaemonFiles, Instance};
use crate::{InProcessCore, Projectors};

/// How long a daemon starting waits for the single-instance lock.
const INSTANCE_WAIT: Duration = Duration::from_secs(5);

/// How long a daemon exiting waits for each one-shot it stopped to end:
/// the stop's grace (five seconds), and its end's recording.
const ONE_SHOT_END_WAIT: Duration = Duration::from_secs(10);

/// Where the daemon is: the ctxloom home, the temporary root, the
/// operating system's name, the user's own home (`Wiring::user_home`) and
/// the binary's full version stamp (M3).
pub(crate) struct Place {
    pub(crate) stamp: String,
    pub(crate) home: PathBuf,
    /// The single-instance lock's path (`instance_lock_path`).
    pub(crate) instance_lock: PathBuf,
    pub(crate) temp: PathBuf,
    pub(crate) os: String,
    pub(crate) user_home: Option<PathBuf>,
    /// The engines it hosts children on, composed as every launch's are
    /// (`commands::hosted_engines`).
    pub(crate) engines: Vec<daemon_application::HostedEngine>,
    /// The environment its children's checkouts' git runs in (s4-Q11).
    pub(crate) env: Arc<process_env::EnvSnapshot>,
}

/// How the daemon paces itself.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pace {
    /// Exit after this long with nothing hosted and nothing to do.
    pub(crate) idle: Duration,
    /// How often it looks at the store.
    pub(crate) poll: Duration,
}

/// The daemon's log, shared by the loop and the launcher.
type Say = Arc<Mutex<Box<dyn FnMut(&str) + Send>>>;

fn said(say: &Say, line: &str) {
    (say.lock().unwrap_or_else(PoisonError::into_inner))(line);
}

/// Run the daemon in this process until it exits: the instance held, served,
/// advertised, swept, then ticking. `say` is its log.
pub(crate) fn serve(
    fs: Arc<dyn Fs>,
    place: Place,
    pace: Pace,
    say: Box<dyn FnMut(&str) + Send>,
) -> Result<(), String> {
    let Place {
        stamp,
        home,
        instance_lock,
        temp,
        os,
        user_home,
        engines,
        env,
    } = place;
    let home = home.as_path();
    let files = DaemonFiles::with_instance(Arc::clone(&fs), home, instance_lock);
    let Some(held) = files.hold_instance(INSTANCE_WAIT) else {
        return Err("another daemon holds the single-instance lock".to_string());
    };
    let say: Say = Arc::new(Mutex::new(say));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let code = |e: &dyn angzarr_diagnostic::Diagnostic| e.code().to_string();
        let store = Stores::new(Arc::clone(&fs), home.to_path_buf())
            .open()
            .await
            .map_err(|e| code(&e))?;
        // The read models are registered in the kernel (S32), so a SIMPLE
        // request waits for them; the launcher catches the same folds up
        // for what other processes commit.
        let mut kept = Projectors::default();
        let core = InProcessCore::start_with(store, |store| {
            kept = Projectors::filled(store);
            kept.clone()
        })
        .await;
        // Under test-support, the kill switch at a step (U6), on every book
        // the core applies: the launcher's, its one-shots', its clients'.
        #[cfg(feature = "test-support")]
        dies_at_steps(&core.edge(), &files);
        // What the loop shares with the API's use cases (slice 2 step 5).
        let shared = Arc::new(Shared::new(kept.index.clone().unwrap_or_default()));
        let one_shots = one_shots_over(&core, home, &temp);
        let terminals = terminals_over(&core, home, &temp);
        let started = start(
            &fs,
            (home, &temp),
            &core,
            &kept,
            (os, user_home, stamp, pace, engines),
            (&files, &shared, &one_shots, &terminals),
            (&say, &env),
        )
        .await;
        let (listener, mut launcher, own) = match started {
            Ok(started) => started,
            Err(detail) => {
                // A start that fails before it serves closes its store as
                // every exit does (ruling 131): what holds it is let go first.
                drop(shared);
                drop(kept);
                drop(one_shots);
                drop(terminals);
                said(&say, &format!("the start failed: {detail}"));
                match core.close().await {
                    Ok(()) => said(&say, "the store is closed"),
                    Err(unclosed) => said(&say, &format!("the store did not close: {unclosed}")),
                }
                return Err(detail);
            }
        };
        let endpoint = own.endpoint.clone();
        said(&say, &format!("serving at {endpoint}"));
        let result = run(
            &mut launcher,
            &core,
            (&files, &own, &held),
            (pace, &shared, &terminals),
            &say,
        )
        .await;
        files.withdraw_own(&own);
        // Every holder of the store let go before the close (ruling 131):
        // the launcher (its edge and its folds), the edge, the listener's
        // connection and stream tasks (its stop returns once every copy of
        // its service is released), and the read models kept here, each of
        // which holds the store through its gap fill (`StoreFill`) for as
        // long as any copy of it lives; the kernel's copies go with the
        // core.
        launcher.end_all();
        // The one-shots it hosts were stopped with its children: each is
        // waited for, so its run's end is recorded and its copy of the core
        // let go before the store closes.
        if !one_shots.wait_ended(ONE_SHOT_END_WAIT) {
            said(
                &say,
                "a one-shot did not end in time: its run is left to the next sweep",
            );
        }
        drop(launcher);
        let _ = listener.stop().await;
        drop(shared);
        drop(kept);
        drop(one_shots);
        // A session a frontend still runs keeps its thread (and its copy of
        // the core) until its frontend reports: the store's close then says
        // so, and the frontend reports to the next daemon (S17).
        drop(terminals);
        said(&say, "exited");
        // Closed, not dropped (ruling 131).
        if let Err(unclosed) = core.close().await {
            said(&say, &format!("the store did not close: {unclosed}"));
        }
        result
    })
}

/// The clock's seconds since the epoch, as the daemon's start says it.
fn seconds_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The one-shots the daemon hosts (U4), each over `core` shared, on the
/// daemon's runtime (the one this is called on): the API creates and
/// watches them, the launcher stops them.
fn one_shots_over(
    core: &InProcessCore<SqliteStore>,
    home: &Path,
    temp: &Path,
) -> Arc<daemon_application::OneShots> {
    Arc::new(daemon_application::OneShots::new(Box::new(
        crate::oneshot::HostedOneShots {
            core: core.share(),
            runtime: tokio::runtime::Handle::current(),
            home: home.to_path_buf(),
            temp: temp.to_path_buf(),
        },
    )))
}

/// What a daemon's start shares with its loop and its API: its files, the
/// round's state, the one-shots it hosts and the sessions it prepares.
type Hosts<'a> = (
    &'a DaemonFiles,
    &'a Arc<Shared>,
    &'a Arc<OneShots>,
    &'a Arc<daemon_application::Terminals>,
);

/// The interactive sessions the daemon prepares (U5), each over `core`
/// shared, on the daemon's runtime: the API creates and watches them.
fn terminals_over(
    core: &InProcessCore<SqliteStore>,
    home: &Path,
    temp: &Path,
) -> Arc<daemon_application::Terminals> {
    Arc::new(daemon_application::Terminals::new(Box::new(
        crate::frontend::HostedTerminals {
            core: core.share(),
            runtime: tokio::runtime::Handle::current(),
            home: home.to_path_buf(),
            temp: temp.to_path_buf(),
        },
    )))
}

/// The daemon started (its bearer and certificate minted, its listener
/// serving the coordinator and the `Ctxloom` service, the launcher
/// composed, the discovery file published): the listener, the launcher and
/// its own advertisement. On a failure what it started is let go (the
/// listener stopped) before it answers, so its caller can close the store.
async fn start(
    fs: &Arc<dyn Fs>,
    (home, temp): (&Path, &Path),
    core: &InProcessCore<SqliteStore>,
    kept: &Projectors,
    (os, user_home, stamp, pace, engines): (
        String,
        Option<PathBuf>,
        String,
        Pace,
        Vec<daemon_application::HostedEngine>,
    ),
    (files, shared, one_shots, terminals): Hosts<'_>,
    (say, env): (&Say, &Arc<process_env::EnvSnapshot>),
) -> Result<(HookListener, Daemon<CoreEdge, OsTokenSource>, Discovery), String> {
    let code = |e: &dyn angzarr_diagnostic::Diagnostic| e.code().to_string();
    let edge = core.edge();
    // The user bearer (T12): minted at each start, advertised in the
    // discovery file alone.
    let bearer = OsTokenSource.mint();
    let names = CertNames::new(
        vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ],
        Vec::new(),
    );
    let certificate = hook_tls::Minter::lasting(
        hook_tls::SystemClock,
        run_ports::DAEMON_CERT_LIFETIME_SECONDS,
    )
    .mint(&names)
    .map_err(|e| code(&e))?;
    let started_at = seconds_now();
    let calls = served(
        daemon_api::Api::new(daemon_api::Parts {
            edge: Arc::clone(&edge),
            index: kept.index.clone().unwrap_or_default(),
            pending: kept.pending.clone().unwrap_or_default(),
            board: kept.board.clone().unwrap_or_default(),
            hosting: Arc::clone(shared) as Arc<dyn daemon_api::Hosting>,
            states: Arc::new(Locks(Arc::clone(fs))),
            harps: Arc::new(FreshHarps),
            places: Arc::new(super::places::ProjectPlaces {
                fs: Arc::clone(fs),
                home: home.to_path_buf(),
                temp: temp.to_path_buf(),
            }),
            purges: Arc::new(super::places::StorePurges(core.store_handle())),
            about: daemon_api::About {
                pid: std::process::id(),
                version: stamp.clone(),
                start_time: angzarr_embedded::Timestamp {
                    seconds: started_at,
                    nanos: 0,
                },
                idle_limit_seconds: pace.idle.as_secs(),
            },
        })
        // The one-shots it hosts (U4).
        .hosting_one_shots(Arc::clone(one_shots)),
        fs,
        (home, temp),
        (&engines, terminals),
    );
    let listener = HookListener::serve_tcp_beside(
        coordinated(&edge, files),
        UserGate::new(&bearer),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        TlsIdentity::from_pem(certificate.public().to_pem(), certificate.key_pem()),
        Some(daemon_api_grpc::service(calls, UserGate::new(&bearer))),
    )
    .await
    .map_err(|e| code(&e))?;
    let endpoint = match run_ports::endpoint_url("127.0.0.1", listener.local_addr().port()) {
        Ok(endpoint) => endpoint,
        Err(e) => {
            let _ = listener.stop().await;
            return Err(code(&e));
        }
    };
    shared.serving_at(&endpoint);
    let launcher = launcher(
        fs,
        (home, temp),
        &edge,
        kept,
        (engines, user_home.clone(), env),
        Told {
            home: home.to_string_lossy().into_owned(),
            program_dir: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.display().to_string())),
            os,
            user_home: user_home.map(|home| home.to_string_lossy().into_owned()),
            endpoint: endpoint.clone(),
            cert: certificate.public().clone(),
        },
        Arc::clone(say),
    )
    .hosting_one_shots(Arc::clone(one_shots));
    let own = Discovery {
        stamp,
        pid: std::process::id(),
        endpoint,
        cert: certificate.public().clone(),
        bearer,
        home: home.display().to_string(),
    };
    match swept_and_advertised(launcher, core, files, &own, say).await {
        Ok(launcher) => Ok((listener, launcher, own)),
        Err(detail) => {
            let _ = listener.stop().await;
            Err(detail)
        }
    }
}

/// The launcher's first step (T18), before the daemon is advertised: the
/// runs other processes, and a killed daemon, left are swept (their
/// checkouts let go, OD-19), so the first command that finds it finds the
/// record whole; then `own` is published. On a failure to publish, the
/// launcher is let go and why is answered.
async fn swept_and_advertised(
    mut launcher: Daemon<CoreEdge, OsTokenSource>,
    core: &InProcessCore<SqliteStore>,
    files: &DaemonFiles,
    own: &Discovery,
    say: &Say,
) -> Result<Daemon<CoreEdge, OsTokenSource>, String> {
    if let Err(detail) = launcher.start_up(core.store(), core.catalog()).await {
        said(say, &format!("the start's sweep failed: {detail}"));
    }
    if let Err(e) = files.publish(own) {
        drop(launcher);
        return Err(format!("the discovery file cannot be written: {e}"));
    }
    Ok(launcher)
}

/// The API the daemon serves: `api`, a project's context assembled from its
/// registered root (slice 3 step 6), and a child's hand-off planned, judged
/// and kept here (slice 4 U3), over `home`, `temp` and the `engines` its
/// children are hosted on, and the interactive sessions `terminals`
/// prepares (U5).
fn served(
    api: daemon_api::Api<CoreEdge>,
    fs: &Arc<dyn Fs>,
    (home, temp): (&Path, &Path),
    (engines, terminals): (
        &[daemon_application::HostedEngine],
        &Arc<daemon_application::Terminals>,
    ),
) -> Arc<dyn daemon_api::Calls> {
    Arc::new(
        // The sessions it prepares (U5).
        api.preparing_terminals(Arc::clone(terminals))
            .assembling(Arc::new(super::places::ProjectAssemblies {
                home: home.to_path_buf(),
                temp: temp.to_path_buf(),
            }))
            .handing_off(super::files::api_hand_offs(fs, (home, temp), engines)),
    )
}

/// The edge of the daemon's core, which the launcher writes through.
type CoreEdge = embedded_edge::EmbeddedEdge<crate::CoreStores<SqliteStore>>;

/// The edge the coordinator serves the clients' writes through: the core's.
#[cfg(not(feature = "test-support"))]
fn coordinated(edge: &Arc<CoreEdge>, _: &DaemonFiles) -> Arc<CoreEdge> {
    Arc::clone(edge)
}

/// Under test-support, the core's behind the kill switch
/// ([`DaemonFiles::take_die_after_apply`]).
#[cfg(feature = "test-support")]
fn coordinated(edge: &Arc<CoreEdge>, files: &DaemonFiles) -> Arc<Dying<CoreEdge>> {
    Arc::new(Dying {
        inner: Arc::clone(edge),
        files: files.clone(),
    })
}

/// A test-support edge (slice 3 step 4): once a client's `HandleSync` is
/// applied, a daemon that finds the switch file takes it and is killed
/// outright before it answers, as a crash between the apply and the answer
/// would leave it; the daemon started after finds none. Forced, never
/// waited for (the race rule).
#[cfg(feature = "test-support")]
pub(crate) struct Dying<E> {
    inner: Arc<E>,
    files: DaemonFiles,
}

#[cfg(feature = "test-support")]
impl<E: run_ports::Edge> run_ports::Edge for Dying<E> {
    async fn serve(
        &self,
        book: angzarr_embedded::v1::CommandBook,
        arrival: run_ports::Arrival,
    ) -> Result<angzarr_embedded::Answered, run_ports::Unserved> {
        self.inner.serve(book, arrival).await
    }

    async fn handle(
        &self,
        book: angzarr_embedded::v1::CommandBook,
    ) -> Result<angzarr_embedded::Answered, angzarr_embedded::Failure<angzarr_embedded::Rejected>>
    {
        self.inner.handle(book).await
    }

    async fn serve_sync(
        &self,
        book: angzarr_embedded::v1::CommandBook,
        projectors: Vec<String>,
        arrival: run_ports::Arrival,
    ) -> Result<run_ports::SyncAnswered, run_ports::Unserved> {
        let answered = self.inner.serve_sync(book, projectors, arrival).await;
        if answered.is_ok() && self.files.take_die_after_apply() {
            // Killed outright: no answer, no drain, no discovery removed.
            std::process::abort();
        }
        answered
    }

    async fn events(
        &self,
        cover: &angzarr_embedded::v1::Cover,
        from: u64,
    ) -> Result<Vec<angzarr_embedded::v1::EventPage>, angzarr_embedded::StoreError> {
        self.inner.events(cover, from).await
    }
}

/// Under test-support (U6): the core's `edge` tells each book it applied to
/// the kill switch, which kills the daemon outright once a book of the step
/// it names is applied, before it is answered: `enqueued` (a run's
/// `EnqueueRun`), `minted` (a `MintSession`), `started` (a
/// `RunnerStarted`), `turn` (a `TurnStarted`), `hook` (a hook's capture
/// fact). Forced, never waited for (the race rule).
#[cfg(feature = "test-support")]
fn dies_at_steps(edge: &Arc<CoreEdge>, files: &DaemonFiles) {
    let files = files.clone();
    edge.telling(Box::new(move |book, _| {
        if let Some(step) = step_of(book) {
            if files.take_die_at(step) {
                std::process::abort();
            }
        }
    }));
}

/// The kill switch's step a book applied is, if any.
#[cfg(feature = "test-support")]
fn step_of(book: &angzarr_embedded::v1::CommandBook) -> Option<&'static str> {
    use angzarr_embedded::v1::command_page::Body;
    let payload = match book.page.as_ref()?.body.as_ref()? {
        Body::Command(command) => command.payload.as_ref()?,
        Body::Fact(fact) => fact.payload.as_ref()?,
    };
    let name = payload.type_url.rsplit('.').next()?;
    Some(match name {
        "EnqueueRun" => "enqueued",
        "MintSession" => "minted",
        "RunnerStarted" => "started",
        "TurnStarted" => "turn",
        "RecordPromptSubmitted"
        | "RecordAssistantText"
        | "RecordToolCall"
        | "RecordTurnEnded"
        | "RecordSessionEnded"
        | "RecordCompaction" => "hook",
        _ => return None,
    })
}

/// The launcher (T18), composed: its ports over this process's home, the
/// core's `edge` (`home` and `temp` the process's), the folds `kept`
/// registered in the kernel, the daemon's
/// listener as `told`, and the log.
fn launcher(
    fs: &Arc<dyn Fs>,
    (home, temp): (&Path, &Path),
    edge: &Arc<CoreEdge>,
    kept: &Projectors,
    (engines, user_home, env): (
        Vec<daemon_application::HostedEngine>,
        Option<PathBuf>,
        &Arc<process_env::EnvSnapshot>,
    ),
    told: Told,
    say: Say,
) -> Daemon<CoreEdge, OsTokenSource> {
    Daemon::new(Ports {
        edge: Arc::clone(edge),
        tokens: Arc::new(OsTokenSource),
        engines,
        plans: Box::new(Plans {
            home: home.to_path_buf(),
            temp: temp.to_path_buf(),
            user_home,
        }),
        hand_offs: Box::new(HandOffFiles(DaemonFiles::new(Arc::clone(fs), home))),
        homes: Box::new(Homes(Arc::clone(fs))),
        ears: Box::new(Pumps::new()),
        run_locks: Arc::new(liveness::RunLockFiles::new(
            Arc::clone(fs),
            home.to_path_buf(),
        )),
        place: told,
        folds: Folds {
            board: kept.board.clone().unwrap_or_default(),
            pending: kept.pending.clone().unwrap_or_default(),
            index: kept.index.clone().unwrap_or_default(),
        },
        say: Box::new(move |line: &str| said(&say, line)),
    })
    // A child's program found on its own PATH, as every launch's (T10).
    .finding(Arc::new(
        host_launcher::HostLauncher::default().searching(Arc::clone(fs)),
    ))
    // A child follows its agent's `workspace` (s4-Q11): its checkout under
    // ctxloom's home, let go WIP-safe at its exit and, lost, at the next
    // start's sweep (OD-19).
    .placing(Arc::new(Workspaces {
        fs: Arc::clone(fs),
        home: home.to_path_buf(),
        env: Arc::clone(env),
    }))
}

/// Tick until the daemon is asked to stop, its single-instance lock file
/// is no longer the one it holds (`held`: removed or replaced, so it
/// excludes nobody), another daemon's discovery replaced its own (`own`, in
/// `files`), or it has had nothing to do for its idle time. A discovery file that is gone is published again: the daemon
/// still holds the single-instance lock, and a client finds it by the file (M11).
async fn run<E: run_ports::Edge, T: run_ports::TokenSource>(
    launcher: &mut Daemon<E, T>,
    core: &InProcessCore<SqliteStore>,
    (files, own, held): (&DaemonFiles, &Discovery, &Instance),
    (pace, shared, terminals): (Pace, &Arc<Shared>, &daemon_application::Terminals),
    say: &Say,
) -> Result<(), String> {
    let mut quiet_since = Instant::now();
    // Asked to stop through its API (`StopDaemon`): drain, its hosted
    // engines ended first when asked `even_if_live` (S12).
    let mut stopping = false;
    loop {
        shared.tell(
            launcher.hosted_runs(),
            launcher.hosted_one_shots(),
            quiet_since,
        );
        if !files.still_holds(held) {
            said(
                say,
                "the single-instance lock file was replaced or removed: exiting",
            );
            return Ok(());
        }
        match advertised(own, files.discovery().as_ref()) {
            Advertised::Ours => {}
            Advertised::Gone => match files.publish(own) {
                Ok(()) => said(say, "the discovery file was gone: advertised again"),
                Err(e) => said(
                    say,
                    &format!("the discovery file was gone, and cannot be written: {e}"),
                ),
            },
            Advertised::Replaced => {
                said(
                    say,
                    "another daemon's discovery replaced this one's: exiting",
                );
                return Ok(());
            }
        }
        if let Some(end_hosted) = shared.take_stop() {
            if end_hosted {
                said(say, "asked to stop even if live: ending what it hosts");
                launcher.end_all();
            }
            stopping = true;
        }
        let draining = stopping;
        if draining && launcher.hosting() == 0 {
            return Ok(());
        }
        // Armed before the round reads (S-N2): a commit during it is not
        // missed.
        // Under test-support, killed outright while a hosted child's turn is
        // in flight, when the switch says so (U6).
        #[cfg(feature = "test-support")]
        if launcher.in_flight() && files.take_die_at("turn") {
            std::process::abort();
        }
        let commit = core.store().next_commit();
        let round = shared.round_starts();
        let busy = launcher.tick(core.store(), core.catalog(), draining).await;
        shared.round_done(round);
        // A session it prepares (asked for, or its frontend's TUI running)
        // keeps it: its frontend is a client it serves (U5).
        if busy || launcher.hosting() > 0 || terminals.hosting() > 0 {
            quiet_since = Instant::now();
        } else if quiet_since.elapsed() >= pace.idle {
            // M9: a run any process started and has not ended (an
            // interactive session the CLI runs) keeps it, once the runs
            // whose owner is gone are ended; asked only at the idle limit,
            // which is when it matters.
            match launcher.live_runs(core.store(), core.catalog()).await {
                Ok(0) => {
                    said(say, "idle: exiting");
                    return Ok(());
                }
                Ok(_) => quiet_since = Instant::now(),
                Err(detail) => {
                    said(say, &format!("the live runs cannot be read: {detail}"));
                    quiet_since = Instant::now();
                }
            }
        }
        // The push: a commit through this process's store handles (the
        // daemon's own writes, its listener's hooks) wakes the next round at
        // once; another process's (a CLI command's) is seen by the poll on
        // `change_token` (S-N4: the required fallback).
        // A read of the API asking for a fresh round wakes it too.
        let _ = tokio::time::timeout(pace.poll, either(commit, shared.wake.notified())).await;
    }
}

/// Whichever of `a` and `b` is ready first.
async fn either(a: impl std::future::Future, b: impl std::future::Future) {
    let mut a = std::pin::pin!(a);
    let mut b = std::pin::pin!(b);
    std::future::poll_fn(|cx| {
        match a.as_mut().poll(cx).is_ready() || b.as_mut().poll(cx).is_ready() {
            true => std::task::Poll::Ready(()),
            false => std::task::Poll::Pending,
        }
    })
    .await;
}

/// How long a read of the API waits for the loop's next round (its catch-up)
/// before it answers from the folds as they are.
const FRESH_WAIT: Duration = Duration::from_secs(2);

/// What the loop shares with its API's use cases (slice 2 step 5): where
/// it serves, what it hosts and since when it has been quiet, as of its
/// last round, and whether it was asked to stop.
struct Shared {
    index: session_index::SessionIndexes,
    state: Mutex<Round>,
    /// The rounds the loop has started.
    started: Mutex<u64>,
    /// The last round the loop finished.
    done: tokio::sync::watch::Sender<u64>,
    /// Wakes the loop for a round a read asked for.
    wake: tokio::sync::Notify,
}

#[derive(Default)]
struct Round {
    endpoint: String,
    /// Each hosted run's session id, run root and kind.
    hosted: Vec<(String, String, daemon_proto::v1::hosted_run::Kind)>,
    quiet_since: Option<Instant>,
    /// Asked to stop; `true` to end what it hosts first.
    stop: Option<bool>,
}

impl Shared {
    fn new(index: session_index::SessionIndexes) -> Shared {
        Shared {
            index,
            state: Mutex::new(Round::default()),
            started: Mutex::new(0),
            done: tokio::sync::watch::Sender::new(0),
            wake: tokio::sync::Notify::new(),
        }
    }

    /// A round starts: its number.
    fn round_starts(&self) -> u64 {
        let mut started = self.started.lock().unwrap_or_else(PoisonError::into_inner);
        *started += 1;
        *started
    }

    /// The round `round` is done: a read waiting for it answers.
    fn round_done(&self, round: u64) {
        self.done.send_replace(round);
    }

    fn round(&self) -> std::sync::MutexGuard<'_, Round> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn serving_at(&self, endpoint: &str) {
        self.round().endpoint = endpoint.to_string();
    }

    /// What the round found it hosts: its children (`children`) and its
    /// one-shots, each its session id and run root.
    fn tell(
        &self,
        children: Vec<(String, String)>,
        one_shots: Vec<(String, String)>,
        quiet_since: Instant,
    ) {
        use daemon_proto::v1::hosted_run::Kind;
        let mut round = self.round();
        round.hosted = children
            .into_iter()
            .map(|(session, run)| (session, run, Kind::Child))
            .chain(
                one_shots
                    .into_iter()
                    .map(|(session, run)| (session, run, Kind::OneShot)),
            )
            .collect();
        round.quiet_since = Some(quiet_since);
    }

    fn take_stop(&self) -> Option<bool> {
        self.round().stop.take()
    }
}

impl daemon_api::Hosting for Shared {
    fn endpoint(&self) -> String {
        self.round().endpoint.clone()
    }

    fn hosted(&self) -> Vec<daemon_proto::v1::HostedRun> {
        let hosted = self.round().hosted.clone();
        let index = self.index.index();
        hosted
            .into_iter()
            .map(|(session, run, kind)| {
                let record = index.find_by_id(&session).unwrap_or_default();
                let project = index.project_harp(&record.project_id).unwrap_or_default();
                daemon_proto::v1::HostedRun {
                    session: index.name_of(&record),
                    harp: record.harp,
                    run: format!("projects/{project}/runs/{run}"),
                    kind: kind as i32,
                }
            })
            .collect()
    }

    fn idle_seconds(&self) -> u64 {
        self.round()
            .quiet_since
            .map_or(0, |since| since.elapsed().as_secs())
    }

    fn stop(&self, end_hosted: bool) {
        let mut round = self.round();
        round.stop = Some(end_hosted || round.stop.unwrap_or(false));
    }

    /// The next round to START after this call (so its catch-up reads what
    /// was committed before it), waited for within [`FRESH_WAIT`].
    fn fresh(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let mut done = self.done.subscribe();
            let need = *self.started.lock().unwrap_or_else(PoisonError::into_inner) + 1;
            self.wake.notify_one();
            let _ = tokio::time::timeout(FRESH_WAIT, done.wait_for(|round| *round >= need)).await;
        })
    }
}

/// A session's state, from its liveness lock in its project.
struct Locks(Arc<dyn Fs>);

impl daemon_api::SessionStates for Locks {
    fn state(&self, project_dir: &str, harp: &str) -> session_index::State {
        use session_ports::{Liveness, LockState};
        let mut locks = liveness::LockFiles::new(Arc::clone(&self.0), project_dir);
        match locks.lock_state(harp) {
            LockState::Held => session_index::State::Live,
            LockState::Free => session_index::State::Ended,
            LockState::Unproven => session_index::State::Unknown,
        }
    }
}

/// Fresh project harps, minted as a CLI command's are.
struct FreshHarps;

impl daemon_api::Harps for FreshHarps {
    fn fresh(&self) -> Vec<String> {
        crate::fresh_project_harps()
    }
}

/// What a serving daemon's discovery file says of it, at a tick.
#[derive(Debug, PartialEq, Eq)]
enum Advertised {
    /// Its own advertisement: nothing to do.
    Ours,
    /// None (removed, or its home's directory removed): it advertises
    /// itself again, since it still holds the single-instance lock.
    Gone,
    /// Another daemon's: it lost the single-instance lock (its lock file was replaced),
    /// so it exits.
    Replaced,
}

/// What `found` (the file now) is to the daemon that advertised `own`.
fn advertised(own: &Discovery, found: Option<&Discovery>) -> Advertised {
    match found {
        None => Advertised::Gone,
        Some(found) if found == own => Advertised::Ours,
        Some(_) => Advertised::Replaced,
    }
}

/// A child's plan: its project's dry run, as a ctxloom process in that
/// project would make it (`child_plan`).
struct Plans {
    home: PathBuf,
    temp: PathBuf,
    /// The user's own home (the account's): her git configuration's.
    user_home: Option<PathBuf>,
}

impl ChildPlans for Plans {
    fn preview(
        &self,
        engine: &str,
        agent: &str,
        project_dir: &str,
        env: &BTreeMap<String, String>,
    ) -> Result<library_application::Previewed, String> {
        super::child_plan(&self.home, &self.temp, engine, agent, project_dir, env)
    }

    fn git_user(
        &self,
        project_dir: &str,
        env: &BTreeMap<String, String>,
    ) -> Option<run_ports::GitUser> {
        super::child_git_user(&self.temp, self.user_home.as_deref(), project_dir, env)
    }
}

/// The hand-off files under the daemon's directory (T6: read at each of
/// the child's starts, deleted at its close, swept).
struct HandOffFiles(DaemonFiles);

impl HandOffs for HandOffFiles {
    fn read(&self, session: &str) -> Option<daemon_application::HandOff> {
        self.0.read_hand_off(session)
    }

    fn delete(&self, session: &str) {
        self.0.delete_hand_off(session);
    }

    fn claim(&self, session: &str) {
        self.0.claim_hand_off(session);
    }

    fn list(&self) -> Vec<daemon_application::Kept> {
        self.0.hand_offs()
    }
}

/// A child's session directory in its project: `session-files`'.
struct Homes(Arc<dyn Fs>);

impl ChildHomes for Homes {
    fn records(&self, project_dir: &str) -> Box<dyn SessionRecords> {
        Box::new(SessionDirs::new(
            Arc::clone(&self.0),
            PathBuf::from(project_dir),
        ))
    }

    /// Its session's lock, `liveness`' (the platform's file lock), the
    /// sessions directory made first.
    fn hold_session(&self, project_dir: &str, harp: &str) -> Result<run_ports::HeldLock, String> {
        use session_ports::SessionLock;
        let mut locks = liveness::LockFiles::new(Arc::clone(&self.0), project_dir);
        let path = locks.path(harp);
        if let Some(dir) = path.parent() {
            self.0
                .create_dir_all(dir, os::Mode::from_bits(0o755))
                .map_err(|e| e.to_string())?;
        }
        let held = locks.hold(harp).map_err(|e| e.report().to_string())?;
        Ok(run_ports::HeldLock::new(path.display().to_string(), held))
    }
}

/// A child's project's workspaces: worktree-git's, its checkouts under
/// ctxloom's home (`isolation_ports::worktrees_root`).
struct Workspaces {
    fs: Arc<dyn Fs>,
    home: PathBuf,
    env: Arc<process_env::EnvSnapshot>,
}

impl daemon_application::ChildWorkspaces for Workspaces {
    fn of(
        &self,
        project_id: &str,
        project_dir: &str,
    ) -> Arc<dyn isolation_ports::Workspaces + Send + Sync> {
        Arc::new(worktree_git::GitWorkspaces::new(
            Arc::clone(&self.fs),
            PathBuf::from(project_dir),
            Some(isolation_ports::worktrees_root(&self.home, project_id)),
            Arc::clone(&self.env),
        ))
    }
}

/// Each child's observations pumped from a thread of its own onto one
/// channel the launcher drains.
struct Pumps {
    tell: Mutex<Sender<(String, ChildObservation)>>,
    heard: Mutex<Receiver<(String, ChildObservation)>>,
}

impl Pumps {
    fn new() -> Pumps {
        let (tell, heard) = channel();
        Pumps {
            tell: Mutex::new(tell),
            heard: Mutex::new(heard),
        }
    }
}

impl Ears for Pumps {
    fn listen(&self, run: String, observations: Box<dyn Iterator<Item = ChildObservation> + Send>) {
        let tell = self
            .tell
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        std::thread::spawn(move || {
            for observation in observations {
                if tell.send((run.clone(), observation)).is_err() {
                    return;
                }
            }
        });
    }

    fn heard(&self) -> Vec<(String, ChildObservation)> {
        let heard = self.heard.lock().unwrap_or_else(PoisonError::into_inner);
        std::iter::from_fn(|| heard.try_recv().ok()).collect()
    }
}

#[cfg(test)]
#[path = "serve.test.rs"]
mod tests;
