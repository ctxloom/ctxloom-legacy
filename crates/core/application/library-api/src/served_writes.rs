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

use std::sync::{Arc, Mutex};

use ctxloom_ports::{Editor, EditorCommand, Failure};
use daemon_proto::v1;
use library_application::{
    BundleCreated, Flags, ItemCreated, ItemEdited, ItemKind, Library, Outcome,
};

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
            create_fragment (CreateFragment) CreateFragmentRequest => CreateFragmentResponse, "fragment create";
            update_fragment (UpdateFragment) UpdateFragmentRequest => UpdateFragmentResponse, "fragment edit";
            create_command (CreateCommand) CreateCommandRequest => CreateCommandResponse, "command create";
            update_command (UpdateCommand) UpdateCommandRequest => UpdateCommandResponse, "command edit";
            update_config (UpdateConfig) UpdateConfigRequest => UpdateConfigResponse, "config set";
        }
    };
}

/// The id of an editor round trip's need: a write asks one at most.
pub const EDIT_NEED: &str = "edit";

/// What the editor is asked of a write the daemon plans (step 10b U3): the
/// document's text, answered by the text the request gave (`--body`, or
/// the answer to the preview's Edit need), else handed back unchanged and
/// the ask kept, as the preview's Edit need. It launches nothing: the CLI
/// launches the editor, from the need (the client-side round trip is
/// gone).
#[derive(Default)]
pub struct AnsweredEditor {
    given: Option<String>,
    asked: Mutex<Option<(EditorCommand, String, String)>>,
}

impl AnsweredEditor {
    /// The editor that answers `given`, when the request gave it.
    pub fn answering(given: Option<String>) -> Arc<AnsweredEditor> {
        Arc::new(AnsweredEditor {
            given,
            asked: Mutex::new(None),
        })
    }

    /// The Edit need of what it was asked and could not answer, if any:
    /// the document's text always, the EDITOR launch when the caller can
    /// launch one (`can_launch`).
    pub fn need(&self, can_launch: bool) -> Option<v1::Need> {
        if self.given.is_some() {
            return None;
        }
        let asked = self.asked.lock().ok()?.clone()?;
        let (command, file_name, text) = asked;
        let launch = can_launch.then(|| v1::Launch {
            kind: v1::launch::Kind::Editor as i32,
            program: command.program,
            args: command.args,
            ..v1::Launch::default()
        });
        Some(v1::Need {
            id: EDIT_NEED.to_string(),
            kind: Some(v1::need::Kind::Edit(v1::Edit {
                file_name,
                text,
                launch,
            })),
        })
    }
}

impl Editor for AnsweredEditor {
    fn edit(
        &self,
        command: &EditorCommand,
        file_name: &str,
        content: &str,
    ) -> Result<String, Failure> {
        if let Some(given) = &self.given {
            return Ok(given.clone());
        }
        if let Ok(mut asked) = self.asked.lock() {
            *asked = Some((command.clone(), file_name.to_string(), content.to_string()));
        }
        Ok(content.to_string())
    }

    /// Nothing is opened in the daemon (`config edit` opens the file in
    /// place, which only the CLI's editor can).
    fn open(&self, _: &EditorCommand, _: &str) -> Result<(), Failure> {
        Err(Box::new(library_application::AppError::from(
            library_application::WriteError::NotWired {},
        )))
    }
}

/// A write's answer and the needs its preview asks (an Edit need, at most).
#[derive(Clone, Debug, PartialEq)]
pub struct Asked<T> {
    pub done: T,
    pub needs: Vec<v1::Need>,
}

/// The edited text a write's answers give its Edit need, if any.
fn edited(answers: &[v1::Answer]) -> Option<String> {
    answers
        .iter()
        .find(|answer| answer.need_id == EDIT_NEED)
        .and_then(|answer| match &answer.answer {
            Some(v1::answer::Answer::Edited(text)) => Some(text.clone()),
            _ => None,
        })
}

/// `use_case` run over `library` with its editor answering `given`, its
/// answer with the Edit need it could not answer.
fn editing<T>(
    library: &Library,
    given: Option<String>,
    can_launch: bool,
    use_case: impl FnOnce(&Library) -> Outcome<T>,
) -> Outcome<Asked<T>> {
    let editor = AnsweredEditor::answering(given);
    let editing = library
        .clone()
        .editing_with(Arc::clone(&editor) as Arc<dyn Editor>);
    let outcome = use_case(&editing);
    let needs = editor.need(can_launch).into_iter().collect();
    Outcome {
        result: outcome.result.map(|done| Asked { done, needs }),
        notices: outcome.notices,
    }
}

/// The bundle a parent or a resource name is under (`.../bundles/{b}`).
fn bundle_in(name: &str) -> &str {
    name.split_once("/bundles/")
        .map_or("", |(_, rest)| rest.split('/').next().unwrap_or(""))
}

/// The item a resource name names (`.../bundles/{b}/{dir}/{id}`), as its
/// reference (`{b}#{dir}/{id}`).
fn reference_of(name: &str) -> String {
    let rest = name.split_once("/bundles/").map_or(name, |(_, rest)| rest);
    match rest.split_once('/') {
        Some((bundle, item)) => format!("{bundle}#{item}"),
        None => rest.to_string(),
    }
}

/// A body given (`--body`), or the Edit need's answer.
fn given(body: &str, answers: &[v1::Answer]) -> Option<String> {
    (!body.is_empty())
        .then(|| body.to_string())
        .or_else(|| edited(answers))
}

/// Whether the caller can launch the EDITOR.
fn can_launch(presentable: Option<&v1::Presentable>) -> bool {
    presentable.is_some_and(|p| p.can_launch)
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

/// An item created's resource name.
fn item_name(parent: &str, made: &ItemCreated, dir: &str) -> String {
    format!(
        "{}/{dir}/{}",
        names::bundle_name(parent, &made.bundle),
        made.name
    )
}

/// `fragment create <bundle>#fragments/<id> [--body]`: `CreateFragment`,
/// its editor round trip the preview's Edit need.
pub mod create_fragment {
    use super::*;

    pub fn plan(
        library: &Library,
        flags: &Flags,
        request: &v1::CreateFragmentRequest,
        _apply: bool,
    ) -> Outcome<Asked<ItemCreated>> {
        let body = request.fragment.as_ref().map_or("", |f| f.body.as_str());
        let reference = format!(
            "{}#fragments/{}",
            bundle_in(&request.parent),
            request.fragment_id
        );
        editing(
            library,
            given(body, &request.answers),
            can_launch(request.presentable.as_ref()),
            |library| library.item_create_at(flags, ItemKind::Fragment, &reference),
        )
    }

    pub fn respond(
        request: &v1::CreateFragmentRequest,
        wrote: Wrote<Asked<ItemCreated>>,
    ) -> v1::CreateFragmentResponse {
        let (name, path, bundle_created, needs) = match &wrote.answer {
            Some(asked) => (
                item_name(&wrote.parent, &asked.done, "fragments"),
                asked.done.path.clone(),
                asked.done.bundle_created,
                asked.needs.clone(),
            ),
            None => (
                format!(
                    "{}/fragments/{}",
                    names::bundle_name(&wrote.parent, bundle_in(&request.parent)),
                    request.fragment_id
                ),
                String::new(),
                false,
                Vec::new(),
            ),
        };
        v1::CreateFragmentResponse {
            fragment: Some(v1::Fragment {
                name,
                ..v1::Fragment::default()
            }),
            path,
            applied: Some(wrote.applied),
            bundle_created,
            needs,
            etag: wrote.etag,
            notices: wrote.notices,
            ..v1::CreateFragmentResponse::default()
        }
    }
}

/// `command create <bundle>#commands/<id> [--body]`: `CreateCommand`.
pub mod create_command {
    use super::*;

    pub fn plan(
        library: &Library,
        flags: &Flags,
        request: &v1::CreateCommandRequest,
        _apply: bool,
    ) -> Outcome<Asked<ItemCreated>> {
        let body = request.command.as_ref().map_or("", |c| c.body.as_str());
        let reference = format!(
            "{}#commands/{}",
            bundle_in(&request.parent),
            request.command_id
        );
        editing(
            library,
            given(body, &request.answers),
            can_launch(request.presentable.as_ref()),
            |library| library.item_create_at(flags, ItemKind::Command, &reference),
        )
    }

    pub fn respond(
        request: &v1::CreateCommandRequest,
        wrote: Wrote<Asked<ItemCreated>>,
    ) -> v1::CreateCommandResponse {
        let (name, path, bundle_created, needs) = match &wrote.answer {
            Some(asked) => (
                item_name(&wrote.parent, &asked.done, "commands"),
                asked.done.path.clone(),
                asked.done.bundle_created,
                asked.needs.clone(),
            ),
            None => (
                format!(
                    "{}/commands/{}",
                    names::bundle_name(&wrote.parent, bundle_in(&request.parent)),
                    request.command_id
                ),
                String::new(),
                false,
                Vec::new(),
            ),
        };
        v1::CreateCommandResponse {
            command: Some(v1::Command {
                name,
                ..v1::Command::default()
            }),
            path,
            applied: Some(wrote.applied),
            bundle_created,
            needs,
            etag: wrote.etag,
            notices: wrote.notices,
            ..v1::CreateCommandResponse::default()
        }
    }
}

/// An edit's distilled state, as its message says it: `FRESH` once
/// refreshed, else no claim.
fn distilled_of(edited: &ItemEdited) -> i32 {
    if edited.distilled {
        v1::DistilledState::Fresh as i32
    } else {
        v1::DistilledState::Unspecified as i32
    }
}

/// `fragment edit <ref> [--body] [--no-distill]`: `UpdateFragment`, its
/// editor round trip the preview's Edit need. Only an edit that changed
/// the file is applied.
pub mod update_fragment {
    use super::*;

    pub fn plan(
        library: &Library,
        flags: &Flags,
        request: &v1::UpdateFragmentRequest,
        _apply: bool,
    ) -> Outcome<Asked<ItemEdited>> {
        let fragment = request.fragment.clone().unwrap_or_default();
        editing(
            library,
            given(&fragment.body, &request.answers),
            can_launch(request.presentable.as_ref()),
            |library| {
                library.item_edit(
                    flags,
                    ItemKind::Fragment,
                    &reference_of(&fragment.name),
                    fragment.no_distill.unwrap_or(false),
                )
            },
        )
    }

    pub fn respond(
        request: &v1::UpdateFragmentRequest,
        wrote: Wrote<Asked<ItemEdited>>,
    ) -> v1::UpdateFragmentResponse {
        let name = request
            .fragment
            .as_ref()
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let (path, distilled, needs, changed) = match &wrote.answer {
            Some(asked) => (
                asked.done.path.clone(),
                distilled_of(&asked.done),
                asked.needs.clone(),
                asked.done.status != "unchanged",
            ),
            None => (String::new(), 0, Vec::new(), true),
        };
        v1::UpdateFragmentResponse {
            fragment: Some(v1::Fragment {
                name,
                distilled,
                ..v1::Fragment::default()
            }),
            path,
            applied: Some(wrote.applied && changed),
            needs,
            etag: wrote.etag,
            notices: wrote.notices,
            ..v1::UpdateFragmentResponse::default()
        }
    }
}

/// `command edit <ref> [--body] [--no-distill]`: `UpdateCommand`.
pub mod update_command {
    use super::*;

    pub fn plan(
        library: &Library,
        flags: &Flags,
        request: &v1::UpdateCommandRequest,
        _apply: bool,
    ) -> Outcome<Asked<ItemEdited>> {
        let command = request.command.clone().unwrap_or_default();
        editing(
            library,
            given(&command.body, &request.answers),
            can_launch(request.presentable.as_ref()),
            |library| {
                library.item_edit(
                    flags,
                    ItemKind::Command,
                    &reference_of(&command.name),
                    command.no_distill.unwrap_or(false),
                )
            },
        )
    }

    pub fn respond(
        request: &v1::UpdateCommandRequest,
        wrote: Wrote<Asked<ItemEdited>>,
    ) -> v1::UpdateCommandResponse {
        let name = request
            .command
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let (path, distilled, needs, changed) = match &wrote.answer {
            Some(asked) => (
                asked.done.path.clone(),
                distilled_of(&asked.done),
                asked.needs.clone(),
                asked.done.status != "unchanged",
            ),
            None => (String::new(), 0, Vec::new(), true),
        };
        v1::UpdateCommandResponse {
            command: Some(v1::Command {
                name,
                distilled,
                ..v1::Command::default()
            }),
            path,
            applied: Some(wrote.applied && changed),
            needs,
            etag: wrote.etag,
            notices: wrote.notices,
            ..v1::UpdateCommandResponse::default()
        }
    }
}

/// `config set <key> <value> [--home]`: `UpdateConfig` (T29: one key per
/// call).
pub mod update_config {
    use super::*;
    use library_application::ConfigSet;

    pub fn plan(
        library: &Library,
        flags: &Flags,
        request: &v1::UpdateConfigRequest,
        _apply: bool,
    ) -> Outcome<ConfigSet> {
        library.config_set(flags, &request.path, &request.value, request.home)
    }

    pub fn respond(
        request: &v1::UpdateConfigRequest,
        wrote: Wrote<ConfigSet>,
    ) -> v1::UpdateConfigResponse {
        let path = match &wrote.answer {
            Some(set) => set.path.clone(),
            None => wrote.at.clone(),
        };
        v1::UpdateConfigResponse {
            config: Some(v1::Config {
                name: format!("{}/config", wrote.parent),
                ..v1::Config::default()
            }),
            path,
            applied: Some(wrote.applied),
            changes: vec![format!("{} = {}", request.path, request.value)],
            etag: wrote.etag,
            notices: wrote.notices,
            ..v1::UpdateConfigResponse::default()
        }
    }
}

#[cfg(test)]
#[path = "served_writes.test.rs"]
mod tests;
