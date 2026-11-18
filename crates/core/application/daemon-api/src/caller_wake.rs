//! A session's calls for the terminal wake (owner ruling 2026-11-18): what
//! its ctxloom MCP server asks of the daemon for its OWN session (watch it,
//! issue and disarm its wake), and what its prompt and tool hooks record
//! (the messages they delivered, the wake a prompt was). Each write is its
//! own stream's command, keyed by the session and what it names, so a
//! redelivery is a DUPLICATE.

use daemon_ports::{Called, DaemonApi, Daemons, SessionSignals};
use daemon_proto::v1;
use sessions::{DeliverMessages, DisarmWake, IssueWake, SessionName};

use crate::caller::{on_daemon, Caller};
use crate::client::CallError;
use crate::refusal::{ApiRejection, Rejection};

impl<D: Daemons> Caller<D> {
    /// This caller's own session; the user has none to watch or wake.
    fn own(&self) -> Result<&SessionName, CallError> {
        self.session().ok_or_else(|| {
            CallError::Refused(Rejection::Api(ApiRejection::NameInvalid {
                name: String::new(),
                reason: "no session".to_string(),
            }))
        })
    }

    /// `WatchSession` of this caller's own session.
    pub async fn watch_own(&self) -> Result<Box<dyn SessionSignals>, CallError> {
        let request = v1::WatchSessionRequest {
            name: self.own()?.to_string(),
        };
        on_daemon!(self, |api| api.watch_session(request.clone()).await)
    }

    /// Its wake `nonce` issued, keyed `wake:<session>:<nonce>`; whether
    /// this call applied it.
    pub async fn issue_wake(&self, nonce: &str) -> Result<bool, CallError> {
        let me = self.own()?;
        let key = format!("wake:{}:{nonce}", me.uid());
        let command = IssueWake {
            nonce: nonce.to_string(),
        };
        let (_, applied) = on_daemon!(self, |api| self.write(api, me, &command, &key).await)?;
        Ok(applied)
    }

    /// Its wake `nonce` disarmed (nothing if it is not outstanding), keyed
    /// `disarm:<session>:<nonce>`.
    pub async fn disarm_wake(&self, nonce: &str) -> Result<(), CallError> {
        let me = self.own()?;
        let key = format!("disarm:{}:{nonce}", me.uid());
        let command = DisarmWake {
            nonce: nonce.to_string(),
        };
        on_daemon!(self, |api| self.write(api, me, &command, &key).await)?;
        Ok(())
    }

    /// The messages `ids` its hook delivered, and the wake `nonce` its
    /// prompt was (empty for none), keyed `deliver:<session>:<nonce>:<ids,
    /// comma-joined>`; whether this call applied it.
    pub async fn deliver(&self, ids: &[String], nonce: &str) -> Result<bool, CallError> {
        let me = self.own()?;
        let key = format!("deliver:{}:{nonce}:{}", me.uid(), ids.join(","));
        let command = DeliverMessages {
            message_ids: ids.to_vec(),
            wake_nonce: nonce.to_string(),
        };
        let (_, applied) = on_daemon!(self, |api| self.write(api, me, &command, &key).await)?;
        Ok(applied)
    }
}
