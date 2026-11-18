//! A read's first step (step 10a U2): its tree observed, and what changed
//! recorded as the library domain's one fact, before the read opens the
//! library. A project's tree outside git, or one git ignores, is refused
//! before anything is minted or recorded (G1). The checkout is named at its
//! first read (L2), and what the record holds of it is read from the warm
//! `library-checkout` (U0), never folded per read.
//!
//! No lock is held across the recording: two reads that see the same thing
//! against the same state make one fact key, so the second is a DUPLICATE
//! (L4), and one made against a state another read has moved since records
//! only what it adds (the aggregate's rule). The kernel projects the
//! checkouts inline, so a read after another's recording reconciles against
//! it.

use std::sync::Arc;

use ctxloom_ports::{Failure, TreeError, TreeObserver};
use library_checkout::LibraryCheckouts;
use library_tree::{observation_book, LibraryTreeRejection, TreeName};
use run_ports::Edge;
use sessions::ProjectName;

use crate::{locate, reconcile, LibraryApiNotice};

/// Where a checkout's harp is kept (`<tree>/state/checkout`, out of git),
/// minted at its tree's first read and read back after.
pub trait CheckoutHarps: Send + Sync {
    /// The harp of the checkout whose library is the directory `tree`.
    fn harp(&self, tree: &str) -> Result<String, Failure>;
}

/// git as a read's caller runs it: on the caller's `PATH`, in the caller's
/// environment (protos.md §2), and the home whose library every read reads
/// beside its project's.
pub trait Observers: Send + Sync {
    /// The observer of a read whose caller's environment is `env`.
    fn observer(&self, env: &[(String, String)]) -> Box<dyn TreeObserver>;

    /// The home's library directory (its `.ctxloom`), when it has one.
    fn home_tree(&self) -> Option<String>;
}

/// The tree a read is of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Site {
    /// A project checkout's library: the project, the checkout's root (what
    /// the refusals name) and its `.ctxloom` there.
    Checkout {
        project: ProjectName,
        root: String,
        tree: String,
    },
    /// The home's library, its `.ctxloom`.
    Home { tree: String },
}

impl Site {
    /// The library of the checkout of `project` at `root`: its `.ctxloom`.
    pub fn checkout(project: ProjectName, root: &str) -> Site {
        let tree = format!("{}/.ctxloom", root.trim_end_matches(['/', '\\']));
        Site::Checkout {
            project,
            root: root.to_string(),
            tree,
        }
    }

    /// The tree's directory, its `.ctxloom`.
    pub fn tree(&self) -> &str {
        match self {
            Site::Checkout { tree, .. } | Site::Home { tree } => tree,
        }
    }
}

/// What the observation came to: the tree's name, and what the read is to
/// say beside its answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Observed {
    pub tree: TreeName,
    pub notices: Vec<LibraryApiNotice>,
    /// The git operation in progress there, if any (a write is refused
    /// `TreeInMotion` on it, L11).
    pub motion: Option<ctxloom_ports::Motion>,
}

/// What a write's observations came to: each tree observed, with what it
/// came to, and everything they are to say, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Trees {
    pub seen: Vec<(Site, Observed)>,
    pub notices: Vec<LibraryApiNotice>,
}

/// Why a read was not answered.
#[derive(Debug)]
pub enum ReadError {
    /// The library domain's refusal (exit 20).
    Refused(LibraryTreeRejection),
    /// git missing, too old or failing, or the record not written (exit 10).
    Failed(Failure),
}

impl From<TreeError> for ReadError {
    fn from(error: TreeError) -> ReadError {
        ReadError::Failed(error.into_failure())
    }
}

/// The observations of the daemon's reads, recorded through its edge.
pub struct Observations<E> {
    edge: Arc<E>,
    checkouts: LibraryCheckouts,
    harps: Arc<dyn CheckoutHarps>,
}

impl<E: Edge> Observations<E> {
    /// Recorded through `edge`, reconciled against `checkouts` (the
    /// kernel's, projected inline), each checkout named by `harps`.
    pub fn new(
        edge: Arc<E>,
        checkouts: LibraryCheckouts,
        harps: Arc<dyn CheckoutHarps>,
    ) -> Observations<E> {
        Observations {
            edge,
            checkouts,
            harps,
        }
    }

    /// Observe `site` through `observer` (git in the caller's environment),
    /// recording what changed since the record's state of it.
    pub async fn observe(
        &self,
        observer: &dyn TreeObserver,
        site: &Site,
    ) -> Result<Observed, ReadError> {
        if let Site::Checkout { root, .. } = site {
            if let Some(refused) = locate::refusal(observer, root)? {
                return Err(ReadError::Refused(refused));
            }
        }
        let harp = self.harps.harp(site.tree()).map_err(ReadError::Failed)?;
        let tree = match site {
            Site::Checkout { project, .. } => TreeName::checkout(project.clone(), &harp),
            Site::Home { .. } => TreeName::home(&harp),
        }
        .map_err(|rejection| ReadError::Failed(Box::new(rejection)))?;
        let held = {
            let checkouts = self.checkouts.checkouts();
            checkouts.tree(&tree.root()).cloned().unwrap_or_default()
        };
        let sight = reconcile::sight(observer, site.tree(), &tree, &held)?;
        if let Some(fact) = &sight.fact {
            self.edge
                .handle(observation_book(&tree, fact))
                .await
                .map_err(|failure| ReadError::Failed(Box::new(failure)))?;
        }
        let notices = sight
            .motion
            .map(|motion| LibraryApiNotice::TreeInMotion {
                motion,
                root: site.tree().to_string(),
            })
            .into_iter()
            .collect();
        Ok(Observed {
            tree,
            notices,
            motion: sight.motion,
        })
    }
}

impl<E: Edge> Observations<E> {
    /// A read's observations (U2): the project's checkout at `root`, then
    /// the home's library, each through git in the caller's environment
    /// `env`. A home whose library is in no work tree, or in one that
    /// ignores it, is made a repository of its own first (G2, the home
    /// step, said once: `library-api-notice-home-repository-made`). What
    /// both are to say, in order.
    pub async fn observe_read(
        &self,
        observers: &dyn Observers,
        env: &[(String, String)],
        project: Option<(ProjectName, &str)>,
    ) -> Result<Vec<LibraryApiNotice>, ReadError> {
        Ok(self.observe_trees(observers, env, project).await?.notices)
    }

    /// A write's observations (step 10b U3): [`Observations::observe_read`]'s,
    /// each tree with what it came to, in order: the project's checkout,
    /// then the home's library (when git holds it). The home step's notice
    /// rides the home's.
    pub async fn observe_trees(
        &self,
        observers: &dyn Observers,
        env: &[(String, String)],
        project: Option<(ProjectName, &str)>,
    ) -> Result<Trees, ReadError> {
        let observer = observers.observer(env);
        let mut seen = Vec::new();
        let mut notices = Vec::new();
        if let Some((project, root)) = project {
            let site = Site::checkout(project, root);
            let observed = self.observe(observer.as_ref(), &site).await?;
            notices.extend(observed.notices.iter().cloned());
            seen.push((site, observed));
        }
        if let Some(home) = observers.home_tree() {
            // The home step (G2, QUESTIONS.md B): the home's library made a
            // repository of ctxloom's own files at its first read, once.
            let made = library_application::ensure_home_repository(observer.as_ref(), &home)?;
            if made == library_application::HomeRepository::Made {
                notices.push(LibraryApiNotice::HomeRepositoryMade { path: home.clone() });
            }
            let held = observer.work_tree(&home)?.is_some()
                && observer.check_ignore(&home, "config.yaml")?.is_none();
            if held {
                let site = Site::Home { tree: home };
                let observed = self.observe(observer.as_ref(), &site).await?;
                notices.extend(observed.notices.iter().cloned());
                seen.push((site, observed));
            }
        }
        Ok(Trees { seen, notices })
    }

    /// The record's state of `tree` now (a default one for a tree never
    /// observed): what a write's token is read from.
    pub fn held(&self, tree: &TreeName) -> library_tree::Library {
        let checkouts = self.checkouts.checkouts();
        checkouts.tree(&tree.root()).cloned().unwrap_or_default()
    }

    /// The intent recorded under `request_id`, in any tree (step 10b U3).
    pub fn known(&self, request_id: &str) -> Option<library_checkout::Known> {
        self.checkouts.checkouts().intent(request_id).cloned()
    }

    /// `book` (a library command or fact) through the edge.
    pub async fn send(
        &self,
        book: angzarr_embedded::v1::CommandBook,
    ) -> Result<angzarr_embedded::Answered, angzarr_embedded::Failure<angzarr_embedded::Rejected>>
    {
        self.edge.handle(book).await
    }
}

#[cfg(test)]
#[path = "observe.test.rs"]
mod tests;
