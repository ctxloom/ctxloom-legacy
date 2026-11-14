//! A project's places as the daemon's destroy reaches them, under a
//! temporary root (never the real home).

use daemon_api::Places;

use super::ProjectPlaces;
use crate::fixtures;

fn places(root: &file_system::Rooted) -> ProjectPlaces {
    ProjectPlaces {
        fs: fixtures::fs(),
        home: root.root().join("home"),
        temp: root.root().join("tmp"),
    }
}

// The configuration is read from the project's registered root (owner,
// 2026-11-12), whatever the daemon's own directory: its purge age there.
#[test]
fn the_purge_age_is_read_from_the_registered_root() {
    let root = fixtures::root();
    let project = root.root().join("work/calm");
    fixtures::mkdirs(project.join(".ctxloom"));
    fixtures::put_mode(
        project.join(".ctxloom/config.yaml"),
        "schema_version: 8\nsession_purge_age: 45d\n",
        0o644,
    );
    fixtures::mkdirs(root.root().join("home"));
    let mut settings = places(&root).settings(&project.display().to_string(), &[]);
    assert_eq!(settings.purge_age().unwrap(), Some("45d".to_string()));
}

// A session's files are its project's session directory.
#[test]
fn a_sessions_files_are_its_projects() {
    let root = fixtures::root();
    let project = root.root().join("work/calm");
    let dir = project.join(".ctxloom/sessions/amber-swift-owl");
    fixtures::mkdirs(dir.join("native"));
    fixtures::put_mode(dir.join("design-notes.md"), "notes", 0o644);
    let mut files = places(&root).files(&project.display().to_string());
    let rels: Vec<String> = files
        .survey("amber-swift-owl")
        .unwrap()
        .into_iter()
        .map(|item| item.rel)
        .collect();
    assert_eq!(rels, ["design-notes.md"]);
}

// Now is the clock's: after the epoch, and not a fixed instant.
#[test]
fn now_is_the_clocks() {
    let root = fixtures::root();
    assert!(places(&root).now().seconds > 1_700_000_000);
}

/// git in `dir` with a clean identity and none of the caller's environment.
fn git(dir: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", path())
        .env("HOME", dir)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The host's `PATH`, which finds git.
fn path() -> String {
    crate::tests::host_snapshot()
        .utf8("PATH")
        .unwrap_or_default()
        .to_string()
}

// Merge item 1 (main into the daemon line, 2026-11-13): a session's
// checkouts are under the account's ctxloom home, by its project's id
// (owner ruling 2026-11-12, Q1), so the daemon's destroy finds them there: a
// checkout holding uncommitted work is spared, and keeps its session.
#[test]
fn a_sessions_checkout_under_the_account_home_is_found_and_spared() {
    let root = fixtures::root();
    let home = root.root().join("home");
    let project = root.root().join("work/calm");
    fixtures::mkdirs(project.join(".ctxloom/sessions/amber-swift-owl"));
    fixtures::put_mode(
        project.join(".ctxloom/sessions/amber-swift-owl.lock"),
        "",
        0o644,
    );
    fixtures::put_mode(project.join("README.md"), "calm", 0o644);
    git(&project, &["init", "-q"]);
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-q", "-m", "first"]);
    let checkout =
        isolation_ports::session_worktrees(&home, "calm-otter", "amber-swift-owl").join("dev");
    fixtures::mkdirs(checkout.parent().unwrap());
    git(
        &project,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "ctxloom/amber-swift-owl/dev",
            &checkout.display().to_string(),
        ],
    );
    fixtures::put_mode(checkout.join("wip.txt"), "unsaved", 0o644);
    let mut worktrees = places(&root).worktrees(
        &project.display().to_string(),
        "calm-otter",
        &[("PATH".to_string(), path())],
    );
    let report = worktrees.worktrees(Some("amber-swift-owl"), false).unwrap();
    assert_eq!(report.worktrees.len(), 1, "{report:?}");
    assert_eq!(report.spared, 1, "{report:?}");
}

// s3-Q5: the daemon assembles for a session's `context_assemble` tool, so
// the context rides a launch where ctxloom's server is offered: a premise
// index in it names the tool, never the CLI.
#[test]
fn an_assembly_for_a_session_words_its_index_for_the_launch() {
    use daemon_api::Assemblies;
    let root = fixtures::root();
    let project = root.root().join("work/calm");
    let bundle = project.join(".ctxloom/content/bundles/v2/demo");
    fixtures::mkdirs(bundle.join("fragments"));
    fixtures::put_mode(
        bundle.join("bundle.yaml"),
        "schema_version: 4\nversion: 1.0.0\n",
        0o644,
    );
    fixtures::put_mode(
        bundle.join("fragments/later.md"),
        "---\ndescription: about to do the later thing\n---\nLATER-BODY\n",
        0o644,
    );
    fixtures::put_mode(bundle.join("fragments/rules.md"), "RULES-BODY\n", 0o644);
    let profiles = project.join(".ctxloom/content/bundles/v2/project/profiles");
    fixtures::mkdirs(&profiles);
    fixtures::put_mode(
        project.join(".ctxloom/content/bundles/v2/project/bundle.yaml"),
        "schema_version: 4\nversion: 1.0.0\n",
        0o644,
    );
    fixtures::put_mode(profiles.join("dev.yaml"), "bundles: [demo]\n", 0o644);
    fixtures::put_mode(
        project.join(".ctxloom/config.yaml"),
        "schema_version: 9\n",
        0o644,
    );
    fixtures::mkdirs(root.root().join("home"));
    fixtures::mkdirs(root.root().join("tmp"));
    let assemblies = super::ProjectAssemblies {
        home: root.root().join("home"),
        temp: root.root().join("tmp"),
    };
    let assembled = assemblies
        .assemble(
            &project.display().to_string(),
            &daemon_proto::v1::AssembleContextRequest {
                profiles: vec!["dev".to_string()],
                ..daemon_proto::v1::AssembleContextRequest::default()
            },
        )
        .expect("assembled");
    let context = assembled.context;
    assert!(context.contains("RULES-BODY"), "{context}");
    assert!(!context.contains("LATER-BODY"), "held back: {context}");
    assert!(
        context.contains("the ctxloom `context_assemble` tool"),
        "{context}"
    );
    assert!(!context.contains("ctxloom fragment show"), "{context}");
}
