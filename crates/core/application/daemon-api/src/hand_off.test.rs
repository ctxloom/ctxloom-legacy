//! `CreateHandOff` and `DeleteHandOff` (slice 4 U3, T6, S10) over the real
//! aggregates and the warm index: the owner and the credential are checked
//! BEFORE anything is written, so a refused launch leaves nothing.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use daemon_proto::v1;

use crate::server::tests::{fixture, refused, resolve, Fixture, COMMIT};
use crate::{HandOffContent, HandOffs, HandingOff, Planned};

const PARENT: &str = "projects/calm-quiet-heron";
const CHILD: &str = "5f0c7a52-1111-5111-8111-000000000002";

/// The hand-offs a test scripts: a plan whose credential is or is not set,
/// and what was written and deleted.
#[derive(Default)]
struct Kept {
    planned: Mutex<Vec<HandingOff>>,
    written: Mutex<BTreeMap<String, HandOffContent>>,
    deleted: Mutex<Vec<String>>,
    credential_set: bool,
    unplanned: bool,
}

impl HandOffs for Kept {
    fn plan(&self, request: &HandingOff) -> Result<Planned, String> {
        self.planned.lock().unwrap().push(request.clone());
        if self.unplanned {
            return Err("library-application-config-unreadable".to_string());
        }
        // None asked: the agent's configured engine, here opencode.
        let engine = match request.engine.is_empty() {
            true => "opencode".to_string(),
            false => request.engine.clone(),
        };
        Ok(Planned {
            engine,
            drivable_engines: vec!["claude-code".to_string(), "opencode".to_string()],
            credential: runs::CredentialEvidence {
                file_only: false,
                builtin_variables: vec!["CLAUDE_CODE_OAUTH_TOKEN".to_string()],
                accepted_variables: vec!["CLAUDE_CODE_OAUTH_TOKEN".to_string()],
                accepted_set: self.credential_set,
            },
            fatal_findings: 0,
            content: HandOffContent {
                env: request.env.clone(),
                launcher: BTreeMap::new(),
                agent: request.agent.clone(),
            },
        })
    }

    fn write(&self, child: &str, content: &HandOffContent) -> Result<(), String> {
        self.written
            .lock()
            .unwrap()
            .insert(child.to_string(), content.clone());
        Ok(())
    }

    fn delete(&self, child: &str) -> Result<(), String> {
        self.deleted.lock().unwrap().push(child.to_string());
        self.written.lock().unwrap().remove(child);
        Ok(())
    }
}

/// A fixture whose project is registered and whose owner is minted, its
/// hand-offs `kept`.
async fn with_owner(kept: &Arc<Kept>) -> (Fixture, crate::Api<crate::server::tests::World>) {
    let f = fixture(vec!["calm-quiet-heron"]);
    f.api.resolve_project(resolve(COMMIT, "")).await.unwrap();
    f.mint("amber-quiet-heron").await;
    f.catch_up().await;
    let api = f
        .restarted()
        .await
        .handing_off(Arc::clone(kept) as Arc<dyn HandOffs>);
    (f, api)
}

fn create(owner: &str) -> v1::CreateHandOffRequest {
    v1::CreateHandOffRequest {
        parent: PARENT.to_string(),
        hand_off_id: CHILD.to_string(),
        owner: owner.to_string(),
        agent: "dev".to_string(),
        env: Some(v1::EnvVars {
            vars: vec![v1::EnvVar {
                name: "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
                value: "a-token".to_string(), // gitleaks:allow
            }],
        }),
        ..v1::CreateHandOffRequest::default()
    }
}

// The child is planned at the project's registered root, on the one engine
// a child is hosted on when none is named, for its agent, in the sent
// environment; then its hand-off is written, named under its project.
#[tokio::test]
async fn a_hand_off_is_planned_judged_and_written() {
    let kept = Arc::new(Kept {
        credential_set: true,
        ..Kept::default()
    });
    let (_f, api) = with_owner(&kept).await;
    let made = api
        .create_hand_off(create("amber-quiet-heron"))
        .await
        .unwrap();
    assert_eq!(made.name, format!("{PARENT}/handOffs/{CHILD}"));
    let planned = kept.planned.lock().unwrap().clone();
    let [planned] = planned.as_slice() else {
        panic!("planned once: {planned:?}");
    };
    assert_eq!(planned.root, crate::server::tests::ROOT);
    assert_eq!(planned.engine, "", "none asked: the plan's to resolve");
    // Ruling 4: the engine the child was planned on, its agent's, is named.
    assert_eq!(made.engine, "opencode");
    assert_eq!(planned.agent, "dev");
    assert_eq!(planned.child, CHILD);
    assert_eq!(
        planned
            .env
            .get("CLAUDE_CODE_OAUTH_TOKEN")
            .map(String::as_str),
        Some("a-token")
    );
    let written = kept.written.lock().unwrap().clone();
    assert_eq!(written.get(CHILD).map(|c| c.agent.as_str()), Some("dev"));
}

// An owner nobody recorded is refused in the sessions domain's words, and
// nothing is planned or written.
#[tokio::test]
async fn a_hand_off_for_an_owner_nobody_recorded_is_refused_and_writes_nothing() {
    let kept = Arc::new(Kept {
        credential_set: true,
        ..Kept::default()
    });
    let (_f, api) = with_owner(&kept).await;
    let (code, unknown): (_, sessions_proto::v1::SessionUnknown) =
        refused(api.create_hand_off(create("nobody-ever-ran")).await);
    assert_eq!(unknown.harp, "nobody-ever-ran");
    assert_eq!(
        code,
        crate::ApiRefusal::typed(
            &angzarr_embedded::markup::TypedRefusal::of(&sessions::SessionUnknown {
                harp: String::new()
            }),
            <sessions::SessionUnknown as sessions::Refusal>::REFUSAL_STATUS,
        )
        .code
    );
    assert!(kept.planned.lock().unwrap().is_empty());
    assert!(kept.written.lock().unwrap().is_empty());
}

// A child with no credential is refused as its enqueue would refuse it
// (rulings 53, 73e), before its hand-off is written.
#[tokio::test]
async fn a_hand_off_whose_child_has_no_credential_is_refused_and_writes_nothing() {
    let kept = Arc::new(Kept::default());
    let (_f, api) = with_owner(&kept).await;
    let refused = api.create_hand_off(create("amber-quiet-heron")).await;
    let Err(crate::ApiError::Refused(refusal)) = refused else {
        panic!("refused: {refused:?}");
    };
    assert!(kept.written.lock().unwrap().is_empty());
    let rejection = crate::rejection_of(&refusal.refusal);
    assert_eq!(
        rejection,
        Some(crate::Rejection::Runs(runs::RunRejection::NoCredential {
            engine: "opencode".to_string(),
            variables: "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
        }))
    );
    assert_eq!(
        refusal.code,
        crate::ApiRefusal::typed(
            &angzarr_embedded::markup::TypedRefusal::of(&runs::NoCredential {
                engine: String::new(),
                variables: String::new(),
            }),
            <runs::NoCredential as runs::Refusal>::REFUSAL_STATUS,
        )
        .code
    );
}

// A child that cannot be planned is refused, and nothing is written.
#[tokio::test]
async fn a_hand_off_that_cannot_be_planned_is_refused_and_writes_nothing() {
    let kept = Arc::new(Kept {
        unplanned: true,
        ..Kept::default()
    });
    let (_f, api) = with_owner(&kept).await;
    let (_, unreadable): (_, v1::ConfigUnreadable) =
        refused(api.create_hand_off(create("amber-quiet-heron")).await);
    assert_eq!(unreadable.root, crate::server::tests::ROOT);
    assert!(kept.written.lock().unwrap().is_empty());
}

// A hand-off no open session holds is deleted; one an open session holds is
// its child's, kept.
#[tokio::test]
async fn a_hand_off_is_deleted_only_while_no_open_session_holds_it() {
    let kept = Arc::new(Kept::default());
    let (f, api) = with_owner(&kept).await;
    let deleted = api
        .delete_hand_off(v1::DeleteHandOffRequest {
            name: format!("{PARENT}/handOffs/{CHILD}"),
            ..v1::DeleteHandOffRequest::default()
        })
        .await
        .unwrap();
    assert_eq!(deleted.name, format!("{PARENT}/handOffs/{CHILD}"));
    assert_eq!(*kept.deleted.lock().unwrap(), [CHILD.to_string()]);
    let owner = sessions::SessionName::parse(&format!("{PARENT}/sessions/amber-quiet-heron"))
        .unwrap()
        .uid();
    let _ = f;
    api.delete_hand_off(v1::DeleteHandOffRequest {
        name: format!("{PARENT}/handOffs/{owner}"),
        ..v1::DeleteHandOffRequest::default()
    })
    .await
    .unwrap();
    assert_eq!(
        *kept.deleted.lock().unwrap(),
        [CHILD.to_string()],
        "the open session's is kept"
    );
    let (_, invalid): (_, v1::NameInvalid) = refused(
        api.delete_hand_off(v1::DeleteHandOffRequest {
            name: format!("{PARENT}/sessions/x"),
            ..v1::DeleteHandOffRequest::default()
        })
        .await,
    );
    assert_eq!(invalid.reason, "shape");
}

// Ruling 4: an engine asked is the one the child is planned and judged on,
// and named back.
#[tokio::test]
async fn a_hand_off_on_an_engine_asked_is_planned_on_it() {
    let kept = Arc::new(Kept {
        credential_set: true,
        ..Kept::default()
    });
    let (_f, api) = with_owner(&kept).await;
    let made = api
        .create_hand_off(v1::CreateHandOffRequest {
            engine: "claude-code".to_string(),
            ..create("amber-quiet-heron")
        })
        .await
        .unwrap();
    assert_eq!(made.engine, "claude-code");
    assert_eq!(kept.planned.lock().unwrap()[0].engine, "claude-code");
}

// A launch whose hand-off was made before the drain began, and whose
// `LaunchChild` the drain then refused, withdraws its hand-off: the
// withdrawal starts no work (OD-62 refuses NEW invocations), so the
// draining daemon serves it, and no environment of a child that will never
// start is left for the sweep to find.
#[tokio::test]
async fn a_draining_daemon_withdraws_a_hand_off_made_before_the_drain() {
    use crate::Calls;
    let kept = Arc::new(Kept {
        credential_set: true,
        ..Kept::default()
    });
    let (_f, api) = with_owner(&kept).await;
    let calls: &dyn Calls = &api;
    let made = calls
        .create_hand_off(create("amber-quiet-heron"))
        .await
        .unwrap();
    calls
        .stop_daemon(v1::StopDaemonRequest {
            drain: true,
            ..v1::StopDaemonRequest::default()
        })
        .await
        .unwrap();
    let withdrawn = calls
        .delete_hand_off(v1::DeleteHandOffRequest {
            name: made.name.clone(),
            ..v1::DeleteHandOffRequest::default()
        })
        .await
        .map_err(|error| format!("{error:?}"))
        .expect("a draining daemon withdraws a hand-off");
    assert_eq!(withdrawn.name, made.name);
    assert_eq!(*kept.deleted.lock().unwrap(), [CHILD.to_string()]);
    assert!(kept.written.lock().unwrap().is_empty(), "nothing left");
    let (_, draining): (_, v1::DaemonDraining) =
        refused(calls.create_hand_off(create("amber-quiet-heron")).await);
    assert!(draining.harps.is_empty(), "{draining:?}");
}
