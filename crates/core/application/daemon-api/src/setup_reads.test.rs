//! A setup read is answered by the setup use cases where the caller stands
//! (step 10c U5, D1): its directory, its environment and its flags.

use std::sync::{Arc, Mutex};

use daemon_proto::v1;
use library_application::Library;

use super::{SetupAnswer, SetupRead, SetupReads};
use crate::library_reads::Libraries;
use crate::server::tests::fixture;
use crate::{ApiError, Calls};

/// Every directory is its own project's root.
struct Rooted;

impl Libraries for Rooted {
    fn root(&self, dir: &str, _: &[(String, String)]) -> Option<String> {
        Some(dir.to_string())
    }

    fn read(
        &self,
        _: &str,
        _: &[(String, String)],
        _: &mut dyn FnMut(&Library),
    ) -> Result<(), ApiError> {
        Ok(())
    }

    fn read_deps(
        &self,
        _: &str,
        _: &[(String, String)],
        _: &mut dyn FnMut(&deps_application::Deps),
    ) -> Result<(), ApiError> {
        Ok(())
    }
}

/// One read as told: what was asked, where, in which environment.
type Asked = (String, String, Vec<(String, String)>);

/// What each read was asked; it answers with the directory it was asked
/// at.
#[derive(Default)]
struct Told(Mutex<Vec<Asked>>);

impl SetupReads for Told {
    fn read(
        &self,
        dir: &str,
        env: &[(String, String)],
        _: &library_application::Flags,
        ask: SetupRead,
    ) -> Result<SetupAnswer, ApiError> {
        self.0
            .lock()
            .unwrap()
            .push((format!("{ask:?}"), dir.to_string(), env.to_vec()));
        Ok(match ask {
            SetupRead::Doctor { .. } => SetupAnswer::Doctor(v1::RunDoctorResponse {
                checks: vec![v1::DoctorCheck {
                    marker: dir.to_string(),
                    ..v1::DoctorCheck::default()
                }],
                ..v1::RunDoctorResponse::default()
            }),
            SetupRead::AuthStatus => SetupAnswer::AuthStatus(v1::GetAuthStatusResponse {
                rows: vec![v1::AuthStatusRow {
                    engine: dir.to_string(),
                    ..v1::AuthStatusRow::default()
                }],
                ..v1::GetAuthStatusResponse::default()
            }),
            SetupRead::CheckManage => SetupAnswer::CheckManage(v1::CheckManageResponse {
                work_dir: dir.to_string(),
                ..v1::CheckManageResponse::default()
            }),
            SetupRead::InitPrompt => SetupAnswer::InitPrompt(v1::GetInitPromptResponse {
                text: dir.to_string(),
                ..v1::GetInitPromptResponse::default()
            }),
        })
    }
}

fn env() -> Option<v1::EnvVars> {
    Some(v1::EnvVars {
        vars: vec![v1::EnvVar {
            name: "PATH".to_string(),
            value: "/opt/bin".to_string(),
        }],
    })
}

// Step 10c U5 (D1): doctor, auth status, manage check and init prompt are
// each answered by the setup use cases at the caller's directory, in its
// environment, never in the daemon's own place.
#[tokio::test]
async fn each_setup_read_is_answered_where_the_caller_stands() {
    let told = Arc::new(Told::default());
    let api = fixture(vec![])
        .api
        .reading(Arc::new(Rooted))
        .reading_setup(Arc::clone(&told) as Arc<dyn SetupReads>);
    let doctor = api
        .run_doctor(v1::RunDoctorRequest {
            deps: true,
            project: "users/me".to_string(),
            cwd: "/work/a".to_string(),
            env: env(),
            ..v1::RunDoctorRequest::default()
        })
        .await
        .unwrap();
    assert_eq!(doctor.checks[0].marker, "/work/a");
    let auth = api
        .get_auth_status(v1::GetAuthStatusRequest {
            project: "users/me".to_string(),
            cwd: "/work/b".to_string(),
            env: env(),
            ..v1::GetAuthStatusRequest::default()
        })
        .await
        .unwrap();
    assert_eq!(auth.rows[0].engine, "/work/b");
    let check = api
        .check_manage(v1::CheckManageRequest {
            project: "users/me".to_string(),
            cwd: "/work/c".to_string(),
            env: env(),
            ..v1::CheckManageRequest::default()
        })
        .await
        .unwrap();
    assert_eq!(check.work_dir, "/work/c");
    let prompt = api
        .get_init_prompt(v1::GetInitPromptRequest {
            project: "users/me".to_string(),
            cwd: "/work/d".to_string(),
            env: env(),
            ..v1::GetInitPromptRequest::default()
        })
        .await
        .unwrap();
    assert_eq!(prompt.text, "/work/d");
    let path = vec![("PATH".to_string(), "/opt/bin".to_string())];
    assert_eq!(
        *told.0.lock().unwrap(),
        [
            (
                "Doctor { deps: true }".to_string(),
                "/work/a".to_string(),
                path.clone()
            ),
            (
                "AuthStatus".to_string(),
                "/work/b".to_string(),
                path.clone()
            ),
            (
                "CheckManage".to_string(),
                "/work/c".to_string(),
                path.clone()
            ),
            ("InitPrompt".to_string(), "/work/d".to_string(), path),
        ]
    );
}

// A daemon given no setup use cases answers no setup read: it fails,
// naming why, and never answers an empty report.
#[tokio::test]
async fn a_daemon_with_no_setup_reads_fails_them() {
    let api = fixture(vec![]).api.reading(Arc::new(Rooted));
    let failed = api
        .get_auth_status(v1::GetAuthStatusRequest {
            project: "users/me".to_string(),
            ..v1::GetAuthStatusRequest::default()
        })
        .await
        .unwrap_err();
    assert!(matches!(failed, ApiError::Failed { .. }), "{failed:?}");
}
