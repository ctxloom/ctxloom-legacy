//! The library's resource names (OD-40, B1; design §1.2 and §3.2 to §3.6):
//! a bundle's name from its catalog name, with its origin in the pattern
//! (OQ-4), an item's below its bundle's, and a configured reference
//! resolved to a name where its text says which (a reference it cannot
//! place keeps its text, OQ-16). The one naming, the daemon's answers' and
//! (until each noun is served, U2) the CLI's alike.
//!
//! An ID on disk that breaks the item grammar (OQ-6) is named as it is: the
//! grammar is enforced where a write mints a name, and a name already on
//! disk still reads.

use daemon_proto::v1;

/// A companion's loadout's catalog name begins so (`Catalog::with_companions`).
const COMPANION: &str = "ctxloom+companion:";
/// A project bundle's canonical reference begins so.
const LOCAL: &str = "ctxloom+local:";
/// The bundle a project's own profiles are items of.
const PROJECT_BUNDLE: &str = "project";

/// The resource name of the bundle a catalog (or local canonical) name
/// names: `<parent>/bundles/<b>`, `<parent>/remotes/<r>/bundles/<b>` for one
/// installed from a remote, `<parent>/companions/<c>/bundles/loadout` for a
/// companion's loadout.
pub fn bundle_name(parent: &str, catalog: &str) -> String {
    let catalog = catalog.strip_prefix(LOCAL).unwrap_or(catalog);
    if let Some(companion) = catalog.strip_prefix(COMPANION) {
        return format!("{parent}/companions/{companion}/bundles/loadout");
    }
    match catalog.split_once('/') {
        Some((remote, bundle)) => format!("{parent}/remotes/{remote}/bundles/{bundle}"),
        None => format!("{parent}/bundles/{catalog}"),
    }
}

/// The catalog name a bundle's resource name under `parent` stands for
/// ([`bundle_name`] read back): `<b>`, `<r>/<b>` for a remote's, a
/// companion's loadout by its companion; a name under no parent is its
/// text.
pub fn catalog_name(parent: &str, name: &str) -> String {
    let Some(rest) = name
        .strip_prefix(parent)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return name.to_string();
    };
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        ["bundles", bundle] => (*bundle).to_string(),
        ["remotes", remote, "bundles", bundle] => format!("{remote}/{bundle}"),
        ["companions", companion, "bundles", "loadout"] => format!("{COMPANION}{companion}"),
        _ => name.to_string(),
    }
}

/// Where a bundle of `catalog` name comes from; `builtin` when the catalog
/// says it is ctxloom's own.
pub fn origin(catalog: &str, builtin: bool) -> v1::bundle::Origin {
    let catalog = catalog.strip_prefix(LOCAL).unwrap_or(catalog);
    if builtin {
        v1::bundle::Origin::Builtin
    } else if catalog.starts_with(COMPANION) {
        v1::bundle::Origin::Companion
    } else if catalog.contains('/') {
        v1::bundle::Origin::Remote
    } else {
        v1::bundle::Origin::Project
    }
}

/// The collection a reference's kind directory names (`prompts` is
/// `commands`, `mcp` `mcpServers`), or none for a kind with no resource.
fn collection(dir: &str) -> Option<&'static str> {
    Some(match dir {
        "fragments" => "fragments",
        "commands" | "prompts" => "commands",
        "skills" => "skills",
        "mcp" => "mcpServers",
        "profiles" => "profiles",
        _ => return None,
    })
}

/// An item's resource name: its bundle's, then `<collection>/<id>`.
pub fn item_name(parent: &str, bundle: &str, dir: &str, id: &str) -> String {
    let collection = collection(dir).unwrap_or(dir);
    format!("{}/{collection}/{id}", bundle_name(parent, bundle))
}

/// The resource name an item reference (`<bundle>#<kind>/<name>`, its
/// bundle any catalog or local canonical name) names, or none when it is
/// not one (a bundle on a remote's URL keeps its text, OQ-16).
pub fn reference_name(parent: &str, reference: &str) -> Option<String> {
    let (bundle, path) = reference.split_once('#')?;
    let (dir, id) = path.split_once('/')?;
    if bundle.is_empty() || id.is_empty() || id.contains('/') || bundle.contains("://") {
        return None;
    }
    collection(dir)?;
    Some(item_name(parent, bundle, dir, id))
}

/// A profile as a profile's `parents` (or a command line) names it: a bare
/// name is the project's own, a reference names its bundle's. One that
/// names no place keeps its text (OQ-16).
pub fn profile_ref(parent: &str, text: &str) -> String {
    if text.contains('#') {
        return reference_name(parent, text).unwrap_or_else(|| text.to_string());
    }
    if text.is_empty() || text.contains('/') || text.contains(':') {
        return text.to_string();
    }
    item_name(parent, PROJECT_BUNDLE, "profiles", text)
}

/// A bundle as a profile's `bundles` names it: a catalog or local canonical
/// name, or one item of one (`<bundle>#<kind>/<name>`). One on a remote's
/// URL keeps its text (OQ-16).
pub fn bundle_ref(parent: &str, text: &str) -> String {
    if text.contains('#') {
        return reference_name(parent, text).unwrap_or_else(|| text.to_string());
    }
    if text.is_empty() || text.contains("://") || text.contains('@') {
        return text.to_string();
    }
    let bare = text.strip_prefix(LOCAL).unwrap_or(text);
    if bare.contains(':') && !bare.starts_with(COMPANION) {
        return text.to_string();
    }
    bundle_name(parent, text)
}

#[cfg(test)]
#[path = "names.test.rs"]
mod tests;
