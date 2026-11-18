//! Each served write's use case and response.

use daemon_proto::v1;
use library_application::fakes::{plain, Setup, CONFIG, ENVELOPE};
use library_application::BundleCreated;

use super::{create_bundle, Wrote};

fn wrote<T>(answer: Option<T>) -> Wrote<T> {
    Wrote {
        answer,
        applied: true,
        etag: "4".to_string(),
        at: "/work/demo/.ctxloom/content/bundles/v2/notes".to_string(),
        parent: "projects/amber-quiet-heron".to_string(),
        notices: Vec::new(),
    }
}

fn create_request() -> v1::CreateBundleRequest {
    v1::CreateBundleRequest {
        bundle_id: "notes".to_string(),
        bundle: Some(v1::Bundle {
            description: "my notes".to_string(),
            ..v1::Bundle::default()
        }),
        ..v1::CreateBundleRequest::default()
    }
}

#[test]
fn test_a_bundle_create_plans_the_use_cases_bundle_with_its_description() {
    let (library, _) = Setup {
        bundles: &[("demo", &[ENVELOPE])],
        layers: &[CONFIG],
        ..Setup::default()
    }
    .fixture();
    let (recording, recorder) = library.recording();
    let made = create_bundle::plan(&recording, &plain(), &create_request(), true)
        .result
        .unwrap();
    assert_eq!(made.name, "notes");
    let planned = recorder.take();
    let library_application::write::recording::Planned::NewBundle { files, .. } = &planned[0]
    else {
        panic!("{planned:?}")
    };
    let envelope = files.iter().find(|f| f.path == "bundle.yaml").unwrap();
    assert!(String::from_utf8_lossy(&envelope.bytes).contains("my notes"));
}

#[test]
fn test_a_bundle_create_answers_its_bundle_path_and_etag_done_or_retried() {
    let made = BundleCreated {
        status: "created".to_string(),
        name: "notes".to_string(),
        path: "/made/bundle.yaml".to_string(),
    };
    let answered = create_bundle::respond(&create_request(), wrote(Some(made)));
    assert_eq!(answered.path, "/made/bundle.yaml");
    assert_eq!(answered.applied, Some(true));
    assert_eq!(answered.etag, "4");
    assert_eq!(
        answered.bundle.unwrap().name,
        "projects/amber-quiet-heron/bundles/notes"
    );
    let retried = create_bundle::respond(&create_request(), wrote(None));
    assert_eq!(
        retried.path,
        "/work/demo/.ctxloom/content/bundles/v2/notes/bundle.yaml"
    );
}

fn demo() -> library_application::Library {
    Setup {
        bundles: &[("demo", &[ENVELOPE, ("fragments/example.md", "EXAMPLE\n")])],
        layers: &[CONFIG],
        ..Setup::default()
    }
    .fixture()
    .0
}

fn launching() -> Option<v1::Presentable> {
    Some(v1::Presentable {
        can_ask: false,
        can_launch: true,
    })
}

#[test]
fn test_a_fragment_create_with_a_body_stores_it_and_asks_nothing() {
    let (library, recorder) = demo().recording();
    let request = v1::CreateFragmentRequest {
        parent: "projects/amber-quiet-heron/bundles/demo".to_string(),
        fragment_id: "rules".to_string(),
        fragment: Some(v1::Fragment {
            body: "be kind".to_string(),
            ..v1::Fragment::default()
        }),
        presentable: launching(),
        ..v1::CreateFragmentRequest::default()
    };
    let asked = super::create_fragment::plan(&library, &plain(), &request, true)
        .result
        .unwrap();
    assert!(asked.needs.is_empty());
    let planned = recorder.take();
    let library_application::write::recording::Planned::Changed { changes, .. } = &planned[0]
    else {
        panic!("{planned:?}")
    };
    let ctxloom_ports::Change::Write(put) = &changes[0] else {
        panic!()
    };
    assert_eq!(put.path, "fragments/rules.md");
    assert_eq!(put.bytes, b"be kind");
}

#[test]
fn test_a_fragment_edit_with_no_body_asks_its_edit_need_and_applies_the_answer() {
    let name = "projects/amber-quiet-heron/bundles/demo/fragments/example";
    let preview = v1::UpdateFragmentRequest {
        fragment: Some(v1::Fragment {
            name: name.to_string(),
            ..v1::Fragment::default()
        }),
        presentable: launching(),
        ..v1::UpdateFragmentRequest::default()
    };
    let (library, recorder) = demo().recording();
    let asked = super::update_fragment::plan(&library, &plain(), &preview, false)
        .result
        .unwrap();
    assert!(recorder.take().is_empty(), "the preview writes nothing");
    let [need] = &asked.needs[..] else {
        panic!("{:?}", asked.needs)
    };
    assert_eq!(need.id, super::EDIT_NEED);
    let Some(v1::need::Kind::Edit(edit)) = &need.kind else {
        panic!()
    };
    assert_eq!(edit.file_name, "example.md");
    assert_eq!(edit.text, "EXAMPLE\n");
    let launch = edit.launch.as_ref().expect("the editor's launch");
    assert_eq!(launch.kind, v1::launch::Kind::Editor as i32);
    let answered = v1::UpdateFragmentRequest {
        answers: vec![v1::Answer {
            need_id: super::EDIT_NEED.to_string(),
            answer: Some(v1::answer::Answer::Edited("EDITED\n".to_string())),
        }],
        ..preview
    };
    let (library, recorder) = demo().recording();
    let asked = super::update_fragment::plan(&library, &plain(), &answered, true)
        .result
        .unwrap();
    assert!(asked.needs.is_empty());
    assert_eq!(recorder.take().len(), 1);
    let response = super::update_fragment::respond(&answered, wrote(Some(asked)));
    assert_eq!(response.applied, Some(true));
    assert_eq!(response.fragment.unwrap().name, name);
}

#[test]
fn test_an_edit_refreshed_says_its_distilled_form_fresh_and_one_unchanged_applies_nothing() {
    let edited = |status: &str, distilled: bool| super::Asked {
        done: library_application::ItemEdited {
            status: status.to_string(),
            bundle: "demo".to_string(),
            name: "one".to_string(),
            path: "x/one.md".to_string(),
            distilled,
            kind: "command".to_string(),
            reference: String::new(),
            distill_skipped: false,
        },
        needs: Vec::new(),
    };
    let request = v1::UpdateCommandRequest::default();
    let fresh = super::update_command::respond(&request, wrote(Some(edited("updated", true))));
    assert_eq!(
        fresh.command.unwrap().distilled,
        v1::DistilledState::Fresh as i32
    );
    assert_eq!(fresh.applied, Some(true));
    let same = super::update_command::respond(&request, wrote(Some(edited("unchanged", false))));
    assert_eq!(same.applied, Some(false));
    assert_eq!(same.command.unwrap().distilled, 0);
}

#[test]
fn test_a_created_skill_answers_its_directory_done_or_retried() {
    let made = library_application::SkillCreated {
        status: "created".to_string(),
        bundle: "demo".to_string(),
        name: "reviewer".to_string(),
        dir: "skills/reviewer".to_string(),
    };
    let request = v1::CreateSkillRequest {
        parent: "projects/amber-quiet-heron/bundles/demo".to_string(),
        skill_id: "reviewer".to_string(),
        ..v1::CreateSkillRequest::default()
    };
    let answered = super::create_skill::respond(&request, wrote(Some(made)));
    let skill = answered.skill.unwrap();
    assert_eq!(
        skill.name,
        "projects/amber-quiet-heron/bundles/demo/skills/reviewer"
    );
    assert_eq!(skill.path, "skills/reviewer");
    assert_eq!(answered.path, "skills/reviewer");
    let retried = super::create_skill::respond(&request, wrote(None));
    assert_eq!(
        retried.skill.unwrap().name,
        "projects/amber-quiet-heron/bundles/demo/skills/reviewer"
    );
    assert_eq!(retried.path, "/work/demo/.ctxloom/content/bundles/v2/notes");
}

// Step 10b U3: a removal's preview asks its Confirm need, the target in
// the use case's own words; a removal done, or retried once done, is
// applied and asks nothing.
#[test]
fn test_a_removals_preview_asks_its_confirm_need_and_a_removal_done_asks_none() {
    use library_application::{Preview, Removal};
    let preview: Removal<()> = Removal::Preview(Preview::of(
        "bundle \"notes\"".to_string(),
        vec!["1 fragment(s)".to_string()],
        "ctxloom bundle remove notes --yes".to_string(),
    ));
    let removed = super::removed_of(Some(&preview), false);
    assert!(!removed.applied);
    assert_eq!(removed.detail, ["1 fragment(s)"]);
    assert_eq!(removed.apply_command, "ctxloom bundle remove notes --yes");
    let [need] = removed.needs.as_slice() else {
        panic!("{:?}", removed.needs)
    };
    assert_eq!(need.id, super::CONFIRM_NEED);
    let Some(v1::need::Kind::Confirm(confirm)) = &need.kind else {
        panic!("{need:?}")
    };
    assert_eq!(confirm.text, "Remove bundle \"notes\"?");
    assert_eq!(confirm.args[0].name, "target");
    assert_eq!(confirm.args[0].value, "bundle \"notes\"");
    let done = super::removed_of(Some(&Removal::Done(())), false);
    assert!(done.applied && done.needs.is_empty());
    assert!(super::removed_of::<()>(None, true).applied);
    assert!(!super::removed_of::<()>(None, false).applied);
}

// A bundle removal plans its bundle's whole tree gone, the bundle named by
// its resource name, and previews it without planning anything gone.
#[test]
fn test_a_bundle_removal_plans_its_bundle_gone_and_its_preview_nothing() {
    use super::delete_bundle;
    use library_application::write::recording::Planned;
    let request = v1::DeleteBundleRequest {
        name: "projects/amber-quiet-heron/bundles/demo".to_string(),
        ..v1::DeleteBundleRequest::default()
    };
    let (recording, recorder) = demo().recording();
    let previewed = delete_bundle::plan(&recording, &plain(), &request, false)
        .result
        .unwrap();
    assert!(matches!(
        previewed,
        library_application::Removal::Preview(_)
    ));
    assert!(recorder.take().is_empty());
    let (recording, recorder) = demo().recording();
    delete_bundle::plan(&recording, &plain(), &request, true)
        .result
        .unwrap();
    assert_eq!(
        recorder.take(),
        [Planned::BundleGone {
            bundle: "demo".to_string()
        }]
    );
    let answered = delete_bundle::respond(&request, wrote(None));
    assert_eq!(answered.applied, Some(true));
    assert_eq!(
        answered.path,
        "/work/demo/.ctxloom/content/bundles/v2/notes"
    );
    assert_eq!(
        answered.bundle.unwrap().name,
        "projects/amber-quiet-heron/bundles/demo"
    );
}

// A profile removal answers the profile's canonical name, the project
// bundle's, whatever form it was asked by.
#[test]
fn test_a_profile_removal_answers_the_profiles_canonical_name() {
    let request = v1::DeleteProfileRequest {
        name: "projects/amber-quiet-heron/profiles/dev".to_string(),
        ..v1::DeleteProfileRequest::default()
    };
    let answered = super::delete_profile::respond(&request, wrote(None));
    assert_eq!(
        answered.profile.unwrap().name,
        "projects/amber-quiet-heron/bundles/project/profiles/dev"
    );
    assert_eq!(answered.applied, Some(true));
}

// Step 10b U3 (G4 superseded): an edit's preview with no text to give
// still plans the one file its apply writes, so the daemon answers that
// path's etag for the apply to send back (stale once the file moved);
// it asks its Edit need all the same, and a preview performs nothing.
#[test]
fn test_an_edits_preview_plans_its_file_for_its_etag_and_still_asks_the_edit_need() {
    let preview = v1::UpdateFragmentRequest {
        fragment: Some(v1::Fragment {
            name: "projects/amber-quiet-heron/bundles/demo/fragments/example".to_string(),
            ..v1::Fragment::default()
        }),
        presentable: launching(),
        validate_only: true,
        ..v1::UpdateFragmentRequest::default()
    };
    let (library, recorder) = demo().recording();
    let asked = super::update_fragment::plan(&library, &plain(), &preview, true)
        .result
        .unwrap();
    assert_eq!(asked.needs.len(), 1, "{:?}", asked.needs);
    let planned = recorder.take();
    let [library_application::write::recording::Planned::Changed { changes, .. }] = &planned[..]
    else {
        panic!("{planned:?}")
    };
    let [ctxloom_ports::Change::Write(put)] = &changes[..] else {
        panic!("{changes:?}")
    };
    assert_eq!(put.path, "fragments/example.md");
}

fn bundle_edit(
    mask: &[&str],
    edit: impl FnOnce(&mut v1::UpdateBundleRequest),
) -> v1::UpdateBundleRequest {
    let mut request = v1::UpdateBundleRequest {
        bundle: Some(v1::Bundle {
            name: "projects/amber-quiet-heron/bundles/demo".to_string(),
            ..v1::Bundle::default()
        }),
        update_mask: Some(daemon_proto::google::protobuf::FieldMask {
            paths: mask.iter().map(|p| p.to_string()).collect(),
        }),
        ..v1::UpdateBundleRequest::default()
    };
    edit(&mut request);
    request
}

/// The one change a use case planned of a bundle, by its path and text.
fn planned_writes(
    planned: &[library_application::write::recording::Planned],
) -> Vec<(String, String)> {
    planned
        .iter()
        .flat_map(|planned| match planned {
            library_application::write::recording::Planned::Changed { changes, .. } => changes
                .iter()
                .map(|change| match change {
                    ctxloom_ports::Change::Write(put) => (
                        put.path.clone(),
                        String::from_utf8_lossy(&put.bytes).into_owned(),
                    ),
                    other => (format!("{other:?}"), String::new()),
                })
                .collect::<Vec<_>>(),
            other => vec![(format!("{other:?}"), String::new())],
        })
        .collect()
}

#[test]
fn test_a_bundle_edit_writes_the_fields_its_mask_names_and_its_tags_into_the_one_manifest() {
    let (library, recorder) = demo().recording();
    let request = bundle_edit(&["description", "tags"], |r| {
        r.bundle.as_mut().unwrap().description = "updated-desc".to_string();
        r.bundle.as_mut().unwrap().version = "9.9.9".to_string();
        r.add_tags = vec!["golang".to_string()];
    });
    let edited = super::update_bundle::plan(&library, &plain(), &request, true)
        .result
        .unwrap();
    assert_eq!(edited.name, "demo");
    assert_eq!(edited.changes.len(), 2, "{:?}", edited.changes);
    let writes = planned_writes(&recorder.take());
    assert_eq!(writes.len(), 1, "{writes:?}");
    let (path, text) = &writes[0];
    assert_eq!(path, "bundle.yaml");
    assert!(text.contains("updated-desc"), "{text}");
    assert!(text.contains("golang"), "{text}");
    // `version` was given but not named by the mask: not written.
    assert!(!text.contains("9.9.9"), "{text}");
}

#[test]
fn test_a_bundle_edit_adds_and_removes_each_kind_of_item_by_its_list() {
    let (library, recorder) = demo().recording();
    let request = bundle_edit(&["fragments", "commands", "mcp_servers"], |r| {
        r.add_fragments = vec!["a".to_string()];
        r.remove_fragments = vec!["example".to_string()];
        r.add_commands = vec!["b".to_string()];
        r.add_mcp_servers = vec!["tools".to_string()];
    });
    let edited = super::update_bundle::plan(&library, &plain(), &request, true)
        .result
        .unwrap();
    assert_eq!(
        edited.changes,
        [
            "set fragment: a",
            "removed fragment: example",
            "set prompt: b",
            "set mcp: tools"
        ]
    );
    let changes: Vec<String> = match &recorder.take()[0] {
        library_application::write::recording::Planned::Changed { changes, .. } => changes
            .iter()
            .map(|change| match change {
                ctxloom_ports::Change::Write(put) => format!("write {}", put.path),
                ctxloom_ports::Change::Remove { path } => format!("remove {path}"),
                ctxloom_ports::Change::RemoveDir { path } => format!("remove dir {path}"),
            })
            .collect(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        changes,
        [
            "write fragments/a.md",
            "remove fragments/example.md",
            "write prompts/b.md",
            "write mcp/tools.yaml"
        ]
    );
}

#[test]
fn test_a_bundle_edit_with_no_mask_path_given_its_field_changes_nothing() {
    let (library, recorder) = demo().recording();
    let request = bundle_edit(&[], |r| {
        r.bundle.as_mut().unwrap().description = "updated-desc".to_string();
    });
    let edited = super::update_bundle::plan(&library, &plain(), &request, true)
        .result
        .unwrap();
    assert!(edited.changes.is_empty(), "{:?}", edited.changes);
    assert!(recorder.take().is_empty());
}

#[test]
fn test_a_bundle_edit_answers_its_changes_and_whether_it_applied() {
    let request = bundle_edit(&["description"], |_| {});
    let edited = |changes: &[&str]| library_application::BundleEdited {
        status: "updated".to_string(),
        name: "demo".to_string(),
        changes: changes.iter().map(|c| c.to_string()).collect(),
        path: "/work/demo/.ctxloom/content/bundles/v2/demo/bundle.yaml".to_string(),
    };
    let answered = super::update_bundle::respond(
        &request,
        wrote(Some(edited(&["description: updated-desc"]))),
    );
    assert_eq!(answered.applied, Some(true));
    assert_eq!(answered.changes, ["description: updated-desc"]);
    assert_eq!(answered.etag, "4");
    assert_eq!(
        answered.path,
        "/work/demo/.ctxloom/content/bundles/v2/demo/bundle.yaml"
    );
    assert_eq!(
        answered.bundle.unwrap().name,
        "projects/amber-quiet-heron/bundles/demo"
    );
    // Nothing to change: nothing applied, and no change listed.
    let unchanged = Wrote {
        applied: false,
        ..wrote(Some(edited(&[])))
    };
    let answered = super::update_bundle::respond(&request, unchanged);
    assert_eq!(answered.applied, Some(false));
    assert!(answered.changes.is_empty());
    // The retry of an edit already applied.
    let retried = super::update_bundle::respond(&request, wrote(None));
    assert_eq!(retried.applied, Some(true));
    assert_eq!(retried.path, "/work/demo/.ctxloom/content/bundles/v2/notes");
}

// Owner ruling 2026-11-18: several mask paths of the one manifest are one
// step; a manifest field beside an item is two files, refused before any
// command, naming both.
#[test]
fn test_a_bundle_edit_of_the_manifest_alone_is_one_step_and_beside_an_item_is_refused() {
    let shaped = |request: &v1::UpdateBundleRequest| {
        let library = demo();
        let (recording, recorder) = library.clone().recording();
        super::update_bundle::plan(&recording, &plain(), request, true)
            .result
            .unwrap();
        crate::writes::shape(recorder.take(), |bundle, dir| {
            library.bundle_files(bundle, dir)
        })
    };
    let manifest = bundle_edit(&["description", "version", "tags"], |r| {
        r.bundle.as_mut().unwrap().description = "updated-desc".to_string();
        r.bundle.as_mut().unwrap().version = "2.0.0".to_string();
        r.add_tags = vec!["golang".to_string()];
    });
    let one = shaped(&manifest).unwrap().unwrap();
    assert_eq!(one.shape.path(), "content/bundles/v2/demo/bundle.yaml");
    let two = bundle_edit(&["version", "fragments"], |r| {
        r.bundle.as_mut().unwrap().version = "2.0.0".to_string();
        r.add_fragments = vec!["testing".to_string()];
    });
    match shaped(&two) {
        Err(library_tree::LibraryTreeRejection::OneChangePerCall { paths }) => {
            assert_eq!(paths, ["bundle.yaml", "fragments/testing.md"]);
        }
        other => panic!("{other:?}"),
    }
}
