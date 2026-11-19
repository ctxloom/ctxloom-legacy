//! A primary's hooks reach its pending messages THROUGH THE DAEMON (the
//! terminal wake, owner ruling 2026-11-18), as the library reads do
//! (`hook_library`): the hook's session (`CTXLOOM_SESSION`) as a caller of
//! the user's daemon, within the hook's bound. A child's messages are the
//! daemon's to feed as turns, so its hooks deliver none; with no session
//! named, or no daemon in time, none either.

use std::sync::Arc;
use std::time::Instant;

use daemon_api::Caller;
use process_env::EnvSnapshot;
use setup_application::{HookMessage, HookMessages};

use crate::hook_library::HookDaemons;

/// The hook's session's messages over the user's daemon.
pub(crate) struct DaemonHookMessages {
    pub(crate) env: Arc<EnvSnapshot>,
    /// When the hook's bound ends.
    pub(crate) until: Instant,
}

impl DaemonHookMessages {
    /// A hook's messages, its bound starting now.
    pub(crate) fn starting_now(env: Arc<EnvSnapshot>) -> DaemonHookMessages {
        DaemonHookMessages {
            env,
            until: Instant::now() + crate::daemon::HOOK_BOUND,
        }
    }

    /// `call` with the hook's session as the daemon's caller, within the
    /// bound; none when no session is named or nothing answers.
    fn ask<T>(&self, call: impl AsyncFnOnce(&Caller<HookDaemons>, &str) -> Option<T>) -> Option<T> {
        let session = self.env.non_empty(run_ports::SESSION_VARIABLE)?;
        let session = session.to_string_lossy().into_owned();
        let starts = self
            .env
            .utf8(run_ports::ENDPOINT_VARIABLE)
            .is_none_or(str::is_empty);
        let daemons = HookDaemons {
            env: Arc::clone(&self.env),
            until: self.until,
            starts,
        };
        let caller = Caller::new(
            daemons,
            &session,
            crate::daemon::env_vars(&self.env),
            Box::new(crate::daemon::request_id),
        )
        .ok()?;
        let within = self.until.saturating_duration_since(Instant::now());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        runtime
            .block_on(async { tokio::time::timeout(within, call(&caller, &session)).await })
            .ok()
            .flatten()
    }
}

impl HookMessages for DaemonHookMessages {
    fn pending(&self) -> Option<Vec<HookMessage>> {
        self.ask(async |caller, session| {
            let shown = caller.get_session(session).await.ok()?;
            // A child's are fed as its turns.
            if !shown.owner.is_empty() {
                return None;
            }
            Some(
                shown
                    .pending
                    .into_iter()
                    .map(|message| HookMessage {
                        id: message.message_id,
                        urgency: message.urgency,
                        text: message.text,
                        sender: message.sender,
                    })
                    .collect(),
            )
        })
    }

    fn delivered(&self, ids: &[String], wake: &str) -> bool {
        self.ask(async |caller, _| caller.deliver(ids, wake).await.ok())
            .is_some()
    }
}
