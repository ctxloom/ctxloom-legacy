//! The observation's key: ids alone, chained on the last cause.

use sha2::{Digest, Sha256};

use super::*;

const TREE: &str = "projects/amber-quiet-heron/checkouts/brisk-copper-moth/library";

fn dirty(path: &str, oid: &str) -> DirtyPath {
    DirtyPath {
        path: path.to_string(),
        oid: oid.to_string(),
    }
}

#[test]
fn the_key_is_the_tree_the_hashed_cause_the_head_and_the_hashed_dirty_lines() {
    let key = observation_key(TREE, "c-0", "h-1", &[dirty("b", "2"), dirty("a", "1")]);
    let cause = hex::encode(Sha256::digest(b"c-0"));
    let lines = hex::encode(Sha256::digest(b"a\x001\nb\x002\n"));
    assert_eq!(key, format!("obs:{TREE}:{cause}:h-1:{lines}"));
}

#[test]
fn the_dirty_set_is_keyed_in_path_order_whatever_order_it_came_in() {
    assert_eq!(
        observation_key(TREE, "c", "h", &[dirty("b", "2"), dirty("a", "1")]),
        observation_key(TREE, "c", "h", &[dirty("a", "1"), dirty("b", "2")]),
    );
}

#[test]
fn each_part_of_what_was_seen_changes_the_key() {
    let base = observation_key(TREE, "c", "h", &[dirty("a", "1")]);
    for other in [
        observation_key("users/me/library", "c", "h", &[dirty("a", "1")]),
        observation_key(TREE, "c2", "h", &[dirty("a", "1")]),
        observation_key(TREE, "c", "h2", &[dirty("a", "1")]),
        observation_key(TREE, "c", "h", &[dirty("a", "2")]),
        observation_key(TREE, "c", "h", &[dirty("b", "1")]),
        observation_key(TREE, "c", "h", &[]),
    ] {
        assert_ne!(base, other);
    }
}

#[test]
fn a_path_and_its_oid_cannot_be_confused_across_the_separator() {
    assert_ne!(
        observation_key(TREE, "c", "h", &[dirty("a", "b1")]),
        observation_key(TREE, "c", "h", &[dirty("ab", "1")]),
    );
}

#[test]
fn a_to_b_to_a_to_b_never_repeats_a_key_because_each_chains_on_the_last() {
    // Each key becomes the next observation's last cause, as the record
    // folds it: the dirty sets repeat, the keys never do.
    let a: Vec<DirtyPath> = Vec::new();
    let b = vec![dirty("content/bundles/v2/demo/bundle.yaml", "b0b")];
    let mut cause = "baseline".to_string();
    let mut keys = Vec::new();
    for seen in [&b, &a, &b, &a, &b] {
        let key = observation_key(TREE, &cause, "h", seen);
        keys.push(key.clone());
        cause = key;
    }
    let mut unique = keys.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), keys.len(), "{keys:?}");
}
