//! A served read's message read back as the view its text is drawn from
//! (step 10a U2, D10: the text is rendered from the message alone): each
//! function takes what the daemon answered and gives the view the text
//! renderer in [`text`](crate::text) prints, line for line as it printed
//! the view the use case answered in process.

use daemon_proto::v1;
use library_application::{DistilledState, ItemList, ItemRow, ItemShow, PremiseRow, Premises};

/// The parent a resource name is under: its first two segments,
/// `projects/{p}` or `users/me`.
pub(crate) fn parent_of(name: &str) -> &str {
    match name.match_indices('/').nth(1) {
        Some((offset, _)) => &name[..offset],
        None => name,
    }
}

/// An item's bundle and its own name, as its reference
/// (`<bundle>#<kind>/<name>`) says them.
fn reference_parts(reference: &str) -> (String, String) {
    let (bundle, path) = reference.split_once('#').unwrap_or((reference, ""));
    let id = path.rsplit_once('/').map_or(path, |(_, id)| id);
    (bundle.to_string(), id.to_string())
}

/// A distilled form's state as the message says it.
fn state_of(state: i32) -> DistilledState {
    match v1::DistilledState::try_from(state) {
        Ok(v1::DistilledState::Fresh) => DistilledState::Fresh,
        Ok(v1::DistilledState::Stale) => DistilledState::Stale,
        _ => DistilledState::None,
    }
}

/// The hint an empty listing ends with: a project's, or none's.
pub(crate) fn hint(in_project: bool) -> String {
    let hint = if in_project {
        library_application::LISTING_HINT
    } else {
        library_application::NO_PROJECT_HINT
    };
    hint.to_string()
}

/// What a `fragment|command list` asked and was answered: each item's
/// reference and tags, the bundle it was filtered to, how many items the
/// catalog holds of every bundle, and whether it is a project's.
pub(crate) struct Listed<'a> {
    pub(crate) kind: &'a str,
    pub(crate) items: Vec<(&'a str, &'a [String])>,
    pub(crate) bundle_filter: &'a str,
    pub(crate) total: usize,
    pub(crate) in_project: bool,
}

/// A `fragment|command list` as its view.
pub(crate) fn item_list(listed: &Listed<'_>) -> ItemList {
    let rows = listed
        .items
        .iter()
        .map(|(reference, tags)| {
            let (bundle, name) = reference_parts(reference);
            ItemRow {
                name,
                tags: tags.to_vec(),
                bundle_label: bundle.clone(),
                bundle,
                reference: (*reference).to_string(),
                remote: String::new(),
            }
        })
        .collect();
    ItemList {
        rows,
        kind: listed.kind.to_string(),
        bundle_filter: listed.bundle_filter.to_string(),
        total: listed.total,
        hint: hint(listed.in_project),
    }
}

/// A `fragment|command show` as its view.
pub(crate) fn item_show(reference: &str, body: &str, distilled: bool, state: i32) -> ItemShow {
    let (bundle, name) = reference_parts(reference);
    ItemShow {
        name,
        bundle,
        reference: reference.to_string(),
        content: body.to_string(),
        distilled,
        state: state_of(state),
    }
}

/// `fragment premises` as its view, each fragment by its reference.
pub(crate) fn premises(index: &v1::PremiseIndex) -> Premises {
    let parent = parent_of(&index.name);
    Premises {
        instruction: index.instruction.clone(),
        fragments: index
            .fragments
            .iter()
            .map(|entry| PremiseRow {
                name: library_api::names::reference_under(parent, &entry.fragment)
                    .unwrap_or_else(|| entry.fragment.clone()),
                premise: entry.premise.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
#[path = "read_back.test.rs"]
mod tests;
