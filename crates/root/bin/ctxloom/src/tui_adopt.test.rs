//! A daemon dying under a live TUI (owner rulings, 2026-11-14), end to end:
//! the real binary, a temporary home, the terminal stood in
//! (`CTXLOOM_TEST_TERMINAL`) by a stub claude that records itself and runs
//! until it is released.
//!
//! - U5's start-report race, FORCED: the frontend waits at a gate (test
//!   support) between the daemon's answer to its plan request and its start
//!   report; the daemon is killed there and the next command starts a new
//!   one. The frontend took the run's own lock BEFORE it asked, so the new
//!   daemon's start reads the run's owner alive: nothing is orphaned, the
//!   run is adopted, and the TUI's exit ends its incarnation `runner-exit`.
//! - A daemon killed under a live TUI: the next daemon adopts the session,
//!   and the TUI's exit report ends the incarnation and lets its checkout go
//!   WIP-safe, as the daemon that prepared it would have.
//! - The frontend's reports held: no daemon can start while the start is
//!   reported, so it is held in the session's hold; the exit's attempt
//!   reaches a new daemon and delivers the held start, then the exit, in
//!   order.
//!
//! Nothing is timed: each step waits, bounded, for what the store, the stub
//! or the files say happened.

use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::library::Project;
use crate::tests::files;

/// The TUI: it records its harp and directory, says it started, and runs
/// until `release` is beside it.
const TUI: &str = r#"here="$(dirname "$0")"
echo "${CTXLOOM_SESSION##*/}" > "$here/tui-harp"
pwd -P > "$here/tui-cwd"
: > "$here/tui-started"
i=0
while [ ! -e "$here/release" ] && [ "$i" -lt 1200 ]; do sleep 0.1; i=$((i+1)); done
"#;

/// What holds the daemon's single-instance lock while `unblock` is not
/// beside it, so no daemon can start (`flock`, the platform's lock).
const BLOCKS_DAEMONS: &str = r#"exec flock -w 20 "$1" sh -c 'i=0; while [ ! -e "$0" ] && [ "$i" -lt 1200 ]; do sleep 0.1; i=$((i+1)); done' "$2"
"#;

/// How long any one thing is waited for.
const WAIT: Duration = Duration::from_secs(60);

struct Rig {
    project: Project,
    bin: PathBuf,
    path: String,
}

/// A project with a `dev` profile, the TUI stub as `claude` leading the
/// run's `PATH`, in a git repository of one commit; none where this host
/// runs no scripts (said).
fn rig() -> Option<Rig> {
    if !files::runs_scripts() {
        return None;
    }
    let project = Project::initialized();
    project.bundle("demo");
    project.profile("dev", "demo");
    let bin = project.home().join("bin");
    files::put_program(bin.join("claude"), TUI);
    let path = match crate::tests::host_var("PATH") {
        Some(host) => format!("{}:{}", bin.display(), host.to_string_lossy()),
        None => bin.display().to_string(),
    };
    let rig = Rig { project, bin, path };
    rig.git(&["init", "-q"]);
    rig.git(&["add", "-A"]);
    rig.git(&["commit", "-q", "-m", "the project"]);
    Some(rig)
}

impl Rig {
    fn env(&self) -> Vec<(&str, &str)> {
        vec![
            ("PATH", self.path.as_str()),
            ("CTXLOOM_TEST_TERMINAL", "1"),
            // A daemon a command starts idles out soon after the test.
            ("CTXLOOM_DAEMON_IDLE_SECONDS", "30"),
        ]
    }

    fn git(&self, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Alice",
                "-c",
                "user.email=alice@example.com",
            ])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(self.project.dir())
            .env_clear()
            .env("PATH", crate::tests::host_var("PATH").unwrap_or_default())
            .env("HOME", self.project.home())
            .env("TMPDIR", crate::tests::temp_root())
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?}: {output:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn home_file(&self, rel: &str) -> PathBuf {
        self.project.home().join(".ctxloom").join(rel)
    }

    /// `ctxloom run` with `args`, started, the TUI stood in.
    fn start_session(&self, args: &[&str]) -> Child {
        let mut all = vec!["run", "--profile", "dev"];
        all.extend_from_slice(args);
        self.project.start_with(&self.env(), &all)
    }

    fn run(&self, args: &[&str]) -> super::Ran {
        self.project.run_with(&self.env(), args)
    }

    /// The launch the frontend was handed, once it was.
    fn launch(&self) -> Value {
        let path = self.home_file("test-frontend-launch.json");
        wait(&self.log(), "the frontend's launch", || {
            files::exists(&path)
        });
        serde_json::from_str(&files::read(&path).expect("a launch")).expect("json")
    }

    /// The session's harp, as its launch's lock names it.
    fn harp(&self) -> String {
        let launch = self.launch();
        let lock = launch["session_lock"].as_str().expect("its lock");
        Path::new(lock)
            .file_stem()
            .expect("a lock file")
            .to_string_lossy()
            .into_owned()
    }

    /// The daemon serving now: its pid.
    fn daemon_pid(&self) -> Option<u64> {
        let status = self.run(&["daemon", "status", "--format", "json"]).json();
        status["pid"]
            .as_u64()
            .or_else(|| status["pid"].as_str().and_then(|pid| pid.parse().ok()))
    }

    /// Kill the serving daemon outright, and wait it gone.
    fn kill_daemon(&self) -> u64 {
        let pid = self.daemon_pid().expect("a daemon serves");
        let killed = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .expect("kill runs");
        assert!(killed.success(), "the daemon was killed");
        wait(&self.log(), "the killed daemon's end", || !alive(pid));
        pid
    }

    /// The events of `harp`'s session (or of its first run), each its type
    /// and (for an end) its cause.
    fn events(&self, harp: &str, run: bool) -> Vec<(String, String)> {
        let mut args = vec!["test", "session-events", harp];
        if run {
            args.extend(["--run", "1"]);
        }
        let ran = self.run(&args);
        if ran.code != 0 {
            return Vec::new();
        }
        ran.json()
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|e| {
                (
                    e["type"].as_str().unwrap_or_default().to_string(),
                    e["cause"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }

    fn types(&self, harp: &str, run: bool) -> Vec<String> {
        self.events(harp, run).into_iter().map(|(t, _)| t).collect()
    }

    /// The incarnation ends of `harp`'s session, each its cause.
    fn incarnations_ended(&self, harp: &str) -> Vec<String> {
        self.events(harp, false)
            .into_iter()
            .filter(|(t, _)| t == "IncarnationEnded")
            .map(|(_, cause)| cause)
            .collect()
    }

    /// Wait for `harp`'s incarnation to end, its cause.
    fn ended(&self, harp: &str) -> String {
        let mut cause = Vec::new();
        wait(&self.log(), "the incarnation's end", || {
            cause = self.incarnations_ended(harp);
            !cause.is_empty()
        });
        assert_eq!(cause.len(), 1, "{cause:?}");
        cause.remove(0)
    }

    fn release_tui(&self) {
        files::put(self.bin.join("release"), "go");
    }

    fn log(&self) -> String {
        files::read(self.home_file("daemon/daemon.log")).unwrap_or_default()
    }

    fn stop(&self) {
        let _ = self.run(&["daemon", "stop", "--even-if-live"]);
    }
}

/// Wait, bounded, for `ready`; `what` names it when it never comes.
fn wait(log: &str, what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "{what} never came; the daemon's log:\n{log}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Whether `pid` is there and not a zombie.
fn alive(pid: u64) -> bool {
    match files::read(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat
            .rfind(')')
            .and_then(|end| stat[end + 1..].split_whitespace().next())
            .is_some_and(|state| state != "Z"),
        Err(_) => std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success()),
    }
}

/// Whether `program` is on the host's `PATH`.
fn on_path(program: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn finished(child: Child) -> super::Ran {
    super::Ran::of(child.wait_with_output().expect("ctxloom finishes"))
}

// THE FORCING TEST of U5's start-report race (owner 2026-11-14): the daemon
// is killed between the frontend's plan request and its start report; the
// next daemon reads the run lock the frontend took before it asked as its
// owner alive, so nothing is orphaned, and the TUI's exit ends its
// incarnation `runner-exit` through the new daemon's adoption.
#[test]
fn test_a_daemon_killed_between_the_plan_and_the_start_report_orphans_nothing() {
    let Some(rig) = rig() else {
        return;
    };
    let gate = rig.home_file("test-frontend-gate");
    files::put(&gate, "wait");
    let session = rig.start_session(&[]);
    let harp = rig.harp();
    // The daemon answered the plan; the frontend waits at the gate.
    let killed = rig.kill_daemon();
    // The next command starts a new daemon, whose start sweeps.
    let listed = rig.run(&["session", "list"]);
    assert_eq!(listed.code, 0, "{}", listed.said());
    let now = rig.daemon_pid().expect("a new daemon serves");
    assert_ne!(now, killed, "another daemon");
    assert!(
        !rig.types(&harp, true).iter().any(|t| t == "RunEnded"),
        "the new daemon's start orphaned the run: {:?}\n{}",
        rig.events(&harp, true),
        rig.log()
    );
    files::remove(&gate);
    wait(&rig.log(), "the TUI's start", || {
        files::exists(rig.bin.join("tui-started"))
    });
    rig.release_tui();
    let ran = finished(session);
    assert_eq!(ran.code, 0, "{}", ran.said());
    assert_eq!(rig.ended(&harp), "runner-exit", "{}", rig.log());
    let run = rig.events(&harp, true);
    let ends: Vec<&(String, String)> = run.iter().filter(|(t, _)| t == "RunEnded").collect();
    assert_eq!(ends.len(), 1, "{run:?}");
    assert_eq!(ends[0].1, "RunnerExit", "{run:?}");
    assert_eq!(
        run.iter()
            .filter(|(t, _)| t == "RunnerStartedRecorded")
            .count(),
        1,
        "{run:?}"
    );
    rig.stop();
}

// A daemon killed under a live TUI (owner 2026-11-14, (1)): the next
// daemon adopts the session at its start, and the TUI's exit ends its
// incarnation and lets its checkout go, as the daemon that prepared it
// would have.
#[test]
fn test_a_daemon_killed_under_a_live_tui_is_adopted_and_the_exit_releases_the_checkout() {
    let Some(rig) = rig() else {
        return;
    };
    let session = rig.start_session(&["--workspace", "worktree"]);
    let harp = rig.harp();
    wait(&rig.log(), "the TUI's start", || {
        files::exists(rig.bin.join("tui-started"))
    });
    wait(&rig.log(), "the start recorded", || {
        rig.types(&harp, true)
            .iter()
            .any(|t| t == "RunnerStartedRecorded")
    });
    let checkout = files::read(rig.bin.join("tui-cwd"))
        .expect("its directory")
        .trim()
        .to_string();
    assert!(
        checkout.contains("/.ctxloom/worktrees/"),
        "the TUI works in its checkout: {checkout}"
    );
    let killed = rig.kill_daemon();
    let listed = rig.run(&["session", "list", "--format", "json"]);
    assert_eq!(listed.code, 0, "{}", listed.said());
    assert_ne!(rig.daemon_pid(), Some(killed), "another daemon");
    assert!(rig.incarnations_ended(&harp).is_empty(), "still live");
    assert!(
        rig.log().contains("adopted"),
        "the new daemon adopted it: {}",
        rig.log()
    );
    rig.release_tui();
    let ran = finished(session);
    assert_eq!(ran.code, 0, "{}", ran.said());
    assert_eq!(rig.ended(&harp), "runner-exit", "{}", rig.log());
    wait(&rig.log(), "the checkout let go", || {
        !files::exists(&checkout)
    });
    assert!(
        !rig.git(&["worktree", "list", "--porcelain"])
            .contains(&checkout),
        "its worktree record is gone"
    );
    let run = rig.types(&harp, true);
    assert_eq!(
        run.iter().filter(|t| *t == "RunEnded").count(),
        1,
        "{run:?}"
    );
    assert!(!run.iter().any(|t| t == "RunnerLostRecorded"), "{run:?}");
    rig.stop();
}

// The frontend's reports held (owner 2026-11-14, (2)): with the daemon
// killed in the gap and no daemon able to start, the start report is HELD
// in the session's hold (no 60 s retry), the run's own lock kept; once a
// daemon can start, the exit's attempt reaches it, which adopts the run (its
// lock held), and delivers the held start, then the exit, in order.
#[test]
fn test_the_frontends_reports_are_held_and_delivered_in_order() {
    let Some(rig) = rig() else {
        return;
    };
    if !on_path("flock") {
        eprintln!("SKIP test_the_frontends_reports_are_held_and_delivered_in_order: no flock");
        return;
    }
    let gate = rig.home_file("test-frontend-gate");
    files::put(&gate, "wait");
    let session = rig.start_session(&[]);
    let harp = rig.harp();
    // No daemon can start: its single-instance lock is held, by a blocker
    // waiting on it BEFORE the daemon is killed, so it takes the lock ahead
    // of the daemon the frontend's watch starts at once.
    let blocker = rig.bin.join("blocks-daemons");
    files::put_program(&blocker, BLOCKS_DAEMONS);
    let unblock = rig.bin.join("unblock");
    let mut blocking = std::process::Command::new(&blocker)
        .args([
            rig.home_file("daemon/daemon.lock").as_os_str(),
            unblock.as_os_str(),
        ])
        .spawn()
        .expect("the blocker starts");
    rig.kill_daemon();
    files::remove(&gate);
    wait(&rig.log(), "the TUI's start", || {
        files::exists(rig.bin.join("tui-started"))
    });
    let held = rig.home_file("held");
    // Its hold, not the hold's lock (taken before the hold is written).
    wait(&rig.log(), "the start held", || {
        files::children(&held)
            .iter()
            .any(|path| path.extension().is_some_and(|e| e == "held"))
    });
    files::put(&unblock, "go");
    let _ = blocking.wait();
    rig.release_tui();
    let ran = finished(session);
    assert_eq!(ran.code, 0, "{}", ran.said());
    assert!(
        ran.stderr.contains("held the session's start"),
        "{}",
        ran.said()
    );
    assert_eq!(rig.ended(&harp), "runner-exit", "{}", rig.log());
    let run = rig.events(&harp, true);
    let order: Vec<&str> = run
        .iter()
        .map(|(t, _)| t.as_str())
        .filter(|t| matches!(*t, "RunnerStartedRecorded" | "RunEnded"))
        .collect();
    assert_eq!(order, ["RunnerStartedRecorded", "RunEnded"], "{run:?}");
    assert!(
        run.iter()
            .any(|(t, cause)| t == "RunEnded" && cause == "RunnerExit"),
        "{run:?}"
    );
    rig.stop();
}

// Owner, 2026-11-15 (OD-62): `ctxloom shutdown now` asks a live TUI's
// frontend to stop (a frontend outlives its daemon, so the daemon alone is
// not enough), then the daemon the plain stop: the TUI is stopped as a
// SIGTERM to its frontend would stop it (ruling 114), its end is recorded,
// and nothing of ctxloom runs after.
#[test]
fn test_a_shutdown_now_stops_a_live_tuis_frontend_and_the_daemon() {
    let Some(rig) = rig() else {
        return;
    };
    let session = rig.start_session(&[]);
    let harp = rig.harp();
    wait(&rig.log(), "the TUI's start", || {
        files::exists(rig.bin.join("tui-started"))
    });
    wait(&rig.log(), "the start recorded", || {
        rig.types(&harp, true)
            .iter()
            .any(|t| t == "RunnerStartedRecorded")
    });
    let daemon = rig.daemon_pid().expect("a daemon serves");
    let shut = rig.run(&["shutdown", "now"]);
    assert_eq!(shut.code, 0, "{}\n{}", shut.said(), rig.log());
    assert!(
        shut.stdout.contains("asked the frontend pid"),
        "{}",
        shut.said()
    );
    assert!(
        shut.stdout
            .contains("stopped: nothing of ctxloom is running"),
        "{}",
        shut.said()
    );
    let _ = finished(session);
    wait(&rig.log(), "the stopped daemon's exit", || !alive(daemon));
    assert_eq!(rig.ended(&harp), "runner-exit", "{}", rig.log());
    rig.stop();
}

// A drain leaves a TUI to its human for the drain's whole timeout (no TUI
// is handed over): `ctxloom shutdown 2s` waits the two seconds, says the
// frontend still runs, then asks it to stop.
#[test]
fn test_a_shutdown_with_a_span_waits_it_out_for_a_live_tui_then_stops_it() {
    let Some(rig) = rig() else {
        return;
    };
    let session = rig.start_session(&[]);
    wait(&rig.log(), "the TUI's start", || {
        files::exists(rig.bin.join("tui-started"))
    });
    let shut = rig.run(&["shutdown", "2s"]);
    if shut.code != 0 {
        rig.release_tui();
        let ran = finished(session);
        panic!("DEBUG {}\nSESSION {}", shut.said(), ran.said());
    }
    assert!(
        shut.stdout.contains("after the drain's 2s, still running")
            && shut.stdout.contains("asked the frontend pid"),
        "{}",
        shut.said()
    );
    let _ = finished(session);
    rig.stop();
}
