//! What the daemon's destroy reaches (slice 2 step 6, owner 2026-11-12):
//! a session's project, by its directory (its session directories and
//! locks, `session-files` and `liveness`; its scratch worktrees,
//! `worktree-git` run in the caller's environment; its configuration, read
//! from its registered root as a command in it reads it), and the daemon's
//! ONE store, purged stream by stream (`sessions_application::purge`, head
//! guarded), and the atomic operations a removal's trash is made by
//! (content-fs's, O2). No second connection to the store is opened.

use std::path::PathBuf;
use std::sync::Arc;

use angzarr_embedded::angzarr_store_sqlite::SqliteStore;
use angzarr_embedded::v1::Uuid;
use angzarr_embedded::{StreamRef, Timestamp};
use config_files::ConfigFilesFs;
use daemon_api::{Answer, ApiError};
use file_system::Fs;
use isolation_application::{WorktreeUse, Worktrees};
use liveness::LockFiles;
use process_env::EnvSnapshot;
use reclaim_application::SweepConfig;
use session_files::SessionDirs;
use session_ports::SessionFiles;
use worktree_git::GitWorkspaces;

use crate::{SweepSettings, Wiring};

/// A project's places, over this process's filesystem, ctxloom's `home`
/// and the temporary root.
pub(crate) struct ProjectPlaces {
    pub(crate) fs: Arc<dyn Fs>,
    pub(crate) home: PathBuf,
    pub(crate) temp: PathBuf,
}

impl ProjectPlaces {
    /// The caller's environment `env` (S7), as a process at `dir` sees it.
    fn snapshot(&self, dir: &str, env: &[(String, String)]) -> Arc<EnvSnapshot> {
        Arc::new(super::caller_env(
            os::host().flavor(),
            &self.temp,
            (dir, None),
            pairs(env),
        ))
    }
}

impl daemon_api::Places for ProjectPlaces {
    fn files(&self, project_dir: &str) -> Box<dyn SessionFiles + Send> {
        Box::new(SessionDirs::new(
            Arc::clone(&self.fs),
            PathBuf::from(project_dir),
        ))
    }

    fn worktrees(
        &self,
        project_dir: &str,
        project_id: &str,
        env: &[(String, String)],
    ) -> Box<dyn WorktreeUse + Send> {
        let dir = PathBuf::from(project_dir);
        Box::new(Worktrees::new(
            GitWorkspaces::new(
                Arc::clone(&self.fs),
                dir.clone(),
                // Its checkouts are under the account's ctxloom home, by its
                // project's id (owner ruling 2026-11-12, Q1); none, and the
                // destroy would find none and never reclaim them.
                Some(isolation_ports::worktrees_root(&self.home, project_id)),
                self.snapshot(project_dir, env),
            ),
            LockFiles::new(Arc::clone(&self.fs), dir),
        ))
    }

    fn settings(&self, root: &str, env: &[(String, String)]) -> Box<dyn SweepConfig + Send> {
        // The project's layered configuration as a command run at its
        // registered root finds it, under this daemon's home.
        let wiring = super::caller_wiring(&self.home, &self.temp, (root, Some(root)), pairs(env));
        let mut env: Vec<(String, String)> = env
            .iter()
            .filter(|(name, _)| name != "CTXLOOM_ROOT")
            .cloned()
            .collect();
        env.push(("CTXLOOM_ROOT".to_string(), root.to_string()));
        Box::new(SweepSettings::new(
            Arc::new(ConfigFilesFs::new(
                Arc::clone(&self.fs),
                wiring.locate().1,
                &self.home,
            )),
            env,
        ))
    }

    fn now(&self) -> Timestamp {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp {
            seconds: i64::try_from(now.as_secs()).unwrap_or(i64::MAX),
            nanos: i32::try_from(now.subsec_nanos()).unwrap_or(0),
        }
    }

    fn operations(&self) -> Arc<dyn ctxloom_ports::Operations> {
        // A removal's trash is its checkout's own (O2, QUESTIONS 8), made
        // by the same atomic operations as a library write's.
        Arc::new(content_fs::TreeOperations::new(Arc::clone(&self.fs)))
    }
}

/// A tree's regenerable cache (step 10b U9, `clean`): cache-files' over
/// this process's filesystem, its `.ctxloom` the tree's directory.
pub(crate) struct TreeCaches(pub(crate) Arc<dyn Fs>);

impl daemon_api::Caches for TreeCaches {
    fn cache(&self, tree: &str) -> Arc<dyn ctxloom_ports::ProjectCache> {
        Arc::new(cache_files::CacheFiles::new(
            Arc::clone(&self.0),
            std::path::Path::new(tree),
        ))
    }
}

/// `env` as name and value pairs.
fn pairs(env: &[(String, String)]) -> impl Iterator<Item = (&str, &str)> {
    env.iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
}

/// git as a library read's caller runs it (step 10a U2, protos.md §2): on
/// the caller's `PATH`, in the caller's environment (git-cli scrubs its
/// repository variables), over this process's filesystem; and the home's
/// library, `<home>/.ctxloom`, when there is one.
pub(crate) struct CallerGits {
    pub(crate) fs: Arc<dyn Fs>,
    pub(crate) home: PathBuf,
    pub(crate) temp: PathBuf,
}

impl library_api::Observers for CallerGits {
    fn observer(&self, env: &[(String, String)]) -> Box<dyn ctxloom_ports::TreeObserver> {
        let home = self.home.display().to_string();
        let snapshot =
            super::caller_env(os::host().flavor(), &self.temp, (&home, None), pairs(env));
        Box::new(library_git::GitTrees::new(
            Arc::clone(&self.fs),
            Arc::new(snapshot),
        ))
    }

    fn home_tree(&self) -> Option<String> {
        let app = self.home.join(".ctxloom");
        let is_dir = self
            .fs
            .metadata(&app)
            .is_ok_and(|meta| meta.kind == file_system::Kind::Dir);
        is_dir.then(|| app.display().to_string())
    }
}

/// Each checkout's harp, in its tree's `state/checkout`: minted at the
/// tree's first read (L2), read back after.
pub(crate) struct TreeHarps(pub(crate) Arc<dyn Fs>);

impl library_api::CheckoutHarps for TreeHarps {
    fn harp(&self, tree: &str) -> Result<String, ctxloom_ports::Failure> {
        crate::checkout::mint_checkout_harp(
            self.0.as_ref(),
            std::path::Path::new(tree),
            crate::session_name,
        )
        .map_err(|e| Box::new(e) as ctxloom_ports::Failure)
    }
}

/// The daemon's one store, as the destroy purges it.
pub(crate) struct StorePurges(pub(crate) Arc<SqliteStore>);

impl daemon_api::Purges for StorePurges {
    fn purge<'a>(&'a self, domain: &'a str, root: Uuid) -> Answer<'a, u64> {
        Box::pin(async move {
            let root = uuid::Uuid::from_slice(&root.value).map_err(|e| ApiError::Failed {
                detail: e.to_string(),
            })?;
            sessions_application::purge(self.0.as_ref(), StreamRef::main(domain, root))
                .await
                .map_err(|e| ApiError::Failed {
                    detail: angzarr_diagnostic::Diagnostic::code(&e).to_string(),
                })
        })
    }
}

#[cfg(test)]
#[path = "places.test.rs"]
mod tests;

/// The library a read is answered from (step 10a U2, E1): opened as a
/// process at the caller's directory, in the caller's environment with
/// ctxloom's `home` as the platform's home variable, would open it, the
/// project's installed remote bundles in its catalog.
pub(crate) struct ProjectLibraries {
    pub(crate) home: PathBuf,
    pub(crate) temp: PathBuf,
}

impl ProjectLibraries {
    /// A process at `dir` in the caller's environment `env`.
    fn at(&self, dir: &str, env: &[(String, String)]) -> Wiring {
        super::caller_wiring(&self.home, &self.temp, (dir, None), pairs(env))
    }
}

impl daemon_api::Libraries for ProjectLibraries {
    fn root(&self, dir: &str, env: &[(String, String)]) -> Option<String> {
        if dir.is_empty() {
            return None;
        }
        let (_, app) = self.at(dir, env).locate();
        if !matches!(app.source, ctxloom_ports::Source::Project) {
            return None;
        }
        app.path.parent().map(|root| root.display().to_string())
    }

    fn read(
        &self,
        dir: &str,
        env: &[(String, String)],
        read: &mut dyn FnMut(&library_application::Library),
    ) -> Result<(), ApiError> {
        let failed = |e: &dyn angzarr_diagnostic::Diagnostic| ApiError::Failed {
            detail: e.code().to_string(),
        };
        let at = if dir.is_empty() {
            self.home.display().to_string()
        } else {
            dir.to_string()
        };
        let wiring = self.at(&at, env);
        let deps = std::rc::Rc::new(crate::commands::build_deps(&wiring).map_err(|e| failed(&e))?);
        let library = crate::library::build(&wiring, Some(deps)).map_err(|e| failed(&e))?;
        read(&library);
        Ok(())
    }

    fn read_deps(
        &self,
        dir: &str,
        env: &[(String, String)],
        read: &mut dyn FnMut(&deps_application::Deps),
    ) -> Result<(), ApiError> {
        let at = if dir.is_empty() {
            self.home.display().to_string()
        } else {
            dir.to_string()
        };
        let wiring = self.at(&at, env);
        let deps = crate::commands::build_deps(&wiring).map_err(|e| ApiError::Failed {
            detail: angzarr_diagnostic::Diagnostic::code(&e).to_string(),
        })?;
        read(&deps);
        Ok(())
    }
}

/// What trusting a project would allow (step 8 U1), read in the daemon as
/// a process at the project's REGISTERED root reads its configuration and
/// bundles: its own layer's project-only settings and its own `ctxloom`
/// bundle, with ctxloom's `home`.
pub(crate) struct ProjectTrusts {
    pub(crate) home: PathBuf,
    pub(crate) temp: PathBuf,
}

impl daemon_api::Trusts for ProjectTrusts {
    fn settings(&self, root: &str) -> Result<Vec<daemon_proto::v1::TrustSetting>, ApiError> {
        // Read as a process there would, with ctxloom's home and nothing
        // else of an environment.
        let wiring = super::caller_wiring(&self.home, &self.temp, (root, Some(root)), []);
        let failed = |e: &dyn angzarr_diagnostic::Diagnostic| ApiError::Failed {
            detail: e.code().to_string(),
        };
        let library = crate::library::build(&wiring, None).map_err(|e| failed(&e))?;
        let held = library.trust_settings().map_err(|e| failed(&e))?;
        Ok(held
            .into_iter()
            .map(|held| daemon_proto::v1::TrustSetting {
                path: held.path,
                value: held.value,
            })
            .collect())
    }
}

#[cfg(test)]
#[path = "reconcile_bench.test.rs"]
mod reconcile_bench;
