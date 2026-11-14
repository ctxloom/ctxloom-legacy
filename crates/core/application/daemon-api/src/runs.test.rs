//! `CreateRun`, `GetRun` and `WatchRun` (U4, T7) over the daemon's
//! one-shots, their host scripted: what the daemon is asked to host, and
//! how a placed and an ended run are answered and watched.

use std::sync::{Arc, Mutex};

use daemon_application::{HostRequest, OneShotControl, OneShotHost, OneShots, Told};
use daemon_proto::v1;
use run_application::{Answer, Done, Placement, RunError};

use crate::server::tests::{fixture, refused};
use crate::ApiError;

const ROOT: &str = "0195f0a4-7b1e-7000-8000-000000000001";
const SESSION: &str = "projects/calm-quiet-heron/sessions/amber-quiet-heron";

/// A host that answers each one-shot at once, as scripted, keeping what it
/// was asked.
struct Scripted {
    refuse: bool,
    asked: Arc<Mutex<Vec<HostRequest>>>,
}

struct Nothing;

impl OneShotControl for Nothing {
    fn stop(&self) {}
}

impl OneShotHost for Scripted {
    fn host(&self, request: HostRequest, told: Told) -> Box<dyn OneShotControl> {
        self.asked.lock().unwrap().push(request);
        if self.refuse {
            told.done(Done {
                result: Err(RunError::Refused(runs::RunRejection::NoPrompt)),
                notices: vec![library_application::Notice::text("before")],
            });
        } else {
            told.placed(&Placement {
                run: ROOT.to_string(),
                session: "s".to_string(),
                name: SESSION.to_string(),
                harp: "amber-quiet-heron".to_string(),
                engine: "claude-code".to_string(),
            });
            told.done(Done {
                result: Ok(Answer {
                    harp: "amber-quiet-heron".to_string(),
                    session: "s".to_string(),
                    engine: "claude-code".to_string(),
                    answer: "HELLO".to_string(),
                }),
                notices: Vec::new(),
            });
        }
        Box::new(Nothing)
    }
}

fn one_shots(refuse: bool) -> (Arc<OneShots>, Arc<Mutex<Vec<HostRequest>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let hosted = Arc::new(OneShots::new(Box::new(Scripted {
        refuse,
        asked: Arc::clone(&asked),
    })));
    (hosted, asked)
}

fn one_shot(parent: &str) -> v1::CreateRunRequest {
    v1::CreateRunRequest {
        parent: parent.to_string(),
        mode: v1::run::Mode::OneShot as i32,
        prompt: "hi".to_string(),
        agent: "dev".to_string(),
        cwd: "/work/sub".to_string(),
        env: Some(v1::EnvVars {
            vars: vec![v1::EnvVar {
                name: "PATH".to_string(),
                value: "/bin".to_string(),
            }],
        }),
        ..v1::CreateRunRequest::default()
    }
}

#[tokio::test]
async fn a_one_shot_is_hosted_in_its_project_and_answered_once_placed() {
    let f = fixture(vec!["calm-quiet-heron"]);
    f.api
        .resolve_project(v1::ResolveProjectRequest {
            cwd: "/work".to_string(),
            project_id: "calm".to_string(),
            root: "/work".to_string(),
            ..v1::ResolveProjectRequest::default()
        })
        .await
        .unwrap();
    f.catch_up().await;
    let (hosted, asked) = one_shots(false);
    let api = f.restarted().await.hosting_one_shots(Arc::clone(&hosted));
    let run = api
        .create_run(one_shot("projects/calm-quiet-heron"))
        .await
        .unwrap();
    let name = format!("projects/calm-quiet-heron/runs/{ROOT}");
    assert_eq!(run.name, name);
    assert_eq!(run.session, SESSION);
    assert_eq!(run.engine, "projects/calm-quiet-heron/engines/claude-code");
    assert_eq!(run.mode, v1::run::Mode::OneShot as i32);
    assert_eq!(run.agent, "dev");
    assert!(run.end.is_none());
    let request = asked.lock().unwrap()[0].clone();
    let project = request.project.expect("its project");
    assert_eq!(
        (
            project.id.as_str(),
            project.harp.as_str(),
            project.root.as_str()
        ),
        ("calm", "calm-quiet-heron", "/work")
    );
    assert_eq!(request.cwd, "/work/sub");
    assert_eq!(request.env.get("PATH").map(String::as_str), Some("/bin"));
    assert_eq!(request.request.run.prompt, "hi");
    let got = api
        .get_run(v1::GetRunRequest { name: name.clone() })
        .await
        .unwrap();
    assert_eq!(got.session, SESSION);
    assert_eq!(got.name, name);
    let events: Vec<v1::RunEvent> = api
        .watch_run(v1::WatchRunRequest { name })
        .await
        .unwrap()
        .collect();
    assert_eq!(events.len(), 2, "its reply and its end: {events:?}");
    assert_eq!(
        events[0].event,
        Some(v1::run_event::Event::Reply("HELLO".to_string()))
    );
}

#[tokio::test]
async fn a_one_shot_outside_a_project_is_hosted_with_none() {
    let f = fixture(vec![]);
    let (hosted, asked) = one_shots(false);
    let api = f.restarted().await.hosting_one_shots(hosted);
    let run = api.create_run(one_shot("users/me")).await.unwrap();
    assert_eq!(run.name, format!("projects/calm-quiet-heron/runs/{ROOT}"));
    assert_eq!(run.engine, "claude-code");
    assert!(asked.lock().unwrap()[0].project.is_none());
}

#[tokio::test]
async fn a_one_shot_that_ended_before_it_was_hosted_is_answered_with_its_end() {
    let f = fixture(vec![]);
    let (hosted, _) = one_shots(true);
    let api = f.restarted().await.hosting_one_shots(hosted);
    let run = api.create_run(one_shot("users/me")).await.unwrap();
    assert_eq!(run.name, "users/me/runs/-");
    assert_eq!(run.notices.len(), 1);
    let end = run.end.expect("its end");
    assert_eq!(end.code, "runs-no-prompt");
    assert_eq!(end.exit, 20);
}

#[tokio::test]
async fn a_one_shot_in_a_project_nobody_named_is_not_found() {
    let f = fixture(vec![]);
    let (hosted, asked) = one_shots(false);
    let api = f.restarted().await.hosting_one_shots(hosted);
    let _: (_, v1::ProjectNotFound) = refused(api.create_run(one_shot("projects/far-heron")).await);
    let _: (_, v1::NameInvalid) = refused(api.create_run(one_shot("projects/")).await);
    assert!(asked.lock().unwrap().is_empty(), "nothing hosted");
}

#[tokio::test]
async fn a_terminal_run_and_a_preview_are_not_served_yet() {
    let f = fixture(vec![]);
    let (hosted, asked) = one_shots(false);
    let api = f.restarted().await.hosting_one_shots(hosted);
    for request in [
        v1::CreateRunRequest {
            mode: v1::run::Mode::Terminal as i32,
            ..one_shot("users/me")
        },
        v1::CreateRunRequest {
            mode: v1::run::Mode::Unspecified as i32,
            ..one_shot("users/me")
        },
        v1::CreateRunRequest {
            validate_only: true,
            ..one_shot("users/me")
        },
    ] {
        let answered = api.create_run(request).await;
        assert!(
            matches!(answered, Err(ApiError::Unimplemented { .. })),
            "{answered:?}"
        );
    }
    assert!(asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_run_it_does_not_host_is_not_found() {
    let f = fixture(vec![]);
    let (hosted, _) = one_shots(false);
    let api = f.restarted().await.hosting_one_shots(hosted);
    let name = "projects/calm-quiet-heron/runs/nope".to_string();
    let (_, missing): (_, v1::RunNotFound) =
        refused(api.get_run(v1::GetRunRequest { name: name.clone() }).await);
    assert_eq!(missing.name, name);
    let watched = api.watch_run(v1::WatchRunRequest { name }).await;
    assert!(matches!(watched, Err(ApiError::Refused(_))), "not found");
}

#[tokio::test]
async fn a_daemon_that_hosts_no_one_shot_fails_its_runs() {
    let f = fixture(vec![]);
    let answered = f.api.create_run(one_shot("users/me")).await;
    assert!(
        matches!(answered, Err(ApiError::Failed { .. })),
        "{answered:?}"
    );
    let answered = f
        .api
        .get_run(v1::GetRunRequest {
            name: "x".to_string(),
        })
        .await;
    assert!(
        matches!(answered, Err(ApiError::Failed { .. })),
        "{answered:?}"
    );
}
