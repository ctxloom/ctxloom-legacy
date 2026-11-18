//! A change is reacted to (step 10c U13, G5-G7; protos.md §2, §3).
//!
//! A change is seen at each read, and, for a tree with a live session, at
//! each round of the daemon's loop (G5(b), owner): [`Api::observe_live`]
//! observes, once each, the trees the live sessions run in, as a read
//! observes them, in the daemon's own environment (a round has no
//! caller). A tree no live session runs in is never observed between
//! reads (no watcher). A session is live by its lock's verdict; one with
//! an end stamp is never probed, so a round over a home of ended sessions
//! reads no lock.

use daemon_proto::v1;
use run_ports::Edge;
use session_index::{SessionRecord, State};

use crate::library_reads::pairs;
use crate::server::Api;

/// The directories the live sessions of `records` run in, each once with
/// its project's id, in their first session's order: `state` is a
/// session's lock's verdict (its project directory and harp), asked only
/// of one with no end stamp.
pub(crate) fn live_dirs(
    records: &[SessionRecord],
    state: impl Fn(&str, &str) -> State,
) -> Vec<(String, String)> {
    let mut dirs: Vec<(String, String)> = Vec::new();
    for record in records.iter().filter(|record| record.ended_at.is_none()) {
        let known = dirs.iter().any(|(_, dir)| dir == &record.project_dir);
        if known || state(&record.project_dir, &record.harp) != State::Live {
            continue;
        }
        dirs.push((record.project_id.clone(), record.project_dir.clone()));
    }
    dirs
}

impl<E: Edge> Api<E> {
    /// A round's observation (G5(b)): each tree a live session runs in,
    /// its checkout's and the home's, observed as a read observes them,
    /// git run in `env`; how many sites were observed. A project the
    /// daemon cannot name, or a tree that does not observe, is passed
    /// over: the round goes on.
    pub async fn observe_live(&self, env: &v1::EnvVars) -> u32 {
        let Some((observations, observers)) = self.observing.as_ref() else {
            return 0;
        };
        let live = {
            let index = self.parts.index.index();
            let records = index.records();
            live_dirs(&records, |dir, harp| self.parts.states.state(dir, harp))
                .into_iter()
                .filter_map(|(project, dir)| {
                    index
                        .project_harp(&project)
                        .map(|harp| (format!("projects/{harp}"), dir))
                })
                .collect::<Vec<_>>()
        };
        let mut observed = 0;
        for (parent, dir) in live {
            let Ok((site, project)) = self.resolve(&parent, &dir, Some(env), None).await else {
                continue;
            };
            let trees = observations
                .observe_trees(
                    observers.as_ref(),
                    &pairs(Some(env)),
                    project
                        .as_ref()
                        .map(|(name, root)| (name.clone(), root.as_str())),
                )
                .await;
            if let Ok(trees) = trees {
                observed += u32::try_from(trees.seen.len()).unwrap_or(u32::MAX);
            }
            drop(site);
        }
        observed
    }
}

#[cfg(test)]
#[path = "reactions.test.rs"]
mod tests;
