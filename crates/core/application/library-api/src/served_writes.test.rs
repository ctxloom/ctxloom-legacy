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
