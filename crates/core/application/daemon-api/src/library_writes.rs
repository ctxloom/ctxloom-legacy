//! The library's writes the daemon serves (step 10b U3; protos.md §1.1).
//! A write is, in order:
//!
//! 1. its project resolved and its trees OBSERVED, as a read's are (a
//!    tree outside git, or one git ignores, refuses it);
//! 2. its request id looked up: a write its retry repeats (the same id) is
//!    answered as it was applied, or finished from the step it got to;
//! 3. PLANNED: the use case run over the library's recording ports, which
//!    write nothing (`library_application::write::recording`), and its plan
//!    shaped into the one step it is (`library_api::writes::shape`; more
//!    than one file is `OneChangePerCall`);
//! 4. INTENDED: the step's command (its path, git's ids for what it
//!    writes, the path's token, the evidence) sent under the request id, so
//!    the domain decides it and records `ChangeIntended`;
//! 5. ONE OPERATION: the plan performed through the real ports
//!    (`Library::perform`, the seam U8's atomic operations replace);
//! 6. RE-OBSERVED: what the step left read back and recorded
//!    (`RecordStepDone`), its `StepApplied` the paths' new etag;
//! 7. answered, with that etag.
//!
//! A preview (`validate_only`) is step 1 and the use case's preview alone:
//! nothing is intended or written, and its etag is the token of the path
//! its apply would change.

use std::sync::Arc;

use daemon_proto::v1;
use library_api::served_writes::Wrote;
use library_api::writes::{self, Lands};
use library_api::{Observed, Site};
use library_application::{Flags, Library, Outcome};
use library_checkout::End;
use library_tree::{Evidence, RecordStepDone, RecordStepFailed, TreeName};
use run_ports::Edge;

use crate::library_reads::read_error;
use crate::refusal::{ApiError, ApiRefusal};
use crate::server::Api;

/// What every library write request carries besides its own fields.
pub trait LibraryWriteRequest {
    /// What it writes: a resource name, or a parent.
    fn target(&self) -> &str;
    /// The request, writing `target` (a client names its parent so).
    fn set_target(&mut self, target: String);
    /// The request, carrying the caller's environment (S7, E1).
    fn set_env(&mut self, env: v1::EnvVars);
    /// The request, at the caller's working directory (E1, extended to the
    /// writes): the daemon resolves the project and its checkout from it.
    fn set_cwd(&mut self, cwd: String);
    /// The caller's working directory; empty, the project's registered root.
    fn cwd(&self) -> &str;
    /// The request, under the request id `id` (AIP-155), where it names none.
    fn set_request_id(&mut self, id: String);
    fn env(&self) -> Option<&v1::EnvVars>;
    fn globals(&self) -> Option<&v1::Globals>;
    /// The client's id for the write (AIP-155): the command's id.
    fn request_id(&self) -> &str;
    fn validate_only(&self) -> bool;
}

/// A test-support point of a write the daemon may be killed at (the
/// `test-die-at` switch): the wiring's, called with the point's word.
pub type DiesAt = Arc<dyn Fn(&str) + Send + Sync>;

/// The tree a write lands in, as it was observed.
struct Landing<'a> {
    site: &'a Site,
    observed: &'a Observed,
}

impl Landing<'_> {
    /// The root a refusal names: a checkout's root, the home's library.
    fn root(&self) -> String {
        match self.site {
            Site::Checkout { root, .. } => root.clone(),
            Site::Home { tree } => tree.clone(),
        }
    }
}

/// The request id as the command id's bytes: the client's UUID, or (a
/// client that named none) one minted for this call alone.
fn request_bytes(request_id: &str) -> [u8; 16] {
    uuid::Uuid::parse_str(request_id)
        .unwrap_or_else(|_| uuid::Uuid::now_v7())
        .into_bytes()
}

fn notice_of(notice: &library_api::LibraryApiNotice) -> v1::Notice {
    v1::Notice {
        code: angzarr_diagnostic::Diagnostic::code(notice).to_string(),
        args: run_application::client::flat_args(notice),
        text: notice.to_string(),
    }
}

fn refused(rejection: &library_tree::LibraryTreeRejection) -> ApiError {
    ApiError::Refused(crate::library_refusal(rejection))
}

/// What a use case planned: its outcome, and its recorded writes shaped.
struct Planned<T> {
    outcome: Outcome<T>,
    shaped: Result<Option<writes::Shaped>, library_tree::LibraryTreeRejection>,
}

impl<E: Edge> Api<E> {
    /// Serve the write `request`, its use case `plan` (given whether it is
    /// to apply: false for a preview), recorded as `verb` (step 10b U3).
    pub(crate) async fn library_write<Q: LibraryWriteRequest, T>(
        &self,
        request: &Q,
        verb: &str,
        plan: impl Fn(&Library, &Flags, bool) -> Outcome<T>,
    ) -> Result<Wrote<T>, ApiError> {
        self.open()?;
        let (site, trees) = self.write_site(request).await?;
        let (observations, _) = self.observing.as_ref().ok_or_else(|| ApiError::Failed {
            detail: "this daemon observes no library".to_string(),
        })?;
        let mut notices: Vec<v1::Notice> = trees.notices.iter().map(notice_of).collect();
        let request_id = request.request_id();
        let known = (!request_id.is_empty())
            .then(|| observations.known(request_id))
            .flatten();
        if let Some(known) = &known {
            match known.end {
                End::Done => {
                    let (etag, at) = done_before(&trees.seen, known, observations);
                    return Ok(Wrote {
                        answer: None,
                        applied: true,
                        etag,
                        at,
                        parent: site.parent.clone(),
                        notices,
                    });
                }
                End::Failed | End::Superseded => {
                    return Err(ApiError::Failed {
                        detail: format!(
                            "the write {request_id} was tried before and did not complete"
                        ),
                    });
                }
                End::Pending { .. } => {}
            }
        }
        let apply = !request.validate_only();
        let (answer, shaped) = {
            let Planned { outcome, shaped } = self.planned(&site, apply, &plan)?;
            notices.extend(
                outcome
                    .notices
                    .iter()
                    .map(run_application::client::notice_of),
            );
            (outcome.result.map_err(|error| read_error(&error)), shaped)
        };
        let answer = answer?;
        let shaped = match shaped {
            Ok(shaped) => shaped,
            Err(rejection) => return Err(refused(&rejection)),
        };
        let Some(shaped) = shaped else {
            // A preview, or a write that changes nothing: nothing to intend.
            return Ok(Wrote {
                answer: Some(answer),
                applied: false,
                etag: String::new(),
                at: String::new(),
                parent: site.parent.clone(),
                notices,
            });
        };
        let landing = landing_of(&trees.seen, shaped.lands).ok_or_else(|| ApiError::Failed {
            detail: "the tree this write lands in was not observed".to_string(),
        })?;
        let path = shaped.shape.path().to_string();
        let held = observations.held(&landing.observed.tree);
        let at = format!("{}/{path}", landing.site.tree());
        if !apply {
            return Ok(Wrote {
                answer: Some(answer),
                applied: false,
                etag: held.token(&path).to_string(),
                at,
                parent: site.parent.clone(),
                notices,
            });
        }
        let pending = known.as_ref().and_then(|known| match known.end {
            End::Pending { done } => Some(done),
            _ => None,
        });
        let etag = self
            .carry_out(&site, &landing, verb, &shaped, (request_id, pending))
            .await?;
        Ok(Wrote {
            answer: Some(answer),
            applied: true,
            etag,
            at,
            parent: site.parent.clone(),
            notices,
        })
    }

    /// Where a write is made, and its trees observed (a tree outside git,
    /// or one git ignores, refuses it).
    async fn write_site<Q: LibraryWriteRequest>(
        &self,
        request: &Q,
    ) -> Result<(crate::library_reads::ReadSite<'_>, library_api::Trees), ApiError> {
        let (site, project) = self
            .resolve(
                request.target(),
                request.cwd(),
                request.env(),
                request.globals(),
            )
            .await?;
        let (observations, observers) =
            self.observing.as_ref().ok_or_else(|| ApiError::Failed {
                detail: "this daemon observes no library".to_string(),
            })?;
        let trees = observations
            .observe_trees(
                observers.as_ref(),
                &site.env,
                project
                    .as_ref()
                    .map(|(name, root)| (name.clone(), root.as_str())),
            )
            .await
            .map_err(|error| match error {
                library_api::ReadError::Refused(rejection) => refused(&rejection),
                library_api::ReadError::Failed(failure) => ApiError::diagnosed(failure.as_ref()),
            })?;
        Ok((site, trees))
    }

    /// The write `shaped` intended under `request_id` (unless its intent
    /// is recorded already: `pending`, the step it got to), made, and its
    /// step recorded done: the path's new token.
    async fn carry_out(
        &self,
        site: &crate::library_reads::ReadSite<'_>,
        landing: &Landing<'_>,
        verb: &str,
        shaped: &writes::Shaped,
        (request_id, pending): (&str, Option<u32>),
    ) -> Result<String, ApiError> {
        let (observations, observers) =
            self.observing.as_ref().ok_or_else(|| ApiError::Failed {
                detail: "this daemon observes no library".to_string(),
            })?;
        let path = shaped.shape.path().to_string();
        let held = observations.held(&landing.observed.tree);
        let observer = observers.observer(&site.env);
        let evidence = Evidence {
            tree_reads: true,
            plan_rejection: None,
            builtin: false,
            in_motion: landing
                .observed
                .motion
                .map(library_api::reconcile::motion_of),
            in_git: true,
            ignored: None,
            root: landing.root(),
        };
        let intended = writes::intent_of(
            observer.as_ref(),
            landing.site.tree(),
            &landing.observed.tree.to_string(),
            verb,
            &shaped.shape,
            held.token(&path),
            evidence,
        )
        .map_err(|error| ApiError::diagnosed(error.into_failure().as_ref()))?;
        let id = request_bytes(request_id);
        let request_id = uuid::Uuid::from_bytes(id).to_string();
        let step = match pending {
            Some(done) => done,
            None => {
                let book = writes::command_book(&landing.observed.tree, &intended, id);
                observations
                    .send(book)
                    .await
                    .map_err(|failure| match failure {
                        angzarr_embedded::Failure::Rejected(rejected) => {
                            ApiError::Refused(ApiRefusal::rejected(&rejected))
                        }
                        other => ApiError::failed(other),
                    })?;
                observations
                    .known(&request_id)
                    .and_then(|known| match known.end {
                        End::Pending { done } => Some(done),
                        _ => None,
                    })
                    .unwrap_or(0)
            }
        };
        self.dies_at("intended");
        let tree = &landing.observed.tree;
        let performed = self.perform(site, &shaped.planned);
        let made = match performed {
            Ok(made) => made,
            Err(failure) => {
                self.step_failed(tree, &request_id, step, &failure).await?;
                return Err(failure);
            }
        };
        if !made {
            let exists = ApiError::Refused(crate::library_refusal(
                &library_tree::LibraryTreeRejection::ResourceExists { path: path.clone() },
            ));
            self.step_failed(tree, &request_id, step, &exists).await?;
            return Err(exists);
        }
        self.dies_at("renamed");
        let observed = writes::left_by(
            observer.as_ref(),
            landing.site.tree(),
            &writes::expected(&intended, &shaped.shape),
        )
        .map_err(|error| ApiError::diagnosed(error.into_failure().as_ref()))?;
        let fact = RecordStepDone {
            request_id: request_id.clone(),
            step,
            observed,
        };
        let key = format!("done:{request_id}:{step}");
        observations
            .send(writes::step_book(tree, &fact, &key))
            .await
            .map_err(ApiError::failed)?;
        self.dies_at("done");
        Ok(observations.held(tree).token(&path).to_string())
    }

    /// The use case planned over the recording library at `site`: its
    /// outcome, and what it would write shaped into its one step. A
    /// preview's etag is its apply's path's, so its apply is planned too
    /// (recorded, never performed) to name that path.
    fn planned<T>(
        &self,
        site: &crate::library_reads::ReadSite<'_>,
        apply: bool,
        plan: &impl Fn(&Library, &Flags, bool) -> Outcome<T>,
    ) -> Result<Planned<T>, ApiError> {
        let mut planned = None;
        site.libraries.read(&site.at, &site.env, &mut |library| {
            if planned.is_some() {
                return;
            }
            let (recording, recorder) = library.clone().recording();
            let outcome = plan(&recording, &site.flags, apply);
            let mut written = recorder.take();
            if !apply && outcome.result.is_ok() {
                let (recording, recorder) = library.clone().recording();
                let _ = plan(&recording, &site.flags, true);
                written = recorder.take();
            }
            let shaped = writes::shape(written, |bundle, dir| library.bundle_files(bundle, dir));
            planned = Some(Planned { outcome, shaped });
        })?;
        planned.ok_or_else(|| ApiError::Failed {
            detail: "the library was not read".to_string(),
        })
    }

    /// The write's one operation (step 10b U3; U8 makes it atomic): false
    /// when the new directory it makes is there by now.
    fn perform(
        &self,
        site: &crate::library_reads::ReadSite<'_>,
        planned: &library_application::write::recording::Planned,
    ) -> Result<bool, ApiError> {
        let mut made = None;
        site.libraries.read(&site.at, &site.env, &mut |library| {
            if made.is_none() {
                made = Some(library.perform(planned));
            }
        })?;
        match made {
            Some(Ok(made)) => Ok(made),
            Some(Err(failure)) => Err(ApiError::diagnosed(failure.as_ref())),
            None => Err(ApiError::Failed {
                detail: "the library was not read".to_string(),
            }),
        }
    }

    /// The step `step` of the write `request_id` failed (final): recorded,
    /// so no intent stays owed.
    async fn step_failed(
        &self,
        tree: &TreeName,
        request_id: &str,
        step: u32,
        why: &ApiError,
    ) -> Result<(), ApiError> {
        let Some((observations, _)) = &self.observing else {
            return Ok(());
        };
        let fact = RecordStepFailed {
            request_id: request_id.to_string(),
            step,
            ext_err_txt: format!("{why:?}"),
        };
        let key = format!("failed:{request_id}:{step}");
        observations
            .send(writes::step_book(tree, &fact, &key))
            .await
            .map(|_| ())
            .map_err(ApiError::failed)
    }

    /// The test-support kill point `point` of a write, when the wiring
    /// set one.
    fn dies_at(&self, point: &str) {
        if let Some(dies) = &self.write_dies_at {
            dies(point);
        }
    }
}

/// The token and the place of the path a write already done changed (its
/// retry's answer): from its tree as observed now.
fn done_before(
    seen: &[(Site, Observed)],
    known: &library_checkout::Known,
    observations: &library_api::Observations<impl Edge>,
) -> (String, String) {
    let landing = seen
        .iter()
        .find(|(_, observed)| observed.tree.root().value.to_vec() == known.root);
    landing.map_or_else(Default::default, |(site, observed)| {
        let path = known
            .intent
            .steps
            .first()
            .map(step_path)
            .unwrap_or_default();
        let token = observations.held(&observed.tree).token(&path);
        (token.to_string(), format!("{}/{path}", site.tree()))
    })
}

/// The path a recorded step changes.
fn step_path(step: &library_tree::Step) -> String {
    if let Some(replace) = &step.replace {
        return replace.path.clone();
    }
    if let Some(create) = &step.create {
        return create.path.clone();
    }
    if let Some(trash) = &step.trash {
        return trash.path.clone();
    }
    step.r#move
        .as_ref()
        .map(|moved| moved.from.clone())
        .unwrap_or_default()
}

/// The observed tree a write landing `lands` changes: the content's is the
/// project's checkout (else the home's), a layer's its scope's.
fn landing_of(seen: &[(Site, Observed)], lands: Lands) -> Option<Landing<'_>> {
    let checkout = || {
        seen.iter()
            .find(|(site, _)| matches!(site, Site::Checkout { .. }))
    };
    let home = || {
        seen.iter()
            .find(|(site, _)| matches!(site, Site::Home { .. }))
    };
    let found = match lands {
        Lands::Content => checkout().or_else(home),
        Lands::Layer(writes::Scope::Project) => checkout(),
        Lands::Layer(writes::Scope::Home) => home(),
    };
    found.map(|(site, observed)| Landing { site, observed })
}

/// Each write request's target, as its message names it.
macro_rules! write_requests {
    ($($request:ident . $($field:ident).+ ;)*) => {
        $(
            impl LibraryWriteRequest for v1::$request {
                fn target(&self) -> &str {
                    write_requests!(@get self $($field).+)
                }
                fn set_target(&mut self, target: String) {
                    write_requests!(@set self target $($field).+)
                }
                fn env(&self) -> Option<&v1::EnvVars> {
                    self.env.as_ref()
                }
                fn set_env(&mut self, env: v1::EnvVars) {
                    self.env = Some(env);
                }
                fn set_cwd(&mut self, cwd: String) {
                    self.cwd = cwd;
                }
                fn cwd(&self) -> &str {
                    &self.cwd
                }
                fn set_request_id(&mut self, id: String) {
                    self.request_id = id;
                }
                fn globals(&self) -> Option<&v1::Globals> {
                    self.globals.as_ref()
                }
                fn request_id(&self) -> &str {
                    &self.request_id
                }
                fn validate_only(&self) -> bool {
                    self.validate_only
                }
            }
        )*
    };
    (@get $self:ident $field:ident) => { &$self.$field };
    (@get $self:ident $outer:ident . $inner:ident) => {
        $self.$outer.as_ref().map_or("", |outer| outer.$inner.as_str())
    };
    (@set $self:ident $target:ident $field:ident) => { $self.$field = $target };
    (@set $self:ident $target:ident $outer:ident . $inner:ident) => {
        $self.$outer.get_or_insert_with(Default::default).$inner = $target
    };
}

write_requests! {
    CreateBundleRequest.parent;
    CreateFragmentRequest.parent;
    UpdateFragmentRequest.fragment.name;
    CreateCommandRequest.parent;
    UpdateCommandRequest.command.name;
    UpdateConfigRequest.name;
    CreateSkillRequest.parent;
    DeleteBundleRequest.name;
    DeleteFragmentRequest.name;
    DeleteCommandRequest.name;
    DeleteSkillRequest.name;
    DeleteProfileRequest.name;
    DeleteAgentRequest.name;
    DeleteCompanionRequest.name;
}

#[cfg(test)]
#[path = "library_writes.test.rs"]
mod tests;
