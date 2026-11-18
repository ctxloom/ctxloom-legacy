//! The engines' hooks read the library THROUGH THE DAEMON (step 10a U2, O3,
//! owner rulings 2026-11-16 and 2026-11-17): the session-start nudge asks
//! `ListAgents` and `ListProfiles`, the skill mates `ListSkills`, each of
//! the user's daemon (found, else started, as a hook's delivery is), all
//! within the hook's bound (S16, `HOOK_BOUND`), degraded. With no daemon
//! in time, or a read that failed, the hook says nothing of the library: it
//! never reads it in process. A hook whose environment names an endpoint
//! (a container's arrival, OD-13) never starts a daemon, so it asks only a
//! daemon that serves already.

use std::sync::Arc;
use std::time::{Duration, Instant};

use daemon_api::LibraryReads;
use daemon_ports::{Daemons, Unreached};
use daemon_proto::v1;
use process_env::EnvSnapshot;
use setup_application::{hook_skills, HookLibrary, HookSkill};

use crate::Wiring;

/// The user's daemon as a hook's library read reaches it, until `until`.
pub(crate) struct HookDaemons {
    env: Arc<EnvSnapshot>,
    until: Instant,
    /// Whether it may start one (none where the environment names an
    /// endpoint).
    starts: bool,
}

impl HookDaemons {
    fn remaining(&self) -> Duration {
        self.until.saturating_duration_since(Instant::now())
    }

    fn client(&self, found: &run_ports::Discovery) -> daemon_client::DaemonClient {
        daemon_client::DaemonClient::new(found, self.remaining())
    }

    fn found(&self, wait: Duration) -> Option<run_ports::Discovery> {
        if self.starts {
            crate::daemon::hook_daemon(&self.env, wait)
        } else {
            crate::daemon::hook_daemon_serving(&self.env)
        }
    }
}

impl Daemons for HookDaemons {
    type Api = daemon_client::DaemonClient;

    async fn serving(&self) -> Result<Option<daemon_client::DaemonClient>, Unreached> {
        Ok(crate::daemon::hook_daemon_serving(&self.env).map(|found| self.client(&found)))
    }

    async fn reach(&self) -> Result<daemon_client::DaemonClient, Unreached> {
        let wait = self
            .remaining()
            .saturating_sub(crate::daemon::DELIVERY_MARGIN)
            .min(crate::daemon::SPAWN_WAIT);
        match self.found(wait) {
            Some(found) => Ok(self.client(&found)),
            None => Err(Unreached::NotStarted { log: String::new() }),
        }
    }
}

/// The hooks' library reads over the user's daemon (O3).
pub(crate) struct DaemonHookLibrary {
    pub(crate) env: Arc<EnvSnapshot>,
    /// When the hook's bound ends.
    pub(crate) until: Instant,
}

impl DaemonHookLibrary {
    /// A hook's reads, its bound starting now.
    pub(crate) fn starting_now(env: Arc<EnvSnapshot>) -> DaemonHookLibrary {
        DaemonHookLibrary {
            env,
            until: Instant::now() + crate::daemon::HOOK_BOUND,
        }
    }

    /// `read` over this hook's client of the daemon, within the bound;
    /// none when anything on the way does not answer.
    fn ask<T>(
        &self,
        read: impl AsyncFnOnce(&daemon_api::Client<HookDaemons>) -> Option<T>,
    ) -> Option<T> {
        let wiring = Wiring::new(Arc::clone(&self.env)).ok()?;
        let registration = wiring.library_registration(crate::Minting::AtRoot).ok()?;
        let cwd = wiring
            .env()
            .cwd()
            .map(|cwd| cwd.display().to_string())
            .unwrap_or_default();
        let starts = self
            .env
            .utf8(run_ports::ENDPOINT_VARIABLE)
            .is_none_or(str::is_empty);
        let daemons = HookDaemons {
            env: Arc::clone(&self.env),
            until: self.until,
            starts,
        };
        let client = daemon_api::Client::new(
            daemons,
            registration,
            cwd,
            crate::daemon::env_vars(wiring.env()),
        );
        let within = self.until.saturating_duration_since(Instant::now());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        runtime
            .block_on(async { tokio::time::timeout(within, read(&client)).await })
            .ok()
            .flatten()
    }
}

/// A hook's reads are degraded: a config a strict load refuses must not
/// silence it.
fn degraded() -> v1::Globals {
    v1::Globals {
        degraded: true,
        ..v1::Globals::default()
    }
}

impl HookLibrary for DaemonHookLibrary {
    fn agents_and_profiles(&self) -> Option<(bool, bool)> {
        self.ask(async |client| {
            let agents = client
                .list_agents(
                    "",
                    v1::ListAgentsRequest {
                        globals: Some(degraded()),
                        ..v1::ListAgentsRequest::default()
                    },
                )
                .await
                .ok()?;
            let profiles = client
                .list_profiles(
                    "",
                    v1::ListProfilesRequest {
                        globals: Some(degraded()),
                        ..v1::ListProfilesRequest::default()
                    },
                )
                .await
                .ok()?;
            Some((
                !agents.answer.agents.is_empty(),
                !profiles.answer.profiles.is_empty(),
            ))
        })
    }

    /// The context the live session `harp` is owed at `hook`, as the
    /// daemon re-assembled and its engine's delivery put it (step 10c U13);
    /// none owed, or no daemon in time, none.
    fn redelivered(&self, harp: &str, hook: &str) -> Option<String> {
        if harp.is_empty() {
            return None;
        }
        self.ask(async |client| {
            let answered = client.redeliver_context(harp, hook).await.ok()?;
            Some(answered.context).filter(|context| !context.is_empty())
        })
    }

    fn skills(&self) -> Option<Vec<HookSkill>> {
        self.ask(async |client| {
            let listed = client
                .list_skills(
                    "",
                    v1::ListSkillsRequest {
                        globals: Some(degraded()),
                        ..v1::ListSkillsRequest::default()
                    },
                )
                .await
                .ok()?;
            Some(hook_skills(&listed.answer))
        })
    }
}
