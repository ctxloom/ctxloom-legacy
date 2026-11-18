//! The library checkouts (step 10 U0): a read model a projector folds from
//! the library streams, one row per tree (`projects/{p}/checkouts/{c}/
//! library`, `users/me/library`): the commit, the library's tree id and
//! the dirty paths last observed, the last cause, the pending intents, and
//! each resource's `{last sequence, oid}` with the baseline's sequence (G4
//! superseded). It is what a read reconciles against without folding the
//! stream on every read (U2), and what answers each resource's etag. It
//! folds with the domain's own fold ([`library_tree::Library::apply`]), so
//! its token is the one `decide` judges. It holds no content and writes no
//! file (87a).
//!
//! It is HELD in memory, like `trust-index`: registered in the daemon's
//! kernel ([`LibraryCheckouts`] on the projector port, S32) and caught up
//! from the store by the daemon's loop ([`caught_up`]), so a tree a daemon
//! before this one recorded is held too.

use std::collections::BTreeMap;
use std::sync::Arc;

use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
use angzarr_embedded::v1::{event_page, Cover, EventBook};
use angzarr_embedded::{Any, CatchUp, Payload, PayloadError, ProjectorHandler};
use held_projection::{Fill, Held, Lock, Model};
use library_tree::{Library, LibraryEvent, LIBRARY_DOMAIN};

/// The checkouts.
#[derive(Default)]
pub struct LibraryCheckout {
    /// Each tree stream's state, by its root.
    rows: BTreeMap<Vec<u8>, Library>,
    /// Every intent recorded, by its request id (step 10b U3): what a
    /// write's retry finds, whether it is done or not.
    intents: BTreeMap<String, Known>,
}

/// An intent as the record holds it, by its request id: its tree's root,
/// the intent as recorded, and how it ended.
#[derive(Clone, Debug, PartialEq)]
pub struct Known {
    pub root: Vec<u8>,
    pub intent: library_tree::Intent,
    pub end: End,
}

/// Where an intent is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// Steps owed: the next is `done`.
    Pending { done: u32 },
    /// Every step done.
    Done,
    /// A step failed (final).
    Failed,
    /// Superseded before it was done.
    Superseded,
}

fn root_of(cover: Option<&Cover>) -> Vec<u8> {
    cover
        .and_then(|cover| cover.root.as_ref())
        .map(|root| root.value.to_vec())
        .unwrap_or_default()
}

impl LibraryCheckout {
    /// An empty model.
    pub fn new() -> LibraryCheckout {
        LibraryCheckout::default()
    }

    /// Fold `book`, one stream's events after the last projected of it: a
    /// book from sequence 0 starts the stream afresh; a book with no event
    /// and `next_sequence` 0 says the stream is gone, and it leaves the
    /// model. Another domain's book is none of its.
    pub fn project(&mut self, book: &EventBook) -> Result<(), PayloadError> {
        if book.cover.as_ref().map(|c| c.domain.as_str()) != Some(LIBRARY_DOMAIN) {
            return Ok(());
        }
        let root = root_of(book.cover.as_ref());
        if book.pages.first().is_none_or(|page| page.sequence == 0) {
            self.rows.remove(&root);
            self.intents.retain(|_, known| known.root != root);
        }
        for page in &book.pages {
            let Some(event_page::Body::Event(event)) = &page.body else {
                continue;
            };
            let event = LibraryEvent::from_any(event)?;
            let row = self.rows.entry(root.clone()).or_default();
            row.apply_page(&event, page.sequence, page.meta.as_ref());
            let pending = row.pending.clone();
            self.track(&root, &event, &pending);
        }
        Ok(())
    }

    /// The intents `event` names, kept by their request ids; `pending` the
    /// tree's after it.
    fn track(
        &mut self,
        root: &[u8],
        event: &LibraryEvent,
        pending: &BTreeMap<String, library_tree::Intent>,
    ) {
        let (request, end) = match event {
            LibraryEvent::ChangeIntended(intended) => {
                let intent = intended.intent.clone();
                self.intents.insert(
                    intent.request_id.clone(),
                    Known {
                        root: root.to_vec(),
                        intent,
                        end: End::Pending { done: 0 },
                    },
                );
                return;
            }
            LibraryEvent::StepApplied(applied) => (&applied.request_id, None),
            LibraryEvent::TreeObserved(observed) => match &observed.intended {
                Some(request) => (request, None),
                None => return,
            },
            LibraryEvent::ChangeFailed(failed) => (&failed.request_id, Some(End::Failed)),
            LibraryEvent::IntentSuperseded(superseded) => {
                (&superseded.request_id, Some(End::Superseded))
            }
        };
        let Some(known) = self.intents.get_mut(request) else {
            return;
        };
        known.end = end.unwrap_or_else(|| match pending.get(request) {
            Some(intent) => End::Pending { done: intent.done },
            None => End::Done,
        });
    }

    /// The intent recorded under `request_id`, in any tree.
    pub fn intent(&self, request_id: &str) -> Option<&Known> {
        self.intents.get(request_id)
    }

    /// The tree whose stream root is `root`, if any event of it was folded.
    pub fn tree(&self, root: &angzarr_embedded::v1::Uuid) -> Option<&Library> {
        self.rows.get(&root.value.to_vec())
    }

    /// The token (etag) of `path` in the tree rooted at `root`: its last
    /// sequence, a directory's the latest under it, else the baseline's;
    /// none for a tree never observed.
    pub fn token(&self, root: &angzarr_embedded::v1::Uuid, path: &str) -> Option<u64> {
        self.tree(root).map(|tree| tree.token(path))
    }
}

impl Model for LibraryCheckout {
    const NAME: &'static str = "library-checkout";

    fn project(&mut self, book: &EventBook) -> Result<(), PayloadError> {
        LibraryCheckout::project(self, book)
    }

    /// The stream's tree, as the domain's state message.
    fn view(&self, cover: &Cover) -> Option<Any> {
        let tree = self.rows.get(&root_of(Some(cover)))?;
        Any::from_msg(&tree.wire()).ok()
    }
}

/// The model on angzarr-embedded's projector port: every clone is the same
/// fold ([`Held`]), so one instance can be both a catch-up's and the
/// kernel's.
#[derive(Clone, Default)]
pub struct LibraryCheckouts(Held<LibraryCheckout>);

impl LibraryCheckouts {
    /// An empty model whose gaps are read through `fill`.
    pub fn filled(fill: Arc<dyn Fill>) -> LibraryCheckouts {
        LibraryCheckouts(Held::new(Some(fill)))
    }

    /// The model, locked.
    pub fn checkouts(&self) -> Lock<'_, LibraryCheckout> {
        self.0.lock()
    }
}

impl ProjectorHandler for LibraryCheckouts {
    fn name(&self) -> &str {
        self.0.name()
    }

    async fn dispatch(&self, request: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        self.0.dispatch(request).await
    }
}

/// `model` caught up from a store: its catch-up, sharing its fold (S32).
pub fn caught_up(model: LibraryCheckouts) -> CatchUp<LibraryCheckouts> {
    CatchUp::new(&SOURCES, model).expect("the library checkouts read the library domain")
}

/// The domains the model folds.
pub const SOURCES: [&str; 1] = [LIBRARY_DOMAIN];

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
