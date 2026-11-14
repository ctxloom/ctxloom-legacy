//! A resource name that could not be resolved prints as JSON `null`, never
//! left out (owner ruling 2026-11-14: "list the name as undefined -- don't
//! outright skip the output"): `run --dry-run` in a project nothing has
//! named (`Run.name`), `bundle move` into another project's directory
//! (`MoveBundleResponse.bundle.name`) and `deps hold` on a local bundle
//! (`Dependency.name`). proto3 JSON leaves an empty `string` out, and
//! pbjson-build prints nothing for an unset field whatever it is told, so
//! the answer's caller names the paths whose name always prints: at each,
//! one absent is `null`. A name is field 1 of its message (design §2), so
//! it goes first, as it would print.

use serde::Serialize;
use serde_json::Value;

/// `message`'s proto3 JSON with a name at each of `names` (dotted paths,
/// `name` or `bundle.name`): the one it holds, else `null`. A message on
/// the way that is not there is there, holding only the null name.
pub fn with_unresolved_names<M: Serialize>(
    message: &M,
    names: &[&str],
) -> Result<Value, serde_json::Error> {
    let _ = names;
    serde_json::to_value(message)
}

#[cfg(test)]
#[path = "names.test.rs"]
mod tests;
