//! `InitProject` (step 10c U6, protos.md §4.3): `ctxloom init` is the
//! daemon's write. Its preview asks the questions as needs, every one
//! OPTIONAL (QUESTIONS.md 2): the output folder, the conditional guidance,
//! each shipped companion found on the caller's machine and companions by
//! name. Its apply creates the project's `.ctxloom` as ONE `CreateDirectory`
//! of the tree's root on a NEW checkout stream: the project is named first
//! and the checkout's harp minted and staged with the scaffold (at
//! `state/checkout`), so the stream's name exists before the command; then
//! the tree is made by one atomic operation, recorded done, and its baseline
//! observed. No git is asked or run: the library is git-free (owner,
//! 2026-11-18). A re-run (the directory is there) writes nothing. A retry
//! under the same request id finds its tree's names in its intent and
//! finishes it.
//!
//! The companions she accepts are registered by the client as `companion
//! add` registers them (`CreateCompanion`, a write of the home's library),
//! and the interview is a terminal `CreateRun`, as every `ctxloom run`.

use daemon_proto::v1;
use init_application::needs::{self, Given};
use library::write::scaffold::Answered;
use library_api::writes::{self, Shape};
use library_api::Site;
use library_application::ProjectInitialized;
use library_checkout::End;
use library_tree::{Evidence, TreeName};
use run_ports::Edge;
use sessions::ProjectName;

use crate::library_writes::{notice_of, LibraryWriteRequest, Made};
use crate::refusal::{ApiError, ApiRefusal};
use crate::server::Api;

/// The verb an init's intent is recorded under.
pub(crate) const VERB: &str = "init";

/// `need` as the preview asks it: optional, so an apply that leaves it
/// unanswered takes its suggestion or writes nothing it alone would.
pub(crate) fn need_of(need: &needs::Need) -> v1::Need {
    let kind = match &need.kind {
        needs::Kind::Ask { suggestion } => v1::need::Kind::Ask(v1::Ask {
            text: need.text.clone(),
            suggestion: suggestion.clone(),
            ..v1::Ask::default()
        }),
        needs::Kind::Choose { choices } => v1::need::Kind::Choose(v1::Choose {
            text: need.text.clone(),
            choices: choices.clone(),
            ..v1::Choose::default()
        }),
        needs::Kind::Confirm => v1::need::Kind::Confirm(v1::Confirm {
            text: need.text.clone(),
            ..v1::Confirm::default()
        }),
    };
    v1::Need {
        id: need.id.clone(),
        kind: Some(kind),
        optional: true,
    }
}

/// The apply's answers, by need id; an answer of no kind is none.
pub(crate) fn given_of(answers: &[v1::Answer]) -> Vec<(String, Given)> {
    answers
        .iter()
        .filter_map(|answer| {
            let given = match answer.answer.as_ref()? {
                v1::answer::Answer::Text(text) | v1::answer::Answer::Edited(text) => {
                    Given::Text(text.clone())
                }
                v1::answer::Answer::Chosen(word) => Given::Chosen(word.clone()),
                v1::answer::Answer::Confirmed(yes) => Given::Confirmed(*yes),
            };
            Some((answer.need_id.clone(), given))
        })
        .collect()
}

/// `shape` (the scaffold's one create) with the checkout's harp staged in
/// it at `state/checkout`, as the harp's file is written: the stream the
/// tree is recorded on is named by it before the tree exists.
pub(crate) fn with_checkout(shape: Shape, harp: &str) -> Shape {
    match shape {
        Shape::Create { path, mut files } => {
            files.push(ctxloom_ports::Put::file(
                library_tree::CHECKOUT_FILE,
                format!("{harp}\n"),
            ));
            Shape::Create { path, files }
        }
        other => other,
    }
}

/// The tree an init tried before under the same request id is recorded on:
/// its intent's one create names it.
pub(crate) fn tree_of(known: &library_checkout::Known) -> Option<TreeName> {
    let create = known.intent.steps.first()?.create.as_ref()?;
    TreeName::parse_checkout(&create.name)
}

/// What `manage install` answers: the project's `.ctxloom` at `app_dir`,
/// whether it was created and its `.gitignore`'s status, the id a new one
/// was named by.
pub(crate) fn installed_of(
    app_dir: &str,
    (initialized, gitignore_status): (bool, String),
    project_id: String,
    notices: Vec<v1::Notice>,
) -> v1::InstallManageResponse {
    let root = app_dir
        .strip_suffix("/.ctxloom")
        .or_else(|| app_dir.strip_suffix("\\.ctxloom"))
        .unwrap_or(app_dir);
    v1::InstallManageResponse {
        app_dir: app_dir.to_string(),
        initialized,
        gitignore: format!("{root}/.gitignore"),
        gitignore_status,
        project_id,
        notices,
    }
}

/// What init's apply answers.
fn answered_init(
    initialized: &ProjectInitialized,
    project_id: &str,
    answered: &Answered,
    notices: Vec<v1::Notice>,
) -> v1::InitProjectResponse {
    let written = if initialized.existed || project_id.is_empty() {
        Vec::new()
    } else {
        vec![initialized.app_dir.clone()]
    };
    v1::InitProjectResponse {
        written,
        app_dir: initialized.app_dir.clone(),
        existed: initialized.existed,
        project_id: project_id.to_string(),
        output_dir: answered.output_dir.clone().unwrap_or_default(),
        premised: answered.premised.clone().unwrap_or_default(),
        notices,
        ..v1::InitProjectResponse::default()
    }
}

impl<E: Edge> Api<E> {
    /// `InitProject`: its preview's needs, or its apply (module docs).
    pub(crate) async fn init_project(
        &self,
        request: v1::InitProjectRequest,
    ) -> Result<v1::InitProjectResponse, ApiError> {
        self.init_with(request, ("", VERB)).await
    }

    /// `InstallManage` (step 10c U6): a new project created whole as
    /// init creates it, its default agent on `engine`, recorded as `manage
    /// install`; one that is there gets ctxloom's `.gitignore` (the
    /// `manage gitignore install` write).
    pub(crate) async fn install_manage(
        &self,
        request: v1::InstallManageRequest,
    ) -> Result<v1::InstallManageResponse, ApiError> {
        let init = v1::InitProjectRequest {
            name: request.project.clone(),
            cwd: request.cwd.clone(),
            request_id: request.request_id.clone(),
            validate_only: request.validate_only,
            env: request.env.clone(),
            globals: request.globals.clone(),
            ..v1::InitProjectRequest::default()
        };
        let done = self
            .init_with(init, (&request.engine, "manage install"))
            .await?;
        let mut notices = done.notices;
        let mut status = "unchanged".to_string();
        if done.existed && !request.validate_only {
            let gitignore = v1::InstallGitignoreRequest {
                project: request.project.clone(),
                request_id: request.request_id.clone(),
                env: request.env.clone(),
                globals: request.globals.clone(),
                cwd: request.cwd.clone(),
                ..v1::InstallGitignoreRequest::default()
            };
            let wrote = self
                .library_write(&gitignore, "manage gitignore install", |library, _, _| {
                    library.gitignore_install()
                })
                .await?;
            let installed = crate::setup_reads::gitignore_installed(&wrote);
            status = installed.status;
            notices.extend(installed.notices);
        }
        Ok(installed_of(
            &done.app_dir,
            (!done.existed, status),
            done.project_id,
            notices,
        ))
    }

    /// [`Api::init_project`]'s preview or apply, the new project's default
    /// agent on `engine` (empty: claude-code), its intent recorded as
    /// `verb`.
    async fn init_with(
        &self,
        request: v1::InitProjectRequest,
        (engine, verb): (&str, &str),
    ) -> Result<v1::InitProjectResponse, ApiError> {
        self.open()?;
        let (site, _) = self
            .resolve(
                request.target(),
                request.cwd(),
                request.env(),
                request.globals(),
            )
            .await?;
        let chosen = needs::chosen(&given_of(&request.answers));
        let answered = Answered {
            output_dir: chosen.output_dir,
            premised: chosen.premised,
        };
        let (observations, _) = self.observing.as_ref().ok_or_else(|| ApiError::Failed {
            detail: "this daemon observes no library".to_string(),
        })?;
        let request_id = request.request_id.clone();
        let known = (!request_id.is_empty())
            .then(|| observations.known(&request_id))
            .flatten();
        // Taken out of the plan before anything is awaited: what it says
        // is not `Send`.
        let (mut notices, initialized, shaped) = {
            let planned = self.planned(&site, !request.validate_only, &|library, _, _| {
                library.project_init(engine, &answered)
            })?;
            let notices: Vec<v1::Notice> = planned
                .outcome
                .notices
                .iter()
                .map(run_application::client::notice_of)
                .collect();
            let initialized = planned
                .outcome
                .result
                .map_err(|error| crate::library_reads::read_error(&error));
            (notices, initialized, planned.shaped)
        };
        let initialized = initialized?;
        if request.validate_only {
            return self.previewed(&site, initialized, notices);
        }
        let pending = match known.as_ref() {
            Some(known) => match known.end {
                End::Pending { done } => Some((done, known)),
                End::Done => {
                    // Made, its answer lost: answered from its tree's names.
                    let (_, project_id) = self.named_before(known).await?;
                    let made = ProjectInitialized {
                        existed: false,
                        ..initialized
                    };
                    return Ok(answered_init(&made, &project_id, &answered, notices));
                }
                End::Failed | End::Superseded => {
                    return Err(ApiError::Failed {
                        detail: format!(
                            "the init {request_id} was tried before and did not complete"
                        ),
                    });
                }
            },
            None => None,
        };
        if initialized.existed && pending.is_none() {
            return Ok(answered_init(&initialized, "", &answered, notices));
        }
        let shape = match shaped
            .map_err(|rejection| ApiError::Refused(crate::library_refusal(&rejection)))?
        {
            Some(shaped) => shaped.shape,
            None => Shape::Create {
                path: String::new(),
                files: Vec::new(),
            },
        };
        let root = site.at.clone();
        let (tree, project_id) = match pending {
            Some((_, known)) => {
                let (tree, project_id) = self.named_before(known).await?;
                (tree, project_id)
            }
            None => self.name_new(observations, &root, &request).await?,
        };
        let shape = with_checkout(shape, tree.checkout_harp());
        let made = self
            .create_tree(
                (&tree, &initialized.app_dir, &root),
                (verb, &shape),
                &request_id,
                pending.map(|(done, known)| (done, known.intent.steps.clone())),
            )
            .await?;
        notices.extend(made);
        let initialized = ProjectInitialized {
            existed: false,
            ..initialized
        };
        Ok(answered_init(&initialized, &project_id, &answered, notices))
    }

    /// A preview's answer: the project's `.ctxloom`, whether it is there,
    /// and (it is not) init's needs.
    fn previewed(
        &self,
        site: &crate::library_reads::ReadSite<'_>,
        initialized: ProjectInitialized,
        notices: Vec<v1::Notice>,
    ) -> Result<v1::InitProjectResponse, ApiError> {
        let asked = if initialized.existed {
            Vec::new()
        } else {
            needs::needs(&self.shipped_available(site)?)
                .iter()
                .map(need_of)
                .collect()
        };
        Ok(v1::InitProjectResponse {
            app_dir: initialized.app_dir,
            existed: initialized.existed,
            needs: asked,
            notices,
            ..v1::InitProjectResponse::default()
        })
    }

    /// The shipped companions whose programs are on the caller's machine,
    /// as the site's library looks for them (never run).
    fn shipped_available(
        &self,
        site: &crate::library_reads::ReadSite<'_>,
    ) -> Result<Vec<String>, ApiError> {
        let mut found = Vec::new();
        site.libraries.read(&site.at, &site.env, &mut |library| {
            if found.is_empty() {
                found = library::companion::SHIPPED
                    .iter()
                    .filter(|name| library.companion_available(&site.flags, name))
                    .map(ToString::to_string)
                    .collect();
            }
        })?;
        Ok(found)
    }

    /// A new project named: its id minted, the project registered at
    /// `root` under it (as a client's registration names it), and its
    /// checkout's harp minted; the tree it is recorded on, and the id.
    async fn name_new(
        &self,
        observations: &library_api::Observations<E>,
        root: &str,
        request: &v1::InitProjectRequest,
    ) -> Result<(TreeName, String), ApiError> {
        let project_id = observations.fresh_harp();
        let project = self
            .resolve_project(v1::ResolveProjectRequest {
                project_id: project_id.clone(),
                root: root.to_string(),
                cwd: request.cwd.clone(),
                env: request.env.clone(),
                ..v1::ResolveProjectRequest::default()
            })
            .await?;
        let name = ProjectName::parse(&project.name)
            .map_err(|rejection| crate::server::invalid(&project.name, &rejection))?;
        let checkout = observations.fresh_harp();
        let tree = TreeName::checkout(name, &checkout)
            .map_err(|rejection| crate::server::invalid(&checkout, &rejection))?;
        Ok((tree, project_id))
    }

    /// The tree an init tried before is recorded on, and its project's id
    /// as the daemon's index holds it.
    async fn named_before(
        &self,
        known: &library_checkout::Known,
    ) -> Result<(TreeName, String), ApiError> {
        let tree = tree_of(known).ok_or_else(|| ApiError::Failed {
            detail: "an init's intent names no checkout tree".to_string(),
        })?;
        let TreeName::Checkout { project, .. } = &tree else {
            unreachable!("parse_checkout parses a checkout's tree")
        };
        let (_, project_id) = self.registered_project(project).await?;
        Ok((tree, project_id))
    }

    /// The tree `tree` (its `.ctxloom` at `dir`, its checkout's root
    /// `root`) created whole: its one `CreateDirectory` intended (unless
    /// `pending` holds it already, a retry's), made by one atomic
    /// operation, recorded done, then its baseline observed. What it says.
    async fn create_tree(
        &self,
        (tree, dir, root): (&TreeName, &str, &str),
        (verb, shape): (&str, &Shape),
        request_id: &str,
        pending: Option<(u32, Vec<library_tree::Step>)>,
    ) -> Result<Vec<v1::Notice>, ApiError> {
        let (observations, observers) =
            self.observing.as_ref().ok_or_else(|| ApiError::Failed {
                detail: "this daemon observes no library".to_string(),
            })?;
        let operations = observations.operations().ok_or_else(|| ApiError::Failed {
            detail: "this daemon makes no library write".to_string(),
        })?;
        let scanner = observers.scanner();
        let id = uuid::Uuid::parse_str(request_id).unwrap_or_else(|_| uuid::Uuid::now_v7());
        let request_id = id.to_string();
        let evidence = Evidence {
            tree_reads: true,
            plan_rejection: None,
            builtin: false,
            in_motion: None,
            in_git: true,
            ignored: None,
            root: root.to_string(),
        };
        let (start, steps) = match pending {
            Some(pending) => pending,
            None => {
                let intended = writes::intent_of(
                    &tree.to_string(),
                    verb,
                    shape,
                    (0, &|_: &str| 0),
                    evidence,
                    &request_id,
                );
                observations
                    .send(writes::command_book(tree, &intended, id.into_bytes()))
                    .await
                    .map_err(|failure| match failure {
                        angzarr_embedded::Failure::Rejected(rejected) => {
                            ApiError::Refused(ApiRefusal::rejected(&rejected))
                        }
                        other => ApiError::failed(other),
                    })?;
                (0, intended.steps(&request_id))
            }
        };
        self.dies_at("intended");
        let made = Made {
            tree,
            dir,
            request_id: &request_id,
        };
        for (at, step) in steps.iter().enumerate().skip(start as usize) {
            let at = u32::try_from(at).unwrap_or(u32::MAX);
            self.make_step(
                &made,
                scanner.as_ref(),
                operations.as_ref(),
                (at, step),
                shape,
            )
            .await?;
        }
        let project = match tree {
            TreeName::Checkout { project, .. } => project.clone(),
            TreeName::Home { .. } => unreachable!("an init's tree is a checkout's"),
        };
        let observed = observations
            .observe_created(scanner.as_ref(), &Site::checkout(project, root))
            .await
            .map_err(|error| match error {
                library_api::ReadError::Refused(rejection) => {
                    ApiError::Refused(crate::library_refusal(&rejection))
                }
                library_api::ReadError::Failed(failure) => ApiError::diagnosed(failure.as_ref()),
            })?;
        self.dies_at("done");
        Ok(observed.notices.iter().map(notice_of).collect())
    }
}

#[cfg(test)]
#[path = "init.test.rs"]
mod tests;
