//! An observation's fact key (L4, unchanged by G4's supersession): a key of
//! ids alone, chained on the last cause so that A→B→A→B never repeats one.
//!
//! `obs:{tree}:{H(last_cause)}:{head}:{H(sorted dirty)}`, H the SHA-256 in
//! hex, `sorted dirty` each `path NUL oid NL` line in path order. Two reads
//! that see the same thing against the same state give one key, so the
//! second is a DUPLICATE. No sequence enters it: a fact carries none.

use angzarr_embedded::markup::DomainMessage;
use angzarr_embedded::v1::{command_page, CommandBook, CommandPage, Cover, Fact, FactOrigin};
use sha2::{Digest, Sha256};

use crate::{DirtyPath, RecordTreeObserved, TreeName, LIBRARY_DOMAIN};

/// The external id of an observation of `tree` (its name) made against the
/// state whose last cause is `last_cause`, seeing `head` and `dirty`.
pub fn observation_key(tree: &str, last_cause: &str, head: &str, dirty: &[DirtyPath]) -> String {
    let mut sorted: Vec<&DirtyPath> = dirty.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut lines = Sha256::new();
    for path in sorted {
        lines.update(path.path.as_bytes());
        lines.update([0]);
        lines.update(path.oid.as_bytes());
        lines.update([b'\n']);
    }
    format!(
        "obs:{tree}:{}:{head}:{}",
        hex::encode(Sha256::digest(last_cause.as_bytes())),
        hex::encode(lines.finalize()),
    )
}

/// The book that records `fact`, an observation of `tree`: a Fact on the
/// tree's stream, its origin keyed by [`observation_key`] over the cause
/// the observer read (`base_cause`), the head and the dirty set it saw.
pub fn observation_book(tree: &TreeName, fact: &RecordTreeObserved) -> CommandBook {
    let external_id = observation_key(
        &tree.to_string(),
        &fact.base_cause,
        &fact.head_after,
        &fact.dirty_after,
    );
    CommandBook {
        cover: Some(Cover {
            domain: LIBRARY_DOMAIN.to_string(),
            root: Some(tree.root()),
            edition: None,
        }),
        page: Some(CommandPage {
            body: Some(command_page::Body::Fact(Fact {
                payload: Some(
                    angzarr_embedded::Any::from_msg(&fact.clone().to_wire())
                        .expect("an observation encodes"),
                ),
                origin: Some(FactOrigin {
                    external_id,
                    description: "a read of the library tree".to_string(),
                }),
            })),
            ..CommandPage::default()
        }),
    }
}

#[cfg(test)]
#[path = "key.test.rs"]
mod tests;
