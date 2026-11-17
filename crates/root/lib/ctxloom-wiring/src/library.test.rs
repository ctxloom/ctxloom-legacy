//! The library's nouns as the command line mounts them.

use std::sync::Arc;

use ctxloom_cli::Extension;
use process_env::EnvSnapshot;

use super::LibraryCommands;

#[test]
fn the_nouns_are_the_library_clis() {
    let commands = LibraryCommands::new(Arc::new(crate::tests::host_snapshot()));
    let ours: Vec<String> = commands
        .nouns()
        .iter()
        .map(|n| n.get_name().to_string())
        .collect();
    let theirs: Vec<String> = library_cli::nouns()
        .iter()
        .map(|n| n.get_name().to_string())
        .collect();
    assert!(!theirs.is_empty());
    assert_eq!(ours, theirs);
}

// Owner rulings 110, 111 (narrowed by 117): the composed library lists
// ctxloom's built-in bundle in a project; the test-only variable withholds it.
#[test]
fn the_composed_library_has_the_built_in_bundles_unless_the_tests_withhold_them() {
    let mut world = crate::commands::tests::world_without_programs("builtins");
    let names = |env: Arc<EnvSnapshot>| -> Vec<String> {
        let wiring = crate::Wiring::new(env).unwrap();
        let library = super::build(&wiring, None).unwrap();
        library
            .bundle_list(&library_application::Flags::default())
            .result
            .unwrap()
            .rows
            .into_iter()
            .map(|row| row.name)
            .collect()
    };
    let listed = names(world.env());
    assert!(listed.contains(&"ctxloom".to_string()), "{listed:?}");
    assert!(!listed.contains(&"testing".to_string()), "{listed:?}");
    assert!(listed.contains(&"demo".to_string()), "{listed:?}");
    world.set("CTXLOOM_TEST_NO_BUILTINS", "1");
    assert_eq!(names(world.env()), ["demo", "project"]);
}

// Owner ruling 168: a plan is shown to the engines the run drives, so what
// they say of it (opencode's word on a missing model) is said.
#[test]
fn the_plan_is_shown_to_the_engines_the_run_drives() {
    let names: Vec<String> = super::advising_engines()
        .iter()
        .map(|engine| engine.name().to_string())
        .collect();
    assert_eq!(names, ["claude-code", "opencode"]);
}

// Step 10, G2 (QUESTIONS.md B): every library command first makes the
// home's library a git repository, with the allowlist; a home with no
// library yet is passed over.
#[test]
fn the_homes_library_is_made_a_repository_at_its_first_read() {
    let mut world = crate::commands::tests::World::new("home-repository");
    world.set(
        "PATH",
        crate::tests::host_snapshot()
            .var("PATH")
            .unwrap_or_default(),
    );
    let home = world.dir().join("home/.ctxloom");
    super::home_repository(&world.wiring()).expect("nothing to make");
    assert!(!crate::fixtures::exists(home.join(".git")));
    crate::fixtures::mkdirs(&home);
    super::home_repository(&world.wiring()).expect("made");
    assert!(crate::fixtures::exists(home.join(".git/HEAD")));
    assert_eq!(
        crate::fixtures::fs()
            .read_to_string(&home.join(".gitignore"))
            .unwrap(),
        library_application::HOME_GITIGNORE
    );
}

// G11: with no git on the PATH, a library command fails before it reads,
// exit 10.
#[test]
fn without_git_a_library_command_fails_before_it_reads() {
    let world = crate::commands::tests::world_without_programs("no-git");
    crate::fixtures::mkdirs(world.dir().join("home/.ctxloom"));
    let failure = super::home_repository(&world.wiring()).expect_err("no git");
    assert_eq!(failure.exit, cli_support::Exit::FAILED);
    assert_eq!(failure.cause.code(), "library-git-git-missing");
}

// G1, QUESTIONS.md E: `config create`'s git step is init's, over the
// directory the command was run in.
#[test]
fn config_creates_git_step_makes_the_directory_a_repository_once() {
    let mut world = crate::commands::tests::World::new("config-create-git");
    world.set(
        "PATH",
        crate::tests::host_snapshot()
            .var("PATH")
            .unwrap_or_default(),
    );
    let step = super::repository_step(&world.wiring());
    let made = step().expect("made").expect("a notice");
    assert_eq!(made.code(), "init-application-notice-git-initialized");
    assert!(crate::fixtures::exists(world.dir().join("work/.git/HEAD")));
    assert!(step().expect("in git").is_none(), "made once");
}
