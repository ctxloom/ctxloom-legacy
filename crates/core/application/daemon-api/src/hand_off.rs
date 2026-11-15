//! A child's hand-off (T6, S10; daemon slice 4 U3): `CreateHandOff` and
//! `DeleteHandOff`, served by the daemon.
//!
//! A launch hands the daemon the environment its child is launched with
//! BEFORE the owner's `LaunchChild` is recorded. The daemon checks, in
//! order and before it writes anything, so a refused launch leaves nothing:
//!
//! 1. the owner: a session its warm index holds in the project, else the
//!    sessions domain's `SessionUnknown` ("there is no session named ...");
//! 2. the child's plan: its project's dry run on its engine (none asked: its
//!    agent's configured one, the config's default engine for an agent
//!    naming none, which the answer names) and agent, in the environment
//!    the launch sent ([`HandOffs::plan`], the root's);
//! 3. its credential, as the child's enqueue will judge it: the runs
//!    aggregate's own pre-launch checks (`runs::pre_launch_refusal`, rulings
//!    53, 73e), refused with the typed refusal the enqueue would answer
//!    (`runs-no-credential`, `runs-file-only-credential`, ...).
//!
//! Only then is the hand-off written (0600, the root's [`HandOffs::write`]),
//! marked in launch: the launch's creator holds its launch lock from before
//! `CreateHandOff` to after its `LaunchChild`, which claims it, so the sweep
//! keeps it meanwhile, and clears it once its creator is proven gone.
//! A hand-off is withdrawn only while no open session holds it (a child its
//! `LaunchChild` made is the daemon's to delete, at its close).

use std::collections::BTreeMap;

use daemon_proto::v1;
use sessions::{ProjectName, SessionName};

use crate::refusal::{ApiError, ApiRefusal};
use crate::server::{invalid, Api};

/// What a hand-off is made for: the child of a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandingOff {
    /// The project's registered root.
    pub root: String,
    /// The child's stable id.
    pub child: String,
    pub engine: String,
    /// The agent it is planned for; empty, the project's default.
    pub agent: String,
    /// The launch's environment (S7), whole.
    pub env: BTreeMap<String, String>,
}

/// What the child is handed: the environment it may be given, where its
/// launcher finds its user's settings, and the agent it is planned for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandOffContent {
    pub env: BTreeMap<String, String>,
    pub launcher: BTreeMap<String, String>,
    pub agent: String,
}

/// What the child's plan says, as its start's enqueue is judged on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
    /// The engine it is planned on: the one asked, else its agent's.
    pub engine: String,
    /// Every engine the daemon hosts a child on.
    pub drivable_engines: Vec<String>,
    /// Its credential evidence (ruling 53), by name only.
    pub credential: runs::CredentialEvidence,
    /// How many fatal findings its plan has.
    pub fatal_findings: u32,
    /// What it is to be handed.
    pub content: HandOffContent,
}

/// The hand-offs, outside the core: the root's.
pub trait HandOffs: Send + Sync {
    /// The child's plan (its project's dry run); the plan's error code where
    /// it cannot be made.
    fn plan(&self, request: &HandingOff) -> Result<Planned, String>;

    /// Write the hand-off of `child`, whole (0600), and mark it in launch:
    /// the sweep keeps a marked hand-off while its launch's creator holds
    /// its launch lock, until the `LaunchChild` it was made for claims it.
    fn write(&self, child: &str, content: &HandOffContent) -> Result<(), String>;

    /// Delete the hand-off of `child`, and its mark, if there are any.
    fn delete(&self, child: &str) -> Result<(), String>;
}

/// The collection a hand-off's name has under its project.
const HAND_OFFS: &str = "/handOffs/";

fn failed(detail: String) -> ApiError {
    ApiError::Failed { detail }
}

impl<E: run_ports::Edge> Api<E> {
    /// `CreateHandOff`: the owner checked, the child planned and judged,
    /// then its hand-off written; refused, nothing is written.
    pub async fn create_hand_off(
        &self,
        request: v1::CreateHandOffRequest,
    ) -> Result<v1::HandOff, ApiError> {
        let project = ProjectName::parse(&request.parent)
            .map_err(|rejection| invalid(&request.parent, &rejection))?;
        let child = uuid::Uuid::parse_str(&request.hand_off_id)
            .map_err(|_| {
                ApiError::from(ApiRefusal::of(&v1::NameInvalid {
                    name: request.hand_off_id.clone(),
                    reason: "shape".to_string(),
                }))
            })?
            .hyphenated()
            .to_string();
        let hand_offs = self
            .hand_offs
            .as_ref()
            .ok_or_else(|| failed("this daemon makes no hand-off".to_string()))?;
        self.parts.hosting.fresh().await;
        let index = self.parts.index.index();
        let row = index.project_named_by(project.harp()).ok_or_else(|| {
            ApiError::from(ApiRefusal::of(&v1::ProjectNotFound {
                name: request.parent.clone(),
            }))
        })?;
        let owner = SessionName::new(project.harp(), &request.owner)
            .ok()
            .and_then(|owner| index.find_by_id(&owner.uid()));
        if owner.is_none() {
            let unknown = sessions::SessionUnknown {
                harp: request.owner.clone(),
            };
            return Err(ApiRefusal::typed(
                &angzarr_embedded::markup::TypedRefusal::of(&unknown),
                <sessions::SessionUnknown as sessions::Refusal>::REFUSAL_STATUS,
            )
            .into());
        }
        let handing = HandingOff {
            root: row.root.clone(),
            child: child.clone(),
            engine: request.engine.clone(),
            agent: request.agent.clone(),
            env: request
                .env
                .unwrap_or_default()
                .vars
                .into_iter()
                .map(|var| (var.name, var.value))
                .collect(),
        };
        let planned = hand_offs.plan(&handing).map_err(|code| {
            ApiError::from(ApiRefusal::of(&v1::ConfigUnreadable {
                root: row.root.clone(),
                detail: code,
            }))
        })?;
        let checks = runs::EnqueueRun {
            session: child.clone(),
            incarnation: 1,
            mode: runs::RunMode::Structured,
            prompt_given: true,
            engine: planned.engine.clone(),
            drivable_engines: planned.drivable_engines,
            credential: planned.credential,
            fatal_findings: planned.fatal_findings,
            launcher_lock: String::new(),
        };
        if let Some((refusal, status)) = runs::pre_launch_refusal(&checks) {
            return Err(ApiRefusal::typed(&refusal, status).into());
        }
        hand_offs.write(&child, &planned.content).map_err(failed)?;
        Ok(v1::HandOff {
            name: format!("{project}{HAND_OFFS}{child}"),
            engine: planned.engine,
        })
    }

    /// `DeleteHandOff`: the hand-off of a child no open session holds
    /// deleted (a launch whose `LaunchChild` was refused); one an open
    /// session holds is kept, its child's to the end.
    pub async fn delete_hand_off(
        &self,
        request: v1::DeleteHandOffRequest,
    ) -> Result<v1::HandOff, ApiError> {
        let shape = || {
            ApiError::from(ApiRefusal::of(&v1::NameInvalid {
                name: request.name.clone(),
                reason: "shape".to_string(),
            }))
        };
        let (project, child) = request.name.split_once(HAND_OFFS).ok_or_else(shape)?;
        ProjectName::parse(project).map_err(|rejection| invalid(project, &rejection))?;
        let child = uuid::Uuid::parse_str(child)
            .map_err(|_| shape())?
            .hyphenated()
            .to_string();
        let hand_offs = self
            .hand_offs
            .as_ref()
            .ok_or_else(|| failed("this daemon makes no hand-off".to_string()))?;
        self.parts.hosting.fresh().await;
        let open = self
            .parts
            .index
            .index()
            .find_by_id(&child)
            .is_some_and(|record| !record.closed);
        if !open {
            hand_offs.delete(&child).map_err(failed)?;
        }
        Ok(v1::HandOff {
            name: request.name,
            engine: String::new(),
        })
    }
}

#[cfg(test)]
#[path = "hand_off.test.rs"]
mod tests;
