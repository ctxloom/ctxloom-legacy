//! The capacity the daemon's runtime keeps (owner, 2026-11-19 evening): a
//! turn is waited on by PARKING a thread of the blocking pool, never an async
//! worker, so the workers stay free for messages, steers, hooks and requests.

use run_ports::{BoxFuture, LaunchFailure, Parks, Running, TurnOutput};

/// A turn's end waited for on a thread of the runtime's blocking pool
/// (`finish` stays blocking). The parked threads are bounded by
/// construction: a runner runs one agent, and an agent has one turn at a
/// time; the pool's threads carry the daemon's stack.
pub(crate) struct BlockingParks;

impl Parks for BlockingParks {
    fn park(
        &self,
        running: Box<dyn Running>,
    ) -> BoxFuture<'static, Result<TurnOutput, LaunchFailure>> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || running.finish())
                .await
                .unwrap_or_else(|joined| {
                    Err(LaunchFailure {
                        deterministic: false,
                        detail: joined.to_string(),
                    })
                })
        })
    }
}
