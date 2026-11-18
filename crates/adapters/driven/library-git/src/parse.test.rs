//! git's output read, on the bytes git prints.

use ctxloom_ports::{IgnoreRule, PathStatus, StatusEntry, TreeChange};

use super::{diff_tree, ignore_rule, status, version, version_word};

const OID_A: &str = "78981922613b2afb6025042ff6bd878ac1994e85";
const OID_B: &str = "975fbec8256d3e8a3797e7a3611380f27c49f4ac";
const ZEROS: &str = "0000000000000000000000000000000000000000";

#[test]
fn a_version_is_its_major_and_minor_however_the_build_words_the_rest() {
    assert_eq!(version("git version 2.43.0"), Some((2, 43)));
    assert_eq!(version("git version 2.39.3 (Apple Git-145)"), Some((2, 39)));
    assert_eq!(version("git version 2.42.0.windows.1"), Some((2, 42)));
    assert_eq!(version("git version 3"), None);
    assert_eq!(version("git version x.y"), None);
    assert_eq!(version("not git"), None);
    assert_eq!(version_word("git version 2.39.3 (Apple Git-145)"), "2.39.3");
    assert_eq!(version_word("not git"), "not git");
}

#[test]
fn an_ignore_record_is_the_rule_and_a_negation_none() {
    assert_eq!(
        ignore_rule(b".gitignore\x002\x00.ctxloom/*\x00config.yaml\x00"),
        Some(IgnoreRule {
            source: ".gitignore".to_string(),
            line: 2,
            pattern: ".ctxloom/*".to_string(),
        })
    );
    assert_eq!(
        ignore_rule(b".gitignore\x003\x00!config.yaml\x00config.yaml\x00"),
        None
    );
    assert_eq!(ignore_rule(b""), None);
    assert_eq!(ignore_rule(b"\x00\x00\x00config.yaml\x00"), None);
    assert_eq!(
        ignore_rule(b".gitignore\x00x\x00a\x00config.yaml\x00"),
        None
    );
}

fn entry(path: &str, status: PathStatus) -> StatusEntry {
    StatusEntry {
        path: path.to_string(),
        status,
        from_path: None,
        untracked: false,
    }
}

#[test]
fn each_status_record_is_read_below_the_prefix_and_sorted() {
    let out = format!(
        "1 .M N... 100644 100644 100644 {OID_A} {OID_A} .ctxloom/b c.md\0\
         2 R. N... 100644 100644 100644 {OID_A} {OID_A} R100 .ctxloom/moved.md\0.ctxloom/was.md\0\
         u UU N... 100644 100644 100644 100644 {OID_A} {OID_A} {OID_A} .ctxloom/conflict.md\0\
         ? .ctxloom/new dir/n.md\0\
         1 A. N... 000000 100644 100644 {ZEROS} {OID_A} .ctxloom/added.md\0\
         1 D. N... 100644 000000 000000 {OID_A} {ZEROS} .ctxloom/gone.md\0\
         1 .D N... 100644 100644 000000 {OID_A} {OID_A} .ctxloom/gone2.md\0\
         1 .T N... 100644 100644 120000 {OID_A} {OID_A} .ctxloom/link.md\0\
         1 T. N... 100644 120000 120000 {OID_A} {OID_B} .ctxloom/link2.md\0\
         1 M. N... 100644 100644 100644 {OID_A} {OID_B} outside.md\0\
         ? elsewhere.md\0\
         # branch.oid {OID_A}\0\
         ! .ctxloom/ignored\0"
    );
    let entries = status(out.as_bytes(), ".ctxloom/");
    assert_eq!(
        entries,
        vec![
            entry("added.md", PathStatus::Added),
            entry("b c.md", PathStatus::Modified),
            entry("conflict.md", PathStatus::Unmerged),
            entry("gone.md", PathStatus::Deleted),
            entry("gone2.md", PathStatus::Deleted),
            entry("link.md", PathStatus::TypeChanged),
            entry("link2.md", PathStatus::TypeChanged),
            StatusEntry {
                path: "moved.md".to_string(),
                status: PathStatus::Renamed,
                from_path: Some("was.md".to_string()),
                untracked: false,
            },
            StatusEntry {
                path: "new dir/n.md".to_string(),
                status: PathStatus::Added,
                from_path: None,
                untracked: true,
            },
        ]
    );
}

#[test]
fn at_the_repositorys_root_every_path_is_kept_whole() {
    let out = format!("1 .M N... 100644 100644 100644 {OID_A} {OID_A} config.yaml\0? z\0");
    let entries = status(out.as_bytes(), "");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].path, "config.yaml");
    assert_eq!(entries[1].path, "z");
}

#[test]
fn a_rename_into_the_directory_from_outside_has_no_path_it_was() {
    let out =
        format!("2 R. N... 100644 100644 100644 {OID_A} {OID_A} R100 .ctxloom/in.md\0outside.md\0");
    let entries = status(out.as_bytes(), ".ctxloom/");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].from_path, None);
}

/// A change as its path, status, the path it was and its two ids.
type Summary<'a> = (
    &'a str,
    PathStatus,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
);

#[test]
fn each_diff_record_is_read_with_its_ids_and_sorted() {
    let out = format!(
        ":100644 100644 {OID_A} {OID_B} M\0z.md\0\
         :100644 100644 {OID_A} {OID_A} R100\0was.md\0moved.md\0\
         :000000 100644 {ZEROS} {OID_B} A\0added.md\0\
         :100644 000000 {OID_A} {ZEROS} D\0gone.md\0\
         :100644 120000 {OID_A} {OID_B} T\0link.md\0\
         :100644 100644 {OID_A} {OID_A} C075\0src.md\0copy.md\0\
         :100644 100644 {OID_A} {OID_B} U\0conflict.md\0"
    );
    let changes = diff_tree(out.as_bytes());
    let summary: Vec<Summary<'_>> = changes
        .iter()
        .map(|c: &TreeChange| {
            (
                c.path.as_str(),
                c.status,
                c.from_path.as_deref(),
                c.from_oid.as_deref(),
                c.to_oid.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("added.md", PathStatus::Added, None, None, Some(OID_B)),
            (
                "conflict.md",
                PathStatus::Unmerged,
                None,
                Some(OID_A),
                Some(OID_B)
            ),
            ("copy.md", PathStatus::Added, None, Some(OID_A), Some(OID_A)),
            ("gone.md", PathStatus::Deleted, None, Some(OID_A), None),
            (
                "link.md",
                PathStatus::TypeChanged,
                None,
                Some(OID_A),
                Some(OID_B)
            ),
            (
                "moved.md",
                PathStatus::Renamed,
                Some("was.md"),
                Some(OID_A),
                Some(OID_A)
            ),
            ("z.md", PathStatus::Modified, None, Some(OID_A), Some(OID_B)),
        ]
    );
}

#[test]
fn a_diff_cut_short_or_holding_no_record_reads_what_it_can() {
    assert!(diff_tree(b"").is_empty());
    assert!(diff_tree(b"garbage\0").is_empty());
    assert!(diff_tree(b":100644 100644\0x\0").is_empty());
    let cut = format!(":100644 100644 {OID_A} {OID_B} M\0");
    assert!(diff_tree(cut.as_bytes()).is_empty());
}

#[test]
fn a_renamed_path_that_was_named_like_a_record_is_not_read_as_one() {
    // The path a rename was is its own field: one named `? old` is that
    // path, never an untracked entry of its own.
    let out = format!("2 R. N... 100644 100644 100644 {OID_A} {OID_A} R100 new.md\0? old\0");
    let entries = status(out.as_bytes(), "");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "new.md");
    assert_eq!(entries[0].from_path.as_deref(), Some("? old"));
}

#[test]
fn a_changed_path_named_like_a_record_is_not_read_as_one() {
    // A path is its own field: one spelled as a raw record is that path.
    let named = format!(":100644 100644 {OID_A} {OID_B} M");
    let out = format!(":100644 100644 {OID_A} {OID_B} M\0{named}\0z.md\0");
    let changes = diff_tree(out.as_bytes());
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, named);
}
