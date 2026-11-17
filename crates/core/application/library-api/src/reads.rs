//! The library's reads as the daemon answers them (step 10a U2): each a
//! use case of the library opened for the read, its view mapped to the
//! API's message ([`crate::resources`]) under the parent it was asked
//! under. What the use case noticed is answered beside it; what refused it
//! is the use case's own error, which the daemon answers as its typed
//! refusal (E5).
//!
//! Each read is `fn(library, flags, parent, request)`, the parent the
//! target is under (`projects/{harp}` or `users/me`), so one table,
//! [`served!`](crate::served), lists what the daemon serves, and each layer
//! generates its part from it (D12).

use daemon_proto::v1;
use library_application::{AssembleRequest, DefaultAgent, Flags, ItemKind, Library, Outcome};

use crate::{names, resources};

/// A notice a read said, as the command says it: its code and args,
/// rendered by the command's own catalog.
pub fn said(notice: &v1::Notice) -> library_application::Notice {
    run_application::client::noticed(notice)
}

/// A command's global flags as a read's request carries them (E1).
pub fn globals_of(flags: &Flags) -> v1::Globals {
    v1::Globals {
        degraded: flags.degraded,
        no_companions: flags.no_companions,
        config_set: flags.config_set.clone(),
    }
}

/// `outcome`'s answer as `f` maps it, its notices kept.
fn mapped<T, U>(outcome: Outcome<T>, f: impl FnOnce(T) -> U) -> Outcome<U> {
    Outcome {
        result: outcome.result.map(f),
        notices: outcome.notices,
    }
}

/// The library reads the daemon serves (step 10a U2, D12), in the shape
/// of `daemon_proto::library_reads!`'s rows, handed to `$each`: each row's
/// `reads::$method` answers it.
#[macro_export]
macro_rules! served {
    ($each:ident) => {
        $each! {
            list_bundles (ListBundles) ListBundlesRequest.parent => ListBundlesResponse,
            list_agents (ListAgents) ListAgentsRequest.parent => ListAgentsResponse,
            list_profiles (ListProfiles) ListProfilesRequest.parent => ListProfilesResponse,
            list_skills (ListSkills) ListSkillsRequest.parent => ListSkillsResponse,
            list_fragments (ListFragments) ListFragmentsRequest.parent => ListFragmentsResponse,
            list_commands (ListCommands) ListCommandsRequest.parent => ListCommandsResponse,
            get_fragment (GetFragment) GetFragmentRequest.name => Fragment,
            get_command (GetCommand) GetCommandRequest.name => Command,
            get_premise_index (GetPremiseIndex) GetPremiseIndexRequest.name => PremiseIndex,
        }
    };
}

/// `ListBundles`: the whole catalog, every origin.
pub fn list_bundles(
    library: &Library,
    flags: &Flags,
    parent: &str,
    _: &v1::ListBundlesRequest,
) -> Outcome<v1::ListBundlesResponse> {
    mapped(library.bundle_list(flags), |list| {
        resources::bundles(parent, &list)
    })
}

/// `ListAgents`: every agent under `parent`, the one a bare run binds
/// marked `default`.
pub fn list_agents(
    library: &Library,
    flags: &Flags,
    parent: &str,
    _: &v1::ListAgentsRequest,
) -> Outcome<v1::ListAgentsResponse> {
    let read = library.agent_list(flags);
    // Asked only once the listing was had, so its notices are the
    // listing's alone.
    let default = match library.agent_default(flags, None).result {
        Ok(DefaultAgent::Current(name)) => name,
        _ => String::new(),
    };
    mapped(read, |rows| resources::agents(parent, &rows, &default))
}

/// `ListProfiles`: every profile of the catalog.
pub fn list_profiles(
    library: &Library,
    flags: &Flags,
    parent: &str,
    _: &v1::ListProfilesRequest,
) -> Outcome<v1::ListProfilesResponse> {
    let read = library.profile_list(flags);
    mapped(read, |list| resources::profiles(parent, &list))
}

/// `ListSkills`: every skill of the catalog, or of the one bundle the
/// request's parent names (`.../bundles/{b}`).
pub fn list_skills(
    library: &Library,
    flags: &Flags,
    parent: &str,
    request: &v1::ListSkillsRequest,
) -> Outcome<v1::ListSkillsResponse> {
    let bundle = names::bundle_under(parent, &request.parent).unwrap_or_default();
    let read = library.skill_list(flags, &bundle);
    mapped(read, |list| resources::skills(parent, &list))
}

/// The reference a Get's `name` reads back to under `parent`
/// (`names::reference_target`'s), or the name's own text where it names
/// no bundle: the use case then refuses it in its own words.
fn reference_of(parent: &str, name: &str) -> String {
    names::reference_under(parent, name).unwrap_or_else(|| name.to_string())
}

/// `ListFragments`: every fragment of the catalog, or of the one bundle the
/// request's parent names.
pub fn list_fragments(
    library: &Library,
    flags: &Flags,
    parent: &str,
    request: &v1::ListFragmentsRequest,
) -> Outcome<v1::ListFragmentsResponse> {
    let bundle = names::bundle_under(parent, &request.parent).unwrap_or_default();
    let read = library.item_list(flags, ItemKind::Fragment, &bundle);
    mapped(read, |list| resources::fragments(parent, &list))
}

/// `ListCommands`: as [`list_fragments`], the commands.
pub fn list_commands(
    library: &Library,
    flags: &Flags,
    parent: &str,
    request: &v1::ListCommandsRequest,
) -> Outcome<v1::ListCommandsResponse> {
    let bundle = names::bundle_under(parent, &request.parent).unwrap_or_default();
    let read = library.item_list(flags, ItemKind::Command, &bundle);
    mapped(read, |list| resources::commands(parent, &list))
}

/// `GetFragment`: the fragment its name references, its distilled form
/// where one is preferred and fresh.
pub fn get_fragment(
    library: &Library,
    flags: &Flags,
    parent: &str,
    request: &v1::GetFragmentRequest,
) -> Outcome<v1::Fragment> {
    let reference = reference_of(parent, &request.name);
    let read = library.item_show(
        flags,
        ItemKind::Fragment,
        &reference,
        request.prefer_distilled,
    );
    mapped(read, |show| resources::fragment(parent, &show))
}

/// `GetCommand`: as [`get_fragment`], a command.
pub fn get_command(
    library: &Library,
    flags: &Flags,
    parent: &str,
    request: &v1::GetCommandRequest,
) -> Outcome<v1::Command> {
    let reference = reference_of(parent, &request.name);
    let read = library.item_show(
        flags,
        ItemKind::Command,
        &reference,
        request.prefer_distilled,
    );
    mapped(read, |show| resources::command(parent, &show))
}

/// `GetPremiseIndex`: what a premise holds back of the default profile's
/// assembly, and the instruction that offers it.
pub fn get_premise_index(
    library: &Library,
    flags: &Flags,
    parent: &str,
    _: &v1::GetPremiseIndexRequest,
) -> Outcome<v1::PremiseIndex> {
    mapped(library.premises(flags), |premises| {
        resources::premise_index(parent, &premises)
    })
}

/// `AssembleContext`: the context of the profiles, fragments and tags
/// asked (none: the default profile's), without its premise index, which
/// is answered as data for the edge that shows it to render (E2).
pub fn assemble_context(
    library: &Library,
    flags: &Flags,
    parent: &str,
    request: &v1::AssembleContextRequest,
) -> Outcome<v1::AssembledContext> {
    let asked = AssembleRequest {
        profiles: request.profiles.clone(),
        fragments: request.fragments.clone(),
        tags: request.tags.clone(),
        surface: None,
    };
    mapped(library.assemble(flags, &asked), |assembled| {
        resources::assembled(parent, &assembled)
    })
}

#[cfg(test)]
#[path = "reads.test.rs"]
mod tests;
