//! The asks an engine makes over its own protocol (opencode's ACP
//! `session/request_permission`, a hosted child's or one-shot's), routed to
//! the user (step 8 U7): a [`run_ports::PermissionRouter`] over the daemon's
//! own API, in process. Each ask is recorded with the engine's own options
//! (`Api::create_routed_approval`), waited on until the user answers, it
//! expires or it is withdrawn (`Api::routed_answer`, bounded by its
//! `approval_timeout` and a margin), and withdrawn when its turn is
//! redirected (`Api::withdraw_routed`).
//!
//! The engine's adapter calls [`PermissionAsks::ask`] and
//! [`PermissionAsks::withdraw`] on threads of its own, so a withdrawal can
//! come while its ask is still being recorded: it is then held, and made
//! once the ask is recorded (a withdrawal of an ask not yet recorded would
//! be a no-op, and its key would make the real one a duplicate). Each call
//! is driven to its end on the daemon's runtime from the caller's thread.
//!
//! Until U5's expiry lands, an ask nobody answers is ended here by its
//! bound: the wait gives up after its `approval_timeout` and the margin,
//! and the engine is answered cancelled.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::time::Duration;

use daemon_proto::v1;
use run_ports::{Asker, PermissionAnswer, PermissionAsk, PermissionAsks, PermissionRouter};

/// How much longer than its `approval_timeout` an ask is waited for before
/// it is given up (the daemon's own expiry is the ask's to say first).
pub(crate) const WAIT_MARGIN: Duration = Duration::from_secs(60);

type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The daemon's three routed calls, as this bridge makes them.
pub(crate) trait Routes: Send + Sync {
    fn create<'a>(
        &'a self,
        asker: &'a Asker,
        ask: &'a PermissionAsk,
    ) -> Boxed<'a, Result<v1::Approval, String>>;
    /// One round of the wait: the answer once the ask is settled, none
    /// while it waits.
    fn answer<'a>(&'a self, approval: &'a v1::Approval) -> Boxed<'a, Option<PermissionAnswer>>;
    fn withdraw<'a>(&'a self, approval: &'a v1::Approval, reason: &'a str) -> Boxed<'a, ()>;
}

impl<E: run_ports::Edge + 'static> Routes for daemon_api::Api<E> {
    fn create<'a>(
        &'a self,
        asker: &'a Asker,
        ask: &'a PermissionAsk,
    ) -> Boxed<'a, Result<v1::Approval, String>> {
        Box::pin(async move {
            self.create_routed_approval(asker, ask)
                .await
                .map_err(|error| error.to_string())
        })
    }

    fn answer<'a>(&'a self, approval: &'a v1::Approval) -> Boxed<'a, Option<PermissionAnswer>> {
        Box::pin(self.routed_settled(approval))
    }

    fn withdraw<'a>(&'a self, approval: &'a v1::Approval, reason: &'a str) -> Boxed<'a, ()> {
        Box::pin(async move {
            let _ = self.withdraw_routed(approval, reason).await;
        })
    }
}

/// The daemon's API, once it is built: the one-shots it hosts are composed
/// before it, so their router is given it late. Held WEAKLY: the API holds
/// the daemon's store, which is closed once its server is let go (ruling
/// 131), and an ask is no reason to keep it (one asked after is cancelled).
pub(crate) type LateRoutes = Arc<OnceLock<Weak<dyn Routes>>>;

/// What a daemon routes its engines' asks through: the API's late cell,
/// and the router over it.
pub(crate) struct Asking {
    pub(crate) routes: LateRoutes,
    pub(crate) router: Arc<dyn PermissionRouter>,
}

impl Asking {
    /// The API that serves, given to the routes (weakly: ruling 131).
    pub(crate) fn serving(&self, api: Arc<dyn Routes>) {
        let _ = self.routes.set(Arc::downgrade(&api));
    }
}

/// The router: each session's asks over the daemon's API, on its runtime.
pub(crate) struct RoutedAsks {
    pub(crate) routes: LateRoutes,
    pub(crate) runtime: tokio::runtime::Handle,
    /// The daemon's log.
    pub(crate) say: Arc<dyn Fn(&str) + Send + Sync>,
}

impl PermissionRouter for RoutedAsks {
    fn asks(&self, asker: &Asker) -> Arc<dyn PermissionAsks> {
        Arc::new(SessionAsks {
            routes: Arc::clone(&self.routes),
            runtime: self.runtime.clone(),
            say: Arc::clone(&self.say),
            asker: asker.clone(),
            asks: Mutex::new(BTreeMap::new()),
        })
    }
}

/// Where an ask of the session stands, by its request id.
enum Standing {
    /// Being recorded.
    Asking,
    /// Recorded: its approval.
    Made(Box<v1::Approval>),
    /// Withdrawn before it was recorded, for this reason.
    Withdrawn(String),
}

/// One session's routed asks.
pub(crate) struct SessionAsks {
    routes: LateRoutes,
    runtime: tokio::runtime::Handle,
    say: Arc<dyn Fn(&str) + Send + Sync>,
    asker: Asker,
    asks: Mutex<BTreeMap<String, Standing>>,
}

impl SessionAsks {
    fn standing(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Standing>> {
        self.asks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn routes(&self) -> Option<Arc<dyn Routes>> {
        self.routes.get().and_then(Weak::upgrade)
    }

    /// The answer to `made`, waited for a round at a time, the API held
    /// for one round only (so a daemon that is closing its store is never
    /// kept from it by an ask), and given up after its `approval_timeout`
    /// and [`WAIT_MARGIN`]: cancelled, as it is once the API is gone.
    fn wait(&self, made: &v1::Approval) -> PermissionAnswer {
        let within = self.asker.approval_timeout + WAIT_MARGIN;
        let rounds = async {
            loop {
                let Some(routes) = self.routes() else {
                    return PermissionAnswer::Cancelled;
                };
                if let Some(answer) = routes.answer(made).await {
                    return answer;
                }
            }
        };
        // The timer is made on the runtime: its reactor is there.
        let waited = self
            .runtime
            .block_on(async { tokio::time::timeout(within, rounds).await });
        waited.unwrap_or(PermissionAnswer::Cancelled)
    }
}

impl PermissionAsks for SessionAsks {
    fn ask(&self, ask: &PermissionAsk) -> PermissionAnswer {
        let Some(routes) = self.routes() else {
            (self.say)("an ask came before the daemon's API: answered cancelled");
            return PermissionAnswer::Cancelled;
        };
        self.standing()
            .insert(ask.request_id.clone(), Standing::Asking);
        let made = match self.runtime.block_on(routes.create(&self.asker, ask)) {
            Ok(made) => made,
            Err(error) => {
                self.standing().remove(&ask.request_id);
                (self.say)(&format!(
                    "{}: its ask {} not recorded: {error}",
                    self.asker.session, ask.request_id
                ));
                return PermissionAnswer::Cancelled;
            }
        };
        let withdrawn = {
            let mut standing = self.standing();
            match standing.remove(&ask.request_id) {
                Some(Standing::Withdrawn(reason)) => Some(reason),
                _ => {
                    standing.insert(
                        ask.request_id.clone(),
                        Standing::Made(Box::new(made.clone())),
                    );
                    None
                }
            }
        };
        if let Some(reason) = withdrawn {
            self.runtime.block_on(routes.withdraw(&made, &reason));
            return PermissionAnswer::Cancelled;
        }
        drop(routes);
        let answer = self.wait(&made);
        self.standing().remove(&ask.request_id);
        answer
    }

    fn withdraw(&self, request_id: &str, reason: &str) {
        let made = {
            let mut standing = self.standing();
            match standing.get(request_id) {
                Some(Standing::Made(made)) => Some((**made).clone()),
                Some(Standing::Asking) => {
                    standing.insert(
                        request_id.to_string(),
                        Standing::Withdrawn(reason.to_string()),
                    );
                    None
                }
                Some(Standing::Withdrawn(_)) | None => None,
            }
        };
        if let (Some(made), Some(routes)) = (made, self.routes()) {
            self.runtime.block_on(routes.withdraw(&made, reason));
        }
    }
}

#[cfg(test)]
#[path = "asks.test.rs"]
mod tests;
