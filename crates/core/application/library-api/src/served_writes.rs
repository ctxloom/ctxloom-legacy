//! The library's writes as the daemon answers them (step 10b U3): each the
//! use case it plans with, and its answer as the API's message. The write
//! itself (observed, intended, made, recorded) is the daemon's; here is
//! what each verb asks of the library and how what it did is said, so
//! the CLI renders its text from the message alone (D10).
//!
//! Each verb is `plan(library, flags, request, apply)`, its use case over
//! the recording library (`apply` false: its preview), and
//! `respond(request, wrote)`, its response; one table,
//! [`served_writes!`](crate::served_writes), lists what the daemon serves
//! with the verb its intent is recorded under.

use daemon_proto::v1;
use library_application::{BundleCreated, Flags, Library, Outcome};

use crate::names;

/// What a write came to (the daemon's `library_write`).
#[derive(Clone, Debug, PartialEq)]
pub struct Wrote<T> {
    /// The use case's answer: a preview's, or a write's planned in this
    /// call; none for the retry of a write already applied.
    pub answer: Option<T>,
    /// Whether the write is applied (by this call or the one it repeats).
    pub applied: bool,
    /// The token of the path the write changes (or, on a preview, would):
    /// the etag a confirmed apply sends back (AIP-154).
    pub etag: String,
    /// The path the write changes, where it is on disk; empty when nothing
    /// was to change.
    pub at: String,
    /// The parent its resources are named under (`projects/{harp}`,
    /// `users/me`).
    pub parent: String,
    /// What the observation and the use case said, in order.
    pub notices: Vec<v1::Notice>,
}

/// The library writes the daemon serves (step 10b U3, D12), in the shape
/// of `daemon_proto::library_writes!`'s rows with each one's verb, handed
/// to `$each`: `served_writes::$method::{plan, respond}` answer it.
#[macro_export]
macro_rules! served_writes {
    ($each:ident) => {
        $each! {
            create_bundle (CreateBundle) CreateBundleRequest => CreateBundleResponse, "bundle create";
        }
    };
}

/// The bundle its catalog name names, by its resource name alone.
fn just_named(parent: &str, bundle: &str) -> Option<v1::Bundle> {
    Some(v1::Bundle {
        name: names::bundle_name(parent, bundle),
        ..v1::Bundle::default()
    })
}

/// `bundle create <name> [-d description]`: `CreateBundle`.
pub mod create_bundle {
    use super::*;

    pub fn plan(
        library: &Library,
        flags: &Flags,
        request: &v1::CreateBundleRequest,
        _apply: bool,
    ) -> Outcome<BundleCreated> {
        let description = request
            .bundle
            .as_ref()
            .map_or("", |bundle| bundle.description.as_str());
        library.bundle_create(flags, &request.bundle_id, description)
    }

    pub fn respond(
        request: &v1::CreateBundleRequest,
        wrote: Wrote<BundleCreated>,
    ) -> v1::CreateBundleResponse {
        let path = match &wrote.answer {
            Some(made) => made.path.clone(),
            None => format!("{}/bundle.yaml", wrote.at),
        };
        v1::CreateBundleResponse {
            bundle: just_named(&wrote.parent, &request.bundle_id),
            path,
            applied: Some(wrote.applied),
            etag: wrote.etag,
            notices: wrote.notices,
            ..v1::CreateBundleResponse::default()
        }
    }
}

#[cfg(test)]
#[path = "served_writes.test.rs"]
mod tests;
