//! The library's reads the daemon serves (step 10a U2; D12): each request
//! of `daemon_proto::library_reads!` names what it reads (a Get's `name`,
//! a List's `parent`) under `projects/{harp}` or `users/me`, and carries
//! the caller's directory, environment and global flags (E1). A read:
//!
//! 1. resolves its project, by its harp, to the project's registered root,
//!    and the directory it is read at: the caller's (`cwd`), else that
//!    root;
//! 2. finds the checkout there as a process in that directory would (a
//!    git worktree of the project is a checkout of its own, L2) and
//!    observes it and the home's library first (U2's reconcile: a tree
//!    outside git, or one git ignores, refuses the read);
//! 3. opens the library there in the caller's environment, under its
//!    globals, and answers by `library-api`'s use case of the read.
//!
//! What the root composes is [`Libraries`]: the library as a process at a
//! directory, in an environment, opens it.

use daemon_proto::v1;
use library_application::{AppError, Flags, Library, Outcome};
use run_ports::Edge;
use sessions::ProjectName;

use crate::refusal::{ApiError, ApiRefusal};
use crate::server::Api;

/// The library, as a process at a directory opens it (the root's).
pub trait Libraries: Send + Sync {
    /// The root of the checkout a process at `dir`, in the environment
    /// `env`, finds (its `.ctxloom`'s directory); none outside a project.
    fn root(&self, dir: &str, env: &[(String, String)]) -> Option<String>;

    /// `read` over the library a process at `dir`, in `env`, opens.
    fn read(
        &self,
        dir: &str,
        env: &[(String, String)],
        read: &mut dyn FnMut(&Library),
    ) -> Result<(), ApiError>;
}

/// What every library read request carries besides its own fields.
pub trait LibraryRequest {
    /// What it reads: a resource name, or a parent.
    fn target(&self) -> &str;
    fn cwd(&self) -> &str;
    fn env(&self) -> Option<&v1::EnvVars>;
    fn globals(&self) -> Option<&v1::Globals>;
}

macro_rules! library_requests {
    ($($method:ident ($rpc:ident) $request:ident . $target:ident => $response:ident),* $(,)?) => {
        $(
            impl LibraryRequest for v1::$request {
                fn target(&self) -> &str {
                    &self.$target
                }
                fn cwd(&self) -> &str {
                    &self.cwd
                }
                fn env(&self) -> Option<&v1::EnvVars> {
                    self.env.as_ref()
                }
                fn globals(&self) -> Option<&v1::Globals> {
                    self.globals.as_ref()
                }
            }
        )*
    };
}
daemon_proto::library_reads!(library_requests);

/// The parent a read's target is under: `projects/{harp}` (its project) or
/// `users/me` (none), the first two segments of its name.
pub(crate) fn parent_of(target: &str) -> Option<(String, Option<ProjectName>)> {
    let mut segments = target.splitn(3, '/');
    let parent = match (segments.next(), segments.next()) {
        (Some("users"), Some("me")) => return Some(("users/me".to_string(), None)),
        (Some("projects"), Some(harp)) if !harp.is_empty() => format!("projects/{harp}"),
        _ => return None,
    };
    let name = ProjectName::parse(&parent).ok()?;
    Some((parent, Some(name)))
}

/// The global flags of a read, as the library's use cases take them.
pub(crate) fn flags_of(globals: Option<&v1::Globals>) -> Flags {
    let globals = globals.cloned().unwrap_or_default();
    Flags {
        degraded: globals.degraded,
        config_set: globals.config_set,
        no_companions: globals.no_companions,
        trusted: false,
    }
}

/// A read's environment as name and value pairs.
fn pairs(env: Option<&v1::EnvVars>) -> Vec<(String, String)> {
    env.iter()
        .flat_map(|env| env.vars.iter())
        .map(|var| (var.name.clone(), var.value.clone()))
        .collect()
}

/// A use case's error as the read's answer: its refusal or failure.
pub(crate) fn read_error(error: &AppError) -> ApiError {
    crate::refusal::app_error(error)
}

impl<E: Edge> Api<E> {
    /// Answer `request` (step 10a U2): its project resolved, its trees
    /// observed, then `answer` over the library at its directory, under
    /// the parent it was asked under. Its notices are the observation's
    /// and the use case's.
    pub(crate) async fn library_read<Q: LibraryRequest, R>(
        &self,
        request: &Q,
        answer: impl FnOnce(&Library, &Flags, &str) -> Outcome<R>,
    ) -> Result<daemon_ports::Read<R>, ApiError> {
        let (parent, project) = parent_of(request.target()).ok_or_else(|| {
            ApiError::from(ApiRefusal::of(&v1::NameInvalid {
                name: request.target().to_string(),
                reason: "shape".to_string(),
            }))
        })?;
        let libraries = self.libraries.as_ref().ok_or_else(|| ApiError::Failed {
            detail: "this daemon reads no library".to_string(),
        })?;
        let env = pairs(request.env());
        let registered = match &project {
            Some(name) => Some(self.registered_root(name).await?),
            None => None,
        };
        let dir = match (request.cwd(), &registered) {
            (cwd, _) if !cwd.is_empty() => cwd.to_string(),
            (_, Some(root)) => root.clone(),
            (_, None) => String::new(),
        };
        let root = libraries.root(&dir, &env);
        let site = project.zip(root.as_deref().or(registered.as_deref()));
        let mut said = self.observed(site, request.env()).await?;
        let flags = flags_of(request.globals());
        let at = root.unwrap_or(dir);
        let mut answer = Some(answer);
        let mut outcome = None;
        libraries.read(&at, &env, &mut |library| {
            if let Some(answer) = answer.take() {
                outcome = Some(answer(library, &flags, &parent));
            }
        })?;
        let outcome = outcome.ok_or_else(|| ApiError::Failed {
            detail: "the library was not read".to_string(),
        })?;
        said.extend(
            outcome
                .notices
                .iter()
                .map(run_application::client::notice_of),
        );
        let answer = outcome.result.map_err(|error| read_error(&error))?;
        Ok(daemon_ports::Read {
            answer,
            notices: said,
        })
    }
}

#[cfg(test)]
#[path = "library_reads.test.rs"]
mod tests;
