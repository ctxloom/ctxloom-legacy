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
        Ok(Observed { tree, notices })
    }
}

#[cfg(test)]
#[path = "observe.test.rs"]
mod tests;
