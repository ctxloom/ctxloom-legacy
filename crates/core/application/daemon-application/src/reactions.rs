//! A library change reacted to (step 10c U13, G6/G7; protos.md §2): the
//! pure, keyed policy.
//!
//! A session's loadout names what its run was planned to draw on: its
//! agent, its profiles, and the bundles its fragments came from (and its
//! tree's config always). Each recorded change (a `TreeObserved` or a
//! `StepApplied`, the events whose sequences move the resources' tokens:
//! one mechanism) is held against the loadout of every session live when
//! it is recorded, against the loadout of each session whose context was
//! handed to it before then: one whose loadout it [`touched`] is [`owed`] a
//! re-assembly, which its engine is handed at the session's next event and
//! recorded once, keyed by the change's cause. Owing is derived from the
//! record (the session's loadout and last re-assembly, the trees' changes),
//! never kept: an owed delivery stays owed until a re-assembly is recorded
//! (a hook that cannot reach the daemon in time, or an event the engine
//! cannot carry it at, leaves it owed), across a daemon's restart too.
//! Nothing here reads a file or a clock.
//!
//! `daemon_application::Reaction` (distill's seam, 10b) reacts to a hosted
//! run's OUTCOME; a library change is no run's, so this policy is a seam of
//! its own beside it, in the same shape: what follows decided purely, made
//! by the daemon.

use std::collections::BTreeSet;

use daemon_proto::v1;
use sessions::SessionLoadout;

/// Where a bundle's directory is in its tree.
const BUNDLES: &str = "content/bundles/v2/";

/// The config file's path in its tree, which every loadout draws on.
const CONFIG: &str = "config.yaml";

/// What [`touched`] names a config change by.
pub const CONFIG_CHANGED: &str = "config";

/// `names` each once, in the order first named.
fn once(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for name in names {
        if !name.is_empty() && !kept.contains(&name) {
            kept.push(name);
        }
    }
    kept
}

/// The loadout of a plan (moved to run-application, where a run records
/// it too).
pub use run_application::loadout_of_plan;

/// The bundle a fragment's resource name (`.../bundles/<b>/fragments/<f>`)
/// is in.
fn bundle_of_name(name: &str) -> Option<String> {
    let (_, rest) = name.split_once("/bundles/")?;
    let (bundle, _) = rest.split_once("/fragments/")?;
    Some(bundle.to_string())
}

/// The loadout of an assembly of `agent` over `profiles`: the bundles of
/// the fragments it used and of those a premise held back.
pub fn loadout_of_assembled(
    agent: &str,
    profiles: &[String],
    assembled: &v1::AssembledContext,
) -> SessionLoadout {
    let used = assembled.fragments.iter().map(|f| f.fragment.as_str());
    let held = assembled.premised.iter().map(|p| p.fragment.as_str());
    SessionLoadout {
        agent: agent.to_string(),
        profiles: profiles.to_vec(),
        bundles: once(used.chain(held).filter_map(bundle_of_name)),
    }
}

/// What of `loadout` a change of `paths` (relative to its tree) touches:
/// each bundle it names that a path is in, and [`CONFIG_CHANGED`] for the
/// tree's config; sorted, each once. Empty: the change is none of its.
pub fn touched(loadout: &SessionLoadout, paths: &[String]) -> Vec<String> {
    let mut named = BTreeSet::new();
    for path in paths {
        if path == CONFIG {
            named.insert(CONFIG_CHANGED.to_string());
            continue;
        }
        let Some(bundle) = path
            .strip_prefix(BUNDLES)
            .and_then(|rest| rest.split('/').next())
        else {
            continue;
        };
        if loadout.bundles.iter().any(|named| named == bundle) {
            named.insert(bundle.to_string());
        }
    }
    named.into_iter().collect()
}

/// What a session is owed: what changed (sorted), and the cause of the
/// latest change, which its delivery answers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Owing {
    pub changed: Vec<String>,
    pub cause: String,
}

/// What a session whose context was last handed to it at `since` (seconds
/// and nanos) is owed by `log`, for its `loadout` on its `trees`: every
/// change of one of them recorded after `since` that [`touched`] the
/// loadout, together; none when none did.
pub fn owed(
    loadout: &SessionLoadout,
    since: (i64, i32),
    trees: &[String],
    log: &[library_checkout::Recorded],
) -> Option<Owing> {
    let mut changed = BTreeSet::new();
    let mut cause = None;
    for recorded in log {
        if recorded.recorded_at <= since || !trees.contains(&recorded.tree) {
            continue;
        }
        let paths: Vec<String> = recorded
            .changes
            .iter()
            .flat_map(|change| [change.path.clone(), change.from_path.clone()])
            .filter(|path| !path.is_empty())
            .collect();
        let named = touched(loadout, &paths);
        if named.is_empty() {
            continue;
        }
        changed.extend(named);
        cause = Some(recorded.cause.clone());
    }
    cause.map(|cause| Owing {
        changed: changed.into_iter().collect(),
        cause,
    })
}

#[cfg(test)]
#[path = "reactions.test.rs"]
mod tests;
