//! Step 10c U13: the pure, keyed reaction to a library change.

use daemon_proto::v1;

use super::{loadout_of_assembled, owed, touched};

fn loadout(bundles: &[&str]) -> sessions::SessionLoadout {
    sessions::SessionLoadout {
        agent: String::new(),
        profiles: vec!["dev".to_string()],
        bundles: bundles.iter().map(|b| b.to_string()).collect(),
    }
}

fn paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

// An assembly's loadout is the same, its fragments named as resources
// (`.../bundles/<b>/fragments/<f>`), those a premise held back included.
#[test]
fn an_assemblys_loadout_names_its_fragments_bundles_held_back_ones_too() {
    let assembled = v1::AssembledContext {
        fragments: vec![v1::AssembledFragment {
            fragment: "projects/p/bundles/demo/fragments/example".to_string(),
            ..v1::AssembledFragment::default()
        }],
        premised: vec![v1::PremiseEntry {
            fragment: "projects/p/bundles/held/fragments/later".to_string(),
            ..v1::PremiseEntry::default()
        }],
        ..v1::AssembledContext::default()
    };
    let got = loadout_of_assembled("", &paths(&["dev"]), &assembled);
    assert_eq!(got.bundles, ["demo", "held"]);
    assert_eq!(got.profiles, ["dev"]);
}

// A change touches a loadout where one of its paths is in a bundle the
// loadout names, or is the tree's config (in every loadout): named
// sorted, each once; a change of another bundle touches nothing.
#[test]
fn a_change_touches_the_bundles_a_loadout_names_and_the_config() {
    let dev = loadout(&["demo"]);
    let demo = "content/bundles/v2/demo/fragments/example.md";
    assert_eq!(touched(&dev, &paths(&[demo])), ["demo"]);
    assert_eq!(
        touched(&dev, &paths(&["config.yaml", demo, demo])),
        ["config", "demo"]
    );
    assert!(touched(&dev, &paths(&["content/bundles/v2/other/bundle.yaml"])).is_empty());
    assert!(
        touched(&dev, &paths(&["content/bundles/v2/demox/bundle.yaml"])).is_empty(),
        "a bundle named alike is another"
    );
    assert!(touched(&dev, &paths(&["remotes.yaml"])).is_empty());
}

fn change(tree: &str, at: i64, cause: &str, paths: &[&str]) -> library_checkout::Recorded {
    library_checkout::Recorded {
        tree: tree.to_string(),
        cause: cause.to_string(),
        changes: paths
            .iter()
            .map(|path| library_checkout::Changed {
                path: path.to_string(),
                status: "modified".to_string(),
                from_path: String::new(),
            })
            .collect(),
        recorded_at: (at, 0),
        ..library_checkout::Recorded::default()
    }
}

// A session is owed a re-assembly by every change of its trees recorded
// since its context was last handed to it that touches its loadout: what
// they touched, and the latest one's cause, which a delivery answers. A
// change from before, of another tree, or of nothing it names, is none of
// its; nothing such, nothing owed (an owed one stays owed until a
// re-assembly is recorded, which moves `since`).
#[test]
fn a_session_is_owed_the_changes_since_its_context_that_touch_its_loadout() {
    let dev = loadout(&["demo"]);
    let mine = "projects/p/checkouts/c/library";
    let trees = paths(&[mine, "users/me/library"]);
    let demo = "content/bundles/v2/demo/fragments/example.md";
    let log = [
        change(mine, 9, "before", &[demo]),
        change(mine, 11, "c1", &[demo]),
        change(
            "projects/q/checkouts/d/library",
            12,
            "theirs",
            &["config.yaml"],
        ),
        change(mine, 13, "other", &["content/bundles/v2/other/bundle.yaml"]),
        change("users/me/library", 14, "c2", &["config.yaml"]),
    ];
    let owing = owed(&dev, (10, 0), &trees, &log).expect("owed");
    assert_eq!(owing.changed, ["config", "demo"]);
    assert_eq!(owing.cause, "c2");
    let owing = owed(&dev, (11, 0), &trees, &log).expect("owed");
    assert_eq!(owing.changed, ["config"], "c1 is not after its context");
    assert!(owed(&dev, (14, 0), &trees, &log).is_none(), "handed since");
    assert!(owed(&dev, (10, 0), &paths(&[mine]), &log[2..4]).is_none());
    let nanos = [change(mine, 10, "same-second", &[demo])];
    assert!(
        owed(&dev, (10, 5), &trees, &nanos).is_none(),
        "by nanos too"
    );
}
