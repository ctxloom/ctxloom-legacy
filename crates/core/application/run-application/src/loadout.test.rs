//! Step 10c U13: a plan's loadout.

use super::loadout_of_plan;

fn paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

// A plan's loadout is its agent, its profiles and the bundles its
// fragments came from, each once in the order first drawn on.
#[test]
fn a_plans_loadout_names_the_bundles_its_fragments_came_from() {
    let fragments = paths(&[
        "demo#fragments/example",
        "other#fragments/rules",
        "demo#fragments/more",
    ]);
    let got = loadout_of_plan("dev", &paths(&["dev"]), &fragments);
    assert_eq!(got.agent, "dev");
    assert_eq!(got.profiles, ["dev"]);
    assert_eq!(got.bundles, ["demo", "other"]);
}
