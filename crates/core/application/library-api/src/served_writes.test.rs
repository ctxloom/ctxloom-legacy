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
