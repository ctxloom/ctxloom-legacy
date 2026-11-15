//! The steps of the user daemon's spec (ruling 132, the target T1-T12):
//! `journeys/j002700_user_daemon.feature`, `cli/daemon.feature`,
//! `cli/session_children.feature` and the daemon Rule of `cli/run.feature`,
//! as the owner's rulings on the spec calls (S1-S20, 2026-11-11) shaped them;
//! and trust and approvals (ruling 142): `journeys/j002800_trusting_a_project.feature`,
//! `cli/trust.feature` and `cli/session_approvals.feature`; and the
//! architecture rulings T13-T30: `cli/mcp_serve.feature` and
//! `cli/atomic_writes.feature`.
//!
//! RED BY DESIGN. The spec is committed before the daemon is reworked to
//! it, so every step here that the existing harness cannot already take is a
//! stub that FAILS, naming itself: nothing is implemented. Each stub becomes
//! a real step with the milestone that builds what it asserts.

use std::path::PathBuf;
use std::process::Child;
use std::time::{Duration, Instant};

use cucumber::{given, then, when};
use serde_json::{json, Value};

use super::World;
use crate::tests::files;
use crate::tests::gherkin_common::CommonWorld;

/// The variable a client gives the daemon it spawns as its idle time.
const IDLE_VARIABLE: &str = "CTXLOOM_DAEMON_IDLE_SECONDS";

/// The idle time of a daemon a scenario starts, in seconds: short, so a
/// scenario's wait for it is.
const IDLE_SECONDS: u64 = 2;

/// The idle time of a daemon a scenario's command starts: long enough to
/// outlive the scenario's steps, short enough to leave nothing for long.
const SPAWNED_IDLE_SECONDS: u64 = 15;

/// What a scenario started: the daemon, and the run that keeps it busy. A
/// World drops this FIRST (its first field), before its home is removed:
/// the run is released and waited for, the daemon killed outright (T3: it
/// is kill-tolerant) and reaped.
#[derive(Default)]
pub(super) struct State {
    pub(super) daemon: Option<Child>,
    run: Option<Child>,
    /// The file whose presence lets the stub engine end.
    release: Option<PathBuf>,
    /// What a client's call to the daemon came to.
    called: Option<Result<(), daemon_ports::Called>>,
    /// The discovery of a daemon since stopped.
    old: Option<run_ports::Discovery>,
    /// The daemon a command was seen to start.
    first: Option<run_ports::Discovery>,
    /// What two commands run at once came to.
    both: Vec<crate::tests::Ran>,
    /// The engine a registration scenario's session ran on, and where its
    /// stub recorded what it was handed.
    registered: Option<(String, PathBuf)>,
    /// What an engine's call of a ctxloom tool was answered.
    tool: Option<Value>,
    /// The daemon a tool's call found before it was killed.
    before_kill: Option<run_ports::Discovery>,
    /// A second project's directory (U5: two sessions, two projects).
    second: Option<PathBuf>,
    /// The daemon a step killed outright, by its pid.
    killed: Option<u32>,
    /// Where the TUI stub records itself, when not on the scenario's PATH.
    tui_dir: Option<PathBuf>,
    /// The hook the session's assistant fired last.
    fired: Option<Fired>,
}

/// A hook the session's assistant fired (s4-Q14, H2): what it came to, how
/// long it took, its session's events before it, and what the same hook
/// answers the engine when it records nothing.
pub(super) struct Fired {
    ran: crate::tests::Ran,
    took: Duration,
    harp: String,
    events_before: usize,
    unrecorded_answer: String,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(release) = &self.release {
            files::put(release, "go");
        }
        if let Some(mut run) = self.run.take() {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline && matches!(run.try_wait(), Ok(None)) {
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = run.kill();
            let _ = run.wait();
        }
        if let Some(mut daemon) = self.daemon.take() {
            let _ = daemon.kill();
            let _ = daemon.wait();
        }
    }
}

/// The discovery file of the scenario's home.
fn discovery_path(w: &World) -> PathBuf {
    w.common
        .project
        .home()
        .join(".ctxloom/daemon/daemon.discovery")
}

fn daemon_log_text(w: &World) -> String {
    files::read(w.common.project.home().join(".ctxloom/daemon/daemon.log")).unwrap_or_default()
}

/// The scenario's environment, as `(name, value)` pairs.
fn env_of(w: &World) -> Vec<(String, String)> {
    w.common.env.clone()
}

/// `ctxloom args` in the project, now, with the scenario's environment.
fn run_now(w: &World, args: &[&str]) -> crate::tests::Ran {
    let env = env_of(w);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    w.common.project.run_with(&env, args)
}

/// Whether `daemon status` says a daemon serves (its health check): its
/// answer is the daemon, which has a pid; none serving, the empty message.
fn serving_now(w: &World) -> bool {
    let status = run_now(w, &["daemon", "status", "--format", "json"]);
    assert_eq!(status.code, 0, "{}", status.said());
    status.json().get("pid").is_some()
}

/// Start `ctxloom daemon serve` with the scenario's short idle time, and
/// wait for it to answer.
fn start_daemon(w: &mut World) {
    start_daemon_idling(w, IDLE_SECONDS);
}

/// Start `ctxloom daemon serve` with the idle time `idle` (seconds), and
/// wait for it to answer.
fn start_daemon_idling(w: &mut World, idle: u64) {
    let env = env_of(w);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let idle = idle.to_string();
    let daemon = w
        .common
        .project
        .start_with(&env, &["daemon", "serve", "--idle-seconds", &idle]);
    w.daemon_spec.daemon = Some(daemon);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !serving_now(w) {
        assert!(
            Instant::now() < deadline,
            "the daemon never served; its log:\n{}",
            daemon_log_text(w)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The project's config: one agent on claude-code, the default.
const PROJECT_CONFIG: &str =
    "schema_version: 9\nagents:\n  dev:\n    engine: claude-code\ndefault_agent: dev\n";

/// Point claude-code at the stub `body` in the scenario's home (it runs
/// with `$here` its own directory); the stub's directory.
fn stub_claude(w: &mut World, body: &str) -> PathBuf {
    let dir = w.common.project.home().join("fake-claude");
    files::put_program(dir.join("claude"), body);
    w.common
        .project
        .write(".ctxloom/config.yaml", PROJECT_CONFIG);
    w.common.project.write_home(
        ".ctxloom/config.yaml",
        &format!(
            "schema_version: 9\neditor:\n  command: \"true\"\nengines:\n  claude-code:\n    command: {}\n",
            dir.join("claude").display()
        ),
    );
    dir
}

/// The interactive session's stub: it says it started, then runs until
/// released (bounded, so a lost scenario ends anyway).
const INTERACTIVE_STUB: &str = r#"here=$(dirname "$0")
: > "$here/started"
i=0
while [ ! -e "$here/release" ] && [ "$i" -lt 600 ]; do sleep 0.1; i=$((i+1)); done
"#;

/// An interactive session the CLI runs (`ctxloom run` at a terminal, the
/// test's stand-in for one), started and not ended: prepared by the daemon
/// serving (U5), its run in the one store, its session's lock held by the
/// command.
fn interactive_session(w: &mut World) {
    let dir = stub_claude(w, INTERACTIVE_STUB);
    w.daemon_spec.release = Some(dir.join("release"));
    let mut env = env_of(w);
    env.push(("CTXLOOM_TEST_TERMINAL".to_string(), "1".to_string()));
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    w.daemon_spec.run = Some(w.common.project.start_with(&env, &["run"]));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !files::exists(dir.join("started")) {
        assert!(
            Instant::now() < deadline,
            "the session's engine never started"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A hosted child's stub (engine-claude's streaming child): its
/// environment written once, an init, then for each message given a line
/// of `turns` (its conversation's id, then the message) and a result; a
/// turn begun while `hold` is there is left in flight until the interrupt
/// (`interrupted`), which ends it aborted; `/clear` starts a new
/// conversation (a new id), as claude's own does. It ends when its input
/// does.
const CHILD_STUB: &str = r#"here=$(dirname "$0")
env > "$here/child-env"
n=1
sid=S1
busy=
printf '{"type":"system","subtype":"init","cwd":"/work","session_id":"%s","tools":[]}\n' "$sid"
while IFS= read -r line; do
  case "$line" in
    *'"control_request"'*)
      : > "$here/interrupted"
      printf '{"type":"control_response","response":{"subtype":"success"}}\n'
      if [ -n "$busy" ]; then
        busy=
        printf '{"type":"result","subtype":"error_during_execution","is_error":false,"terminal_reason":"aborted_streaming","result":"","session_id":"%s"}\n' "$sid"
      fi
      continue ;;
    *'"/clear"'*)
      n=$((n+1))
      sid="S$n"
      printf '{"type":"conversation_reset","session_id":"%s"}\n' "$sid"
      printf '{"type":"result","subtype":"success","is_error":false,"result":"","session_id":"%s"}\n' "$sid"
      continue ;;
  esac
  printf '%s %s\n' "$sid" "$line" >> "$here/turns"
  : > "$here/child-turn"
  if [ -e "$here/hold" ]; then
    rm -f "$here/hold"
    busy=1
    continue
  fi
  printf '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"%s"}\n' "$sid"
done
: > "$here/child-exited"
"#;

/// The stub child's directory.
fn stub_dir(w: &World) -> PathBuf {
    w.common.project.home().join("fake-claude")
}

/// The lines of `turns` the stub child wrote: `(conversation, message)`.
fn turns(w: &World) -> Vec<(String, String)> {
    files::read(stub_dir(w).join("turns"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(sid, rest)| (sid.to_string(), rest.to_string()))
        .collect()
}

/// Wait, at most `limit`, for `ready` to answer.
fn until<T>(
    w: &World,
    what: &str,
    limit: Duration,
    mut ready: impl FnMut(&World) -> Option<T>,
) -> T {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(found) = ready(w) {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; the daemon's log:\n{}",
            daemon_log_text(w)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// How long a scenario waits for what its daemon does.
const DAEMON_WAIT: Duration = Duration::from_secs(30);

/// The turn whose message holds `text`, once the stub has taken one.
fn turn_on(w: &World, text: &str) -> (String, String) {
    until(w, &format!("a turn on {text:?}"), DAEMON_WAIT, |w| {
        turns(w).into_iter().find(|(_, line)| line.contains(text))
    })
}

/// The session `harp` as `session show` has it.
fn shown(w: &World, harp: &str) -> Value {
    let ran = run_now(w, &["session", "show", harp, "--format", "json"]);
    assert_eq!(ran.code, 0, "{}", ran.said());
    ran.json()
}

/// The daemon's hand-off directory of the scenario's home.
fn launch_dir(w: &World) -> PathBuf {
    daemon_dir(w).join("launch")
}

/// The hand-off files there are.
fn hand_offs(w: &World) -> Vec<PathBuf> {
    files::children(launch_dir(w))
}

/// The child stub's claude and a daemon this scenario owns (it hosts the
/// children for the scenario's life, so its default idle time), for a
/// scenario whose own command launches its first child; every daemon a
/// command spawns idles out soon.
fn hosting_daemon(w: &mut World) {
    if w.daemon_spec.daemon.is_some() {
        return;
    }
    stub_claude(w, CHILD_STUB);
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
    start_daemon_idling(w, DEFAULT_IDLE_SECONDS);
}

/// Before a scenario's command: one that launches a child is given the
/// stub child and a daemon the scenario owns, when its steps set up none
/// (no engine on the host's `PATH` is ever started).
pub(super) fn before_line(w: &mut World, line: &str) {
    if line.trim_start().starts_with("ctxloom session launch") {
        hosting_daemon(w);
    }
}

/// A child the daemon hosts: launched by the user for a seeded owner
/// (`session launch`), and seen to take its first turn.
fn hosted_child(w: &mut World) {
    let dir = stub_claude(w, CHILD_STUB);
    let seeded = run_now(w, &["test", "seed-session", "amber-quiet-heron"]);
    assert_eq!(seeded.code, 0, "{}", seeded.said());
    let mut env = env_of(w);
    env.push(("ANTHROPIC_API_KEY".to_string(), "a-key".to_string())); // gitleaks:allow
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let launched = w.common.project.run_with(
        &env,
        &[
            "session",
            "launch",
            "amber-quiet-heron",
            "--name",
            "brisk-copper-moth",
            "a task",
        ],
    );
    assert_eq!(launched.code, 0, "{}", launched.said());
    let deadline = Instant::now() + Duration::from_secs(30);
    while !files::exists(dir.join("child-turn")) {
        assert!(
            Instant::now() < deadline,
            "the child never took its turn; the daemon's log:\n{}",
            daemon_log_text(w)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A hosted child whose first turn has ended: its owner was told so.
fn idle_child(w: &mut World) {
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
    start_daemon_idling(w, DEFAULT_IDLE_SECONDS);
    hosted_child(w);
    until(w, "the child's first turn to end", DAEMON_WAIT, |w| {
        let owner = shown(w, "amber-quiet-heron");
        owner["pending"]
            .as_array()
            .is_some_and(|pending| {
                pending.iter().any(|m| {
                    m["text"]
                        .as_str()
                        .is_some_and(|t| t.contains("finished its turn"))
                })
            })
            .then_some(())
    });
}

/// A step of the daemon's spec that nothing implements yet.
fn red(step: &str) -> ! {
    panic!("RED (ruling 132, the daemon spec): not built yet: {step}")
}

/// The `Alice ...:` steps of the spec that need a harness not built yet
/// (the generic `Alice ...:` step is the World's `alice_runs`, which hands
/// it here): whether it is one. The ones the existing harness can run (her
/// own commands, typed) are left to it.
pub(super) fn claims(w: &mut World, step: &cucumber::gherkin::Step) -> bool {
    let text = step.value.as_str();
    if text.contains("which her daemon runs") {
        hosted_one_shot(w, step);
        return true;
    }
    if text.contains("presses Ctrl-C while it works") {
        stopped_one_shot(w, step, "INT");
        return true;
    }
    if let Some(signal) = sent_signal(text) {
        stopped_one_shot(w, step, signal);
        return true;
    }
    if text.contains("through her daemon") {
        tui_session(w, step, false);
        return true;
    }
    if text.contains("its daemon is killed outright while it works") {
        tui_session(w, step, true);
        return true;
    }
    if text.contains("in both projects at once") {
        tui_in_both(w, step);
        return true;
    }
    let stubbed = text.contains("at her terminal, and answers yes when asked")
        || text.contains("says yes at her terminal");
    if stubbed {
        red(text);
    }
    false
}

/// The signal a `... ctxloom is sent a <SIG> while the assistant works:`
/// step names, as `kill` names it.
fn sent_signal(text: &str) -> Option<&'static str> {
    let rest = text.split("and ctxloom is sent a ").nth(1)?;
    ["HUP", "INT", "TERM"]
        .into_iter()
        .find(|signal| rest.starts_with(&format!("SIG{signal} ")))
}

/// Where the scenario's stub assistant is, and writes what it saw: the
/// directory on the scenario's PATH ("Alice's assistant is Claude Code").
fn assistant_dir(w: &World) -> PathBuf {
    w.adopt_scratch().join("bin")
}

/// What the stub assistant of a hosted one-shot records of itself before
/// it answers or works: its parent (the process that started it), and
/// whether its session's lock was held by another process while it ran
/// (`held`; `unknown` where no `flock` is there to ask).
const RECORDS_ITSELF: &str = r#"here="$(dirname "$0")"
echo "$PPID" > "$here/ppid"
echo "${CTXLOOM_SESSION##*/}" > "$here/watched-harp"
lock="${CTXLOOM_ROOT}/.ctxloom/sessions/${CTXLOOM_SESSION##*/}.lock"
if command -v flock >/dev/null 2>&1; then
  if flock -n "$lock" true 2>/dev/null; then echo free > "$here/lock-probe"; else echo held > "$here/lock-probe"; fi
else
  echo unknown > "$here/lock-probe"
fi
"#;

/// A hosted one-shot's stub turn: it records itself, takes its prompt from
/// its input, and answers (engine-claude's stream).
fn answering_stub() -> String {
    format!(
        r#"{RECORDS_ITSELF}cat > /dev/null
echo '{{"type":"system","subtype":"init","session_id":"9d764b45-a56a-48d7-ac9d-c2a16c5669f1"}}'
echo '{{"type":"result","subtype":"success","is_error":false,"result":"J001000-CHILD-REPLY","session_id":"9d764b45-a56a-48d7-ac9d-c2a16c5669f1"}}'
"#
    )
}

/// A hosted one-shot's stub turn that works until it is asked to stop: it
/// records itself and its pid (once it works), and on a SIGTERM says it was
/// asked and exits.
fn working_stub() -> String {
    format!(
        r#"{RECORDS_ITSELF}trap 'echo asked > "$here/asked"; exit 0' TERM
echo "$$" > "$here/pid.tmp" && mv "$here/pid.tmp" "$here/pid"
sleep 30 &
wait
"#
    )
}

/// `w`'s environment as `run_with` takes it.
fn env_pairs(env: &[(String, String)]) -> Vec<(&str, &str)> {
    env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

/// The ctxloom line of a step's doc string, as its words.
fn line_words(step: &cucumber::gherkin::Step) -> Vec<String> {
    let lines = crate::tests::gherkin_common::doc(step);
    crate::tests::gherkin_common::split(
        lines
            .trim()
            .strip_prefix("ctxloom ")
            .expect("a ctxloom line"),
    )
}

/// The one-shot of the step, its assistant the answering stub, run to its
/// end: the daemon the command starts hosts it.
fn hosted_one_shot(w: &mut World, step: &cucumber::gherkin::Step) {
    files::put_program(assistant_dir(w).join("claude"), &answering_stub());
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
    w.before_run();
    let words = line_words(step);
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    let env = env_of(w);
    w.common.ran = Some(w.common.project.run_with(&env_pairs(&env), &args));
}

/// The one-shot of the step, its assistant the working stub: once the stub
/// says it works (forced: its pid file, never a wait for a race), the
/// command is sent `signal`, and waited for.
fn stopped_one_shot(w: &mut World, step: &cucumber::gherkin::Step, signal: &str) {
    let dir = assistant_dir(w);
    files::put_program(dir.join("claude"), &working_stub());
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
    w.before_run();
    let words = line_words(step);
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    let env = env_of(w);
    let child = w.common.project.start_with(&env_pairs(&env), &args);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !files::exists(dir.join("pid")) {
        assert!(
            Instant::now() < deadline,
            "the stub never said it works; the daemon's log:\n{}",
            daemon_log_text(w)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let sent = std::process::Command::new("kill")
        .args([&format!("-{signal}"), &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(sent.success(), "the SIG{signal} was sent");
    w.common.ran = Some(crate::tests::Ran::of(
        child.wait_with_output().expect("ctxloom finishes"),
    ));
}

/// What the stub assistant wrote of itself, by name.
fn recorded(w: &World, name: &str) -> String {
    files::read(assistant_dir(w).join(name))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The daemon that logged it serves last, by its pid (a daemon that has
/// idled out since is still the one that ran it).
fn daemon_pid(w: &World) -> u32 {
    let log = daemon_log_text(w);
    let serving = log
        .lines()
        .filter(|line| line.contains(" serving at "))
        .count();
    assert!(serving > 0, "no daemon logged that it served:\n{log}");
    match files::read_bytes(discovery_path(w)) {
        Ok(bytes) => {
            run_ports::Discovery::decode(&bytes)
                .expect("a discovery file it can read")
                .pid
        }
        Err(e) => panic!("no discovery file ({e}); the daemon's log:\n{log}"),
    }
}

/// The stub assistant was started by the daemon (its parent the daemon's
/// pid), not by the command typed.
fn ran_under_the_daemon(w: &World) {
    let parent = recorded(w, "ppid");
    assert!(!parent.is_empty(), "the assistant ran: {}", w.ran().said());
    assert_eq!(
        parent,
        daemon_pid(w).to_string(),
        "the assistant's parent is the daemon; its log:\n{}",
        daemon_log_text(w)
    );
}

/// The stub assistant was asked to stop (its trap ran) and is gone.
fn asked_and_gone(w: &World) {
    assert_eq!(
        recorded(w, "asked"),
        "asked",
        "the assistant was not asked to stop: {}\n{}",
        w.ran().said(),
        daemon_log_text(w)
    );
    let pid = recorded(w, "pid");
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid])
        .stderr(std::process::Stdio::null())
        .status()
        .expect("kill runs");
    assert!(
        !alive.success(),
        "the assistant ({pid}) outlived the command"
    );
    ran_under_the_daemon(w);
}

/// The command exited `code`, saying the run was stopped by `signal`.
fn exited_stopped(w: &World, code: i32, signal: &str) {
    let ran = w.ran();
    assert_eq!(ran.code, code, "{}", ran.said());
    assert!(
        ran.stderr.contains(&format!("stopped by {signal}")),
        "{}",
        ran.said()
    );
}

/// A one-shot the daemon hosts and that works until the scenario ends: its
/// stub released by the World's end.
fn hosted_working_one_shot(w: &mut World) {
    let dir = stub_claude(w, INTERACTIVE_STUB);
    w.daemon_spec.release = Some(dir.join("release"));
    let mut env = env_of(w);
    env.push(("ANTHROPIC_API_KEY".to_string(), "a-key".to_string())); // gitleaks:allow
    w.daemon_spec.run = Some(
        w.common
            .project
            .start_with(&env_pairs(&env), &["run", "--one-shot", "work"]),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !files::exists(dir.join("started")) {
        assert!(
            Instant::now() < deadline,
            "the one-shot's engine never started; the daemon's log:\n{}",
            daemon_log_text(w)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ------------------------------------------------- the TUI frontend (U5)

/// The interactive session's stub assistant: it records itself (its
/// arguments, its whole environment, its directory, whether its session's
/// lock was held), says it started, and runs until released; a SIGTERM is
/// said, never obeyed (the scenario that asks for a stop wants the kill).
/// Its records are named by its project's directory, so two projects'
/// sessions keep their own.
fn tui_stub() -> String {
    format!(
        r#"{RECORDS_ITSELF}p="$here/$(basename "$CTXLOOM_ROOT")"
mkdir -p "$p"
printf '%s\n' "$@" > "$p/tui-args"
env -0 > "$p/tui-env"
pwd -P > "$p/tui-cwd"
echo "${{CTXLOOM_SESSION##*/}}" > "$p/tui-harp"
trap 'echo asked > "$p/asked"' TERM
: > "$p/tui-started"
i=0
while [ ! -e "$here/release" ] && [ "$i" -lt 600 ]; do sleep 0.1; i=$((i+1)); done
"#
    )
}

/// Where the stub assistant recorded itself for `project`'s session.
fn tui_record(w: &World, project: &std::path::Path, name: &str) -> String {
    let dir = project.file_name().expect("a project directory");
    let stub = w
        .daemon_spec
        .tui_dir
        .clone()
        .unwrap_or_else(|| assistant_dir(w));
    files::read(stub.join(dir).join(name)).unwrap_or_default()
}

/// The scenario's environment for a session on a stood-in terminal.
fn tui_env(w: &mut World) -> Vec<(String, String)> {
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
    w.common.set_env("CTXLOOM_TEST_TERMINAL", "1".to_string());
    env_of(w)
}

/// Wait, bounded, for `ready`; `what` names it when it never comes.
fn wait_for(w: &World, what: &str, ready: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "{what} never came; the daemon's log:\n{}",
            daemon_log_text(w)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The session of the step (`ctxloom run` on a stood-in terminal), its
/// assistant the TUI stub. Unless `kill`, the stub is released at once and
/// the command waited for; with `kill`, once the daemon has recorded the
/// TUI's start and let its own run lock go (forced: its run's stream and
/// its lock file, never a wait for a race), the daemon is killed outright,
/// THEN the TUI is released, and the command waited for.
fn tui_session(w: &mut World, step: &cucumber::gherkin::Step, kill: bool) {
    files::put_program(assistant_dir(w).join("claude"), &tui_stub());
    let release = assistant_dir(w).join("release");
    if !kill {
        files::put(&release, "go");
    }
    let env = tui_env(w);
    w.before_run();
    let words = line_words(step);
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    let child = w.common.project.start_with(&env_pairs(&env), &args);
    if kill {
        let project = w.common.project.dir().to_path_buf();
        wait_for(w, "the TUI's start", || {
            files::exists(
                assistant_dir(w)
                    .join(project.file_name().unwrap())
                    .join("tui-started"),
            )
        });
        let harp = tui_record(w, &project, "tui-harp").trim().to_string();
        wait_for(w, "the daemon's record of the start", || {
            run_events(w, &harp).contains(&"RunnerStartedRecorded".to_string())
                && files::children(w.common.project.home().join(".ctxloom/runs")).is_empty()
        });
        let pid = discovery_now(w).pid;
        let killed = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .expect("kill runs");
        assert!(killed.success(), "the daemon was killed");
        wait_for(w, "the killed daemon's end", || !alive(pid));
        w.daemon_spec.killed = Some(pid);
        files::put(&release, "go");
    }
    w.common.ran = Some(crate::tests::Ran::of(
        child.wait_with_output().expect("ctxloom finishes"),
    ));
}

/// Whether the process `pid` is there, and not a zombie (a daemon the
/// command typed spawned is its child until that command exits, and is
/// reaped only then).
fn alive(pid: u32) -> bool {
    if let Ok(stat) = files::read(format!("/proc/{pid}/stat")) {
        let state = stat
            .rfind(')')
            .and_then(|end| stat[end + 1..].split_whitespace().next());
        return state.is_some_and(|state| state != "Z");
    }
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The types of the events of `harp`'s first run, as the store holds them.
fn run_events(w: &World, harp: &str) -> Vec<String> {
    let ran = run_now(w, &["test", "session-events", harp, "--run", "1"]);
    if ran.code != 0 {
        return Vec::new();
    }
    ran.json()
        .as_array()
        .map(|events| {
            events
                .iter()
                .filter_map(|e| e["type"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The launch the frontend was handed (test-support's record of it).
fn frontend_launch(w: &World) -> Value {
    let path = w
        .common
        .project
        .home()
        .join(".ctxloom/test-frontend-launch.json");
    let text = files::read(&path)
        .unwrap_or_else(|e| panic!("no launch was recorded ({e}): {}", w.ran().said()));
    serde_json::from_str(&text).expect("a launch record")
}

/// The variables a shell sets of its own, which no launch names.
const SHELL_OWN: [&str; 4] = ["PWD", "OLDPWD", "SHLVL", "_"];

/// The TUI of `project` was started exactly as the frontend's launch says:
/// its arguments, its whole environment (nothing else, but what the shell
/// sets of its own) and its directory.
fn started_exactly_as_launched(w: &World, project: &std::path::Path) {
    let launch = frontend_launch(w);
    let args: Vec<String> = tui_record(w, project, "tui-args")
        .lines()
        .map(str::to_string)
        .collect();
    let launched: Vec<String> = launch["args"]
        .as_array()
        .expect("its args")
        .iter()
        .map(|a| a.as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(args, launched, "the TUI's arguments are the launch's");
    let env: std::collections::BTreeMap<String, String> = tui_record(w, project, "tui-env")
        .split('\0')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(name, _)| !SHELL_OWN.contains(name))
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    let launched: std::collections::BTreeMap<String, String> = launch["env"]
        .as_object()
        .expect("its env")
        .iter()
        .map(|(name, value)| (name.clone(), value.as_str().unwrap_or_default().to_string()))
        .collect();
    let extra: Vec<&String> = env.keys().filter(|k| !launched.contains_key(*k)).collect();
    let missing: Vec<&String> = launched.keys().filter(|k| !env.contains_key(*k)).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "the TUI's environment is the launch's: extra {extra:?}, missing {missing:?}"
    );
    for (name, value) in &launched {
        assert_eq!(env.get(name), Some(value), "{name} is the launch's");
    }
    let cwd = tui_record(w, project, "tui-cwd");
    let want = launch["cwd"].as_str().expect("its directory");
    assert_eq!(
        files::canonical(cwd.trim()),
        files::canonical(want),
        "the TUI's directory"
    );
}

/// Two sessions at once, one in each project, each its TUI stub's: both
/// seen started (forced, never waited for a race), then released.
fn tui_in_both(w: &mut World, step: &cucumber::gherkin::Step) {
    files::put_program(assistant_dir(w).join("claude"), &tui_stub());
    let env = tui_env(w);
    w.before_run();
    let words = line_words(step);
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    let first = w.common.project.dir().to_path_buf();
    let second = w.daemon_spec.second.clone().expect("a second project");
    let started: Vec<Child> = [&first, &second]
        .iter()
        .map(|dir| {
            let mut env = env.clone();
            env.push(("CTXLOOM_ROOT".to_string(), dir.display().to_string()));
            w.common.project.start_in_with(dir, &env_pairs(&env), &args)
        })
        .collect();
    for dir in [&first, &second] {
        wait_for(w, "both TUIs' start", || {
            files::exists(
                assistant_dir(w)
                    .join(dir.file_name().unwrap())
                    .join("tui-started"),
            )
        });
    }
    files::put(assistant_dir(w).join("release"), "go");
    w.daemon_spec.both = started
        .into_iter()
        .map(|child| crate::tests::Ran::of(child.wait_with_output().expect("it ran")))
        .collect();
}

/// Whether no project of the home keeps a store of its own (T14: the one
/// store is the daemon's, `~/.ctxloom/ctxloom.db`).
fn no_project_store(w: &World) -> bool {
    files::children(w.common.project.home().join(".ctxloom/projects"))
        .iter()
        .all(|project| !files::exists(project.join("ctxloom.db")))
}

/// The sessions `dir`'s project lists, by their names.
fn listed_in(w: &World, dir: &std::path::Path) -> Vec<String> {
    let env = env_of(w);
    let ran = w.common.project.run_in_with(
        dir,
        &env_pairs(&env),
        &["session", "list", "--format", "json"],
    );
    assert_eq!(ran.code, 0, "{}", ran.said());
    let listed = ran.json();
    let rows = listed
        .get("sessions")
        .and_then(Value::as_array)
        .or_else(|| listed.as_array())
        .cloned()
        .unwrap_or_default();
    rows.iter()
        .filter_map(|row| {
            row.get("name")
                .or_else(|| row.get("harp"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

// ------------------------------------------------------------------ given

#[given("no ctxloom daemon is serving for Alice")]
fn none_serving_for_alice(w: &mut World) {
    none_serving(w);
}

#[given(regex = r#"^Alice has a second project, "([^"]+)", set up with ctxloom$"#)]
fn second_project(w: &mut World, name: String) {
    let dir = w.adopt_scratch().join(&name);
    files::mkdirs(&dir);
    let env = env_of(w);
    let git = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&dir)
        .status()
        .expect("git runs");
    assert!(git.success(), "the second project is a repository");
    let ran = w
        .common
        .project
        .run_in_with(&dir, &env_pairs(&env), &["init"]);
    assert_eq!(ran.code, 0, "{}", ran.said());
    w.daemon_spec.second = Some(dir);
}

#[given(regex = r#"^Alice's session "([^"]+)" is running$"#)]
fn alices_session_running(w: &mut World, harp: String) {
    // Her session: recorded and running; her daemon serves, and her
    // environment holds a credential a child of claude takes.
    hosting_daemon(w);
    let seeded = run_now(w, &["test", "seed-session", &harp]);
    assert_eq!(seeded.code, 0, "{}", seeded.said());
    w.common.set_env("ANTHROPIC_API_KEY", "a-key".to_string()); // gitleaks:allow
}

#[given(regex = r#"^Alice's session "([^"]+)" is running, with a child "([^"]+)" working for it$"#)]
fn alices_session_with_child(_: &mut World, _owner: String, _child: String) {
    red("Alice's session is running, with a child working for it")
}

#[given(
    "Alice's daemon has nothing to do: no session runs, no child is hosted and no command is open"
)]
fn alices_daemon_idle(w: &mut World) {
    start_daemon(w);
    w.daemon_spec.old = Some(discovery_now(w));
    // The next command's daemon idles out at once too.
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
}

#[given("no ctxloom daemon is serving for this home")]
fn none_serving(w: &mut World) {
    // A fresh home: no daemon has advertised itself there, and one a
    // command starts idles out at once.
    assert!(!files::exists(discovery_path(w)), "a fresh home");
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
}

#[given("the discovery file names a daemon that no longer runs")]
fn stale_discovery(w: &mut World) {
    start_daemon(w);
    let old = discovery_now(w);
    // Killed outright (T3): its discovery file is left as it was.
    let mut daemon = w.daemon_spec.daemon.take().expect("the daemon started");
    let _ = daemon.kill();
    let _ = daemon.wait();
    assert_eq!(
        discovery_now(w),
        old,
        "the file names the daemon that is gone"
    );
    w.daemon_spec.old = Some(old);
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
}

/// The daemon directory of the scenario's home.
fn daemon_dir(w: &World) -> PathBuf {
    w.common.project.home().join(".ctxloom/daemon")
}

#[given("no ctxloom daemon can be started for this home")]
fn unstartable(w: &mut World) {
    // A file where the daemon's directory goes: no daemon can take its
    // single-instance lock there, advertise itself or log.
    assert!(!files::exists(daemon_dir(w)), "a fresh home");
    files::put(daemon_dir(w), "not a directory");
}

#[given("a ctxloom daemon is serving for this home")]
fn serving(w: &mut World) {
    start_daemon(w);
}

/// Start `ctxloom daemon serve` of the stamp `stamp` (test-support), with
/// the scenario's short idle time; one a later command starts idles out too.
fn start_daemon_stamped(w: &mut World, stamp: &str) {
    let mut env = env_of(w);
    env.push((STAMP_VARIABLE.to_string(), stamp.to_string()));
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let idle = SPAWNED_IDLE_SECONDS.to_string();
    let daemon = w
        .common
        .project
        .start_with(&env, &["daemon", "serve", "--idle-seconds", &idle]);
    w.daemon_spec.daemon = Some(daemon);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !files::exists(discovery_path(w)) || discovery_now(w).stamp != stamp {
        assert!(
            Instant::now() < deadline,
            "the daemon never served; its log:\n{}",
            daemon_log_text(w)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    while !serving_now(w) {
        assert!(Instant::now() < deadline, "{}", daemon_log_text(w));
        std::thread::sleep(Duration::from_millis(50));
    }
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
}

#[given("a ctxloom daemon of an older version is serving for this home")]
fn serving_older(w: &mut World) {
    start_daemon_stamped(w, OLDER_STAMP);
}

#[given("a ctxloom daemon of a newer version is serving for this home")]
fn serving_newer(w: &mut World) {
    start_daemon_stamped(w, NEWER_STAMP);
}

#[given("a ctxloom daemon of this version is serving for this home")]
fn serving_this(w: &mut World) {
    start_daemon(w);
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
}

#[given("the session holds a prompt its hook held while an older daemon served")]
fn holds_a_prompt(w: &mut World) {
    start_daemon_stamped(w, OLDER_STAMP);
    fire(w, "UserPromptSubmit", USER_PROMPT_SUBMIT);
    let harp = fired(w).harp.clone();
    warned_for_prompt(w, "hook-cli-held");
    prompt_held(w, harp);
    // The older daemon goes (killed outright: its discovery file is left,
    // stale, for the next daemon to publish over).
    let mut daemon = w.daemon_spec.daemon.take().expect("the daemon started");
    let _ = daemon.kill();
    let _ = daemon.wait();
}

/// The stem of the hold that stands for every other session's of a full
/// home: a session root no session has.
const FULL_HOME_HOLD: &str = "00000000-0000-0000-0000-000000000000";

#[given("the holds of this home are at their bound")]
fn holds_at_bound(w: &mut World) {
    // Another session's hold of the home's whole bound (256 MiB, H-Q4),
    // sparse: the bound counts its size, and nothing reads it.
    let path = held_dir(w).join(format!("{FULL_HOME_HOLD}.held"));
    files::mkdirs(held_dir(w));
    let file = files::fs()
        .open(
            &path,
            file_system::Open::CreateNew(os::Mode::from_bits(0o600)),
        )
        .expect("a hold");
    file.set_len(256 << 20).expect("a sparse hold");
}

#[given(expr = "a ctxloom daemon is serving for this home, hosting a child {string}")]
fn serving_hosting(w: &mut World, child: String) {
    assert_eq!(child, "brisk-copper-moth", "the child the harness hosts");
    // Its default idle time: a daemon that hosts a child never idles out.
    start_daemon_idling(w, DEFAULT_IDLE_SECONDS);
    hosted_child(w);
}

/// The daemon's idle time when nothing says (`daemon serve`'s default).
const DEFAULT_IDLE_SECONDS: u64 = 600;

/// The discovery file the serving daemon wrote.
fn discovery_now(w: &World) -> run_ports::Discovery {
    let bytes = files::read_bytes(discovery_path(w)).expect("a discovery file");
    run_ports::Discovery::decode(&bytes).expect("a discovery file it can read")
}

/// GetDaemon of the daemon `found` names, presenting `found`'s bearer.
fn get_daemon(found: &run_ports::Discovery) -> Result<(), daemon_ports::Called> {
    use daemon_ports::DaemonApi;
    let client = daemon_client::DaemonClient::new(found, Duration::from_secs(5));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(client.get_daemon(daemon_proto::v1::GetDaemonRequest::default()))
        .map(|_| ())
}

#[given("a ctxloom daemon is serving for this home, and has nothing running, hosted or open")]
fn serving_idle(w: &mut World) {
    start_daemon(w);
}

#[given(regex = r"^a ctxloom daemon is serving for this home, with (.+)$")]
fn serving_busy(w: &mut World, busy: String) {
    match busy.as_str() {
        // The session's daemon is the one serving (U5: a `run` prepares
        // its session through it), so it is started first.
        "an interactive session running" => {
            start_daemon(w);
            interactive_session(w);
        }
        "a child it hosts" => {
            start_daemon(w);
            hosted_child(w);
        }
        "a one-shot it hosts" => {
            start_daemon(w);
            hosted_working_one_shot(w);
        }
        "a session's frontend subscribed" => {
            start_daemon(w);
            interactive_session(w);
        }
        _ => red("a ctxloom daemon is serving for this home, with something to do"),
    }
}

#[given(regex = r"^the daemon will be killed outright (.+)$")]
fn killed_at(w: &mut World, when: String) {
    if when != "after the write is applied, before it is answered" {
        red("the daemon will be killed outright at a step")
    }
    // The test-support switch (slice 3 step 4): the daemon that finds it
    // takes it, applies the next write it is sent through its coordinator,
    // and is killed before it answers; the daemon started after it finds
    // none.
    w.daemon_spec.before_kill = Some(discovery_now(w));
    files::put(daemon_dir(w).join(DIE_AFTER_APPLY), "once");
}

/// The daemon directory's switch that kills a test-support daemon once a
/// write it was sent is applied, before it is answered.
const DIE_AFTER_APPLY: &str = "test-die-after-apply";

#[given(expr = "a child {string} that is open and idle")]
fn child_idle(w: &mut World, child: String) {
    assert_eq!(child, "brisk-copper-moth", "the child the harness hosts");
    idle_child(w);
}

#[given(expr = "a child {string} that is in the middle of a turn")]
fn child_mid_turn(w: &mut World, child: String) {
    assert_eq!(child, "brisk-copper-moth", "the child the harness hosts");
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
    start_daemon_idling(w, DEFAULT_IDLE_SECONDS);
    // Its first turn is taken and left in flight.
    files::put(stub_dir(w).join("hold"), "once");
    hosted_child(w);
}

#[given(expr = "a child {string} that has been closed")]
fn child_closed(w: &mut World, child: String) {
    assert_eq!(child, "brisk-copper-moth", "the child the harness hosts");
    idle_child(w);
    let closed = run_now(w, &["session", "close", &child]);
    assert_eq!(closed.code, 0, "{}", closed.said());
    until(w, "the child closed", DAEMON_WAIT, |w| {
        (shown(w, "brisk-copper-moth")["closed"] == Value::Bool(true)).then_some(())
    });
}

#[given(expr = "a child {string} of {string} that is open and idle")]
fn child_of_idle(w: &mut World, child: String, owner: String) {
    assert_eq!(
        (child.as_str(), owner.as_str()),
        ("brisk-copper-moth", "amber-quiet-heron"),
        "the child and owner the harness hosts"
    );
    idle_child(w);
}

/// A child no session holds open: one nothing launched.
const STALE_CHILD: &str = "0b0b0b0b-0b0b-4b0b-8b0b-0b0b0b0b0b0b";

#[given("a hand-off file is left for a child that is no longer open")]
fn stale_hand_off(w: &mut World) {
    // Left by a launch whose child was never recorded, longer ago than a
    // launch takes to record its child.
    let path = launch_dir(w).join(STALE_CHILD);
    files::put_mode(
        &path,
        "ctxloom-launch-env 3\nANTHROPIC_API_KEY=left\n",
        0o600,
    ); // gitleaks:allow
    let old = std::time::SystemTime::now() - Duration::from_secs(3_600);
    files::open_append(&path)
        .and_then(|file| file.set_modified(old))
        .expect("the hand-off is made old");
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
}

// ------------------------------------------------------------------- when

#[when(
    "Alice's daemon is killed outright while her assistant works, and her assistant goes on firing its hooks"
)]
fn alices_daemon_killed(_: &mut World) {
    red("Alice's daemon is killed outright while her assistant works")
}

#[when("her daemon's idle time passes")]
fn her_idle_passes(w: &mut World) {
    idle_passes(w);
}

#[when("the daemon's idle time passes")]
fn idle_passes(_: &mut World) {
    // Its idle time, and a margin for its poll and its last round.
    std::thread::sleep(Duration::from_secs(IDLE_SECONDS + 2));
}

#[when(expr = "two commands are run at once, {string} and {string}")]
fn two_at_once(w: &mut World, first: String, second: String) {
    let env = env_of(w);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let started: Vec<Child> = [first, second]
        .iter()
        .map(|line| {
            let args: Vec<&str> = line.split_whitespace().skip(1).collect();
            w.common.project.start_with(&env, &args)
        })
        .collect();
    w.daemon_spec.both = started
        .into_iter()
        .map(|child| crate::tests::Ran::of(child.wait_with_output().expect("it ran")))
        .collect();
}

#[when("a client calls the daemon with a bearer it did not mint")]
fn foreign_bearer(w: &mut World) {
    let found = run_ports::Discovery {
        bearer: run_ports::Token::from_bytes([0x5a; 32]),
        ..discovery_now(w)
    };
    w.daemon_spec.called = Some(get_daemon(&found));
}

#[when("the daemon is stopped and the next command starts another")]
fn restarted(w: &mut World) {
    let old = discovery_now(w);
    let stopped = run_now(w, &["daemon", "stop"]);
    assert_eq!(stopped.code, 0, "{}", stopped.said());
    if let Some(mut daemon) = w.daemon_spec.daemon.take() {
        let _ = daemon.wait();
    }
    // The next command's daemon idles out at once.
    w.common.set_env(IDLE_VARIABLE, IDLE_SECONDS.to_string());
    let next = run_now(w, &["daemon", "resolve"]);
    assert_eq!(next.code, 0, "{}\n{}", next.said(), daemon_log_text(w));
    w.daemon_spec.old = Some(old);
}

const USER_PROMPT_SUBMIT: &str =
    include_str!("../../../../adapters/driven/engine-claude/src/fixtures/user_prompt_submit.json");
const STOP: &str = include_str!("../../../../adapters/driven/engine-claude/src/fixtures/stop.json");

/// What the fixture's prompt says.
const PROMPT: &str = "Run the shell command: echo hi";

/// The stamps of a daemon older and newer than any build of this one.
const OLDER_STAMP: &str = "v0.0.1-0000000";
const NEWER_STAMP: &str = "v999.0.0-0000000";

/// The variable a test names a ctxloom's stamp by (test-support).
const STAMP_VARIABLE: &str = "CTXLOOM_TEST_STAMP";

/// S16's bound on a hook, and what a process's start and exit add to it.
const HOOK_BOUND: Duration = Duration::from_secs(10);
const PROCESS_SLACK: Duration = Duration::from_secs(1);

/// `ctxloom hook claude-code execute <native>` as the session's engine runs
/// it: the scenario's environment, the session's name when there is one
/// (none: a hook with no session records nothing), `payload` on stdin.
fn hook_run(
    w: &World,
    native: &str,
    payload: &str,
    session: Option<&str>,
) -> (crate::tests::Ran, Duration) {
    let mut env = env_of(w);
    if let Some(session) = session {
        env.push(("CTXLOOM_SESSION".to_string(), session.to_string()));
    }
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let started = Instant::now();
    let mut child = w
        .common
        .project
        .start_talking(&env, &["hook", "claude-code", "execute", native]);
    crate::tests::feed(child.stdin.take().expect("stdin"), payload);
    let ran = crate::tests::Ran::of(child.wait_with_output().expect("the hook finishes"));
    (ran, started.elapsed())
}

/// The session's assistant fires its `native` hook with `payload`.
fn fire(w: &mut World, native: &str, payload: &str) {
    w.seed();
    let harp = w.seeded_harps.last().cloned().expect("a session");
    // Named in process, as the seed named it: no daemon is reached for it
    // (a command of this version would stop an older daemon, M3).
    let named = run_now(w, &["test", "session-name", &harp]);
    assert_eq!(named.code, 0, "{}", named.said());
    let name = named.stdout.trim().to_string();
    let events_before = session_events(w, &harp).len();
    let (unrecorded, _) = hook_run(w, native, payload, None);
    let (ran, took) = hook_run(w, native, payload, Some(&name));
    w.daemon_spec.fired = Some(Fired {
        ran,
        took,
        harp,
        events_before,
        unrecorded_answer: unrecorded.stdout,
    });
}

fn fired(w: &World) -> &Fired {
    w.daemon_spec.fired.as_ref().expect("a hook fired")
}

#[when("the session's assistant fires its UserPromptSubmit hook")]
fn hook_fires(w: &mut World) {
    fire(w, "UserPromptSubmit", USER_PROMPT_SUBMIT);
}

#[when("the session's assistant fires its Stop hook")]
fn stop_hook_fires(w: &mut World) {
    fire(w, "Stop", STOP);
}

#[when("Alice asks her assistant one thing, which the killed daemon leaves unfinished")]
fn unfinished(_: &mut World) {
    red("Alice asks her assistant one thing, which the killed daemon leaves unfinished")
}

// ------------------------------------------------------- then: the journey

/// How many daemons logged that they served, in the scenario's home.
fn daemons_logged(w: &World) -> usize {
    daemon_log_text(w).matches(" serving at ").count()
}

#[then("a ctxloom daemon is serving for Alice, started by that command")]
fn started_by_command(w: &mut World) {
    started_detached(w);
    // The one it started, beside the one an earlier step had started.
    let before = usize::from(w.daemon_spec.old.is_some());
    assert_eq!(daemons_logged(w), before + 1, "{}", daemon_log_text(w));
    assert!(serving_now(w), "{}", daemon_log_text(w));
    w.daemon_spec.first = Some(discovery_now(w));
}

#[then("that command used the same daemon, and started none")]
fn same_daemon(w: &mut World) {
    let first = w.daemon_spec.first.clone().expect("a daemon was started");
    assert_eq!(discovery_now(w), first, "the same daemon advertises itself");
    assert_eq!(daemons_logged(w), 1, "{}", daemon_log_text(w));
}

#[then("both sessions were served by the one daemon")]
fn one_daemon(w: &mut World) {
    for ran in &w.daemon_spec.both {
        assert_eq!(ran.code, 0, "{}", ran.said());
    }
    assert_eq!(daemons_logged(w), 1, "{}", daemon_log_text(w));
}

#[then("each session's record is its own project's, in the one store her daemon keeps")]
fn records_in_one_store(w: &mut World) {
    let home = w.common.project.home().join(".ctxloom");
    assert!(
        files::exists(home.join("ctxloom.db")),
        "the one store is the daemon's"
    );
    assert!(no_project_store(w), "no project keeps a store of its own");
    let first = w.common.project.dir().to_path_buf();
    let second = w.daemon_spec.second.clone().expect("a second project");
    for dir in [&first, &second] {
        let harp = tui_record(w, dir, "tui-harp").trim().to_string();
        assert!(
            files::is_dir(dir.join(".ctxloom/sessions").join(&harp)),
            "{harp}'s directory is its own project's"
        );
    }
}

#[then("each project lists its own session, and not the other's")]
fn own_listing(w: &mut World) {
    let first = w.common.project.dir().to_path_buf();
    let second = w.daemon_spec.second.clone().expect("a second project");
    let (a, b) = (listed_in(w, &first), listed_in(w, &second));
    let harp = |dir: &std::path::Path| tui_record(w, dir, "tui-harp").trim().to_string();
    let (harp_a, harp_b) = (harp(&first), harp(&second));
    assert!(
        a.iter().any(|n| n.ends_with(&harp_a)),
        "{a:?} lists {harp_a}"
    );
    assert!(
        !a.iter().any(|n| n.ends_with(&harp_b)),
        "{a:?} lists {harp_b}"
    );
    assert!(
        b.iter().any(|n| n.ends_with(&harp_b)),
        "{b:?} lists {harp_b}"
    );
    assert!(
        !b.iter().any(|n| n.ends_with(&harp_a)),
        "{b:?} lists {harp_a}"
    );
}

#[then("her assistant ran on her terminal exactly as her daemon prepared it")]
fn ran_as_prepared(w: &mut World) {
    w.ran().succeeds();
    // The capture stub records what its hooks need: each is the launch's.
    let launch = frontend_launch(w);
    let recorded = files::read(assistant_dir(w).join("engine-env")).unwrap_or_default();
    assert!(
        !recorded.is_empty(),
        "the assistant ran: {}",
        w.ran().said()
    );
    for line in recorded.lines() {
        let (name, value) = line.split_once('=').expect("a variable");
        assert_eq!(
            launch["env"][name].as_str(),
            Some(value),
            "{name} is the launch's"
        );
    }
    let cwd = files::read(assistant_dir(w).join("engine-cwd")).unwrap_or_default();
    assert_eq!(Some(cwd.trim()), launch["cwd"].as_str(), "its directory");
}

#[then(
    "the command she typed recorded nothing itself: her daemon recorded the session and prepared its session home"
)]
fn frontend_recorded_nothing(w: &mut World) {
    let launch = frontend_launch(w);
    let home = launch["session_home"].as_str().expect("its session home");
    assert!(files::is_dir(home), "the daemon prepared {home}");
    assert!(
        files::exists(PathBuf::from(home).join(".claude.json")),
        "its session home holds what the daemon seeded"
    );
    // The session is the daemon's record: listed through it, by its name.
    let harp = w.harp();
    let names = listed_in(w, w.common.project.dir());
    assert!(names.iter().any(|n| n.ends_with(&harp)), "{names:?}");
    assert!(
        no_project_store(w),
        "the command opened no store of its own"
    );
}

#[then(
    "the session's transcript holds what her assistant's hooks delivered to her daemon, each once"
)]
fn transcript_via_daemon(w: &mut World) {
    let harp = w.harp();
    let shown = run_now(
        w,
        &["session", "transcript", "show", &harp, "--format", "json"],
    );
    assert_eq!(shown.code, 0, "{}", shown.said());
    let prompts = shown
        .stdout
        .matches("Run the shell command: echo hi")
        .count();
    assert_eq!(
        prompts, 1,
        "the prompt is in the transcript once: {}",
        shown.stdout
    );
}

#[then("her assistant ran under her daemon, not under the command she typed")]
fn ran_under_daemon(w: &mut World) {
    ran_under_the_daemon(w);
}

#[then("her daemon stopped her assistant, and it was gone before the command exited")]
fn daemon_stopped_it(w: &mut World) {
    asked_and_gone(w);
}

#[then("ctxloom exited 130, saying the run was stopped by SIGINT")]
fn exited_sigint(w: &mut World) {
    exited_stopped(w, 130, "SIGINT");
}

#[then(regex = r#"^"([^"]+)" is listed as a child of "([^"]+)", run by her daemon$"#)]
fn listed_child(w: &mut World, child: String, owner: String) {
    let shown_child = until(w, "the child minted", DAEMON_WAIT, |w| {
        let ran = run_now(w, &["session", "show", &child, "--format", "json"]);
        (ran.code == 0).then(|| ran.json())
    });
    assert_eq!(
        shown_child["owner"],
        shown(w, &owner)["name"],
        "{shown_child}"
    );
    until(w, "the child hosted by her daemon", DAEMON_WAIT, |w| {
        let status = run_now(w, &["daemon", "status", "--format", "json"]);
        status.stdout.contains(&child).then_some(())
    });
}

#[then(regex = r#"^"([^"]+)"'s first turn was "([^"]*)"$"#)]
fn first_turn(w: &mut World, _child: String, text: String) {
    turn_on(w, &text);
    assert!(
        turns(w)[0].1.contains(&text),
        "its first turn: {:?}",
        turns(w)
    );
}

#[then(regex = r#"^"([^"]+)"'s next turn was "([^"]*)"$"#)]
fn next_turn(w: &mut World, _child: String, text: String) {
    turn_on(w, &text);
}

#[then(regex = r#"^"([^"]+)" is listed as closed, and its assistant has exited$"#)]
fn listed_closed(w: &mut World, child: String) {
    child_is_gone(w, &child, false);
}

#[then(regex = r#"^nothing "([^"]+)" was launched with is left at rest$"#)]
fn nothing_at_rest(w: &mut World, _child: String) {
    until(w, "the child's hand-off gone", DAEMON_WAIT, |w| {
        hand_offs(w).is_empty().then_some(())
    });
}

/// The child `child` is closed and its engine exited; with `hand_off`, its
/// hand-off gone as well.
fn child_is_gone(w: &World, child: &str, hand_off: bool) {
    until(w, "the child closed", DAEMON_WAIT, |w| {
        (shown(w, child)["closed"] == Value::Bool(true)).then_some(())
    });
    until(w, "the child's engine exited", DAEMON_WAIT, |w| {
        files::exists(stub_dir(w).join("child-exited")).then_some(())
    });
    if hand_off {
        until(w, "the child's hand-off gone", DAEMON_WAIT, |w| {
            hand_offs(w).is_empty().then_some(())
        });
    }
}

#[then("a new ctxloom daemon is serving for Alice")]
fn new_daemon(_: &mut World) {
    red("a new ctxloom daemon is serving for Alice")
}

#[then(regex = r#"^"([^"]+)" is listed as live, and its assistant never stopped$"#)]
fn still_live(_: &mut World, _harp: String) {
    red("the session is listed as live, and its assistant never stopped")
}

#[then(
    regex = r#"^"([^"]+)" found the new daemon and delivered again what the old one had not answered, and the record holds each once$"#
)]
fn redelivered(_: &mut World, _harp: String) {
    red("the session redelivered to the new daemon, each once")
}

#[then(
    regex = r#"^"([^"]+)" is working again in its own conversation, with what it was launched with$"#
)]
fn rehosted(_: &mut World, _child: String) {
    red("the child is re-hosted from its hand-off file")
}

#[then("her daemon has exited, and nothing says it serves")]
fn her_daemon_exited(w: &mut World) {
    daemon_exited(w);
}

#[then("it proves itself with a new certificate, and knows her by a new credential")]
fn new_credentials(w: &mut World) {
    rotated(w);
}

// ---------------------------------------------------- then: cli/daemon

#[then("a ctxloom daemon serves for this home, started detached in a session of its own")]
fn started_detached(w: &mut World) {
    let bytes = files::read_bytes(discovery_path(w)).unwrap_or_default();
    let found = run_ports::Discovery::decode(&bytes).expect("a daemon advertises itself");
    // T3: detached, it leads a session of its own (its session id is its
    // pid). Seen through procfs, so on Linux alone; elsewhere its discovery
    // file is what is seen.
    if let Ok(stat) = files::read(format!("/proc/{}/stat", found.pid)) {
        let after = &stat[stat.rfind(')').expect("a stat line") + 2..];
        let session: u32 = after
            .split(' ')
            .nth(3)
            .and_then(|s| s.parse().ok())
            .expect("a session id");
        assert_eq!(session, found.pid, "it leads a session of its own");
    }
}

#[then(expr = "the daemon's output goes to {string}")]
fn daemon_log(w: &mut World, path: String) {
    let rel = path.strip_prefix("~/").expect("a path under the home");
    let log = files::read(w.common.project.home().join(rel)).unwrap_or_default();
    assert!(
        log.contains(" serving at "),
        "the daemon logs there: {log:?}"
    );
}

#[then("both commands succeed")]
fn both_succeed(w: &mut World) {
    assert_eq!(w.daemon_spec.both.len(), 2, "two commands ran");
    for ran in &w.daemon_spec.both {
        assert_eq!(ran.code, 0, "{}", ran.said());
    }
}

#[then("exactly one daemon served them both")]
fn exactly_one(w: &mut World) {
    // One daemon took the single-instance lock and served; any other the
    // two started found it held, and never served.
    assert_eq!(daemons_logged(w), 1, "{}", daemon_log_text(w));
    assert!(serving_now(w), "{}", daemon_log_text(w));
}

#[then("a new daemon serves, and the discovery file names it")]
fn replaced(w: &mut World) {
    let old = w.daemon_spec.old.clone().expect("a daemon was killed");
    let new = discovery_now(w);
    assert_ne!(new.pid, old.pid, "another daemon advertises itself");
    let status = run_now(w, &["daemon", "status", "--format", "json"]);
    assert_eq!(
        status.json()["pid"],
        Value::from(new.pid),
        "{}",
        status.said()
    );
}

#[then("no project store was opened, and no engine was started")]
fn nothing_opened(w: &mut World) {
    let home = w.common.project.home();
    assert!(
        !files::exists(home.join(".ctxloom/ctxloom.db")),
        "the one store was not opened"
    );
    assert_eq!(per_project_stores(home), Vec::<PathBuf>::new());
    assert!(
        !files::exists(w.common.project.dir().join(".ctxloom/sessions")),
        "no session was set up, so no engine was started"
    );
}

#[then("no ctxloom daemon was started")]
fn none_started(w: &mut World) {
    let dir = w.common.project.home().join(".ctxloom/daemon");
    assert!(!files::exists(discovery_path(w)), "nothing advertised");
    assert!(!files::exists(dir.join("daemon.log")), "no daemon logged");
}

#[then(
    "the discovery file is readable by its owner alone, in a directory only its owner can enter"
)]
fn discovery_private(w: &mut World) {
    // Modes are unix's; on Windows the profile's ACL stands (M8).
    if let Some(mode) = files::mode_of(discovery_path(w)) {
        assert_eq!(mode & 0o777, 0o600, "the file is 0600");
        let dir = files::mode_of(daemon_dir(w)).expect("the directory");
        assert_eq!(dir & 0o777, 0o700, "the directory is 0700");
    }
}

#[then(
    "the discovery file names the daemon's endpoint, its certificate, a user bearer and its pid"
)]
fn discovery_holds(w: &mut World) {
    let found = discovery_now(w);
    assert!(
        found.endpoint.starts_with("https://127.0.0.1:"),
        "{}",
        found.endpoint
    );
    assert!(!found.cert.to_pem().is_empty(), "a certificate");
    assert_eq!(found.bearer.to_text().len(), 64, "a 256-bit bearer");
    assert!(found.pid > 0, "a pid");
    // What it names is what serves: the daemon answers its bearer.
    assert_eq!(get_daemon(&found), Ok(()));
}

#[then(regex = r#"^the output reports "([^"]+)" as the daemon's pid$"#)]
fn reports_pid(w: &mut World, path: String) {
    let doc = w.common.json();
    let pid = crate::tests::library::reports_as(&doc, &path);
    assert_eq!(pid, discovery_now(w).pid.to_string(), "{doc}");
}

#[then(regex = r#"^the output reports "([^"]+)" as the discovery file's endpoint$"#)]
fn reports_endpoint(w: &mut World, path: String) {
    let doc = w.common.json();
    let endpoint = crate::tests::library::reports_as(&doc, &path);
    assert_eq!(endpoint, discovery_now(w).endpoint, "{doc}");
}

#[then("neither the output, the daemon's log nor the project's store holds the user bearer")]
fn bearer_unseen(w: &mut World) {
    let bearer = discovery_now(w).bearer.to_text();
    let ran = w.common.ran();
    assert!(!ran.said().contains(&bearer), "the output holds the bearer");
    assert!(
        !daemon_log_text(w).contains(&bearer),
        "the daemon's log holds the bearer"
    );
    let home = w.common.project.home().join(".ctxloom");
    for name in ["ctxloom.db", "ctxloom.db-wal"] {
        let bytes = files::read_bytes(home.join(name)).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(&bearer), "{name} holds the bearer");
    }
}

#[then("the call is refused as unauthenticated, and the refusal names no reason")]
fn unauthenticated(w: &mut World) {
    // `Called::Unauthenticated` carries nothing: the daemon said no reason a
    // typed refusal would hold.
    assert_eq!(
        w.daemon_spec.called.take(),
        Some(Err(daemon_ports::Called::Unauthenticated))
    );
}

#[then("the new discovery file holds another certificate and another bearer")]
fn rotated(w: &mut World) {
    let old = w.daemon_spec.old.as_ref().expect("a daemon was stopped");
    let new = discovery_now(w);
    assert_ne!(new.pid, old.pid, "another daemon");
    assert_ne!(new.cert, old.cert, "another certificate");
    assert_ne!(new.bearer, old.bearer, "another bearer");
}

#[then("a client still holding the old bearer is refused")]
fn old_bearer_refused(w: &mut World) {
    let old = w.daemon_spec.old.clone().expect("a daemon was stopped");
    let stale = run_ports::Discovery {
        bearer: old.bearer,
        ..discovery_now(w)
    };
    assert_eq!(
        get_daemon(&stale),
        Err(daemon_ports::Called::Unauthenticated)
    );
}

#[then("the daemon has exited, and nothing says it serves")]
fn daemon_exited(w: &mut World) {
    let Some(daemon) = w.daemon_spec.daemon.as_mut() else {
        red("the daemon has exited (a daemon this scenario did not start)")
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    let code = loop {
        if let Some(status) = daemon.try_wait().expect("the daemon is waited for") {
            break status.code();
        }
        assert!(
            Instant::now() < deadline,
            "the daemon still runs; its log:\n{}",
            daemon_log_text(w)
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(code, Some(0), "{}", daemon_log_text(w));
    assert!(!serving_now(w), "nothing says it serves");
}

#[then("the daemon still serves")]
fn still_serves(w: &mut World) {
    let daemon = w
        .daemon_spec
        .daemon
        .as_mut()
        .expect("the scenario started a daemon");
    assert_eq!(
        daemon.try_wait().expect("the daemon is waited for"),
        None,
        "it exited; its log:\n{}",
        daemon_log_text(w)
    );
    assert!(serving_now(w), "{}", daemon_log_text(w));
}

/// The warnings the hook said on stderr: one JSON envelope each.
fn hook_warnings(w: &World) -> Vec<Value> {
    serde_json::Deserializer::from_str(&fired(w).ran.stderr)
        .into_iter::<Value>()
        .map(|w| w.expect("stderr holds JSON warnings only"))
        .collect()
}

#[then("the hook exits 0, and says nothing was left unrecorded")]
fn hook_delivered(w: &mut World) {
    let fired = fired(w);
    assert_eq!(fired.ran.code, 0, "{}", fired.ran.said());
    assert!(
        hook_warnings(w).is_empty(),
        "{}; the daemon's log:\n{}",
        fired.ran.said(),
        daemon_log_text(w)
    );
}

#[then("a ctxloom daemon serves for this home, started by that hook")]
fn started_by_hook(w: &mut World) {
    assert!(serving_now(w), "{}", daemon_log_text(w));
    assert_eq!(daemons_logged(w), 1, "{}", daemon_log_text(w));
}

/// The session's transcript, as `session transcript show` gives it.
fn transcript_lines(w: &World, harp: &str) -> Vec<Value> {
    let ran = run_now(
        w,
        &["session", "transcript", "show", harp, "--format", "json"],
    );
    assert_eq!(ran.code, 0, "{}", ran.said());
    ran.stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l:?}: {e}")))
        .collect()
}

/// Where in `lines` the fixture's prompt is, each place it is.
fn prompt_lines(lines: &[Value]) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l["kind"] == "entry" && l["entry"]["content"] == PROMPT)
        .map(|(n, _)| n)
        .collect()
}

#[then("the session's transcript holds the prompt, once")]
fn prompt_once(w: &mut World) {
    let harp = fired(w).harp.clone();
    let lines = transcript_lines(w, &harp);
    assert_eq!(prompt_lines(&lines).len(), 1, "{lines:#?}");
}

#[then("the session's transcript holds the prompt, then the turn's end, each once")]
fn prompt_then_turn_end(w: &mut World) {
    let harp = fired(w).harp.clone();
    let lines = transcript_lines(w, &harp);
    let prompts = prompt_lines(&lines);
    assert_eq!(prompts.len(), 1, "{lines:#?}");
    let ends = lines[prompts[0]..]
        .iter()
        .filter(|l| l["kind"] == "turn_end")
        .count();
    assert_eq!(ends, 1, "the turn's end after the prompt: {lines:#?}");
}

#[then("the hook exits 0, and its answer to the engine is unchanged")]
fn hook_fails_open(w: &mut World) {
    let fired = fired(w);
    assert_eq!(fired.ran.code, 0, "{}", fired.ran.said());
    assert_eq!(
        fired.ran.stdout, fired.unrecorded_answer,
        "the same answer as a hook that records nothing"
    );
}

/// The hook warned `code` for the prompt: an envelope of that code whose
/// fact is the prompt's.
fn warned_for_prompt(w: &World, code: &str) -> Value {
    let warnings = hook_warnings(w);
    let found = warnings
        .iter()
        .find(|w| {
            w["code"] == code
                && w["args"]["what"]
                    .as_str()
                    .is_some_and(|what| what.starts_with("hook:prompt:"))
        })
        .unwrap_or_else(|| panic!("no {code} for the prompt: {warnings:#?}"));
    assert_eq!(
        warnings.len(),
        1,
        "one warning, the prompt's: {warnings:#?}"
    );
    found.clone()
}

#[then(expr = "the hook warns {string} for the prompt, within its bound")]
fn hook_warns(w: &mut World, code: String) {
    warned_for_prompt(w, &code);
    let took = fired(w).took;
    assert!(
        took <= HOOK_BOUND + PROCESS_SLACK,
        "the hook took {took:?}, past its bound"
    );
}

#[then(expr = "the hook warns {string} for the prompt")]
fn hook_warns_for_prompt(w: &mut World, code: String) {
    warned_for_prompt(w, &code);
}

#[then(expr = "the hook warns {string} for the prompt, saying the hold is at its bound")]
fn hook_warns_bound(w: &mut World, code: String) {
    let warning = warned_for_prompt(w, &code);
    let why = warning["args"]["why"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(why.contains("is at its bound"), "{warning}");
}

/// The holds' directory of the scenario's home.
fn held_dir(w: &World) -> PathBuf {
    w.common.project.home().join(".ctxloom/held")
}

/// The holds in the scenario's home, but the placeholder of a full home.
fn holds(w: &World) -> Vec<PathBuf> {
    files::children(held_dir(w))
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "held"))
        .filter(|p| p.file_stem().is_some_and(|s| s != FULL_HOME_HOLD))
        .collect()
}

#[then(expr = "the prompt is held for {string}, in a file only its owner can read")]
fn prompt_held(w: &mut World, harp: String) {
    assert_eq!(fired(w).harp, harp);
    let holds = holds(w);
    assert_eq!(holds.len(), 1, "one hold, the session's: {holds:?}");
    let bytes = files::read_bytes(&holds[0]).expect("the hold");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("hook:prompt:") && text.contains(PROMPT),
        "the prompt's fact is held"
    );
    if let Some(mode) = files::mode_of(&holds[0]) {
        assert_eq!(mode & 0o777, 0o600, "{:o}", mode);
        assert_eq!(files::mode_of(held_dir(w)).map(|m| m & 0o777), Some(0o700));
    }
}

#[then(expr = "nothing is held for {string}")]
fn nothing_held(w: &mut World, harp: String) {
    assert_eq!(fired(w).harp, harp);
    assert!(holds(w).is_empty(), "{:?}", holds(w));
}

#[then(expr = "nothing was recorded for {string}")]
fn nothing_recorded(w: &mut World, harp: String) {
    let before = fired(w).events_before;
    assert_eq!(
        session_events(w, &harp).len(),
        before,
        "the session's stream is as it was before the hook"
    );
}

#[then("no run is left owed a runner, no incarnation is left open and no fact is recorded twice")]
fn record_whole(_: &mut World) {
    red("the record is whole after the daemon was killed")
}

// ------------------------------------------- then: cli/session_children

#[then(expr = "the child {string} of {string} ran its first turn on {string}")]
fn child_first_turn(w: &mut World, child: String, owner: String, text: String) {
    turn_on(w, &text);
    let shown_child = until(w, "the child minted", DAEMON_WAIT, |w| {
        let ran = run_now(w, &["session", "show", &child, "--format", "json"]);
        (ran.code == 0).then(|| ran.json())
    });
    assert_eq!(
        shown_child["owner"],
        shown(w, &owner)["name"],
        "{shown_child}"
    );
}

#[then("the child's hand-off file is readable by its owner alone while the child is open")]
fn hand_off_private(w: &mut World) {
    let child = until(w, "the child minted", DAEMON_WAIT, |w| {
        let ran = run_now(
            w,
            &["session", "show", "brisk-copper-moth", "--format", "json"],
        );
        (ran.code == 0).then(|| ran.json())
    });
    assert_ne!(child["closed"], Value::Bool(true), "open: {child}");
    // The hand-off itself, not its mark in launch (claimed by its launch).
    let kept: Vec<PathBuf> = hand_offs(w)
        .into_iter()
        .filter(|path| path.extension().is_none())
        .collect();
    let [path] = kept.as_slice() else {
        panic!("one hand-off: {kept:?}");
    };
    assert_eq!(
        path.file_name().and_then(|n| n.to_str()),
        child["uid"].as_str(),
        "named by the child's stable id"
    );
    if let Some(mode) = files::mode_of(path) {
        assert_eq!(mode, 0o600, "{}", path.display());
    }
}

#[then("no hand-off file was left behind")]
fn no_hand_off(w: &mut World) {
    until(w, "no hand-off left", DAEMON_WAIT, |w| {
        hand_offs(w).is_empty().then_some(())
    });
}

#[then(regex = r#"^the child ran a turn on "([^"]*)"$"#)]
fn child_ran_turn(w: &mut World, text: String) {
    turn_on(w, &text);
}

#[then(regex = r#"^the child ran no turn, and lists "([^"]*)" as waiting for one$"#)]
fn child_waits(w: &mut World, text: String) {
    let child = shown(w, "brisk-copper-moth");
    let pending: Vec<&str> = child["pending"]
        .as_array()
        .map(|pending| pending.iter().filter_map(|m| m["text"].as_str()).collect())
        .unwrap_or_default();
    assert!(pending.contains(&text.as_str()), "pending: {child}");
    assert!(
        !turns(w).iter().any(|(_, line)| line.contains(&text)),
        "no turn ran on it: {:?}",
        turns(w)
    );
}

#[then(expr = "the child's turn in flight was interrupted, and its next turn was {string}")]
fn child_steered(w: &mut World, text: String) {
    until(w, "the turn in flight interrupted", DAEMON_WAIT, |w| {
        files::exists(stub_dir(w).join("interrupted")).then_some(())
    });
    turn_on(w, &text);
    let taken = turns(w);
    assert!(
        taken.last().is_some_and(|(_, line)| line.contains(&text)),
        "its next turn: {taken:?}"
    );
}

#[then(expr = "the child ran one turn on {string}")]
fn child_one_turn(w: &mut World, text: String) {
    turn_on(w, &text);
    let events = session_events(w, "brisk-copper-moth");
    let sent: Vec<&Value> = events
        .iter()
        .filter(|e| e["type"] == "MessageSent" && e["text"] == text.as_str())
        .collect();
    assert_eq!(sent.len(), 1, "one message: {events:?}");
    let on: Vec<_> = turns(w)
        .into_iter()
        .filter(|(_, line)| line.contains(&text))
        .collect();
    assert_eq!(on.len(), 1, "one turn: {on:?}");
}

#[then("the child's next turn runs in a new conversation of the same session")]
fn child_cleared(w: &mut World) {
    let before = turns(w)
        .first()
        .map(|(sid, _)| sid.clone())
        .expect("its first turn");
    let uid = shown(w, "brisk-copper-moth")["uid"].clone();
    until(w, "the clear's new conversation bound", DAEMON_WAIT, |w| {
        let native = shown(w, "brisk-copper-moth")["native_session"].clone();
        (native.as_str().is_some_and(|n| n != before)).then_some(())
    });
    let sent = run_now(
        w,
        &[
            "session",
            "send",
            "brisk-copper-moth",
            "wake",
            "after the clear",
        ],
    );
    assert_eq!(sent.code, 0, "{}", sent.said());
    let (sid, _) = turn_on(w, "after the clear");
    assert_ne!(sid, before, "a new conversation: {:?}", turns(w));
    assert_eq!(
        shown(w, "brisk-copper-moth")["uid"],
        uid,
        "the same session"
    );
}

#[then(
    regex = r#"^the child "([^"]+)" is listed as closed, its engine has exited and its hand-off file is gone$"#
)]
fn child_ended(w: &mut World, child: String) {
    child_is_gone(w, &child, true);
}

#[then("the assistant was a child of the daemon, and its session's lock was the daemon's")]
fn one_shot_hosted(w: &mut World) {
    ran_under_the_daemon(w);
    // The command typed is gone: whoever held the lock while the assistant
    // ran was the daemon, its parent.
    let probe = recorded(w, "lock-probe");
    assert!(
        probe == "held" || probe == "unknown",
        "the session's lock was not held while the assistant ran: {probe}"
    );
}

#[then("the daemon asked the assistant to stop, and it was gone before the command exited")]
fn daemon_asked_stop(w: &mut World) {
    asked_and_gone(w);
}

#[then(
    regex = r"^the command exits 128 plus the signal's number, (\d+), saying the run was stopped by (SIG[A-Z]+)$"
)]
fn exits_by_signal(w: &mut World, code: String, signal: String) {
    exited_stopped(w, code.parse().expect("a code"), &signal);
}

#[then(
    "the assistant was started with the program, arguments, environment and directory of the daemon's launch, and nothing else"
)]
fn started_as_launched(w: &mut World) {
    let project = w.common.project.dir().to_path_buf();
    started_exactly_as_launched(w, &project);
}

#[then(
    "the command took the session's lock before the assistant started and let it go after it exited"
)]
fn frontend_lock(w: &mut World) {
    // The command typed held it while the TUI ran (the stub probed it), and
    // let it go: the session no longer reads live.
    let probe = recorded(w, "lock-probe");
    assert!(
        probe == "held" || probe == "unknown",
        "the session's lock was not held while the TUI ran: {probe}"
    );
    let launch = frontend_launch(w);
    let lock = launch["session_lock"].as_str().expect("its lock");
    assert!(
        lock.ends_with(&format!("/{}.lock", w.harp())),
        "the launch names the session's lock: {lock}"
    );
    if let Ok(free) = std::process::Command::new("flock")
        .args(["-n", lock, "true"])
        .status()
    {
        assert!(free.success(), "the lock was let go after the TUI exited");
    }
}

#[then("the daemon recorded the assistant's start and its exit, each once")]
fn start_and_exit(w: &mut World) {
    let events = run_events(w, &w.harp());
    let count = |kind: &str| events.iter().filter(|e| *e == kind).count();
    assert_eq!(count("RunnerStartedRecorded"), 1, "{events:?}");
    assert_eq!(count("RunEnded"), 1, "{events:?}");
}

#[then("no session was minted")]
fn no_mint(w: &mut World) {
    assert!(
        !files::exists(w.common.project.dir().join(".ctxloom/sessions")),
        "no session directory was made"
    );
    assert!(
        !files::exists(w.common.project.home().join(".ctxloom/ctxloom.db")),
        "the store was never opened, so nothing was minted in it"
    );
}

#[then("the assistant was never stopped")]
fn never_stopped(w: &mut World) {
    let project = w.common.project.dir().to_path_buf();
    assert_eq!(
        tui_record(w, &project, "asked"),
        "",
        "the TUI was asked to stop"
    );
    assert!(
        files::exists(
            assistant_dir(w)
                .join(project.file_name().unwrap())
                .join("tui-started")
        ),
        "the TUI ran"
    );
}

#[then("the command started a new daemon, and reported the assistant's exit to it once")]
fn reported_to_new(w: &mut World) {
    let killed = w.daemon_spec.killed.expect("a daemon was killed");
    let now = discovery_now(w);
    assert_ne!(now.pid, killed, "another daemon advertises itself");
    let events = run_events(w, &w.harp());
    let ended = events.iter().filter(|e| *e == "RunEnded").count();
    assert_eq!(ended, 1, "the exit was recorded once: {events:?}");
    assert!(
        !events.iter().any(|e| e == "RunnerLostRecorded"),
        "the run was not swept as lost: {events:?}"
    );
}

// ---------------------------------------- the owner's rulings S8 to S20

#[then("the hosted engines were ended cleanly before the daemon exited")]
fn ended_cleanly(w: &mut World) {
    // The stub's input was closed and it ran to its end, which the daemon
    // waits for (its end grace) before it exits.
    let exited = w.common.project.home().join("fake-claude/child-exited");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !files::exists(&exited) {
        assert!(
            Instant::now() < deadline,
            "the child never ended; the daemon's log:\n{}",
            daemon_log_text(w)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[then(
    "the run the killed daemon hosted ended lost, and its turn in flight ended failed, with its owner told"
)]
fn lost_and_told(_: &mut World) {
    red("the lost run's turn in flight ended failed, and its owner was told")
}

#[then(
    regex = r#"^"([^"]+)" runs again as its session's next incarnation, in its own conversation$"#
)]
fn next_incarnation(_: &mut World, _child: String) {
    red("the child runs again as its session's next incarnation")
}

/// The session write the last command was (the daemon spec's retry rows),
/// as its event's type; a library write is not built yet (T13).
fn session_write(w: &World) -> &'static str {
    let line = w.common.line.clone();
    if line.contains("session send") {
        "MessageSent"
    } else if line.contains("session close") {
        "SessionClosed"
    } else {
        red("a library write retried on a new daemon (T13)")
    }
}

#[then("the command started a new daemon and sent the write once more, under the same id")]
fn retried_once(w: &mut World) {
    let _ = session_write(w);
    let old = w
        .daemon_spec
        .before_kill
        .clone()
        .expect("a daemon was killed");
    let now = discovery_now(w);
    assert_ne!(now.pid, old.pid, "the retry was answered by a new daemon");
    assert!(
        !files::exists(daemon_dir(w).join(DIE_AFTER_APPLY)),
        "the switch was taken"
    );
}

#[then("the write was applied once, and its second delivery was a duplicate")]
fn applied_once(w: &mut World) {
    let kind = session_write(w);
    let events = session_events(w, "brisk-copper-moth");
    // The write the command sent: a send's by its text (the launch's own
    // first message is another).
    let applied = events
        .iter()
        .filter(|e| e["type"] == kind)
        .filter(|e| kind != "MessageSent" || e["text"] == "check the tests")
        .count();
    assert_eq!(applied, 1, "{events:?}");
}

#[then("the daemon's confirmation was asked at her terminal, and the session is gone")]
fn asked_at_terminal(_: &mut World) {
    red("the daemon's confirmation was asked at her terminal")
}

#[when("its orchestrator calls a ctxloom tool whose daemon call asks for confirmation")]
fn tool_asks(_: &mut World) {
    red("its orchestrator calls a ctxloom tool whose daemon call asks for confirmation")
}

#[then(
    "the question reached its engine as an MCP elicitation, and the engine's answer reached the daemon"
)]
fn elicited(_: &mut World) {
    red("the question reached its engine as an MCP elicitation")
}

#[given(expr = "an interactive session {string} whose assistant is running on its terminal")]
fn primary_running(w: &mut World, harp: String) {
    // Its session recorded, its engine's own session bound, and resumed on
    // this terminal (`run --session`), the TUI stub left running.
    let dir = stub_claude(w, &tui_stub());
    w.daemon_spec.tui_dir = Some(dir.clone());
    let seeded = run_now(
        w,
        &[
            "test",
            "seed-session",
            &harp,
            "--ended-ago",
            "60",
            "--native-id",
            "native-1",
            "--resumable",
        ],
    );
    assert_eq!(seeded.code, 0, "{}", seeded.said());
    let env = tui_env(w);
    let mut child = w
        .common
        .project
        .start_with(&env_pairs(&env), &["run", "--session", &harp]);
    let project = w.common.project.dir().to_path_buf();
    let started = dir.join(project.file_name().unwrap()).join("tui-started");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !files::exists(&started) {
        if let Ok(Some(_)) = child.try_wait() {
            let ran = crate::tests::Ran::of(child.wait_with_output().expect("it ran"));
            panic!("the session ended before its TUI started: {}", ran.said());
        }
        assert!(
            Instant::now() < deadline,
            "the TUI's start never came; the daemon's log:\n{}",
            daemon_log_text(w)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    w.daemon_spec.run = Some(child);
}

#[then("the assistant on its terminal was asked to stop, then killed once its grace had passed")]
fn terminal_stopped(w: &mut World) {
    w.ran().succeeds();
    // The frontend was told to stop its TUI: it asked (the stub said so,
    // and went on), then killed it once the grace had passed; the command
    // that ran the session then ended.
    let child = w.daemon_spec.run.take().expect("the session's command");
    let ran = crate::tests::Ran::of(child.wait_with_output().expect("it ends"));
    let project = w.common.project.dir().to_path_buf();
    assert_eq!(
        tui_record(w, &project, "asked").trim(),
        "asked",
        "the TUI was asked to stop: {}\n{}",
        ran.said(),
        daemon_log_text(w)
    );
    let stub = w
        .daemon_spec
        .tui_dir
        .clone()
        .unwrap_or_else(|| assistant_dir(w));
    assert!(
        !files::exists(stub.join("release")),
        "nothing released the TUI: it was killed"
    );
}

#[then(regex = r#"^"([^"]+)" is listed as closed$"#)]
fn closed_listed(w: &mut World, harp: String) {
    let shown = run_now(w, &["session", "show", &harp, "--format", "json"]);
    assert_eq!(shown.code, 0, "{}", shown.said());
    assert!(
        shown.stdout.contains("closed"),
        "{harp} is listed as closed: {}",
        shown.stdout
    );
}

#[given(
    expr = "a session {string} whose engine environment names ANTHROPIC_API_KEY through its engine block"
)]
fn owner_curated_env(w: &mut World, harp: String) {
    let dir = stub_claude(w, CHILD_STUB);
    // Its engine block names the key, so its engine's environment holds it
    // (what its MCP server runs in).
    w.common.project.write_home(
        ".ctxloom/config.yaml",
        &format!(
            "schema_version: 9\neditor:\n  command: \"true\"\nengines:\n  claude-code:\n    command: {}\n    env: [ANTHROPIC_API_KEY]\n",
            dir.join("claude").display()
        ),
    );
    let seeded = run_now(w, &["test", "seed-session", &harp]);
    assert_eq!(seeded.code, 0, "{}", seeded.said());
    w.common
        .set_env("ANTHROPIC_API_KEY", "the-owners-key".to_string()); // gitleaks:allow
    w.common
        .set_env(IDLE_VARIABLE, SPAWNED_IDLE_SECONDS.to_string());
}

#[given("the daemon was started from a shell holding CLAUDE_CODE_OAUTH_TOKEN")]
fn daemon_env_token(w: &mut World) {
    let mut env = env_of(w);
    env.retain(|(name, _)| name != "ANTHROPIC_API_KEY");
    env.push((
        "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
        "the-daemons-token".to_string(), // gitleaks:allow
    ));
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let idle = DEFAULT_IDLE_SECONDS.to_string();
    let daemon = w
        .common
        .project
        .start_with(&env, &["daemon", "serve", "--idle-seconds", &idle]);
    w.daemon_spec.daemon = Some(daemon);
    until(w, "the daemon to serve", DAEMON_WAIT, |w| {
        serving_now(w).then_some(())
    });
}

#[when(expr = "its orchestrator launches a child {string}")]
fn orchestrator_launches(w: &mut World, child: String) {
    engine_calls(
        w,
        "amber-quiet-heron",
        "child_launch",
        json!({"name": child, "task": "a task"}),
    );
    let launched = tool_session(w);
    assert!(
        launched["name"]
            .as_str()
            .is_some_and(|name| name.ends_with(&format!("/sessions/{child}"))),
        "{launched}"
    );
}

#[then(expr = "the child {string} was given ANTHROPIC_API_KEY, and not CLAUDE_CODE_OAUTH_TOKEN")]
fn child_given_curated(w: &mut World, _child: String) {
    let env = until(w, "the child to start", DAEMON_WAIT, |w| {
        files::read(stub_dir(w).join("child-env")).ok()
    });
    assert!(env.contains("ANTHROPIC_API_KEY=the-owners-key\n"), "{env}"); // gitleaks:allow
    assert!(!env.contains("CLAUDE_CODE_OAUTH_TOKEN"), "{env}");
}

// ------------------------------------------------------ trust (ruling 142)

#[given(
    "Alice has cloned a project whose own ctxloom config names the program that runs as her assistant"
)]
fn cloned_project(_: &mut World) {
    red("Alice has cloned a project whose config names its own assistant program")
}

#[then(
    regex = r#"^she is told the project's own "([^"]+)" waits for her trust, and that `ctxloom trust` is how to give it$"#
)]
fn told_untrusted(_: &mut World, _path: String) {
    red("she is told the project-only setting waits for her trust")
}

#[then("the project is listed as trusted by her, just now")]
fn trusted_by_her(_: &mut World) {
    red("the project is listed as trusted by her, just now")
}

#[then("her assistant ran as the program the project names")]
fn ran_projects_program(_: &mut World) {
    red("her assistant ran as the program the project names")
}

#[given("Alice has trusted the cloned project")]
fn has_trusted(_: &mut World) {
    red("Alice has trusted the cloned project")
}

#[given("a pull changes the program the project names")]
fn pull_changes(_: &mut World) {
    red("a pull changes the program the project names")
}

#[then("her assistant ran as the program the project names now")]
fn ran_projects_program_now(_: &mut World) {
    red("her assistant ran as the program the project names now")
}

#[given(expr = "an initialized ctxloom project whose project config says {string}")]
fn project_says(_: &mut World, _setting: String) {
    red("an initialized ctxloom project whose project config says a project-only setting")
}

#[given(
    expr = "an initialized ctxloom project whose own {string} bundle replaces the setup prompt"
)]
fn project_overrides_setup(_: &mut World, _bundle: String) {
    red("a project whose own ctxloom bundle replaces the setup prompt")
}

#[then("the setup used the built-in prompt, and said the project's own waits for trust")]
fn builtin_setup(_: &mut World) {
    red("the setup used the built-in prompt, and said the project's waits for trust")
}

#[then("the project is not trusted")]
fn not_trusted(_: &mut World) {
    red("the project is not trusted")
}

#[then("the project is listed as trusted by the user running ctxloom, with the time it was given")]
fn listed_trusted(_: &mut World) {
    red("the project is listed as trusted, with its actor and time")
}

#[given("the project's settings change after the preview and before the answer")]
fn changed_mid_trust(_: &mut World) {
    red("the project's settings change after the preview and before the answer")
}

#[given("an initialized ctxloom project that the user has trusted")]
fn project_trusted(_: &mut World) {
    red("an initialized ctxloom project that the user has trusted")
}

#[given(expr = "an initialized ctxloom project with a remote {string}")]
fn project_with_remote(_: &mut World, _remote: String) {
    red("an initialized ctxloom project with a remote")
}

#[then(expr = "the remote {string} is listed as trusted by the user running ctxloom")]
fn remote_trusted(_: &mut World, _remote: String) {
    red("the remote is listed as trusted")
}

// -------------------------------------------------- approvals (ruling 142)

#[given(
    regex = r#"^a child "([^"]+)" of "([^"]+)" whose engine has asked to run "([^"]+)" with "([^"]*)"$"#
)]
fn child_asked(_: &mut World, _child: String, _owner: String, _tool: String, _input: String) {
    red("a child whose engine has asked to run a tool")
}

#[then(regex = r"^the child's engine was told (.+)$")]
fn engine_told(_: &mut World, _what: String) {
    red("the child's engine was told the answer")
}

#[then(expr = "no ask is waiting under {string}")]
fn none_waiting(_: &mut World, _root: String) {
    red("no ask is waiting under the root")
}

#[given(expr = "a child {string} of {string} whose ask has been answered")]
fn ask_answered(_: &mut World, _child: String, _owner: String) {
    red("a child whose ask has been answered")
}

#[when("the ask's time runs out")]
fn ask_expires(_: &mut World) {
    red("the ask's time runs out")
}

#[then(regex = r#"^the child's claude was launched with "([^"]+)" "([^"]+)"$"#)]
fn claude_flag(_: &mut World, _flag: String, _value: String) {
    red("the child's claude was launched with the permission prompt tool")
}

#[then("the child's session home names ctxloom's MCP server")]
fn home_names_mcp(_: &mut World) {
    red("the child's session home names ctxloom's MCP server")
}

#[given(
    expr = "an opencode child {string} of {string} that sends a permission request for {string}"
)]
fn opencode_asks(_: &mut World, _child: String, _owner: String, _tool: String) {
    red("an opencode child that sends a permission request")
}

#[then("the child's opencode was answered with its request's allow-once option")]
fn opencode_allowed(_: &mut World) {
    red("the child's opencode was answered allow-once")
}

#[given(regex = r#"^"([^"]+)" asks to run "([^"]+)" with "([^"]*)"$"#)]
fn journey_child_asks(_: &mut World, _child: String, _tool: String, _input: String) {
    red("the child asks to run a tool")
}

#[then(regex = r#"^she sees "([^"]+)" asking to run "([^"]+)" with "([^"]*)"$"#)]
fn sees_ask(_: &mut World, _child: String, _tool: String, _input: String) {
    red("she sees the child's ask")
}

#[then(regex = r#"^"([^"]+)" ran "([^"]*)", and nothing waits for her any more$"#)]
fn ran_after_allow(_: &mut World, _child: String, _input: String) {
    red("the child ran the allowed tool, and nothing waits")
}

// ------------------------------------- the architecture rulings T13-T30

#[then("the removal was previewed first, and applied with her answer and the preview's tag")]
fn previewed_then_applied(_: &mut World) {
    red("the removal was previewed, then applied with the answer and tag")
}

#[given(
    expr = "the bundle {string} changes after its removal is previewed and before Alice answers"
)]
fn bundle_changes_mid(_: &mut World, _b: String) {
    red("the bundle changes between preview and answer")
}

#[then("the apply was refused as stale, and the CLI previewed the removal again")]
fn refused_stale(_: &mut World) {
    red("the apply was refused as stale")
}

#[given(expr = "an initialized ctxloom project with a recorded session {string}")]
fn project_with_session(w: &mut World, harp: String) {
    w.common.project = crate::tests::library::Project::initialized();
    w.plant(&harp, "finished");
}

/// A second project beside the first, under the same home: its directory.
fn second_dir(w: &World) -> std::path::PathBuf {
    w.common
        .project
        .dir()
        .parent()
        .expect("the project has a parent")
        .join("second")
}

#[given(expr = "a second initialized project with a recorded session {string}")]
fn second_project_session(w: &mut World, harp: String) {
    let dir = second_dir(w);
    crate::tests::files::put(
        dir.join(".ctxloom/config.yaml"),
        crate::tests::library::MINIMAL_CONFIG,
    );
    let ran = w.common.project.run_in(
        &dir,
        &[
            "test",
            "seed-session",
            &harp,
            "--transcript",
            "--lock",
            "free",
        ],
    );
    assert_eq!(ran.code, 0, "seeding {harp}: {}", ran.said());
}

/// Every project's own store under the home, from before the one store.
fn per_project_stores(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    crate::tests::files::children(home.join(".ctxloom/projects"))
        .into_iter()
        .map(|dir| dir.join("ctxloom.db"))
        .filter(|db| crate::tests::files::exists(db))
        .collect()
}

#[then("both sessions are in the one store under ctxloom's home, and no project has a store of its own")]
fn one_store(w: &mut World) {
    let home = w.common.project.home().to_path_buf();
    assert!(
        crate::tests::files::exists(home.join(".ctxloom/ctxloom.db")),
        "the one store"
    );
    assert_eq!(per_project_stores(&home), Vec::<std::path::PathBuf>::new());
    let all = w
        .common
        .project
        .run(&["session", "list", "--all", "--format", "json"]);
    assert_eq!(all.code, 0, "{}", all.said());
    for harp in ["amber-swift-owl", "brisk-copper-moth"] {
        assert!(all.stdout.contains(harp), "{harp}: {}", all.said());
    }
}

/// The project id the per-project store step gives the project.
const OLD_PROJECT: &str = "old-store-project";

/// What the per-project store held when it was planted.
const OLD_BYTES: &[u8] = b"a store of the project's own, from before the one store";

#[given("the project has a store of its own from before the one store")]
fn per_project_store(w: &mut World) {
    let project = &w.common.project;
    project.write(".ctxloom/project-id", &format!("{OLD_PROJECT}\n"));
    crate::tests::files::put(
        project
            .home()
            .join(".ctxloom/projects")
            .join(OLD_PROJECT)
            .join("ctxloom.db"),
        OLD_BYTES,
    );
}

#[then("nothing was imported from it")]
fn nothing_imported(w: &mut World) {
    let home = w.common.project.home();
    let old = home
        .join(".ctxloom/projects")
        .join(OLD_PROJECT)
        .join("ctxloom.db");
    assert_eq!(
        crate::tests::files::read_bytes(&old).expect("still there"),
        OLD_BYTES,
        "left as it was"
    );
    assert!(
        !crate::tests::files::exists(home.join(".ctxloom/ctxloom.db")),
        "no store was made from it"
    );
}

#[then(regex = r#"^its incarnation was recorded with the cause "([^"]+)"$"#)]
fn incarnation_cause(_: &mut World, _c: String) {
    red("its incarnation was recorded with the cause")
}

#[given(expr = "a child {string} of {string} that has been re-hosted {int} times in the last hour")]
fn rehosted_n(_: &mut World, _c: String, _o: String, _n: i64) {
    red("a child re-hosted n times in the last hour")
}

#[then(regex = r#"^"([^"]+)" was not re-hosted again, and ended runner-lost$"#)]
fn past_budget(_: &mut World, _c: String) {
    red("the child was not re-hosted past its budget")
}

#[then(regex = r#"^"([^"]+)" was told that its child ended$"#)]
fn owner_told(_: &mut World, _o: String) {
    red("the owner was told its child ended")
}

#[given(expr = "a child {string} whose session settings its agent has edited")]
fn settings_edited(_: &mut World, _c: String) {
    red("a child whose session settings its agent edited")
}

#[then("the re-hosted child's session settings are the ones the user's config makes, and its agent's edit is gone")]
fn settings_regenerated(_: &mut World) {
    red("the re-hosted child's settings were regenerated")
}

/// A stub engine of a registration scenario: it records the directory its
/// config is read from and its arguments, then answers as a one-shot turn
/// answers (claude's stream-json result, opencode's text), or, at the
/// terminal, just exits.
const REGISTERED_STUB: &str = r#"here=$(dirname "$0")
printf '%s\n' "${CLAUDE_CONFIG_DIR:-$OPENCODE_CONFIG_DIR}" > "$here/config-dir"
: > "$here/args"
for a in "$@"; do printf '%s\n' "$a" >> "$here/args"; done
case " $* " in
  *" -p "*)
    cat > /dev/null
    printf '{"type":"system","subtype":"init","cwd":"/work","session_id":"S1","tools":[]}\n'
    printf '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"S1"}\n'
    ;;
  *" run "*)
    cat > /dev/null
    printf '{"type":"text","sessionID":"ses_1","part":{"type":"text","text":"ok","time":{"end":1}}}\n'
    ;;
esac
"#;

#[given(regex = r"^a (child|interactive) session on (claude-code|opencode)$")]
fn session_on_engine(w: &mut World, kind: String, engine: String) {
    let program = match engine.as_str() {
        "claude-code" => "claude",
        _ => "opencode",
    };
    let dir = w.common.project.home().join("registered");
    files::put_program(dir.join(program), REGISTERED_STUB);
    w.common.project.write(
        ".ctxloom/config.yaml",
        &format!("schema_version: 9\nagents:\n  dev:\n    engine: {engine}\ndefault_agent: dev\n"),
    );
    w.common.project.write_home(
        ".ctxloom/config.yaml",
        &format!(
            "schema_version: 9\neditor:\n  command: \"true\"\nengines:\n  {engine}:\n    command: {}\n",
            dir.join(program).display()
        ),
    );
    let mut env = env_of(w);
    let args: Vec<&str> = match kind.as_str() {
        "child" => vec!["run", "--one-shot", "a task"],
        _ => {
            env.push(("CTXLOOM_TEST_TERMINAL".to_string(), "1".to_string()));
            vec!["run"]
        }
    };
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let ran = w.common.project.run_with(&env, &args);
    assert!(
        files::exists(dir.join("config-dir")),
        "the {engine} session never started: {}",
        ran.said()
    );
    w.daemon_spec.registered = Some((engine, dir));
}

#[then(expr = "its session home registers {string} as the MCP server {string}")]
fn registers_mcp(w: &mut World, command: String, name: String) {
    let (engine, dir) = w
        .daemon_spec
        .registered
        .clone()
        .expect("a session ran on an engine");
    let config_dir = PathBuf::from(
        files::read(dir.join("config-dir"))
            .expect("the engine recorded its config dir")
            .trim(),
    );
    let registered: Vec<String> = match engine.as_str() {
        "claude-code" => {
            // The servers the launch carries: the file `--mcp-config` names.
            let args = files::read(dir.join("args")).expect("the engine recorded its arguments");
            let args: Vec<&str> = args.lines().collect();
            let file = args
                .windows(2)
                .find(|pair| pair[0] == "--mcp-config")
                .map(|pair| pair[1].to_string())
                .unwrap_or_else(|| panic!("claude was given no --mcp-config: {args:?}"));
            let config: Value =
                serde_json::from_str(&files::read(&file).expect("the MCP config")).expect("JSON");
            let entry = &config["mcpServers"][&name];
            assert!(entry.is_object(), "no server {name}: {config}");
            assert!(
                entry.get("env").is_none(),
                "no identity in the entry: {entry}"
            );
            let mut words = vec![entry["command"].as_str().unwrap_or_default().to_string()];
            words.extend(
                entry["args"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|a| a.as_str().unwrap_or_default().to_string()),
            );
            words
        }
        _ => {
            // opencode's session config: its `mcp` entry, a local one.
            let config: Value = serde_json::from_str(
                &files::read(config_dir.join("opencode.json")).expect("opencode's config"),
            )
            .expect("JSON");
            let entry = &config["mcp"][&name];
            assert_eq!(entry["type"], "local", "{config}");
            assert!(
                entry.get("environment").is_none(),
                "no identity in the entry: {entry}"
            );
            entry["command"]
                .as_array()
                .unwrap_or_else(|| panic!("no server {name}: {config}"))
                .iter()
                .map(|a| a.as_str().unwrap_or_default().to_string())
                .collect()
        }
    };
    assert_eq!(registered.join(" "), command, "{engine}");
}

#[when("its engine lists the tools of ctxloom's MCP server")]
fn lists_tools(_: &mut World) {
    red("its engine lists ctxloom's MCP tools")
}

#[then(regex = r"^the tools are (.+)$")]
fn tools_are(_: &mut World, _t: String) {
    red("the MCP tools are the spec's")
}

/// The resource name of the session `harp` of the scenario's project.
fn session_name(w: &World, harp: &str) -> String {
    w.common
        .named(&format!("projects/{{project}}/sessions/{harp}"))
}

/// The engine of `owner` calls ctxloom's tool `tool` with `arguments`,
/// through `ctxloom mcp serve` started as its engine starts it (the
/// session's name in its environment): what it was answered.
fn engine_calls(w: &mut World, owner: &str, tool: &str, arguments: Value) {
    let me = session_name(w, owner);
    let env = env_of(w);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut mcp = crate::tests::mcp_serve::Mcp::for_session(&w.common.project, &me, &env);
    let answered = mcp.call(tool, arguments);
    let (code, err) = mcp.finish();
    assert_eq!(code, 0, "the server failed: {err}");
    w.daemon_spec.tool = Some(answered);
}

/// The events of the session `harp`'s stream, as `ctxloom test
/// session-events` lists them: each one's type, and its message id when it
/// has one.
fn session_events(w: &World, harp: &str) -> Vec<Value> {
    let ran = run_now(w, &["test", "session-events", harp]);
    assert_eq!(ran.code, 0, "{}", ran.said());
    ran.json().as_array().cloned().unwrap_or_default()
}

/// How many of `events` are of the type `name` (its message's name).
fn count_of(events: &[Value], name: &str) -> usize {
    events.iter().filter(|e| e["type"] == name).count()
}

/// The tool's answer: not an error, its first text the session it answered.
fn tool_session(w: &World) -> Value {
    let answered = w.daemon_spec.tool.clone().expect("a tool was called");
    assert_ne!(
        answered["isError"],
        true,
        "the tool failed: {answered}; the daemon's log:\n{}",
        daemon_log_text(w)
    );
    crate::tests::mcp_serve::answered_json(&answered)
}

#[when(
    regex = r#"^the engine of "([^"]+)" calls "([^"]+)" for "([^"]+)" with (inform|wake|steer) "([^"]*)"$"#
)]
fn tool_send(
    w: &mut World,
    owner: String,
    tool: String,
    child: String,
    urgency: String,
    text: String,
) {
    engine_calls(
        w,
        &owner,
        &tool,
        json!({"child": child, "urgency": urgency, "text": text}),
    );
}

#[then("the tool answered with the child's session as the daemon's read model has it after the message")]
fn tool_answered_view(w: &mut World) {
    let session = tool_session(w);
    let child = session_name(w, "brisk-copper-moth");
    assert_eq!(session["name"], child, "{session}");
    let shown = run_now(
        w,
        &["session", "show", "brisk-copper-moth", "--format", "json"],
    );
    assert_eq!(shown.code, 0, "{}", shown.said());
    let shown = shown.json();
    for field in ["uid", "owner", "engine"] {
        assert_eq!(
            session[field], shown[field],
            "{field}: {session} vs {shown}"
        );
    }
}

#[then("the message was sent once, under the id the server chose")]
fn sent_once(w: &mut World) {
    let answered = w.daemon_spec.tool.clone().expect("a tool was called");
    let id: Value = serde_json::from_str(
        answered["content"][1]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("the message's id is not answered: {answered}")),
    )
    .expect("JSON");
    let id = id["message_id"].as_str().expect("a message id").to_string();
    let events = session_events(w, "brisk-copper-moth");
    let sent: Vec<&Value> = events
        .iter()
        .filter(|e| e["type"] == "MessageSent")
        .filter(|e| e["text"] == "check the tests")
        .collect();
    assert_eq!(sent.len(), 1, "{events:?}");
    assert_eq!(sent[0]["message_id"], id.as_str(), "{events:?}");
}

#[when(regex = r#"^the engine of "([^"]+)" calls "([^"]+)" for "([^"]+)"$"#)]
fn tool_call(w: &mut World, owner: String, tool: String, child: String) {
    engine_calls(w, &owner, &tool, json!({"child": child}));
}

#[then("the tool answered once the new daemon had the close, and the close was applied once")]
fn tool_retried(w: &mut World) {
    let session = tool_session(w);
    assert_eq!(session["closed"], true, "{session}");
    let old = w
        .daemon_spec
        .before_kill
        .clone()
        .expect("a daemon was killed");
    let now = discovery_now(w);
    assert_ne!(
        now.pid, old.pid,
        "the tool's retry was answered by a new daemon"
    );
    assert!(
        !files::exists(daemon_dir(w).join(DIE_AFTER_APPLY)),
        "the switch was taken"
    );
    let events = session_events(w, "brisk-copper-moth");
    assert_eq!(count_of(&events, "SessionClosed"), 1, "{events:?}");
}

#[when("its engine calls a tool whose change asks for an edited document, and answers the elicitation with new text")]
fn tool_edit(_: &mut World) {
    red("an engine answers an edit need with text")
}

#[then("the daemon applied the new text, and no editor was started")]
fn applied_text(_: &mut World) {
    red("the daemon applied the new text without an editor")
}

#[given(
    regex = r#"^a claude child "([^"]+)" of "([^"]+)" whose engine calls "approval_prompt" for "([^"]+)" with "([^"]*)"$"#
)]
fn approval_prompt_called(_: &mut World, _c: String, _o: String, _t: String, _i: String) {
    red("a claude child calls approval_prompt")
}

#[when(regex = r#"^(Alice runs "[^"]+"|nobody answers within the ask's time)$"#)]
fn approval_answered_by(_: &mut World, _w: String) {
    red("the ask is answered, or times out")
}

#[then(regex = r"^the tool answered the engine (.+)$")]
fn tool_answered_engine(_: &mut World, _a: String) {
    red("the approval tool answered the engine")
}

#[given(expr = "a child {string} whose engine runs in a container")]
fn child_in_container(_: &mut World, _c: String) {
    red("a child whose engine runs in a container")
}

#[then("the container holds no ctxloom binary of the host's, and its MCP server and hooks reach the daemon over TCP and TLS")]
fn container_path(_: &mut World) {
    red("the container reaches the daemon through its MCP server")
}

#[then("the change is in the tree whole, once, and every bundle still reads")]
fn change_whole(_: &mut World) {
    red("the change is in the tree whole, once")
}

#[given(expr = "another process creates the bundle {string} while its creation is staged")]
fn concurrent_create(_: &mut World, _b: String) {
    red("another process creates the bundle meanwhile")
}

#[then(
    regex = r#"^the other process's "([^"]+)" is untouched, and nothing staged is left behind$"#
)]
fn other_untouched(_: &mut World, _b: String) {
    red("the other process's bundle is untouched")
}

#[then(
    regex = r#"^the skill "([^"]+)" is the one directory "([^"]+)" of the bundle "([^"]+)", holding its "\.meta\.yaml"$"#
)]
fn skill_one_dir(_: &mut World, _s: String, _d: String, _b: String) {
    red("the skill is one directory with its metadata inside")
}

#[then(regex = r#"^only "([^"]+)" of the bundle "([^"]+)" changed$"#)]
fn only_changed(_: &mut World, _p: String, _b: String) {
    red("only the source file changed")
}

#[then(
    regex = r#"^the session's directory, its lock inside it, and its output folder are under "([^"]+)"$"#
)]
fn renamed_whole(_: &mut World, _h: String) {
    red("the session's directory, lock and output moved")
}

#[then(
    regex = r#"^the directory, lock and output folder are under "([^"]+)", and its name is claimed once$"#
)]
fn rename_converged(_: &mut World, _h: String) {
    red("the rename converged")
}

#[then(regex = r#"^nothing lists "([^"]+)", its streams are purged and no trash is left$"#)]
fn remove_converged(_: &mut World, _h: String) {
    red("the remove converged")
}

#[then("the files it writes are as one uninterrupted run would leave them")]
fn foreign_converged(_: &mut World) {
    red("the foreign writer converged")
}

// ------------------------------------- the rulings after T30 (S42, S53)

#[then("nothing was applied, and the daemon answered the preview")]
fn preview_only(_: &mut World) {
    red("nothing was applied, and the daemon answered the preview")
}

/// S55: a title moves nothing. Every planted session's lock is under its
/// harp, and nothing under the sessions or the output root is named for
/// anything but a planted harp.
#[then("the session's directory, lock and output folder were not moved")]
fn not_moved(w: &mut World) {
    let harps = w.lock_free_dead.clone();
    assert!(!harps.is_empty(), "a session was planted");
    let project = w.common.project.dir().to_path_buf();
    for harp in &harps {
        assert!(
            crate::tests::files::exists(
                project
                    .join(".ctxloom/sessions")
                    .join(format!("{harp}.lock"))
            ),
            "{harp}'s lock is where it was"
        );
    }
    for root in [project.join(".ctxloom/sessions"), project.join("ctxloom")] {
        for entry in crate::tests::files::children(&root) {
            let name = entry.file_name().unwrap().to_string_lossy().to_string();
            assert!(
                harps
                    .iter()
                    .any(|h| name == *h || name.strip_suffix(".lock") == Some(h.as_str())),
                "{} is named for no planted session",
                entry.display()
            );
        }
    }
}

#[then(
    regex = r#"^the output reports "([^"]+)" as the project's harp and the session's, "([^"]+)"$"#
)]
fn six_word_name(w: &mut World, path: String, shape: String) {
    // Slice 2 B3: `projects/<project harp>/sessions/<harp>`, the project's
    // harp being one minted for it (a harp: no `/`, never empty).
    let ran = w.common.ran();
    let doc: serde_json::Value = serde_json::from_str(&ran.stdout)
        .unwrap_or_else(|e| panic!("not json ({e}): {}", ran.said()));
    let name = crate::tests::library::reports_as(&doc, &path);
    let (before, after) = shape
        .split_once("<project harp>")
        .expect("the shape names the project harp");
    let project = name
        .strip_prefix(before)
        .and_then(|rest| rest.strip_suffix(after))
        .unwrap_or_else(|| panic!("{path} is {name:?}, not {shape:?}: {}", ran.said()));
    assert!(
        !project.is_empty() && !project.contains('/'),
        "{project:?} is no harp"
    );
}

#[then(regex = r#"^its lock is "([^"]+)", inside its directory$"#)]
fn lock_inside(_: &mut World, _path: String) {
    red("the session's lock is inside its directory")
}

#[then(regex = r#"^the bundle "([^"]+)" is the imported one, whole, and no trash is left$"#)]
fn import_converged(_: &mut World, _bundle: String) {
    red("bundle import --force converged")
}
