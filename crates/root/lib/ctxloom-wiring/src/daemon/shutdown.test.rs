//! `ctxloom shutdown`'s pieces; the whole is driven end to end by the
//! binary's acceptance tests (`daemon.test.rs`, `tui_adopt.test.rs`).

use std::sync::Arc;
use std::time::Duration;

use file_system::Rooted;

use super::{
    configured_drain_timeout, epoch_seconds, notice_text, span_text, Running, ShutdownError,
};

fn home(fs: &Rooted, config: Option<&str>) -> std::path::PathBuf {
    let home = std::path::PathBuf::from(os::host().flavor().from_top("/home/u"));
    if let Some(config) = config {
        let dir = home.join(".ctxloom");
        file_system::Fs::create_dir_all(fs, &dir, os::Mode::from_bits(0o700)).unwrap();
        file_system::Fs::write_atomic(
            fs,
            &dir.join("config.yaml"),
            config.as_bytes(),
            file_system::Perms::Exact(os::Mode::from_bits(0o600)),
        )
        .unwrap();
    }
    home
}

// The home config's timeout, read as a command reads the home layer: the
// span it says, none when it is unsaid or the config does not read.
#[test]
fn the_configured_timeout_is_the_home_configs() {
    let fs = Arc::new(Rooted::temp().unwrap());
    let dyn_fs: Arc<dyn file_system::Fs> = fs.clone();
    let at = home(&fs, None);
    assert_eq!(configured_drain_timeout(&dyn_fs, &at), None);
    let at = home(
        &fs,
        Some("schema_version: 9\ndaemon:\n  drain_timeout: 90s\n"),
    );
    assert_eq!(
        configured_drain_timeout(&dyn_fs, &at),
        Some(Duration::from_secs(90))
    );
    let at = home(
        &fs,
        Some("schema_version: 9\ndaemon:\n  drain_timeout: now\n"),
    );
    assert_eq!(configured_drain_timeout(&dyn_fs, &at), Some(Duration::ZERO));
    let at = home(
        &fs,
        Some("schema_version: 9\ndaemon:\n  drain_timeout: soon\n"),
    );
    assert_eq!(
        configured_drain_timeout(&dyn_fs, &at),
        None,
        "refused: none"
    );
}

// What still runs is said by its process ids.
#[test]
fn what_runs_is_said_by_its_pids() {
    let none = Running {
        daemon: None,
        frontends: Vec::new(),
    };
    assert!(none.nothing());
    assert_eq!(none.said(), "no daemon, no frontend");
    let some = Running {
        daemon: Some(42),
        frontends: vec![7, 9],
    };
    assert!(!some.nothing());
    assert_eq!(some.said(), "the daemon (pid 42), frontends pid 7, pid 9");
    let frontend = Running {
        daemon: None,
        frontends: vec![7],
    };
    assert!(!frontend.nothing());
}

// A time that is no span is a decline; what will not stop, a failure.
#[test]
fn the_errors_are_a_decline_and_a_failure() {
    use angzarr_diagnostic::Diagnostic;
    let bad = ShutdownError::BadWhen {
        value: "soon".to_string(),
        cause: "x".to_string(),
    };
    assert_eq!(bad.code(), "ctxloom-wiring-shutdown-bad-time");
    let left = ShutdownError::Incomplete {
        daemon: "pid 1".to_string(),
        frontends: "none".to_string(),
    };
    assert_eq!(left.code(), "ctxloom-wiring-shutdown-incomplete");
}

// A span is said as the one grammar writes it, longest unit first.
#[test]
fn a_span_is_said_longest_unit_first() {
    for (seconds, text) in [
        (0, "0s"),
        (1, "1s"),
        (2, "2s"),
        (59, "59s"),
        (60, "1m"),
        (300, "5m"),
        (3_600, "1h"),
        (5_400, "1h30m"),
        (3_661, "1h1m1s"),
        (86_400, "1d"),
        (90_061, "1d1h1m1s"),
    ] {
        assert_eq!(span_text(Duration::from_secs(seconds)), text, "{seconds}");
        assert_eq!(
            duration_text::parse(text),
            Ok(Duration::from_secs(seconds)),
            "{text} reads back"
        );
    }
}

// What a live frontend's human is told when a drain starts (OD-63).
#[test]
fn a_frontend_is_told_when_it_will_be_stopped() {
    assert_eq!(
        notice_text(Duration::from_secs(120)),
        "ctxloom is shutting down: this session will be stopped in 2m; finish and exit"
    );
}

#[test]
fn the_epoch_seconds_are_the_hosts_clock() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let read = epoch_seconds();
    assert!(read >= now && read <= now + 5, "{read} {now}");
}
