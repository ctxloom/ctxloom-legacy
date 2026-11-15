//! The client's version-skew rule (M3, owner 2026-11-12): a client whose
//! full versionator stamp is not the serving daemon's asks the daemon
//! (`GetDaemon`) whether it hosts live runs. None: it stops that daemon
//! (`StopDaemon`) and starts its own. Some: it is refused, a decline whose
//! fix is `ctxloom daemon stop --even-if-live`, unless the client is of a
//! NEWER version (OD-3 extended, s4-Q4): then it asks the daemon to drain
//! (`StopDaemon{drain}`), which hands its runs over to the client's daemon
//! and exits, and the call is refused meanwhile (`daemon-api-draining`).

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};
use daemon_ports::{Called, DaemonApi};
use exit_code::{Exit, ExitClass};
/// The one semver "newer" (run-ports'), which the hook's hold judges by
/// too (s4-Q14, H-Q1): re-exported where the skew rule always named it.
pub use run_ports::newer;

/// What the client does with the daemon it found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reconciled {
    /// The same stamp: it uses the daemon.
    Same,
    /// Another stamp, hosting nothing: it was asked to stop, and the client
    /// starts its own.
    Stopped,
}

/// Why the client cannot use the daemon it found.
#[derive(Clone, Debug, PartialEq)]
pub enum SkewError {
    /// Another stamp, hosting live runs.
    Busy {
        own: String,
        daemon: String,
        harps: Vec<String>,
    },
    /// The daemon could not be asked.
    Call(Called),
    /// Another stamp, seen by a caller that never stops a daemon (OD-3, a
    /// session's MCP server): its call is refused, naming both.
    Mismatch { own: String, daemon: String },
    /// An older version hosting live runs, asked to hand them over to this
    /// client's (s4-Q4): this call is refused, and the next, once it has
    /// exited, starts this client's daemon.
    Draining {
        own: String,
        daemon: String,
        harps: Vec<String>,
    },
}

/// Reconcile the client's stamp `own` with the serving daemon's, `daemon`,
/// through `api` (that daemon's).
pub async fn reconcile<A: DaemonApi>(
    api: &A,
    own: &str,
    daemon: &str,
) -> Result<Reconciled, SkewError> {
    if own == daemon {
        return Ok(Reconciled::Same);
    }
    let busy = |harps: Vec<String>| SkewError::Busy {
        own: own.to_string(),
        daemon: daemon.to_string(),
        harps,
    };
    let request = daemon_proto::v1::GetDaemonRequest {
        name: "daemon".to_string(),
        ..daemon_proto::v1::GetDaemonRequest::default()
    };
    let found = api.get_daemon(request).await.map_err(SkewError::Call)?;
    // s4-Q4: a newer client hands an older daemon's runs over to its own,
    // busy or idle (an idle one exits at once); any other is stopped only
    // when idle.
    let drains = newer(own, daemon);
    if !found.runs.is_empty() && !drains {
        return Err(busy(found.runs.into_iter().map(|run| run.harp).collect()));
    }
    let stop = daemon_proto::v1::StopDaemonRequest {
        name: found.name,
        drain: drains,
        version: if drains {
            own.to_string()
        } else {
            String::new()
        },
        ..daemon_proto::v1::StopDaemonRequest::default()
    };
    match api.stop_daemon(stop).await {
        Ok(answered) if answered.runs.is_empty() => Ok(Reconciled::Stopped),
        // It drains: this call is refused meanwhile.
        Ok(answered) => Err(SkewError::Draining {
            own: own.to_string(),
            daemon: daemon.to_string(),
            harps: answered.runs.into_iter().map(|run| run.harp).collect(),
        }),
        // A run started between the two calls (or, asked to drain, a daemon
        // of a ctxloom before the drain, which takes it for a stop).
        Err(called) => match called.refusal::<daemon_proto::v1::DaemonHosting>() {
            Some(hosting) => Err(busy(hosting.harps)),
            None => Err(SkewError::Call(called)),
        },
    }
}

/// OD-3 (owner, 2026-11-13, 2026-11-14: "don't stop the daemon, stop new
/// invocations against the old daemon with an appropriate error message.
/// old daemon should drain and shut down, to be started by the new
/// binary"), as a session's call (its MCP server) meets the serving daemon.
/// It never ends what a daemon hosts. Of the same stamp, it is used. Of an
/// OLDER version ([`newer`]), it is asked to drain (`StopDaemon{drain}`,
/// never `even_if_live`): idle, it exits at once, so the caller starts its
/// own and its call goes there; busy, it hands its runs over (s4-Q4) and the
/// call is refused meanwhile ([`SkewError::Draining`]). A daemon that
/// refuses the drain for its runs (one of a ctxloom before it), or one not
/// older (an older or equal version: two binaries never drain each other's
/// daemon in turn), refuses the call ([`SkewError::Mismatch`]).
pub async fn reconcile_session<A: DaemonApi>(
    api: &A,
    own: &str,
    daemon: &str,
) -> Result<Reconciled, SkewError> {
    if own == daemon {
        return Ok(Reconciled::Same);
    }
    let mismatch = || SkewError::Mismatch {
        own: own.to_string(),
        daemon: daemon.to_string(),
    };
    if !newer(own, daemon) {
        return Err(mismatch());
    }
    match reconcile(api, own, daemon).await {
        Err(SkewError::Busy { .. }) => Err(mismatch()),
        other => other,
    }
}

impl fmt::Display for SkewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for SkewError {}

impl Diagnostic for SkewError {
    fn code(&self) -> &str {
        match self {
            SkewError::Busy { .. } => code!("version-skew-busy"),
            SkewError::Mismatch { .. } => code!("version-skew"),
            SkewError::Draining { .. } => code!("draining"),
            SkewError::Call(called) => called.code(),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            SkewError::Busy { own, daemon, harps } => vec![
                ("own", own.as_str().into()),
                ("daemon", daemon.as_str().into()),
                ("harps", harps.join(", ").into()),
            ],
            SkewError::Mismatch { own, daemon } => vec![
                ("own", own.as_str().into()),
                ("daemon", daemon.as_str().into()),
            ],
            SkewError::Draining {
                own, daemon, harps, ..
            } => crate::refusal::draining_args(daemon, own, harps),
            SkewError::Call(called) => called.args(),
        }
    }
}

/// A busy daemon of another version is a decline (exit 20); a call that
/// failed is a failure.
impl ExitClass for SkewError {
    fn exit(&self) -> Exit {
        match self {
            SkewError::Busy { .. } | SkewError::Mismatch { .. } | SkewError::Draining { .. } => {
                Exit::REFUSED
            }
            SkewError::Call(_) => Exit::FAILED,
        }
    }
}

/// Why a client's call came to nothing: refused (its typed refusal, in
/// its own crate's words, a decline), or the call failed.
#[derive(Clone, Debug, PartialEq)]
pub enum CallError {
    Refused(crate::refusal::Rejection),
    Call(Called),
    /// No daemon could be called: none started, or the one serving declined.
    Unreached(daemon_ports::Unreached),
    /// A launch's lock could not be made or opened (T6).
    LaunchLock {
        detail: String,
    },
}

impl From<Called> for CallError {
    fn from(called: Called) -> CallError {
        let rejection = match &called {
            Called::Refused { refusal, .. } => crate::rejection_of(refusal),
            _ => None,
        };
        match rejection {
            Some(rejection) => CallError::Refused(rejection),
            None => CallError::Call(called),
        }
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for CallError {}

impl Diagnostic for CallError {
    fn code(&self) -> &str {
        match self {
            CallError::Refused(crate::refusal::Rejection::Api(api)) => api.code(),
            CallError::Refused(crate::refusal::Rejection::Sessions(ours)) => ours.code(),
            CallError::Refused(crate::refusal::Rejection::Removal(removal)) => removal.code(),
            CallError::Refused(crate::refusal::Rejection::Runs(ran)) => ran.code(),
            CallError::Call(called) => called.code(),
            CallError::Unreached(unreached) => unreached.code(),
            CallError::LaunchLock { .. } => code!("launch-lock"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            CallError::Refused(crate::refusal::Rejection::Api(api)) => api.args(),
            CallError::Refused(crate::refusal::Rejection::Sessions(ours)) => ours.args(),
            CallError::Refused(crate::refusal::Rejection::Removal(removal)) => removal.args(),
            CallError::Refused(crate::refusal::Rejection::Runs(ran)) => ran.args(),
            CallError::Call(called) => called.args(),
            CallError::Unreached(unreached) => unreached.args(),
            CallError::LaunchLock { detail } => vec![("ext_err_txt", detail.as_str().into())],
        }
    }
}

/// A refusal is a decline (20), as is a daemon this client may not use; a
/// call that failed, or a daemon that could not be started, a failure (10).
impl ExitClass for CallError {
    fn exit(&self) -> Exit {
        match self {
            CallError::Refused(_) | CallError::Unreached(daemon_ports::Unreached::Declined(_)) => {
                Exit::REFUSED
            }
            CallError::Call(_)
            | CallError::LaunchLock { .. }
            | CallError::Unreached(daemon_ports::Unreached::NotStarted { .. }) => Exit::FAILED,
        }
    }
}

/// `daemon stop`: ask the daemon to stop (S12), refused while it hosts live
/// runs unless `even_if_live`.
pub async fn stop<A: DaemonApi>(
    api: &A,
    even_if_live: bool,
) -> Result<daemon_proto::v1::Daemon, CallError> {
    let request = daemon_proto::v1::StopDaemonRequest {
        name: "daemon".to_string(),
        even_if_live,
        ..daemon_proto::v1::StopDaemonRequest::default()
    };
    Ok(api.stop_daemon(request).await?)
}

/// Register-or-recheck the client's project (owner, 2026-11-12): what the
/// daemon names it, or why it refused.
pub async fn resolve<A: DaemonApi>(
    api: &A,
    request: daemon_proto::v1::ResolveProjectRequest,
) -> Result<daemon_proto::v1::Project, CallError> {
    Ok(api.resolve_project(request).await?)
}

/// One value of every diagnostic here.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(SkewError::Busy {
            own: "0.2.0+b".to_string(),
            daemon: "0.1.0+a".to_string(),
            harps: vec!["brisk-copper-moth".to_string()],
        }),
        Box::new(SkewError::Mismatch {
            own: "0.2.0+b".to_string(),
            daemon: "0.1.0+a".to_string(),
        }),
        Box::new(SkewError::Draining {
            own: "v0.2.0-b".to_string(),
            daemon: "v0.1.0-a".to_string(),
            harps: vec!["brisk-copper-moth".to_string()],
        }),
        Box::new(CallError::LaunchLock {
            detail: "permission denied".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "client.test.rs"]
mod tests;
