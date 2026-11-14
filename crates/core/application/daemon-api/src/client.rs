//! The client's version-skew rule (M3, owner 2026-11-12): a client whose
//! full versionator stamp is not the serving daemon's asks the daemon
//! (`GetDaemon`) whether it hosts live runs. None: it stops that daemon
//! (`StopDaemon`) and starts its own. Some: it is refused, a decline whose
//! fix is `ctxloom daemon stop --even-if-live`.

use std::fmt;

use angzarr_diagnostic::{code, Arg, Diagnostic};
use daemon_ports::{Called, DaemonApi};
use exit_code::{Exit, ExitClass};

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
    if !found.runs.is_empty() {
        return Err(busy(found.runs.into_iter().map(|run| run.harp).collect()));
    }
    let stop = daemon_proto::v1::StopDaemonRequest {
        name: found.name,
        ..daemon_proto::v1::StopDaemonRequest::default()
    };
    match api.stop_daemon(stop).await {
        Ok(_) => Ok(Reconciled::Stopped),
        // A run started between the two calls.
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
/// OLDER version ([`newer`]) and idle, it is asked to drain (`StopDaemon`,
/// never `even_if_live`: it starts nothing more and exits once its calls in
/// flight are answered), so the caller starts its own and its call goes
/// there. Busy (it hosts runs, or a run started between the look and the
/// drain), or not older (an older or equal version: two binaries never
/// drain each other's daemon in turn), the call is refused
/// ([`SkewError::Mismatch`]), the daemon asked nothing it could end.
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

/// Whether stamp `own` names a newer VERSION than stamp `daemon`: semver
/// precedence of the `MAJOR.MINOR.PATCH[-pre]` each stamp
/// (`v<version>-<hash>[-<time>-dirty]`) carries. The hash and a dirty
/// build's time order nothing, so two builds of one version are neither;
/// a stamp not of the shape is never newer, nor older.
pub fn newer(own: &str, daemon: &str) -> bool {
    match (Version::of_stamp(own), Version::of_stamp(daemon)) {
        (Some(own), Some(daemon)) => own.precedes(&daemon) == std::cmp::Ordering::Greater,
        _ => false,
    }
}

/// A stamp's version, as semver orders it.
struct Version<'a> {
    numbers: [u64; 3],
    /// The pre-release identifiers; none, a release.
    pre: Vec<&'a str>,
}

impl<'a> Version<'a> {
    /// The version of `stamp`, or none when it is not a stamp's shape.
    fn of_stamp(stamp: &'a str) -> Option<Version<'a>> {
        let rest = stamp.strip_prefix('v')?;
        let rest = match rest.strip_suffix("-dirty") {
            Some(dirty) => dirty.rsplit_once('-')?.0,
            None => rest,
        };
        let (version, hash) = rest.rsplit_once('-')?;
        if hash.is_empty() || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let (core, pre) = match version.split_once('-') {
            Some((core, pre)) => (core, pre.split('.').collect()),
            None => (version, Vec::new()),
        };
        let parsed: Vec<u64> = core
            .split('.')
            .map(|n| {
                n.bytes()
                    .all(|b| b.is_ascii_digit())
                    .then(|| n.parse().ok())
                    .flatten()
            })
            .collect::<Option<_>>()?;
        let numbers: [u64; 3] = parsed.try_into().ok()?;
        if pre.iter().any(|id: &&str| id.is_empty()) {
            return None;
        }
        Some(Version { numbers, pre })
    }

    /// Semver precedence: the numbers; then a release above any of its
    /// pre-releases; then identifier by identifier, numbers by value and
    /// below words, a longer list above its prefix.
    fn precedes(&self, other: &Version<'_>) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let by_numbers = self.numbers.cmp(&other.numbers);
        if by_numbers != Ordering::Equal {
            return by_numbers;
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            (false, false) => {}
        }
        for (a, b) in self.pre.iter().zip(&other.pre) {
            let order = match (a.parse::<u64>(), b.parse::<u64>()) {
                (Ok(a), Ok(b)) => a.cmp(&b),
                (Ok(_), Err(_)) => Ordering::Less,
                (Err(_), Ok(_)) => Ordering::Greater,
                (Err(_), Err(_)) => a.cmp(b),
            };
            if order != Ordering::Equal {
                return order;
            }
        }
        self.pre.len().cmp(&other.pre.len())
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
            SkewError::Call(called) => called.args(),
        }
    }
}

/// A busy daemon of another version is a decline (exit 20); a call that
/// failed is a failure.
impl ExitClass for SkewError {
    fn exit(&self) -> Exit {
        match self {
            SkewError::Busy { .. } | SkewError::Mismatch { .. } => Exit::REFUSED,
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
        Box::new(CallError::LaunchLock {
            detail: "permission denied".to_string(),
        }),
    ]
}

#[cfg(test)]
#[path = "client.test.rs"]
mod tests;
