use angzarr_diagnostic::{Diagnostic, Report};

use super::{every, rejection, typed};

// E5: every decline of a library use case is a typed refusal of its own,
// and its type alone maps it back to the same code and args, a wrapped
// cause included.
#[test]
fn every_decline_round_trips_by_its_type_to_its_own_diagnostic() {
    let all = every();
    let mut types = std::collections::BTreeSet::new();
    for error in &all {
        let refused = typed(error).unwrap_or_else(|| panic!("{} is typed", error.code()));
        assert!(
            types.insert(refused.refusal.type_url.clone()),
            "one type per cause: {}",
            refused.refusal.type_url
        );
        let back = rejection(&refused.refusal).expect("mapped back by its type");
        let want = Report::of(error);
        assert_eq!(back.code(), want.code(), "{}", refused.refusal.type_url);
        assert_eq!(back.to_string(), want.to_string(), "{}", error.code());
    }
    assert_eq!(types.len(), all.len());
}

#[test]
fn a_cause_that_is_no_decline_is_no_refusal() {
    let failed =
        library_application::AppError::Cause(Box::new(library_application::AppError::NoBundlesDir));
    assert!(typed(&failed).is_none());
    let aborted = library_application::AppError::StartupAborted {
        count: 1,
        findings: String::new(),
    };
    assert!(typed(&aborted).is_none());
}

#[test]
fn a_status_is_the_message_s_own() {
    let refused = typed(&library_application::AppError::AgentNotFound {
        name: "dev".to_string(),
    })
    .unwrap();
    // google.rpc.Code 5, the status the message itself declares.
    assert_eq!(refused.code, 5);
    let refused = typed(&library_application::AppError::SearchNothing).unwrap();
    assert_eq!(refused.code, 3);
}

#[test]
fn another_message_is_none_of_these() {
    let other = prost_types::Any::from_msg(&daemon_proto::v1::NameInvalid::default()).unwrap();
    assert!(rejection(&other).is_none());
}
