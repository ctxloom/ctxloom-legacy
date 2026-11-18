//! Feature discovery: every `.feature` file under `features/` is run, with the
//! World its `# world:` header line names (`gherkin_common::world_of`). Adding
//! a feature needs no hand-written test, only its place in a cluster
//! ([`CLUSTERS`]); `gherkin_lint.test.rs` has the convention and the parity
//! lint. A `@pending` scenario (Go's, not yet ported: tagged on itself, its
//! Rule or its Feature) is left out of the run; a feature tagged `@pending`
//! as a whole has no World and is not run at all.
//!
//! The features run in CLUSTERS (owner ruling of 2026-11-14): each cluster
//! is one test, `tests::gherkin_features::clusters::<name>`, so nextest runs
//! the clusters at once, each in a process of its own, and each has a limit
//! of its own (`.config/nextest.toml`). Every feature file is in exactly one
//! cluster (`every_feature_file_is_in_exactly_one_cluster`). Within a
//! cluster the features run concurrently, as many at once as the machine's
//! cores shared among the clusters ([`cluster_threads`]). A cucumber run is
//! let run 4 scenarios at once, but its steps block its one executor
//! thread, so a feature's scenarios take turns (the forcing test below saw
//! it), and a cluster takes at least as long as its slowest feature. Each
//! scenario has a temporary root of its own (its project and its home, so
//! its own ctxloom home, daemon, single-instance lock, store and stubs), so
//! nothing a cluster runs is another's (the forcing test
//! `two_clusters_at_once_never_see_each_others_daemon_or_files`).
//!
//! `GHERKIN_ONLY=<text>` runs only the features whose path contains it, and
//! `GHERKIN_SCENARIO=<id>` only the scenario of that id (the ratchet's
//! re-confirmation, alone); a cluster holding none of them runs nothing and
//! passes, and when no feature at all matches, every cluster fails.
//!
//! `GHERKIN_TIMINGS=<absolute file>` appends each scenario's wall time to
//! the file, a line `<seconds>\t<id>` per scenario (the profile of a slow
//! cluster: daemon.feature's, owner ruling of 2026-11-18). Under it a
//! feature's scenarios run one at a time, so no scenario's time holds
//! another's: they take turns anyway (above), and the cluster's time was
//! the same either way when it was measured.
//!
//! `GHERKIN_RATCHET=<absolute file>` asks for the scenario list instead of a
//! verdict: every scenario run, one line each, sorted, `red\t<id>` or
//! `green\t<id>` (the id is `gherkin_common::scenario_id`'s, a failure no
//! scenario owns the feature's `outside_id`), is written to the file, and the
//! test passes whatever failed. Beside it, `<file>.failures` says why each
//! red scenario is red: its failed step and what it said (capped), which the
//! console's output, hidden on a pass, no longer does. The clusters of one
//! nextest run assemble the one list between them: each, holding the lock
//! `<file>.lock`, replaces the lines of the features it ran with its own and
//! keeps the rest, sorted as one list ([`merged_list`], [`merged_failures`]);
//! what an earlier run left is dropped, because `<file>.run` names the run
//! that wrote it (nextest's `NEXTEST_RUN_ID`). So the file is exactly the
//! run's scenarios, as when one test wrote it. `just closeout-ratchet`
//! (scripts/closeout-ratchet.sh) judges that list against its base branch's;
//! nothing else sets it.
//!
//! `GHERKIN_RATCHET_BASE=<absolute file>`, a base's scenario list (`just
//! ratchet-base`'s), judges each cluster against it, in the test itself: it
//! fails naming each of its scenarios red here and not red in the base
//! (scripts/ratchet-judge.sh's rule: one the base lists green `broke green`,
//! one it does not list `added red`), and passes a scenario the base lists
//! red, red or green. So a tree that carries red spec on purpose (an
//! integration branch, owner ruling 165) can be mutation-tested with the
//! runner still judging: its unmutated baseline passes with the base's red,
//! and a mutant that turns a scenario red is killed. `just mutants-shard`
//! and `closeout-ratchet`'s `mutants-batch` set it when the base has red
//! scenarios. Beside `GHERKIN_RATCHET` the list is written first, then
//! judged.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::gherkin_common::{
    feature_files, features_dir, is_pending_feature, outside_id, world_of, Failure, FeatureRun,
};
use super::host_var;
use crate::tests::files;

/// One cluster of feature files, run as one test: its name (its test's
/// name under `clusters::`) and its files, each a path under `features/`,
/// or a directory's (ending `/`: every file below it).
pub(crate) struct Cluster {
    pub(crate) name: &'static str,
    pub(crate) files: &'static [&'static str],
}

/// The clusters, by area (owner ruling of 2026-11-14). Every feature file is
/// in exactly one, by name or by its directory; a new file goes in the
/// cluster of its area, else `other`.
pub(crate) const CLUSTERS: &[Cluster] = &[
    Cluster {
        name: "journeys",
        files: &["journeys/"],
    },
    // The user daemon's spec (gherkin_daemon.test.rs's files), and run.feature
    // whole: since slice 4 every one-shot starts a daemon, so its Rules
    // cannot be told apart by that.
    Cluster {
        name: "daemon",
        files: &[
            "cli/daemon.feature",
            "cli/shutdown.feature",
            "cli/session_children.feature",
            "cli/mcp_serve.feature",
            "cli/run.feature",
            "cli/trust.feature",
            "cli/session_approvals.feature",
            "cli/atomic_writes.feature",
        ],
    },
    Cluster {
        name: "sessions",
        files: &[
            "cli/session.feature",
            "cli/session_hooks.feature",
            "cli/session_remove.feature",
            "cli/session_worktrees.feature",
            "cli/post_tool_hooks.feature",
        ],
    },
    Cluster {
        name: "library",
        files: &[
            "cli/bundle.feature",
            "cli/fragment.feature",
            "cli/fragment_draft_premise.feature",
            "cli/command.feature",
            "cli/profile.feature",
            "cli/skill.feature",
            "cli/mcp.feature",
            "cli/mcp_resources.feature",
            "cli/mcp_tools.feature",
            "cli/search.feature",
            "cli/materialize.feature",
            "cli/read_extras.feature",
            "cli/context_status.feature",
            "cli/content_distill.feature",
            "cli/companion.feature",
            "cli/agent.feature",
            "cli/library_changes.feature",
        ],
    },
    Cluster {
        name: "remotes",
        files: &["cli/remote.feature", "cli/deps.feature"],
    },
    Cluster {
        name: "setup",
        files: &[
            "cli/init.feature",
            "cli/doctor.feature",
            "cli/manage.feature",
            "cli/auth.feature",
            "cli/config.feature",
            "cli/engine.feature",
            "cli/clean.feature",
            "cli/version.feature",
            "cli/completion.feature",
        ],
    },
    Cluster {
        name: "other",
        files: &[
            "fault_tolerance.feature",
            "probes/",
            "cli/container.feature",
            "cli/coordination_contract.feature",
            "cli/startup_boundaries.feature",
            "cli/editing.feature",
            "cli/util.feature",
        ],
    },
];

/// Whether the cluster entry `entry` names the feature file `rel`: the file
/// itself, or (ending `/`) a directory it is below.
fn names(entry: &str, rel: &str) -> bool {
    if entry.ends_with('/') {
        rel.starts_with(entry)
    } else {
        rel == entry
    }
}

/// The feature files of `all` that `cluster` holds, in `all`'s order.
fn members(cluster: &Cluster, all: &[String]) -> Vec<String> {
    all.iter()
        .filter(|rel| cluster.files.iter().any(|entry| names(entry, rel)))
        .cloned()
        .collect()
}

/// Run one feature with its World: whether it passed, and each scenario's
/// outcome. A pending feature is skipped; a pending scenario is left out by
/// the World's run (`gherkin_common::run_feature`). `rel` is a path under
/// `features/`, or an absolute path.
fn run_one(rel: &str, only: Option<&str>) -> Result<FeatureRun, String> {
    let path = features_dir().join(rel);
    let text = files::read(&path).map_err(|e| format!("{rel}: {e}"))?;
    if is_pending_feature(&text) {
        return Ok(FeatureRun {
            passed: true,
            scenarios: Vec::new(),
            failures: Vec::new(),
        });
    }
    match world_of(&text).map_err(|e| format!("{rel}: {e}"))? {
        "base" => Ok(super::gherkin::run_only(&path, only)),
        "surfaces" => Ok(super::gherkin_surfaces::run_only(&path, only)),
        "isolation" => Ok(super::gherkin_isolation::run_only(&path, only)),
        other => Err(format!("{rel}: unknown world {other}")),
    }
}

/// What a run of some features came to.
struct Ran {
    /// Each feature that failed, said.
    failures: Vec<String>,
    /// Each scenario's id and whether it passed.
    outcomes: Vec<(String, bool)>,
    /// Each failed step.
    steps_failed: Vec<Failure>,
}

/// How many features of a cluster run at once: the machine's cores shared
/// among the clusters, which nextest runs at once, so all of them together
/// run about as many features at once as the one test of every feature did
/// (a thread per core); at least 2, so a small host's cluster is not one
/// feature after another.
fn cluster_threads() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4)
        .div_ceil(CLUSTERS.len())
        .max(2)
}

/// Run `features` concurrently, `threads` at a time (`only`: the one
/// scenario id to run, as `GHERKIN_SCENARIO`).
fn run_features(features: &[String], only: Option<&str>, threads: usize) -> Ran {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let outcomes = Mutex::new(Vec::new());
    let steps_failed = Mutex::new(Vec::new());
    let threads = threads.clamp(1, features.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(rel) = features.get(i) else { break };
                let (failed, seen) = match run_one(rel, only) {
                    Ok(run) if run.passed => (None, run.scenarios),
                    Ok(run) => {
                        steps_failed
                            .lock()
                            .expect("not poisoned")
                            .extend(run.failures);
                        (
                            Some(format!("{rel}: a scenario failed (its output is above)")),
                            run.scenarios,
                        )
                    }
                    Err(e) => (Some(e), vec![(outside_id(rel), false)]),
                };
                outcomes.lock().expect("not poisoned").extend(seen);
                if let Some(e) = failed {
                    failures.lock().expect("not poisoned").push(e);
                }
            });
        }
    });
    quiet_hook_undone();
    let mut failures = failures.into_inner().expect("not poisoned");
    failures.sort();
    Ran {
        failures,
        outcomes: outcomes.into_inner().expect("not poisoned"),
        steps_failed: steps_failed.into_inner().expect("not poisoned"),
    }
}

/// The default panic hook, back: a cucumber run swaps in a hook that says
/// nothing while its steps run and puts back the one it took, so two runs at
/// once can leave the quiet one in place, and the test's own assertion would
/// then fail saying nothing.
fn quiet_hook_undone() {
    drop(std::panic::take_hook());
}

/// The scenario list `GHERKIN_RATCHET` asks for: a line per scenario,
/// `red\t<id>` or `green\t<id>`, sorted, each id once (red if any run of it
/// failed).
fn scenario_list(outcomes: &[(String, bool)]) -> String {
    let mut red = std::collections::BTreeSet::new();
    let mut all = std::collections::BTreeSet::new();
    for (id, passed) in outcomes {
        all.insert(id.as_str());
        if !passed {
            red.insert(id.as_str());
        }
    }
    all.into_iter()
        .map(|id| {
            let word = if red.contains(id) { "red" } else { "green" };
            format!("{word}\t{id}\n")
        })
        .collect()
}

/// `existing`, a scenario list, with the lines of the features `ran` taken
/// out and the lines of `ours` (the list of a run of `ran`) put in: sorted by
/// id, as [`scenario_list`] sorts.
fn merged_list(existing: &str, ran: &[String], ours: &str) -> String {
    let kept = existing
        .lines()
        .filter(|line| {
            let id = line.split_once('\t').map_or(*line, |(_, id)| id);
            !ran.iter().any(|f| f == feature_of(id))
        })
        .map(str::to_string);
    let mut lines: Vec<String> = kept.chain(ours.lines().map(str::to_string)).collect();
    lines.sort_by(|a, b| {
        let id = |l: &str| l.split_once('\t').map_or(l, |(_, id)| id).to_string();
        id(a).cmp(&id(b))
    });
    lines.into_iter().map(|line| line + "\n").collect()
}

/// The records of a failures file: each from its `red\t<id>` line to the
/// next, its blank lines with it, and the id it is of.
fn failure_records(text: &str) -> Vec<(String, String)> {
    let mut records: Vec<(String, String)> = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(id) = line.strip_prefix("red\t") {
            records.push((id.trim_end_matches('\n').to_string(), String::new()));
        }
        if let Some((_, record)) = records.last_mut() {
            record.push_str(line);
        }
    }
    records
}

/// `existing`, a failures file, with the records of the features `ran`
/// taken out and those of `ours` put in, sorted by scenario id (stably: a
/// scenario's records stay in their order), as [`failures_file`] sorts.
fn merged_failures(existing: &str, ran: &[String], ours: &str) -> String {
    let mut records: Vec<(String, String)> = failure_records(existing)
        .into_iter()
        .filter(|(id, _)| !ran.iter().any(|f| f == feature_of(id)))
        .chain(failure_records(ours))
        .collect();
    records.sort_by(|a, b| a.0.cmp(&b.0));
    records.into_iter().map(|(_, record)| record).collect()
}

/// The feature of a scenario id: its part before the first ` :: `.
fn feature_of(id: &str) -> &str {
    id.split(" :: ").next().unwrap_or(id)
}

/// How much of a failure's message the failures file keeps.
const FAILURE_MESSAGE_MAX: usize = 2_000;

/// The failures file beside the `GHERKIN_RATCHET` list (`<list>.failures`):
/// a record per failed step, sorted by scenario id, blank-line separated:
/// `red\t<id>`, `  step: <keyword> <text>`, then the message, each line
/// indented, capped at [`FAILURE_MESSAGE_MAX`] bytes (`[+N bytes]` after).
fn failures_file(failures: &[Failure]) -> String {
    let mut sorted: Vec<&Failure> = failures.iter().collect();
    sorted.sort_by(|a, b| a.scenario.cmp(&b.scenario));
    sorted
        .into_iter()
        .map(|failed| {
            let mut message = failed.message.clone();
            if message.len() > FAILURE_MESSAGE_MAX {
                let mut cut = FAILURE_MESSAGE_MAX;
                while !message.is_char_boundary(cut) {
                    cut -= 1;
                }
                let more = message.len() - cut;
                message.truncate(cut);
                message.push_str(&format!(" [+{more} bytes]"));
            }
            let body: Vec<String> = message.lines().map(|l| format!("  {l}")).collect();
            format!(
                "red\t{}\n  step: {}\n{}\n\n",
                failed.scenario,
                failed.step,
                body.join("\n")
            )
        })
        .collect()
}

/// A file beside `file`: its name with `suffix` added.
fn beside(file: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(suffix);
    std::path::PathBuf::from(name)
}

/// Where the failures file of the list `file` is: beside it.
fn failures_path(file: &std::path::Path) -> std::path::PathBuf {
    beside(file, ".failures")
}

/// The run this test is part of: nextest's run id, which every test of one
/// `cargo nextest run` shares; outside nextest, this process.
fn run_id() -> String {
    host_var("NEXTEST_RUN_ID").map_or_else(
        || format!("process {}", std::process::id()),
        |id| id.to_string_lossy().into_owned(),
    )
}

/// Put a run's list and failures (`ran`: the features it ran) into the list
/// `file` and its failures file, under the lock `<file>.lock`: merged into
/// what another cluster of the same run (`run`, kept in `<file>.run`) wrote
/// there, and in place of what an earlier run left.
fn put_ratchet(file: &std::path::Path, run: &str, ran: &[String], list: &str, failures: &str) {
    let lock = beside(file, ".lock");
    let _held = file_lock::lock_within(&*files::fs(), &lock, std::time::Duration::from_secs(120))
        .expect("the ratchet list's lock");
    let marker = beside(file, ".run");
    let ours = files::read(&marker).is_ok_and(|text| text == run);
    let existing = |path: &std::path::Path| {
        if ours {
            files::read(path).unwrap_or_default()
        } else {
            String::new()
        }
    };
    let merged = merged_list(&existing(file), ran, list);
    let merged_why = merged_failures(&existing(&failures_path(file)), ran, failures);
    files::put(&marker, run);
    files::put(file, merged);
    files::put(failures_path(file), merged_why);
}

/// Run the cluster `name`: its features that `GHERKIN_ONLY` and
/// `GHERKIN_SCENARIO` leave in, then either the verdict (every one passed)
/// or, under `GHERKIN_RATCHET`, its part of the scenario list.
fn run_cluster(name: &str) {
    let only = host_var("GHERKIN_ONLY")
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ratchet = host_var("GHERKIN_RATCHET").map(std::path::PathBuf::from);
    let base = host_var("GHERKIN_RATCHET_BASE").map(std::path::PathBuf::from);
    let scenario = host_var("GHERKIN_SCENARIO").map(|v| v.to_string_lossy().into_owned());
    let chosen: Vec<String> = feature_files()
        .into_iter()
        .filter(|f| f.contains(&only))
        .filter(|f| scenario.as_deref().is_none_or(|id| feature_of(id) == f))
        .collect();
    assert!(!chosen.is_empty(), "no feature matches {only:?}");
    let cluster = CLUSTERS
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no cluster {name}"));
    let features = members(cluster, &chosen);
    if let Some(file) = &ratchet {
        assert!(
            file.is_absolute(),
            "GHERKIN_RATCHET must be absolute: {file:?}"
        );
    }
    let base = base.map(|file| {
        assert!(
            file.is_absolute(),
            "GHERKIN_RATCHET_BASE must be absolute: {file:?}"
        );
        let text = files::read(&file)
            .unwrap_or_else(|e| panic!("GHERKIN_RATCHET_BASE {file:?} does not read: {e}"));
        assert!(
            text.lines().any(|l| l.contains('\t')),
            "GHERKIN_RATCHET_BASE {file:?} lists no scenario (a runner that listed nothing has shown nothing)"
        );
        (file, text)
    });
    if features.is_empty() {
        return;
    }
    let ran = run_features(&features, scenario.as_deref(), cluster_threads());
    if let Some(file) = ratchet {
        put_ratchet(
            &file,
            &run_id(),
            &features,
            &scenario_list(&ran.outcomes),
            &failures_file(&ran.steps_failed),
        );
        if base.is_none() {
            return;
        }
    }
    if let Some((file, text)) = base {
        let new = new_red(&text, &ran.outcomes);
        assert!(
            new.is_empty(),
            "{} new red scenarios against the base list {file:?} (GHERKIN_RATCHET_BASE):\n{}\n{}",
            new.len(),
            new.join("\n"),
            failures_file(
                &ran.steps_failed
                    .iter()
                    .filter(|f| new
                        .iter()
                        .any(|said| said.ends_with(&format!("  {}", f.scenario))))
                    .cloned()
                    .collect::<Vec<_>>()
            )
        );
        return;
    }
    assert!(ran.failures.is_empty(), "{}", ran.failures.join("\n"));
}

/// Each scenario of `outcomes` red here and not red in `base` (a scenario
/// list, `GHERKIN_RATCHET`'s form), sorted, said as scripts/ratchet-judge.sh
/// says it: `broke green  <id>` when the base lists it green, `added red
/// <id>` when it does not list it. A scenario the base lists red passes,
/// whatever its colour here.
fn new_red(base: &str, outcomes: &[(String, bool)]) -> Vec<String> {
    let colour = |id: &str| {
        base.lines()
            .filter_map(|line| line.split_once('\t'))
            .find(|(_, listed)| *listed == id)
            .map(|(word, _)| word.to_string())
    };
    scenario_list(outcomes)
        .lines()
        .filter_map(|line| line.strip_prefix("red\t"))
        .filter_map(|id| match colour(id).as_deref() {
            Some("red") => None,
            Some(_) => Some(format!("  broke green  {id}")),
            None => Some(format!("  added red    {id}")),
        })
        .collect()
}

/// One test per cluster, named by it: `clusters::<name>`, which
/// `.config/nextest.toml` gives its limit by.
mod clusters {
    macro_rules! clusters {
        ($($name:ident),* $(,)?) => {
            $(
                #[test]
                fn $name() {
                    super::run_cluster(stringify!($name));
                }
            )*
            /// The clusters that have a test.
            pub(super) const TESTED: &[&str] = &[$(stringify!($name)),*];
        };
    }

    clusters!(journeys, daemon, sessions, library, remotes, setup, other);
}

/// Every feature file is in exactly one cluster; every cluster has a test of
/// its name and every entry of one names a file, so none is stale.
#[test]
fn every_feature_file_is_in_exactly_one_cluster() {
    let all = feature_files();
    assert!(!all.is_empty());
    for rel in &all {
        let holders: Vec<&str> = CLUSTERS
            .iter()
            .filter(|c| c.files.iter().any(|entry| names(entry, rel)))
            .map(|c| c.name)
            .collect();
        assert_eq!(
            holders.len(),
            1,
            "{rel} must be in exactly one cluster, not {holders:?}"
        );
    }
    for cluster in CLUSTERS {
        for entry in cluster.files {
            assert!(
                all.iter().any(|rel| names(entry, rel)),
                "{}'s {entry} names no feature file",
                cluster.name
            );
        }
    }
    let names: Vec<&str> = CLUSTERS.iter().map(|c| c.name).collect();
    assert_eq!(
        names,
        clusters::TESTED,
        "each cluster has a test of its name"
    );
}

/// A cluster entry names its file, or every file below its directory.
#[test]
fn a_cluster_entry_names_its_file_or_its_directory() {
    assert!(names("cli/run.feature", "cli/run.feature"));
    assert!(!names("cli/run.feature", "cli/run.feature.bak"));
    assert!(!names("cli/run", "cli/run.feature"));
    assert!(names("journeys/", "journeys/j1.feature"));
    assert!(!names("journeys/", "cli/journeys.feature"));
    let cluster = Cluster {
        name: "c",
        files: &["a/", "b.feature"],
    };
    let all = ["a/x.feature", "b.feature", "c.feature", "a/y.feature"].map(String::from);
    assert_eq!(
        members(&cluster, &all),
        ["a/x.feature", "b.feature", "a/y.feature"]
    );
}

/// The clusters of a run assemble one list: each replaces the lines of the
/// features it ran and keeps the others', sorted by id.
#[test]
fn a_clusters_list_replaces_its_features_lines_and_keeps_the_others() {
    let existing = "green\ta.feature :: s\nred\tc.feature :: t\n";
    let ours = "red\tb.feature :: u\ngreen\tc.feature :: v\n";
    let ran = ["b.feature", "c.feature"].map(String::from);
    assert_eq!(
        merged_list(existing, &ran, ours),
        "green\ta.feature :: s\nred\tb.feature :: u\ngreen\tc.feature :: v\n"
    );
    assert_eq!(merged_list("", &ran, ""), "");
    // Sorted by id, not by line: `red` and `green` interleave.
    assert_eq!(
        merged_list("red\ta :: 2\n", &[], "green\ta :: 1\n"),
        "green\ta :: 1\nred\ta :: 2\n"
    );
}

/// The failures files of a run's clusters merge the same way, record by
/// record, each kept whole.
#[test]
fn a_clusters_failures_replace_its_features_records_and_keep_the_others() {
    let mine = Failure {
        scenario: "b.feature :: u".to_string(),
        step: "Then it holds".to_string(),
        message: "said\nmore".to_string(),
    };
    let empty = Failure {
        scenario: "d.feature :: w".to_string(),
        step: "Given x".to_string(),
        message: String::new(),
    };
    let theirs = Failure {
        scenario: "a.feature :: s".to_string(),
        step: "When y".to_string(),
        message: "no".to_string(),
    };
    let stale = Failure {
        scenario: "b.feature :: old".to_string(),
        step: "When z".to_string(),
        message: "gone".to_string(),
    };
    let existing = failures_file(&[theirs.clone(), stale]);
    let ours = failures_file(&[mine.clone(), empty.clone()]);
    let ran = ["b.feature", "d.feature"].map(String::from);
    assert_eq!(
        merged_failures(&existing, &ran, &ours),
        failures_file(&[theirs, mine, empty])
    );
}

/// Two clusters of one run assemble their list in the file, and a later run
/// replaces it whole: what the earlier one left is gone.
#[test]
fn the_ratchet_file_holds_the_runs_clusters_and_nothing_an_earlier_run_left() {
    let project = super::library::Project::bare();
    let file = project.dir().join("list");
    put_ratchet(
        &file,
        "run 1",
        &["a.feature".to_string()],
        "red\ta.feature :: s\n",
        "red\ta.feature :: s\n  step: x\n  y\n\n",
    );
    put_ratchet(
        &file,
        "run 1",
        &["b.feature".to_string()],
        "green\tb.feature :: t\n",
        "",
    );
    assert_eq!(
        files::read(&file).unwrap(),
        "red\ta.feature :: s\ngreen\tb.feature :: t\n"
    );
    assert_eq!(
        files::read(failures_path(&file)).unwrap(),
        "red\ta.feature :: s\n  step: x\n  y\n\n"
    );
    put_ratchet(
        &file,
        "run 2",
        &["b.feature".to_string()],
        "red\tb.feature :: t\n",
        "red\tb.feature :: t\n  step: z\n  w\n\n",
    );
    assert_eq!(files::read(&file).unwrap(), "red\tb.feature :: t\n");
    assert_eq!(
        files::read(failures_path(&file)).unwrap(),
        "red\tb.feature :: t\n  step: z\n  w\n\n"
    );
}

/// The forcing test of the clusters' separation: two clusters run at once
/// (here two threads of one process, which share more than nextest's two
/// processes do), each of two features, which run at once, a scenario each
/// (a feature's scenarios take turns: their steps block). Each scenario
/// starts a daemon through a command and holds it until all four serve at
/// once, asking its own all the while. Each was answered by its own daemon
/// alone, and the four daemons, their homes and their endpoints are four.
#[test]
fn two_clusters_at_once_never_see_each_others_daemon_or_files() {
    let project = super::library::Project::bare();
    let witness = project.dir().join("witness");
    files::mkdirs(&witness);
    let feature = |name: &str| {
        let path = project.dir().join(format!("{name}.feature"));
        files::put(
            &path,
            format!(
                "# world: base\nFeature: {name}\n\n  Scenario: {name}\n    Given an initialized ctxloom project\n    And no ctxloom daemon is serving for this home\n    And the daemon a command starts idles for a minute\n    When I run \"ctxloom session list --format json\"\n    Then the command succeeds\n    And the daemon serving this home is witnessed in \"{}\", until \"4\" are\n",
                witness.display()
            ),
        );
        path.display().to_string()
    };
    let one = [feature("one-a"), feature("one-b")];
    let two = [feature("two-a"), feature("two-b")];
    let (a, b) = std::thread::scope(|scope| {
        let a = scope.spawn(|| run_features(&one, None, 2));
        let b = scope.spawn(|| run_features(&two, None, 2));
        (a.join().expect("a cluster"), b.join().expect("a cluster"))
    });
    quiet_hook_undone();
    for ran in [&a, &b] {
        assert!(ran.failures.is_empty(), "{:?}", ran.steps_failed);
        assert_eq!(ran.outcomes.len(), 2, "{:?}", ran.outcomes);
    }
    let pids: Vec<String> = files::children(&witness)
        .iter()
        .filter(|p| p.extension().is_none())
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(pids.len(), 4, "four daemons: {pids:?}");
    let mut homes = Vec::new();
    let mut endpoints = Vec::new();
    for pid in &pids {
        let text = files::read(witness.join(pid)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let [served, endpoint, home] = lines[..] else {
            panic!("a witness: {text:?}")
        };
        assert_eq!(served, home, "the daemon serves its scenario's own home");
        let seen = files::read(witness.join(format!("{pid}.seen"))).unwrap();
        assert!(
            seen.lines().all(|answered| answered == pid),
            "{pid}'s home was answered by another daemon: {seen}"
        );
        homes.push(home.to_string());
        endpoints.push(endpoint.to_string());
    }
    for (i, home) in homes.iter().enumerate() {
        for (j, other) in homes.iter().enumerate() {
            assert!(
                i == j || !std::path::Path::new(home).starts_with(other),
                "{home} is in {other}"
            );
        }
    }
    endpoints.sort();
    endpoints.dedup();
    assert_eq!(endpoints.len(), 4, "four endpoints: {endpoints:?}");
}

/// A feature whose other scenarios run: its `@pending` scenarios, and a
/// `@pending` Rule's, are left out, though no World has their steps.
#[test]
fn a_pending_scenario_is_left_out_and_its_neighbours_run() {
    let project = super::library::Project::bare();
    let feature = |tag: &str| {
        format!(
            "Feature: f\n  Scenario: runs\n    Given an empty project directory\n\n  {tag}\n  Scenario: left out\n    Given a step no World has\n\n  {tag}\n  Rule: r\n    Scenario: left out too\n      Given a step no World has\n"
        )
    };
    let path = project.dir().join("f.feature");
    files::put(&path, feature("@pending"));
    assert!(
        super::gherkin::run(&path).passed,
        "the pending scenarios ran"
    );
    files::put(&path, feature("@other"));
    assert!(
        !super::gherkin::run(&path).passed,
        "a step no World has passed: the check above proves nothing"
    );
}

/// Each scenario is named by its feature, its Rule and its own name, and an
/// outline's example by the row it ran; a scenario's outcome is its own.
#[test]
fn each_scenario_has_a_stable_id_and_its_own_outcome() {
    let project = super::library::Project::bare();
    let path = project.dir().join("ids.feature");
    files::put(
        &path,
        "Feature: f\n  Scenario: plain\n    Given an empty project directory\n\n  Scenario Outline: outline <what>\n    Given <step>\n\n    Examples:\n      | what | step |\n      | ok | an empty project directory |\n      | bad | a step no World has |\n\n  Rule: r\n    Scenario: ruled\n      Given a step no World has\n",
    );
    let run = super::gherkin::run(&path);
    assert!(!run.passed);
    let feature = path.display().to_string();
    let mut got = run.scenarios;
    got.sort();
    let mut want = vec![
        (format!("{feature} :: plain"), true),
        (format!("{feature} :: r :: ruled"), false),
        (
            format!("{feature} :: outline ok :: | ok | an empty project directory |"),
            true,
        ),
        (
            format!("{feature} :: outline bad :: | bad | a step no World has |"),
            false,
        ),
    ];
    want.sort();
    assert_eq!(got, want);
}

/// An outline's placeholder whose name holds a space (Go's `<result count>`)
/// is filled from its column like any other: in the outline's name, a step's
/// text and its docstring. cucumber 0.21 alone leaves it as written, so the
/// command would run `ctxloom <the verb>` and fail.
#[test]
fn a_placeholder_whose_name_holds_a_space_is_filled_from_its_column() {
    let project = super::library::Project::bare();
    let path = project.dir().join("spaced.feature");
    files::put(
        &path,
        "Feature: f\n  Scenario Outline: runs <the verb>\n    When I run \"ctxloom <the verb>\"\n    Then the command succeeds\n    When Alice runs:\n      \"\"\"\n      ctxloom <the verb>\n      \"\"\"\n    Then the command succeeds\n\n    Examples:\n      | the verb |\n      | version  |\n",
    );
    let run = super::gherkin::run(&path);
    assert_eq!(
        run.scenarios,
        vec![(
            format!("{} :: runs version :: | version |", path.display()),
            true
        )]
    );
    assert!(run.passed);
}

/// Each scenario's outcome, by the part of its id after the feature's path,
/// sorted.
fn outcomes(path: &std::path::Path, run: super::gherkin_common::FeatureRun) -> Vec<(String, bool)> {
    let feature = format!("{} :: ", path.display());
    let mut got: Vec<(String, bool)> = run
        .scenarios
        .into_iter()
        .map(|(id, ok)| (id.strip_prefix(&feature).unwrap_or(&id).to_string(), ok))
        .collect();
    got.sort();
    got
}

/// `the output reports "<path>" containing "<value>"` and `... having "<n>"
/// entries` read the json when it is json, and otherwise the text says the
/// value (as `the output reports "<path>" as "<value>"` does): a text row
/// passes on its words, and either form fails on the wrong ones.
#[test]
fn the_list_steps_read_the_json_or_the_words_of_a_text_row() {
    let project = super::library::Project::bare();
    let path = project.dir().join("lists.feature");
    files::put(
        &path,
        "Feature: f\n  Background:\n    Given an initialized ctxloom project\n    And a bundle \"demo\" exists\n\n  Scenario Outline: containing <case>\n    When I run \"ctxloom search --local --tag example <flags>\"\n    Then the command succeeds\n    And the output reports \"local[resource$=/fragments/example].tags\" containing \"<tag>\"\n\n    Examples:\n      | case     | flags         | tag                 |\n      | json     | --format json | example             |\n      | text     | --format text | - example [example] |\n      | bad json | --format json | nope                |\n      | bad text | --format text | - nope              |\n\n  Scenario Outline: having <case>\n    When I run \"ctxloom search --local --tag example <flags>\"\n    Then the command succeeds\n    And the output reports \"local\" having \"<count>\" entries\n\n    Examples:\n      | case     | flags         | count        |\n      | json     | --format json | 1            |\n      | text     | --format text | Results (1): |\n      | bad json | --format json | 2            |\n      | bad text | --format text | Results (2): |\n",
    );
    let got = outcomes(&path, super::gherkin::run(&path));
    let want = |name: &str, row: &str, ok: bool| (format!("{name} :: | {row} |"), ok);
    let mut expected = vec![
        want("containing json", "json | --format json | example", true),
        want(
            "containing text",
            "text | --format text | - example [example]",
            true,
        ),
        want(
            "containing bad json",
            "bad json | --format json | nope",
            false,
        ),
        want(
            "containing bad text",
            "bad text | --format text | - nope",
            false,
        ),
        want("having json", "json | --format json | 1", true),
        want("having text", "text | --format text | Results (1):", true),
        want("having bad json", "bad json | --format json | 2", false),
        want(
            "having bad text",
            "bad text | --format text | Results (2):",
            false,
        ),
    ];
    expected.sort();
    assert_eq!(got, expected);
}

/// `the output reports "<path>" not containing "<value>"` is `containing`'s
/// negation: on json the list at `path` does not hold `value`, and a text row
/// does not say it; either form fails when the value is there.
#[test]
fn the_not_containing_step_fails_where_containing_passes() {
    let project = super::library::Project::bare();
    let path = project.dir().join("lacks.feature");
    files::put(
        &path,
        "Feature: f\n  Background:\n    Given an initialized ctxloom project\n    And a bundle \"demo\" exists\n\n  Scenario Outline: not containing <case>\n    When I run \"ctxloom search --local --tag example <flags>\"\n    Then the command succeeds\n    And the output reports \"local[resource$=/fragments/example].tags\" not containing \"<tag>\"\n\n    Examples:\n      | case     | flags         | tag                 |\n      | json     | --format json | nope                |\n      | text     | --format text | - nope              |\n      | bad json | --format json | example             |\n      | bad text | --format text | - example [example] |\n",
    );
    let got = outcomes(&path, super::gherkin::run(&path));
    let want = |name: &str, row: &str, ok: bool| (format!("{name} :: | {row} |"), ok);
    let mut expected = vec![
        want("not containing json", "json | --format json | nope", true),
        want("not containing text", "text | --format text | - nope", true),
        want(
            "not containing bad json",
            "bad json | --format json | example",
            false,
        ),
        want(
            "not containing bad text",
            "bad text | --format text | - example [example]",
            false,
        ),
    ];
    expected.sort();
    assert_eq!(got, expected);
}

/// `the skill list output includes "<skill>" from bundle "<bundle>"`: the
/// listing has that skill of that bundle, json (named
/// `.../bundles/<bundle>/skills/<skill>`, OD-40) or text
/// (`- <bundle>/<skill>`), and no other.
#[test]
fn the_skill_list_step_finds_a_skill_of_its_bundle_alone() {
    let project = super::library::Project::bare();
    let path = project.dir().join("skills.feature");
    files::put(
        &path,
        "Feature: f\n  Scenario Outline: listed <case>\n    Given Alice's project has a directory-form bundle \"vault\"\n    And I run \"ctxloom skill create vault reviewer -d d\"\n    When I run \"ctxloom skill list <flags>\"\n    Then the skill list output includes \"<skill>\" from bundle \"<bundle>\"\n\n    Examples:\n      | case         | flags         | skill    | bundle |\n      | json         | --format json | reviewer | vault  |\n      | text         | --format text | reviewer | vault  |\n      | other bundle | --format json | reviewer | other  |\n      | other skill  | --format text | ghost    | vault  |\n",
    );
    let got = outcomes(&path, super::gherkin_surfaces::run(&path));
    let want =
        |case: &str, row: &str, ok: bool| (format!("listed {case} :: | {case} | {row} |"), ok);
    let mut expected = vec![
        want("json", "--format json | reviewer | vault", true),
        want("text", "--format text | reviewer | vault", true),
        want("other bundle", "--format json | reviewer | other", false),
        want("other skill", "--format text | ghost | vault", false),
    ];
    expected.sort();
    assert_eq!(got, expected);
}

/// Each red scenario's failure is kept: its id, the step that failed (its
/// keyword and text) and what it said, so a red ratchet run can be read
/// afterwards (`<list>.failures`); a green scenario has none.
#[test]
fn a_red_scenarios_failing_step_and_its_message_are_kept() {
    let project = super::library::Project::bare();
    let path = project.dir().join("why.feature");
    files::put(
        &path,
        "Feature: f\n  Scenario: green\n    When I run \"ctxloom version\"\n    Then the command succeeds\n\n  Scenario: red\n    When I run \"ctxloom no-such-verb\"\n    Then the command succeeds\n",
    );
    let run = super::gherkin::run(&path);
    assert!(!run.passed);
    let feature = path.display().to_string();
    assert_eq!(run.failures.len(), 1, "{:?}", run.failures);
    let failed = &run.failures[0];
    assert_eq!(failed.scenario, format!("{feature} :: red"));
    assert_eq!(failed.step, "Then the command succeeds");
    assert!(
        failed.message.contains("no-such-verb"),
        "{}",
        failed.message
    );
}

/// The failures file: a record per red scenario, its message capped, in the
/// order of the ids.
#[test]
fn the_failures_file_names_each_red_scenario_its_step_and_its_capped_message() {
    use super::gherkin_common::Failure;
    let long = "x".repeat(5_000);
    let failures = vec![
        Failure {
            scenario: "b :: s".to_string(),
            step: "Then it holds".to_string(),
            message: "line one\nline two".to_string(),
        },
        Failure {
            scenario: "a :: t".to_string(),
            step: "Given a start".to_string(),
            message: long,
        },
    ];
    let text = failures_file(&failures);
    let records: Vec<&str> = text.split("\n\n").filter(|r| !r.is_empty()).collect();
    assert_eq!(records.len(), 2, "{text}");
    assert!(
        records[0].starts_with("red\ta :: t\n  step: Given a start\n  "),
        "{text}"
    );
    assert!(records[0].len() < FAILURE_MESSAGE_MAX + 200, "not capped");
    assert!(
        records[0].contains("[+"),
        "no cap marker: {}",
        &records[0][..80]
    );
    assert_eq!(
        records[1],
        "red\tb :: s\n  step: Then it holds\n  line one\n  line two"
    );
}

/// The ratchet's re-confirmation runs one scenario alone (`GHERKIN_SCENARIO`
/// names its id): only it runs, an outline's row as much as a plain one, and
/// an id no scenario has runs nothing.
#[test]
fn one_scenario_runs_alone_by_its_id() {
    let project = super::library::Project::bare();
    let path = project.dir().join("alone.feature");
    files::put(
        &path,
        "Feature: f\n  Scenario: plain\n    Given an empty project directory\n\n  Scenario: other\n    Given a step no World has\n\n  Scenario Outline: outline <what>\n    Given <step>\n\n    Examples:\n      | what | step |\n      | ok | an empty project directory |\n      | bad | a step no World has |\n",
    );
    let feature = path.display().to_string();
    let plain = format!("{feature} :: plain");
    let run = super::gherkin::run_only(&path, Some(&plain));
    assert_eq!(run.scenarios, vec![(plain, true)]);
    assert!(run.passed);
    let row = format!("{feature} :: outline bad :: | bad | a step no World has |");
    let run = super::gherkin::run_only(&path, Some(&row));
    assert_eq!(run.scenarios, vec![(row, false)]);
    let run = super::gherkin::run_only(&path, Some("nobody :: none"));
    assert!(run.scenarios.is_empty(), "{:?}", run.scenarios);
}

/// `GHERKIN_SCENARIO`'s id names its feature: the part before the first
/// ` :: `.
#[test]
fn a_scenario_ids_feature_is_its_first_part() {
    assert_eq!(
        feature_of("journeys/j.feature :: r :: s :: | a |"),
        "journeys/j.feature"
    );
    assert_eq!(feature_of("cli/x.feature"), "cli/x.feature");
}

/// A feature that cannot run is red outside its scenarios.
#[test]
fn a_feature_that_does_not_parse_is_red_outside_a_scenario() {
    let project = super::library::Project::bare();
    let path = project.dir().join("bad.feature");
    files::put(
        &path,
        "Scenario: s, no Feature\n  Given an empty project directory\n",
    );
    let run = super::gherkin::run(&path);
    assert!(!run.passed);
    assert_eq!(
        run.scenarios,
        vec![(outside_id(&path.display().to_string()), false)]
    );
}

/// The list is sorted, each id once, red where any run of it failed.
#[test]
fn the_scenario_list_is_sorted_and_says_red_or_green() {
    let outcomes = [
        ("b".to_string(), true),
        ("a".to_string(), false),
        ("c".to_string(), true),
        ("c".to_string(), false),
    ];
    assert_eq!(scenario_list(&outcomes), "red\ta\ngreen\tb\nred\tc\n");
}

/// Against a base's list a cluster passes the scenarios the base lists red,
/// whichever colour they are here, and fails each other red one: broke
/// green where the base lists it green, added red where it lists none.
#[test]
fn against_a_base_list_its_red_passes_and_a_new_red_fails() {
    let base = "red\tf.feature :: carried\ngreen\tf.feature :: kept\nred\tf.feature :: fixed\n";
    let tolerated = [
        ("f.feature :: carried".to_string(), false),
        ("f.feature :: kept".to_string(), true),
        ("f.feature :: fixed".to_string(), true),
    ];
    assert!(new_red(base, &tolerated).is_empty());
    let broken = [
        ("f.feature :: carried".to_string(), false),
        ("f.feature :: kept".to_string(), false),
        ("f.feature :: new".to_string(), false),
        ("f.feature :: fixed".to_string(), true),
    ];
    assert_eq!(
        new_red(base, &broken),
        [
            "  broke green  f.feature :: kept",
            "  added red    f.feature :: new"
        ]
    );
}
