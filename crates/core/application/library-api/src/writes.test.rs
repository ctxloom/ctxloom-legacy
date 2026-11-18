//! A planned write shaped into its one step, its command named by git's
//! ids, and what it left read back.

use ctxloom_ports::{Change, Put, Scope};
use library_application::write::recording::Planned;
use library_tree::{DirtyPath, Evidence, LibraryTreeRejection};

use super::{expected, intent_of, left_by, operation, shape, Intended, Lands, Shape};
use crate::fakes::Git;

fn put(path: &str, text: &str) -> Put {
    Put {
        path: path.to_string(),
        bytes: text.as_bytes().to_vec(),
        executable: false,
    }
}

fn none(_: &str, _: &str) -> Vec<String> {
    Vec::new()
}

#[test]
fn test_nothing_planned_is_no_step() {
    assert_eq!(shape(Vec::new(), none), Ok(None));
    let empty = Planned::Changed {
        bundle: "demo".to_string(),
        changes: Vec::new(),
    };
    assert_eq!(shape(vec![empty], none), Ok(None));
}

#[test]
fn test_a_new_bundle_is_one_new_directory_of_its_files() {
    let planned = Planned::NewBundle {
        bundle: "notes".to_string(),
        files: vec![put("bundle.yaml", "v: 1\n")],
    };
    let shaped = shape(vec![planned], none).unwrap().unwrap();
    assert_eq!(shaped.lands, Lands::Content);
    assert_eq!(
        shaped.shape,
        Shape::Create {
            path: "content/bundles/v2/notes".to_string(),
            files: vec![put("bundle.yaml", "v: 1\n")],
        }
    );
}

#[test]
fn test_a_layer_is_its_config_file_replaced_in_its_tree() {
    let planned = Planned::Layer {
        scope: Scope::Home,
        text: "a: 1\n".to_string(),
    };
    let shaped = shape(vec![planned], none).unwrap().unwrap();
    assert_eq!(shaped.lands, Lands::Layer(Scope::Home));
    assert_eq!(shaped.shape.path(), "config.yaml");
}

#[test]
fn test_one_file_written_or_taken_away_is_that_one_step() {
    let write = Planned::Changed {
        bundle: "demo".to_string(),
        changes: vec![Change::Write(put("fragments/rules.md", "be kind"))],
    };
    assert_eq!(
        shape(vec![write], none).unwrap().unwrap().shape,
        Shape::Replace {
            path: "content/bundles/v2/demo/fragments/rules.md".to_string(),
            bytes: b"be kind".to_vec(),
            executable: false,
        }
    );
    let gone = Planned::Changed {
        bundle: "demo".to_string(),
        changes: vec![Change::Remove {
            path: "fragments/rules.md".to_string(),
        }],
    };
    assert_eq!(
        shape(vec![gone], none).unwrap().unwrap().shape,
        Shape::Trash {
            path: "content/bundles/v2/demo/fragments/rules.md".to_string(),
            files: Vec::new(),
        }
    );
}

#[test]
fn test_a_bundle_taken_away_lists_its_files() {
    let gone = Planned::BundleGone {
        bundle: "demo".to_string(),
    };
    let files = |bundle: &str, dir: &str| {
        assert_eq!((bundle, dir), ("demo", ""));
        vec!["bundle.yaml".to_string()]
    };
    assert_eq!(
        shape(vec![gone], files).unwrap().unwrap().shape,
        Shape::Trash {
            path: "content/bundles/v2/demo".to_string(),
            files: vec!["bundle.yaml".to_string()],
        }
    );
}

#[test]
fn test_several_files_into_a_directory_nobody_has_are_one_new_directory() {
    let skill = Planned::Changed {
        bundle: "demo".to_string(),
        changes: vec![
            Change::Write(put("skills/review/SKILL.md", "s")),
            Change::Write(put("skills/review/.meta.yaml", "m")),
        ],
    };
    assert_eq!(
        shape(vec![skill.clone()], none).unwrap().unwrap().shape,
        Shape::Create {
            path: "content/bundles/v2/demo/skills/review".to_string(),
            files: vec![put("SKILL.md", "s"), put(".meta.yaml", "m"),],
        }
    );
    let there = |_: &str, _: &str| vec!["SKILL.md".to_string()];
    assert_eq!(
        shape(vec![skill], there),
        Err(LibraryTreeRejection::OneChangePerCall {
            paths: vec![
                "skills/review/SKILL.md".to_string(),
                "skills/review/.meta.yaml".to_string()
            ]
        })
    );
}

#[test]
fn test_two_files_of_an_existing_bundle_are_one_change_per_call() {
    let two = Planned::Changed {
        bundle: "demo".to_string(),
        changes: vec![
            Change::Write(put("fragments/a.md", "a")),
            Change::Write(put("bundle.yaml", "y")),
        ],
    };
    assert_eq!(
        shape(vec![two], none),
        Err(LibraryTreeRejection::OneChangePerCall {
            paths: vec!["fragments/a.md".to_string(), "bundle.yaml".to_string()]
        })
    );
    let two_writes = vec![
        Planned::Layer {
            scope: Scope::Home,
            text: String::new(),
        },
        Planned::BundleGone {
            bundle: "demo".to_string(),
        },
    ];
    assert!(matches!(
        shape(two_writes, none),
        Err(LibraryTreeRejection::OneChangePerCall { .. })
    ));
}

fn evidence() -> Evidence {
    Evidence {
        tree_reads: true,
        plan_rejection: None,
        builtin: false,
        in_motion: None,
        in_git: true,
        ignored: None,
        root: "/work/demo".to_string(),
    }
}

#[test]
fn test_an_intent_names_the_ids_git_would_give_what_it_writes() {
    let git = Git::at("h-0", "t-0");
    let create = Shape::Create {
        path: "content/bundles/v2/notes".to_string(),
        files: vec![put("bundle.yaml", "v")],
    };
    let intended = intent_of(&git, "/d", "t", "bundle create", &create, 0, evidence()).unwrap();
    let Intended::Create(command) = &intended else {
        panic!("{intended:?}")
    };
    assert_eq!(command.verb, "bundle create");
    assert_eq!(
        command.entries,
        vec![DirtyPath {
            path: "bundle.yaml".to_string(),
            oid: "id-of:content/bundles/v2/notes/bundle.yaml:v".to_string(),
        }]
    );
    assert_eq!(
        expected(&intended, &create),
        vec![DirtyPath {
            path: "content/bundles/v2/notes/bundle.yaml".to_string(),
            oid: "id-of:content/bundles/v2/notes/bundle.yaml:v".to_string(),
        }]
    );
    let replace = Shape::Replace {
        path: "config.yaml".to_string(),
        bytes: b"a".to_vec(),
        executable: false,
    };
    let Intended::Replace(command) =
        intent_of(&git, "/d", "t", "config set", &replace, 4, evidence()).unwrap()
    else {
        panic!()
    };
    assert_eq!(command.seen_sequence, 4);
    assert_eq!(command.new_oid, "id-of:config.yaml:a");
}

#[test]
fn test_a_trash_expects_each_of_its_files_gone_and_what_is_left_is_read_back() {
    let git = Git::at("h-0", "t-0").file("content/bundles/v2/demo/kept.md", "k");
    let trash = Shape::Trash {
        path: "content/bundles/v2/demo".to_string(),
        files: vec!["bundle.yaml".to_string(), "kept.md".to_string()],
    };
    let intended = intent_of(&git, "/d", "t", "bundle remove", &trash, 2, evidence()).unwrap();
    let paths = expected(&intended, &trash);
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().all(|p| p.oid.is_empty()));
    let left = left_by(&git, "/d", &paths).unwrap();
    assert_eq!(left[0].oid, "");
    assert_eq!(left[1].oid, "k", "a file still there is read back");
}

#[test]
fn test_a_writes_book_carries_its_request_id_as_the_command_id() {
    use angzarr_embedded::v1::command_page::Body;
    let tree = library_tree::TreeName::home("brisk-copper-moth").unwrap();
    let replace = Shape::Replace {
        path: "config.yaml".to_string(),
        bytes: b"a".to_vec(),
        executable: false,
    };
    let git = Git::at("h-0", "t-0");
    let intended = intent_of(&git, "/d", "t", "config set", &replace, 0, evidence()).unwrap();
    let id = [7u8; 16];
    let book = super::command_book(&tree, &intended, id);
    let page = book.page.unwrap();
    assert_eq!(
        page.command_id,
        Some(angzarr_embedded::v1::Uuid::from_bytes(id))
    );
    let Some(Body::Command(command)) = page.body else {
        panic!()
    };
    assert!(command.payload.unwrap().type_url.ends_with("ReplaceFile"));
    assert_eq!(book.cover.unwrap().root, Some(tree.root()));
    let fact = library_tree::RecordStepDone {
        request_id: "r".to_string(),
        step: 0,
        observed: Vec::new(),
    };
    let book = super::step_book(&tree, &fact, "done:r:0");
    let Some(Body::Fact(fact)) = book.page.unwrap().body else {
        panic!()
    };
    assert_eq!(fact.origin.unwrap().external_id, "done:r:0");
}

// The step is made by the one atomic operation of its kind (step 10b U8):
// a file replaced, a new directory created, a path trashed, each at the
// step's path below its tree.
#[test]
fn test_each_step_is_made_by_the_atomic_operation_of_its_kind() {
    use ctxloom_ports::Operation;
    let replace = Shape::Replace {
        path: "config.yaml".to_string(),
        bytes: b"a: 1\n".to_vec(),
        executable: false,
    };
    assert_eq!(
        operation(&replace),
        Operation::Replace(put("config.yaml", "a: 1\n"))
    );
    let create = Shape::Create {
        path: "content/bundles/v2/notes".to_string(),
        files: vec![put("bundle.yaml", "v")],
    };
    assert_eq!(
        operation(&create),
        Operation::CreateDir {
            path: "content/bundles/v2/notes".to_string(),
            files: vec![put("bundle.yaml", "v")],
        }
    );
    let trash = Shape::Trash {
        path: "content/bundles/v2/demo".to_string(),
        files: vec!["bundle.yaml".to_string()],
    };
    assert_eq!(
        operation(&trash),
        Operation::Trash {
            path: "content/bundles/v2/demo".to_string()
        }
    );
}

// A file the use case wrote executable stays so, written whole or in a
// new directory.
#[test]
fn test_an_executable_file_stays_executable_in_its_operation() {
    use ctxloom_ports::Operation;
    let run = Put {
        executable: true,
        ..put("skills/review/scripts/run.sh", "#!/bin/sh\n")
    };
    let write = Planned::Changed {
        bundle: "demo".to_string(),
        changes: vec![Change::Write(run.clone())],
    };
    let shaped = shape(vec![write], none).unwrap().unwrap();
    let Operation::Replace(made) = operation(&shaped.shape) else {
        panic!("{shaped:?}")
    };
    assert!(made.executable);
    let new = Planned::Changed {
        bundle: "demo".to_string(),
        changes: vec![
            Change::Write(put("skills/review/SKILL.md", "s")),
            Change::Write(run),
        ],
    };
    let shaped = shape(vec![new], none).unwrap().unwrap();
    let Operation::CreateDir { files, .. } = operation(&shaped.shape) else {
        panic!("{shaped:?}")
    };
    assert_eq!(
        files
            .iter()
            .map(|f| (f.path.as_str(), f.executable))
            .collect::<Vec<_>>(),
        vec![("SKILL.md", false), ("scripts/run.sh", true)]
    );
}
