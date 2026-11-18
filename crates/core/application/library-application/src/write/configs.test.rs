//! `config.feature`'s write scenarios at the application port.

use crate::fakes::{Mem, Setup};
use crate::Library;
use std::sync::Arc;

fn world() -> (Library, Arc<Mem>) {
    Setup::default().fixture()
}

fn err<T>(out: crate::Outcome<T>) -> String {
    format!("{:?}", out.result.err().expect("an error"))
}

// config.feature: Create scaffolds the config, the remotes registry and the seeded profile
#[test]
fn test_create_writes_the_four_files_go_writes() {
    let (library, mem) = world();
    let done = library.config_create("").result.unwrap();
    assert_eq!(done.path, "/work/.ctxloom/config.yaml");
    assert_eq!(
        mem.scaffold_paths(),
        [
            ".gitignore",
            "config.yaml",
            "content/bundles/v2/project/bundle.yaml",
            "content/bundles/v2/project/profiles/default.yaml",
            "remotes.yaml"
        ]
    );
    // Hand-test 2026-11-10: the private state is out of git from the start.
    let ignored = mem.scaffold_text(".gitignore").unwrap();
    for pattern in ["/cache", "/sessions", "/project-id", "/state", "*.lock"] {
        assert!(
            ignored.lines().any(|line| line == pattern),
            "{pattern}: {ignored}"
        );
    }
    assert_eq!(
        mem.scaffold_text("config.yaml").unwrap(),
        library::write::scaffold::initial_config("claude-code")
    );
    assert_eq!(
        mem.scaffold_text("remotes.yaml").unwrap(),
        library::write::scaffold::DEFAULT_REMOTES
    );
    assert_eq!(
        mem.scaffold_text("content/bundles/v2/project/bundle.yaml")
            .unwrap(),
        "schema_version: 4\nversion: 1.0.0\n"
    );
    assert_eq!(
        mem.scaffold_text("content/bundles/v2/project/profiles/default.yaml")
            .unwrap(),
        library::write::scaffold::SEED_PROFILE
    );
    assert_eq!(mem.log(), ["scaffold"], "one write");
}

#[test]
fn test_create_records_the_engine_it_is_given() {
    let (library, mem) = world();
    library.config_create("mock").result.unwrap();
    assert_eq!(
        mem.scaffold_text("config.yaml").unwrap(),
        library::write::scaffold::initial_config("mock")
    );
}

#[test]
fn test_create_refuses_an_engine_nobody_knows_and_writes_nothing() {
    let (library, mem) = world();
    let said = err(library.config_create("nosuch"));
    assert!(
        said.contains("ConfigEngine") && said.contains("nosuch"),
        "{said}"
    );
    assert_eq!(mem.writes(), 0);
}

// config.feature: Create refuses an existing config and leaves it byte-for-byte alone
#[test]
fn test_create_refuses_an_existing_config() {
    let (library, mem) = world();
    library.config_create("").result.unwrap();
    let before = mem.scaffold_paths();
    let text = mem.scaffold_text("config.yaml");
    assert!(err(library.config_create("")).contains("ConfigExists"));
    assert_eq!(mem.scaffold_paths(), before);
    assert_eq!(mem.scaffold_text("config.yaml"), text);
    assert_eq!(mem.log(), ["scaffold"]);
}

// operations.scaffoldSeedProfile: the project bundle and its seed profile are kept if there
#[test]
fn test_create_does_not_replace_a_project_bundle_or_seed_profile_that_is_there() {
    let (library, mem) = world();
    mem.set_scaffold_for_test("content/bundles/v2/project/bundle.yaml", "MINE");
    mem.set_scaffold_for_test(
        "content/bundles/v2/project/profiles/default.yaml",
        "MY SEED",
    );
    library.config_create("").result.unwrap();
    assert_eq!(
        mem.scaffold_text("content/bundles/v2/project/bundle.yaml")
            .unwrap(),
        "MINE"
    );
    assert_eq!(
        mem.scaffold_text("content/bundles/v2/project/profiles/default.yaml")
            .unwrap(),
        "MY SEED"
    );
    assert!(mem.scaffold_text("config.yaml").is_some());
}

mod git_step {
    //! Step 10, G1 (QUESTIONS.md E): `config create` makes the directory a
    //! git repository as init does, before it writes anything.

    use std::cell::Cell;
    use std::rc::Rc;

    use angzarr_diagnostic::{Arg, Diagnostic};
    use ctxloom_ports::TreeError;

    use super::{err, world};
    use crate::Notice;

    #[derive(Debug)]
    struct Said(&'static str);

    impl std::fmt::Display for Said {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl Diagnostic for Said {
        fn code(&self) -> &str {
            self.0
        }

        fn args(&self) -> Vec<(&str, Arg)> {
            vec![("said", self.0.into())]
        }
    }

    /// A step answering `made` (or failing, none), counting its calls.
    fn step(made: Option<bool>, calls: Rc<Cell<u32>>) -> crate::RepositoryStep {
        Rc::new(move || {
            calls.set(calls.get() + 1);
            match made {
                Some(true) => Ok(Some(
                    Box::new(Said("made-a-repository")) as run_ports::Advice
                )),
                Some(false) => Ok(None),
                None => Err(TreeError::GitMissing(Box::new(Said("git-missing")))),
            }
        })
    }

    #[test]
    fn a_directory_made_a_repository_is_said_in_the_steps_words_and_then_scaffolded() {
        let (library, mem) = world();
        let calls = Rc::new(Cell::new(0));
        let library = library.with_repository_step(step(Some(true), calls.clone()));
        let created = library.config_create("");
        assert!(created.result.is_ok());
        assert_eq!(calls.get(), 1);
        let [Notice::Git(said)] = created.notices.as_slice() else {
            panic!("{:?}", created.notices);
        };
        let notice: &dyn Diagnostic = &created.notices[0];
        assert_eq!(notice.code(), "made-a-repository");
        assert_eq!(notice.args().len(), 1);
        assert_eq!(said.code(), "made-a-repository");
        assert_eq!(mem.log(), ["scaffold"]);
    }

    #[test]
    fn a_directory_already_in_git_says_nothing() {
        let (library, _) = world();
        let calls = Rc::new(Cell::new(0));
        let created = library
            .with_repository_step(step(Some(false), calls.clone()))
            .config_create("");
        assert!(created.result.is_ok());
        assert!(created.notices.is_empty());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn without_git_nothing_is_written() {
        let (library, mem) = world();
        let calls = Rc::new(Cell::new(0));
        let said = err(library
            .with_repository_step(step(None, calls))
            .config_create(""));
        assert!(said.contains("git-missing"), "{said}");
        assert_eq!(mem.writes(), 0);
    }

    #[test]
    fn a_refused_create_takes_no_git_step() {
        let (library, _) = world();
        let calls = Rc::new(Cell::new(0));
        let library = library.with_repository_step(step(Some(true), calls.clone()));
        assert!(library.config_create("nosuch").result.is_err());
        library.config_create("").result.unwrap();
        assert!(
            library.config_create("").result.is_err(),
            "a config is there"
        );
        assert_eq!(calls.get(), 1, "only the create that wrote");
    }
}

// Step 10b U3: `config edit` served is an editor round trip: the layer's
// text handed to the editor as `config.yaml`, what it leaves written back
// whole (unparsed: a config that does not load is the very thing it is
// for); an editor that leaves it as it was writes nothing; none to edit is
// refused before any editor is asked.
#[test]
fn test_an_edit_round_trips_the_config_and_writes_what_the_editor_left() {
    let broken = "schema_version: 9\nagents: [unclosed\n";
    let (library, mem) = Setup {
        layers: &[broken],
        ..Setup::default()
    }
    .fixture();
    mem.set_editor(|_, text| Ok(format!("{text}# mended\n")));
    let done = library.config_edited().result.unwrap();
    assert!(done.changed);
    assert_eq!(done.path, "/p/.ctxloom/config.yaml");
    assert_eq!(
        mem.edits(),
        [("config.yaml".to_string(), broken.to_string())]
    );
    assert_eq!(
        mem.config(crate::fakes::ConfigScope::Project).unwrap(),
        format!("{broken}# mended\n")
    );
    assert!(mem.opened().is_empty(), "no editor opened in place");
}

#[test]
fn test_an_edit_the_editor_left_as_it_was_writes_nothing() {
    let (library, mem) = Setup {
        layers: &["schema_version: 9\n"],
        ..Setup::default()
    }
    .fixture();
    let done = library.config_edited().result.unwrap();
    assert!(!done.changed);
    assert!(
        !mem.log().iter().any(|l| l.starts_with("write_layer")),
        "{:?}",
        mem.log()
    );
}

#[test]
fn test_an_edit_with_no_config_is_refused_before_any_editor() {
    let (library, mem) = world();
    assert!(err(library.config_edited()).contains("ConfigMissing"));
    assert!(mem.edits().is_empty());
}
