//! Feature discovery: every `.feature` file under `features/` is run, with the
//! World its `# world:` header line names (`gherkin_common::world_of`). Adding
//! a feature needs no hand-written test; `gherkin_lint.test.rs` has the
//! convention and the parity lint. A `@pending` scenario (Go's, not yet
//! ported: tagged on itself, its Rule or its Feature) is left out of the run;
//! a feature tagged `@pending` as a whole has no World and is not run at all.
//!
//! The features run concurrently, a thread each up to the machine's cores
//! (each cucumber run also runs 4 scenarios at once). `GHERKIN_ONLY=<text>`
//! runs only the features whose path contains it, and `GHERKIN_SCENARIO=<id>`
//! only the scenario of that id (the ratchet's re-confirmation, alone).
//!
//! `GHERKIN_RATCHET=<absolute file>` asks for the scenario list instead of a
//! verdict: every scenario run, one line each, sorted, `red\t<id>` or
//! `green\t<id>` (the id is `gherkin_common::scenario_id`'s, a failure no
//! scenario owns the feature's `outside_id`), is written to the file, and the
//! test passes whatever failed. Beside it, `<file>.failures` says why each
//! red scenario is red: its failed step and what it said (capped), which the
//! console's output, hidden on a pass, no longer does. `just closeout-ratchet` (scripts/
//! closeout-ratchet.sh) judges that list against its base branch's; nothing
//! else sets it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::gherkin_common::{
    feature_files, features_dir, is_pending_feature, outside_id, world_of, FeatureRun,
};
use super::host_var;
use crate::tests::files;

/// Run one feature with its World: whether it passed, and each scenario's
/// outcome. A pending feature is skipped; a pending scenario is left out by
/// the World's run (`gherkin_common::run_feature`).
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
fn failures_file(failures: &[super::gherkin_common::Failure]) -> String {
    let mut sorted: Vec<&super::gherkin_common::Failure> = failures.iter().collect();
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

/// Where the failures file of the list `file` is: beside it.
fn failures_path(file: &std::path::Path) -> std::path::PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".failures");
    std::path::PathBuf::from(name)
}

#[test]
fn every_feature_runs() {
    let only = host_var("GHERKIN_ONLY")
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ratchet = host_var("GHERKIN_RATCHET").map(std::path::PathBuf::from);
    let scenario = host_var("GHERKIN_SCENARIO").map(|v| v.to_string_lossy().into_owned());
    let features: Vec<String> = feature_files()
        .into_iter()
        .filter(|f| f.contains(&only))
        .filter(|f| scenario.as_deref().is_none_or(|id| feature_of(id) == f))
        .collect();
    assert!(!features.is_empty(), "no feature matches {only:?}");
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let outcomes = Mutex::new(Vec::new());
    let steps_failed = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4)
        .min(features.len());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(rel) = features.get(i) else { break };
                let (failed, seen) = match run_one(rel, scenario.as_deref()) {
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
    if let Some(file) = ratchet {
        assert!(
            file.is_absolute(),
            "GHERKIN_RATCHET must be absolute: {file:?}"
        );
        let outcomes = outcomes.into_inner().expect("not poisoned");
        files::put(&file, scenario_list(&outcomes));
        let steps_failed = steps_failed.into_inner().expect("not poisoned");
        files::put(failures_path(&file), failures_file(&steps_failed));
        return;
    }
    let mut failures = failures.into_inner().expect("not poisoned");
    failures.sort();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
