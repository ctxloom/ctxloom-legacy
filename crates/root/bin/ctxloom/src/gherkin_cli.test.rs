//! The steps of the base World's CLI contract features (`features/cli/`)
//! that no journey's module has: `version.feature`'s, and the others the
//! un-pending of step 11 ran (each step names its feature).

use cucumber::then;

use super::World;
use crate::stamp;

/// `ctxloom` with `args`, with the scenario's environment, now.
fn run_now(w: &World, args: &[&str]) -> crate::tests::Ran {
    let env: Vec<(&str, &str)> = w
        .common
        .env
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    w.common.project.run_with(&env, args)
}

/// version.feature: `ctxloom version <flags>` reports the version
/// `ctxloom --version` does (cobra's `ctxloom version <v>` line, the fixed
/// point), as a whole stamp; with no `--format` or `--format json` as the
/// JSON document's `version` (off a terminal the derived default is json),
/// with `--format text` as the one line.
#[then(expr = "the version reported for {string} agrees with {string}")]
fn version_agrees(w: &mut World, flags: String, fixed: String) {
    let fixed_words: Vec<&str> = fixed.split_whitespace().collect();
    assert_eq!(fixed_words.first(), Some(&"ctxloom"), "{fixed}");
    let flag = run_now(w, &fixed_words[1..]);
    assert_eq!(flag.code, 0, "{}", flag.said());
    let version = flag
        .stdout
        .strip_prefix("ctxloom version ")
        .and_then(|rest| rest.strip_suffix('\n'))
        .unwrap_or_else(|| panic!("not cobra's line: {:?}", flag.stdout))
        .to_string();
    assert!(stamp::valid(&version), "not a version stamp: {version:?}");
    let mut args = vec!["version"];
    args.extend(flags.split_whitespace());
    let asked = run_now(w, &args);
    assert_eq!(asked.code, 0, "{}", asked.said());
    let reported = if flags.contains("text") {
        asked
            .stdout
            .strip_suffix('\n')
            .unwrap_or_else(|| panic!("not one line: {:?}", asked.stdout))
            .to_string()
    } else {
        asked.json()["version"]
            .as_str()
            .unwrap_or_else(|| panic!("no version string: {}", asked.stdout))
            .to_string()
    };
    assert_eq!(reported, version, "`ctxloom {}`", args.join(" "));
}
