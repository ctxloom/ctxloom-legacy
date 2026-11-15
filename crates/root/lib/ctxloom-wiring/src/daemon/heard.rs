//! What the serving daemon says of itself that a command tells the person
//! (owner ruling OD-52): a daemon whose start-up catch-up (or its sweep)
//! failed serves anyway, and every command that reaches it says so, as
//! the daemon's own warnings (`GetDaemon`'s `Daemon.warnings`), once a
//! process, on stderr as the command ends ([`take_heard`]); what it
//! answered still stands. The `daemon` noun's own verbs hear nothing on
//! their way in: `daemon status` shows the warnings itself.

use std::sync::Mutex;

use daemon_ports::DaemonApi;
use daemon_proto::v1;

use super::flush::WayIn;

/// Whether this process has asked the daemon what it warns of: once a
/// process, however many times it reaches it.
static HEARD_ON_THE_WAY_IN: WayIn = WayIn::new();

/// What this process heard and has not said yet.
static HEARD: Mutex<Vec<v1::Notice>> = Mutex::new(Vec::new());

/// A command reaching the daemon `api` calls: its warnings asked for, the
/// first time this process reaches it, kept to be said ([`take_heard`]). A
/// call that fails is said nothing of (the command's own call says why).
pub(crate) async fn on_the_way_in<A: DaemonApi>(api: &A) {
    if HEARD_ON_THE_WAY_IN.first() {
        hear(warnings_of(api).await);
    }
}

/// The warnings `api`'s daemon says of itself, none where it cannot be asked.
pub(crate) async fn warnings_of<A: DaemonApi>(api: &A) -> Vec<v1::Notice> {
    let request = v1::GetDaemonRequest {
        name: "daemon".to_string(),
        ..v1::GetDaemonRequest::default()
    };
    api.get_daemon(request)
        .await
        .map(|daemon| daemon.warnings)
        .unwrap_or_default()
}

/// Keep `notices` to be said as the command ends.
pub(crate) fn hear(notices: Vec<v1::Notice>) {
    if let Ok(mut heard) = HEARD.lock() {
        heard.extend(notices);
    }
}

/// What this process heard and has not said yet, taken: the command says
/// each once, on stderr, as it ends, in its own code and args (rendered by
/// the command's catalog).
pub(crate) fn take_heard() -> Vec<angzarr_diagnostic::Report> {
    HEARD
        .lock()
        .map(|mut heard| std::mem::take(&mut *heard))
        .unwrap_or_default()
        .iter()
        .map(|notice| run_application::client::report_of(&notice.code, &notice.args))
        .collect()
}

#[cfg(test)]
#[path = "heard.test.rs"]
mod tests;
