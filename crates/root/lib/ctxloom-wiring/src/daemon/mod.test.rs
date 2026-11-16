//! What a client gives the daemon it spawns, and how long it waits for it.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use super::{
    account_home, daemon_env, daemon_home, instance_lock, logger, request_id,
    ACCOUNT_HOME_VARIABLE, CALL_WAIT, DELIVERY_MARGIN, DESTROY_WAIT, HOOK_BOUND, SPAWN_WAIT,
};

fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

fn names(env: &[(OsString, OsString)]) -> Vec<String> {
    let mut names: Vec<String> = env
        .iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn value<'a>(env: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsString> {
    env.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

// S16, M11: a spawn and a delivery fit in a hook's bound together.
#[test]
fn a_spawn_wait_leaves_a_delivery_room_within_a_hooks_bound() {
    assert!(
        SPAWN_WAIT + DELIVERY_MARGIN <= HOOK_BOUND,
        "{SPAWN_WAIT:?} + {DELIVERY_MARGIN:?} > {HOOK_BOUND:?}"
    );
}

// M8: on Windows a variable's name is matched as Windows compares names, so
// `Path` and `SystemRoot` pass, and what a Windows program needs to start
// passes too; the spawner's profile and its folders do not (the daemon's
// home is the account's, never its spawner's).
#[test]
fn a_windows_daemon_is_given_its_path_and_startup_variables_whatever_their_case() {
    let given = daemon_env(
        os::Flavor::Windows,
        vars(&[
            ("Path", "C:\\bin"),
            ("SystemRoot", "C:\\Windows"),
            ("SystemDrive", "C:"),
            ("windir", "C:\\Windows"),
            ("TEMP", "C:\\t"),
            ("TMP", "C:\\t"),
            ("PATHEXT", ".EXE"),
            ("ComSpec", "C:\\cmd.exe"),
            ("APPDATA", "C:\\a"),
            ("LOCALAPPDATA", "C:\\l"),
            ("USERPROFILE", "C:\\u"),
            ("ANTHROPIC_API_KEY", "a key"), // gitleaks:allow
            ("Secret", "s"),
        ]),
        None,
    );
    let mut expected = vec![
        "Path",
        "SystemRoot",
        "SystemDrive",
        "windir",
        "TEMP",
        "TMP",
        "PATHEXT",
        "ComSpec",
    ];
    expected.sort_unstable();
    assert_eq!(names(&given), expected);
    assert_eq!(value(&given, "Path"), Some(&OsString::from("C:\\bin")));
}

// On unix names are case-sensitive: `Path` is not `PATH`, and no credential
// of the spawner's crosses, nor its HOME or its confinement (the daemon's
// home is the account's).
#[test]
fn a_unix_daemon_is_given_what_it_needs_and_no_credential() {
    let given = daemon_env(
        os::Flavor::Unix,
        vars(&[
            ("PATH", "/bin"),
            ("Path", "/elsewhere"),
            ("HOME", "/home/u"),
            ("TMPDIR", "/tmp"),
            ("LANG", "C"),
            ("LC_ALL", "C"),
            ("ANTHROPIC_API_KEY", "a key"), // gitleaks:allow
            ("CLAUDE_CONFIG_DIR", "/confined"),
            ("XDG_CONFIG_HOME", "/confined/xdg"),
        ]),
        None,
    );
    let mut expected = vec!["PATH", "TMPDIR", "LANG", "LC_ALL"];
    expected.sort_unstable();
    assert_eq!(names(&given), expected);
}

// The daemon's ctxloom home is its spawner's CTXLOOM_HOME when it set one
// (once, whatever the spawner's own variable said), and else none: the
// daemon then serves the account's home.
#[test]
fn a_daemons_ctxloom_home_is_its_spawners_ctxloom_home_alone() {
    let given = daemon_env(
        os::Flavor::Unix,
        vars(&[(run_ports::HOME_VARIABLE, "/stale"), ("HOME", "/home/u")]),
        Some(Path::new("/home/u/.ctxloom-rs")),
    );
    assert_eq!(
        value(&given, run_ports::HOME_VARIABLE),
        Some(&OsString::from("/home/u/.ctxloom-rs"))
    );
    assert_eq!(
        given
            .iter()
            .filter(|(n, _)| n == run_ports::HOME_VARIABLE)
            .count(),
        1
    );
    let given = daemon_env(
        os::Flavor::Unix,
        vars(&[(run_ports::HOME_VARIABLE, "/stale"), ("HOME", "/home/u")]),
        None,
    );
    assert!(names(&given).is_empty(), "{given:?}");
}

/// An environment of `pairs`, unix, its working directory `/`.
fn snapshot(pairs: &[(&str, &str)]) -> process_env::EnvSnapshot {
    process_env::EnvSnapshot::from_parts(
        os::Flavor::Unix,
        Ok(std::path::PathBuf::from("/")),
        std::path::PathBuf::from("/tmp"),
        vars(pairs),
    )
}

// One daemon per OS USER: the account's home is the system's (a test's
// override first, and under test-support HOME when none is named), never
// a moved HOME.
#[test]
fn the_accounts_home_is_the_tests_override_first() {
    let env = snapshot(&[(ACCOUNT_HOME_VARIABLE, "/account"), ("HOME", "/moved")]);
    assert_eq!(
        account_home(&env),
        Some(std::path::PathBuf::from("/account"))
    );
    let env = snapshot(&[("HOME", "/moved")]);
    assert_eq!(account_home(&env), Some(std::path::PathBuf::from("/moved")));
    // An engine's hook process: its HOME the session home, ctxloom's home
    // the test's (slice 4 U7).
    let env = snapshot(&[
        ("HOME", "/session-home"),
        (run_ports::HOME_VARIABLE, "/ctxloom-rs"),
    ]);
    assert_eq!(
        account_home(&env),
        Some(std::path::PathBuf::from("/ctxloom-rs"))
    );
    let env = snapshot(&[
        (ACCOUNT_HOME_VARIABLE, "/account"),
        (run_ports::HOME_VARIABLE, "/ctxloom-rs"),
    ]);
    assert_eq!(
        account_home(&env),
        Some(std::path::PathBuf::from("/account"))
    );
}

// The daemon's home (M4) is its CTXLOOM_HOME when it was given one, else the
// account's, never its HOME.
#[test]
fn the_daemons_home_is_its_ctxloom_home_else_the_accounts_never_its_home() {
    let env = snapshot(&[(ACCOUNT_HOME_VARIABLE, "/account"), ("HOME", "/moved")]);
    assert_eq!(
        daemon_home(&env),
        Some(std::path::PathBuf::from("/account"))
    );
    let env = snapshot(&[
        (ACCOUNT_HOME_VARIABLE, "/account"),
        ("HOME", "/moved"),
        (run_ports::HOME_VARIABLE, "/ctxloom-rs"),
    ]);
    assert_eq!(
        daemon_home(&env),
        Some(std::path::PathBuf::from("/ctxloom-rs"))
    );
}

// The single-instance lock is the ACCOUNT's (its home's
// `.ctxloom/daemon/daemon.lock`), whatever ctxloom home the process has.
#[test]
fn the_instance_lock_is_under_the_accounts_home_whatever_the_ctxloom_home() {
    let env = snapshot(&[
        (ACCOUNT_HOME_VARIABLE, "/account"),
        ("HOME", "/moved"),
        (run_ports::HOME_VARIABLE, "/ctxloom-rs"),
    ]);
    let wiring = crate::Wiring::new(std::sync::Arc::new(env)).unwrap();
    assert_eq!(
        instance_lock(&wiring),
        std::path::PathBuf::from("/account/.ctxloom/daemon/daemon.lock")
    );
}

// T21, S7: a launch hands the daemon where its launcher finds its user's
// settings (her claude config dir, her XDG bases), and nothing else of the
// launcher's in that place.
#[test]
fn a_launch_names_where_its_launcher_finds_its_users_settings() {
    let caller: BTreeMap<String, String> = [
        ("CLAUDE_CONFIG_DIR", "/home/alice/.claude-work"),
        ("XDG_CONFIG_HOME", "/home/alice/.cfg"),
        ("XDG_STATE_HOME", ""),
        ("ANTHROPIC_API_KEY", "k"), // gitleaks:allow
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let places = super::settings_places(&caller);
    assert_eq!(
        places.keys().map(String::as_str).collect::<Vec<_>>(),
        ["CLAUDE_CONFIG_DIR", "XDG_CONFIG_HOME"]
    );
}

// A destroy's deadline is sized to its worktrees' git, each invocation
// bounded by worktree-git's own timeout, with a read's wait to spare.
#[test]
fn a_destroys_deadline_outlasts_its_worktrees_git() {
    assert_eq!(
        DESTROY_WAIT,
        worktree_git::DEFAULT_TIMEOUT * 60 + CALL_WAIT,
        "sixty git invocations at their whole timeout, plus a read's wait"
    );
}

// Each write's request id is fresh.
#[test]
fn each_request_id_is_fresh() {
    let (one, two) = (request_id(), request_id());
    assert_ne!(one, two);
    assert!(uuid::Uuid::parse_str(&one).is_ok(), "{one}");
}

// j000100's exit: the daemon's log is made where it starts, and a line said
// once its directory is gone (its home deleted) is dropped, never a reason
// to make the directory again; a line said where it still is is appended.
#[test]
fn the_log_is_made_at_the_start_and_a_line_whose_directory_is_gone_is_dropped() {
    let root = crate::fixtures::root();
    let home = root.root().join("home");
    let log = home.join(".ctxloom/daemon/daemon.log");
    let mut say = logger(crate::fixtures::fs(), log.clone());
    assert!(
        crate::fixtures::exists(log.parent().unwrap()),
        "made at the start"
    );
    say("serving");
    let text = crate::fixtures::fs().read_to_string(&log).unwrap();
    assert!(text.ends_with(" serving\n"), "{text}");
    crate::fixtures::remove_dir(home.join(".ctxloom"));
    say("the single-instance lock file was replaced or removed: exiting");
    say("exited");
    assert!(
        !crate::fixtures::exists(home.join(".ctxloom")),
        "made again"
    );
}

// Step 8 Q1: a launch's `--project ../api` is the sibling directory, by its
// words alone.
#[test]
fn a_projects_directory_is_read_lexically() {
    use std::path::Path;
    assert_eq!(
        super::lexical(Path::new("/w/project/../api")),
        Path::new("/w/api")
    );
    assert_eq!(super::lexical(Path::new("/w/./a/b/..")), Path::new("/w/a"));
    assert_eq!(super::lexical(Path::new("../x")), Path::new("../x"));
}
