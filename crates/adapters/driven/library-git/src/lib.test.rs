//! The observer over real git in temporary repositories, and over stub gits
//! for what a real one will not do on demand (an old version, a failure).

use std::time::Duration;

use ctxloom_ports::{IgnoreRule, Motion, PathStatus, StatusEntry, TreeError, TreeObserver};

use super::{GitTrees, LibraryGitError, DEFAULT_TIMEOUT};
use crate::fixtures::{env, env_at, exists, fs, mkdirs, put, runs_scripts, slurp, Repo};

/// The observer over the real git, in a clean environment rooted at `repo`'s
/// temporary directory.
fn observer(repo: &Repo) -> GitTrees {
    GitTrees::new(fs(), env_at(&repo.top))
}

/// The observer over a stub "git" whose script is `body`.
fn stubbed(body: &str) -> (file_system::Rooted, GitTrees) {
    let (root, top) = Repo::none();
    let path = top.join("git");
    fixture_program::install_program(&*fs(), &path, body).unwrap();
    let trees = GitTrees::with_program(fs(), path, Duration::from_secs(30), env());
    (root, trees)
}

fn code(error: &TreeError) -> String {
    error.diagnostic().code().to_string()
}

/// A repository with a committed library at `.ctxloom`: its config and one
/// fragment, and a file outside it.
fn library() -> Repo {
    let repo = Repo::at("");
    put(repo.path(".ctxloom/config.yaml"), "schema_version: 9\n");
    put(
        repo.path(".ctxloom/content/bundles/v2/demo/fragments/a.md"),
        "a\n",
    );
    put(repo.path("README.md"), "readme\n");
    repo.commit_all("initial");
    repo
}

const PATHS: [&str; 2] = ["content/bundles/v2", "config.yaml"];

#[test]
fn the_default_limit_is_thirty_seconds() {
    assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(30));
}

// ------------------------------------------------------------- work trees

#[test]
fn a_directory_below_a_repositorys_root_is_in_its_work_tree() {
    let repo = Repo::at("dotfiles");
    mkdirs(repo.path(".ctxloom"));
    let top = observer(&repo).work_tree(&repo.dir(".ctxloom")).unwrap();
    assert_eq!(top.map(std::path::PathBuf::from), Some(repo.repo.clone()));
}

#[test]
fn a_directory_in_no_repository_is_in_no_work_tree() {
    let (_root, top) = Repo::none();
    let trees = GitTrees::new(fs(), env_at(&top));
    assert_eq!(trees.work_tree(&top.display().to_string()).unwrap(), None);
}

#[test]
fn a_work_tree_that_cannot_be_asked_is_a_failure_not_no_repository() {
    if !runs_scripts() {
        return;
    }
    let (root, trees) = stubbed(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.43.0'; exit 0; fi\necho 'fatal: something else' >&2\nexit 128\n",
    );
    let error = trees
        .work_tree(&root.root().display().to_string())
        .unwrap_err();
    assert!(matches!(error, TreeError::Failed(_)), "{error:?}");
    assert!(error.diagnostic().to_string().contains("something else"));
}

// ---------------------------------------------------------------- ignores

#[test]
fn a_rule_that_ignores_the_library_is_named_with_its_file_and_line() {
    let repo = library();
    put(repo.path(".gitignore"), "# mine\n.ctxloom/\n");
    let rule = observer(&repo)
        .check_ignore(&repo.dir(".ctxloom"), "config.yaml")
        .unwrap();
    assert_eq!(
        rule,
        Some(IgnoreRule {
            source: ".gitignore".to_string(),
            line: 2,
            pattern: ".ctxloom/".to_string(),
        }),
        "a tracked file is judged by the rules too"
    );
}

#[test]
fn a_negated_rule_or_none_ignores_nothing() {
    let repo = library();
    let trees = observer(&repo);
    let dir = repo.dir(".ctxloom");
    assert_eq!(trees.check_ignore(&dir, "config.yaml").unwrap(), None);
    put(repo.path(".gitignore"), ".ctxloom/*\n!config.yaml\n");
    assert_eq!(trees.check_ignore(&dir, "config.yaml").unwrap(), None);
}

// ------------------------------------------------------------ repositories

#[test]
fn a_repository_is_made_with_its_ignore_file_and_commits_nothing() {
    let (_root, top) = Repo::none();
    let home = top.join(".ctxloom");
    put(home.join("config.yaml"), "x\n");
    let trees = GitTrees::new(fs(), env_at(&top));
    let dir = home.display().to_string();
    trees
        .init_repository(&dir, Some("/*\n!/config.yaml\n"))
        .unwrap();
    assert!(exists(home.join(".git/HEAD")));
    assert_eq!(slurp(home.join(".gitignore")), "/*\n!/config.yaml\n");
    assert_eq!(trees.head(&dir).unwrap(), None, "nothing committed");
    assert_eq!(
        trees.work_tree(&dir).unwrap().map(std::path::PathBuf::from),
        Some(home)
    );
}

#[test]
fn an_ignore_file_already_there_is_left_as_it_is() {
    // The race of two makers, forced: the other one's file is there first.
    let (_root, top) = Repo::none();
    put(top.join(".gitignore"), "hers\n");
    let trees = GitTrees::new(fs(), env_at(&top));
    let dir = top.display().to_string();
    trees.init_repository(&dir, Some("ours\n")).unwrap();
    assert_eq!(slurp(top.join(".gitignore")), "hers\n");
    // Made again, it is still a repository, and still hers.
    trees.init_repository(&dir, Some("ours\n")).unwrap();
    assert!(exists(top.join(".git/HEAD")));
    assert_eq!(slurp(top.join(".gitignore")), "hers\n");
}

#[test]
fn a_repository_made_without_an_ignore_file_has_none() {
    let (_root, top) = Repo::none();
    let trees = GitTrees::new(fs(), env_at(&top));
    trees
        .init_repository(&top.display().to_string(), None)
        .unwrap();
    assert!(exists(top.join(".git/HEAD")));
    assert!(!exists(top.join(".gitignore")));
}

#[test]
fn an_ignore_file_that_cannot_be_written_fails_and_makes_no_repository() {
    let (_root, top) = Repo::none();
    let trees = GitTrees::new(fs(), env_at(&top));
    let gone = top.join("gone");
    let error = trees
        .init_repository(&gone.display().to_string(), Some("x\n"))
        .unwrap_err();
    assert_eq!(code(&error), "library-git-failed");
    assert!(error.diagnostic().to_string().contains(".gitignore"));
    assert!(!exists(gone.join(".git")));
}

// ----------------------------------------------------------------- commits

#[test]
fn the_head_is_the_commit_checked_out_and_none_before_the_first() {
    let repo = Repo::at("");
    let trees = observer(&repo);
    let dir = repo.dir("");
    assert_eq!(trees.head(&dir).unwrap(), None);
    put(repo.path("a"), "a\n");
    repo.commit_all("one");
    let head = repo.git(&["rev-parse", "HEAD"]);
    assert_eq!(trees.head(&dir).unwrap(), Some(head.trim().to_string()));
}

#[test]
fn the_library_tree_is_the_directorys_tree_in_a_commit() {
    let repo = library();
    let trees = observer(&repo);
    let tree = repo.git(&["rev-parse", "HEAD:.ctxloom"]);
    assert_eq!(
        trees.library_tree(&repo.dir(".ctxloom"), "HEAD").unwrap(),
        Some(tree.trim().to_string())
    );
    mkdirs(repo.path("elsewhere"));
    assert_eq!(
        trees.library_tree(&repo.dir("elsewhere"), "HEAD").unwrap(),
        None,
        "the commit holds nothing there"
    );
}

// ------------------------------------------------------------------ status

#[test]
fn status_sees_each_change_under_the_paths_relative_to_the_directory() {
    let repo = library();
    let trees = observer(&repo);
    let dir = repo.dir(".ctxloom");
    put(repo.path(".ctxloom/config.yaml"), "schema_version: 10\n");
    put(
        repo.path(".ctxloom/content/bundles/v2/new/fragments/b.md"),
        "b\n",
    );
    put(
        repo.path(".ctxloom/sessions/x/notes.md"),
        "not the library\n",
    );
    put(repo.path("README.md"), "changed outside\n");
    let entries = trees.status(&dir, &PATHS).unwrap();
    assert_eq!(
        entries,
        vec![
            StatusEntry {
                path: "config.yaml".to_string(),
                status: PathStatus::Modified,
                from_path: None,
                untracked: false,
            },
            StatusEntry {
                path: "content/bundles/v2/new/fragments/b.md".to_string(),
                status: PathStatus::Added,
                from_path: None,
                untracked: true,
            },
        ]
    );
}

#[test]
fn status_pairs_a_staged_move_and_sees_a_deletion() {
    let repo = library();
    let trees = observer(&repo);
    repo.git(&[
        "mv",
        ".ctxloom/content/bundles/v2/demo/fragments/a.md",
        ".ctxloom/content/bundles/v2/demo/fragments/moved.md",
    ]);
    fs().remove_file(&repo.path(".ctxloom/config.yaml"))
        .unwrap();
    let entries = trees.status(&repo.dir(".ctxloom"), &PATHS).unwrap();
    assert_eq!(
        entries,
        vec![
            StatusEntry {
                path: "config.yaml".to_string(),
                status: PathStatus::Deleted,
                from_path: None,
                untracked: false,
            },
            StatusEntry {
                path: "content/bundles/v2/demo/fragments/moved.md".to_string(),
                status: PathStatus::Renamed,
                from_path: Some("content/bundles/v2/demo/fragments/a.md".to_string()),
                untracked: false,
            },
        ]
    );
}

#[test]
fn status_of_a_clean_tree_is_empty_and_writes_no_index() {
    let repo = library();
    let index = repo.path(".git/index");
    let before = fs().metadata(&index).unwrap();
    let entries = observer(&repo)
        .status(&repo.dir(".ctxloom"), &PATHS)
        .unwrap();
    assert!(entries.is_empty(), "{entries:?}");
    assert!(!exists(repo.path(".git/index.lock")));
    assert_eq!(fs().metadata(&index).unwrap(), before);
}

// --------------------------------------------------------------- diff-tree

#[test]
fn a_diff_between_commits_lists_each_change_renames_paired() {
    let repo = library();
    let first = repo.git(&["rev-parse", "HEAD"]).trim().to_string();
    let before = repo
        .git(&["rev-parse", "HEAD:.ctxloom/config.yaml"])
        .trim()
        .to_string();
    put(repo.path(".ctxloom/config.yaml"), "schema_version: 10\n");
    repo.git(&[
        "mv",
        ".ctxloom/content/bundles/v2/demo/fragments/a.md",
        ".ctxloom/content/bundles/v2/demo/fragments/moved.md",
    ]);
    put(repo.path("README.md"), "outside\n");
    repo.commit_all("second");
    let after = repo
        .git(&["rev-parse", "HEAD:.ctxloom/config.yaml"])
        .trim()
        .to_string();
    let changes = observer(&repo)
        .diff_tree(&repo.dir(".ctxloom"), &first, "HEAD", &PATHS)
        .unwrap();
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert_eq!(changes[0].path, "config.yaml");
    assert_eq!(changes[0].status, PathStatus::Modified);
    assert_eq!(changes[0].from_oid, Some(before));
    assert_eq!(changes[0].to_oid, Some(after));
    assert_eq!(
        changes[1].path,
        "content/bundles/v2/demo/fragments/moved.md"
    );
    assert_eq!(changes[1].status, PathStatus::Renamed);
    assert_eq!(
        changes[1].from_path.as_deref(),
        Some("content/bundles/v2/demo/fragments/a.md")
    );
}

// --------------------------------------------------------------- object ids

#[test]
fn a_files_object_id_is_gits_and_nothing_is_stored() {
    let repo = library();
    put(repo.path(".ctxloom/config.yaml"), "never committed\n");
    let ids = observer(&repo)
        .blob_ids(&repo.dir(".ctxloom"), &["config.yaml"])
        .unwrap();
    let expected = git_hash(&repo, "never committed\n");
    assert_eq!(ids.get("config.yaml"), Some(&expected));
    let stored = std::process::Command::new("git")
        .args(["cat-file", "-e", &expected])
        .current_dir(&repo.repo)
        .status()
        .expect("git runs");
    assert!(!stored.success(), "hash-object wrote the object");
}

#[test]
fn a_path_with_no_file_is_left_out_of_the_ids() {
    // The reconcile asks the ids of paths it last saw dirty, some since
    // taken away: absent is an answer, never git's failure.
    let repo = library();
    put(repo.path(".ctxloom/config.yaml"), "here\n");
    let ids = observer(&repo)
        .blob_ids(&repo.dir(".ctxloom"), &["gone.yaml", "config.yaml"])
        .unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids.get("config.yaml"), Some(&git_hash(&repo, "here\n")));
    assert!(observer(&repo)
        .blob_ids(&repo.dir(".ctxloom"), &["gone.yaml"])
        .unwrap()
        .is_empty());
}

#[test]
fn the_id_of_bytes_not_written_is_the_id_they_would_have_there() {
    let repo = library();
    let trees = observer(&repo);
    let dir = repo.dir(".ctxloom");
    let id = trees
        .blob_id_of(&dir, "content/new.md", b"to be written\n")
        .unwrap();
    assert_eq!(id, git_hash(&repo, "to be written\n"));
    assert!(!crate::fixtures::exists(
        repo.path(".ctxloom/content/new.md")
    ));
}

fn git_hash(repo: &Repo, text: &str) -> String {
    put(repo.top.join("scratch"), text);
    crate::fixtures::git_in(
        &repo.repo,
        &repo.top,
        &[
            "hash-object",
            &repo.top.join("scratch").display().to_string(),
        ],
    )
    .trim()
    .to_string()
}

#[test]
fn no_paths_ask_git_nothing_and_a_line_break_cannot_be_named() {
    let repo = library();
    let trees = observer(&repo);
    let dir = repo.dir(".ctxloom");
    assert!(trees.blob_ids(&dir, &[]).unwrap().is_empty());
    let error = trees.blob_ids(&dir, &["a\nb"]).unwrap_err();
    assert_eq!(code(&error), "library-git-failed");
    assert!(error.diagnostic().to_string().contains("line break"));
}

// ------------------------------------------------------------------ motion

#[test]
fn a_tree_at_rest_is_not_in_motion() {
    let repo = library();
    assert_eq!(
        observer(&repo).in_motion(&repo.dir(".ctxloom")).unwrap(),
        None
    );
}

#[test]
fn each_operation_in_progress_is_told_by_what_git_leaves_in_its_directory() {
    // Each state forced as git leaves it, most telling first.
    for (marks, motion) in [
        (&["index.lock"][..], Motion::IndexLocked),
        (&["REVERT_HEAD", "index.lock"][..], Motion::Reverting),
        (&["CHERRY_PICK_HEAD"][..], Motion::CherryPicking),
        (&["MERGE_HEAD", "CHERRY_PICK_HEAD"][..], Motion::Merging),
        (&["rebase-apply/next", "MERGE_HEAD"][..], Motion::Rebasing),
        (&["rebase-merge/done"][..], Motion::Rebasing),
    ] {
        let repo = library();
        for mark in marks {
            put(repo.path(&format!(".git/{mark}")), "");
        }
        assert_eq!(
            observer(&repo).in_motion(&repo.dir(".ctxloom")).unwrap(),
            Some(motion),
            "{marks:?}"
        );
    }
}

// ------------------------------------------------------------- the floor

#[test]
fn a_git_that_is_not_there_is_missing() {
    let (_root, top) = Repo::none();
    let trees = GitTrees::with_program(fs(), top.join("no-git"), DEFAULT_TIMEOUT, env());
    let error = trees.head(&top.display().to_string()).unwrap_err();
    assert!(matches!(error, TreeError::GitMissing(_)), "{error:?}");
    assert_eq!(code(&error), "library-git-git-missing");
}

#[test]
fn a_git_older_than_the_floor_is_too_old_and_says_its_version() {
    if !runs_scripts() {
        return;
    }
    let (root, trees) = stubbed("#!/bin/sh\necho 'git version 2.24.9 (Apple Git-1)'\n");
    let error = trees
        .work_tree(&root.root().display().to_string())
        .unwrap_err();
    assert!(matches!(error, TreeError::GitTooOld(_)), "{error:?}");
    assert_eq!(
        error.diagnostic().to_string(),
        LibraryGitError::GitTooOld {
            version: "2.24.9".to_string()
        }
        .to_string()
    );
}

#[test]
fn a_git_at_the_floor_or_saying_no_version_is_used() {
    if !runs_scripts() {
        return;
    }
    for said in [
        "git version 2.25.0",
        "git version 3.0.0",
        "a git of its own",
    ] {
        let (root, trees) = stubbed(&format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{said}'; exit 0; fi\npwd -P\n"
        ));
        let top = trees
            .work_tree(&root.root().display().to_string())
            .unwrap_or_else(|e| panic!("{said}: {e:?}"));
        assert!(top.is_some(), "{said}");
    }
}

#[test]
fn the_version_is_asked_once() {
    if !runs_scripts() {
        return;
    }
    let (root, trees) = stubbed(
        "#!/bin/sh\nd=$(dirname \"$0\")\nif [ \"$1\" = --version ]; then echo x >> \"$d/asked\"; echo 'git version 2.43.0'; exit 0; fi\necho deadbeef\n",
    );
    let dir = root.root().display().to_string();
    trees.head(&dir).unwrap();
    trees.head(&dir).unwrap();
    assert_eq!(slurp(root.root().join("asked")), "x\n");
}

#[test]
fn a_version_git_cannot_say_fails_every_call() {
    if !runs_scripts() {
        return;
    }
    let (root, trees) = stubbed("#!/bin/sh\necho broken >&2\nexit 2\n");
    let dir = root.root().display().to_string();
    for _ in 0..2 {
        let error = trees.head(&dir).unwrap_err();
        assert_eq!(code(&error), "library-git-failed");
        assert!(error.diagnostic().to_string().contains("broken"));
    }
}

// ---------------------------------------------------------------- failures

#[test]
fn a_failure_names_the_subcommand_and_its_flags_never_the_paths() {
    if !runs_scripts() {
        return;
    }
    let (root, trees) = stubbed(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.43.0'; exit 0; fi\nfor a; do case \"$a\" in --show-prefix) exit 0;; esac; done\necho 'fatal: bad' >&2\nexit 3\n",
    );
    let dir = root.root().display().to_string();
    let error = trees.status(&dir, &["secret/path"]).unwrap_err();
    assert_eq!(
        error.diagnostic().to_string(),
        LibraryGitError::Failed {
            args: "status --porcelain=v2 -z -uall --renames".to_string(),
            dir: dir.clone(),
            ext_err_txt: "fatal: bad".to_string(),
        }
        .to_string()
    );
}

#[test]
fn a_silent_failure_says_its_status_and_a_hung_git_its_limit() {
    if !runs_scripts() {
        return;
    }
    let (root, _) = stubbed("#!/bin/sh\n");
    let path = root.root().join("git");
    fixture_program::install_program(
        &*fs(),
        &path,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.43.0'; exit 0; fi\nif [ \"$4\" = init ]; then exec sleep 30; fi\nexit 5\n",
    )
    .unwrap();
    let dir = root.root().display().to_string();
    let trees = GitTrees::with_program(fs(), &path, Duration::from_secs(30), env());
    let silent = trees.in_motion(&dir).unwrap_err();
    assert!(silent.diagnostic().to_string().contains('5'), "{silent:?}");
    let hung = GitTrees::with_program(fs(), &path, Duration::from_millis(200), env());
    let error = hung.init_repository(&dir, None).unwrap_err();
    assert!(
        error.diagnostic().to_string().contains("did not finish"),
        "{error:?}"
    );
}

#[test]
fn a_query_that_fails_aloud_is_a_failure_not_an_answer() {
    if !runs_scripts() {
        return;
    }
    let (root, trees) = stubbed(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'git version 2.43.0'; exit 0; fi\necho 'fatal: corrupt' >&2\nexit 1\n",
    );
    let dir = root.root().display().to_string();
    assert_eq!(code(&trees.head(&dir).unwrap_err()), "library-git-failed");
    assert_eq!(
        code(&trees.check_ignore(&dir, "config.yaml").unwrap_err()),
        "library-git-failed"
    );
}
