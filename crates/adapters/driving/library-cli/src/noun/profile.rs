//! `profile`: list and show, and its writes (B1), each json answer its AIP
//! resource or method response (OD-40, design §5.5). A write that answered
//! in text only (create, edit, modify, export, import) still does, unless
//! json is asked for by name.

use library_application::{ProfileCreated, ProfileExported, ProfileImported, ProfileModified};

use crate::catalog::{self, ok};
use library_api::resources::{profile_bundles, profile_name, profile_parents};

use crate::prelude::*;

pub(crate) fn read(
    cx: &Context<'_>,
    (verb, sub): (&str, &ArgMatches),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    // The daemon's reads (step 10a U2), their text from the message (D10).
    if verb == "show" {
        let request = v1::GetProfileRequest {
            globals: cx.globals(),
            ..v1::GetProfileRequest::default()
        };
        let under = library_api::names::profile_target(&text_arg(sub, "name"));
        let read = cx.served.get_profile(&under, request);
        return served(cx, read, out, err, false, |profile, out, _| {
            text::profile_show(out, &crate::read_back::profile_show(profile))
        });
    }
    let request = v1::ListProfilesRequest {
        globals: cx.globals(),
        ..v1::ListProfilesRequest::default()
    };
    let in_project = cx.served.in_project();
    let read = cx.served.list_profiles("", request);
    served(cx, read, out, err, false, |listed, out, _| {
        text::profile_list(out, &crate::read_back::profile_list(listed, in_project))
    })
}

pub(crate) fn write(
    cx: &Context<'_>,
    (verb, sub): (&str, &ArgMatches),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Option<Exit>, Failure> {
    let code = match verb {
        "create" => create(cx, sub, out, err)?,
        "modify" | "edit" if verb == "modify" || patch_asked(sub) => modify(cx, sub, out, err)?,
        "edit" => edit_in_editor(cx, sub, out, err)?,
        "export" => export(cx, sub, out, err)?,
        "import" => import(cx, sub, out, err)?,
        "remove" => remove(cx, sub, out, err)?,
        _ => return Ok(None),
    };
    Ok(Some(code))
}

fn create(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let name = text_arg(sub, "name");
    let request = ProfileCreateRequest {
        name,
        description: text_arg(sub, "description"),
        parents: many(sub, "parent"),
        // `--include`, and the vocabulary's `--add-bundle`, in that order.
        bundles: [many(sub, "include"), many(sub, "add-bundle")].concat(),
        skills: many(sub, "add-skill"),
        target: text_arg(sub, "bundle"),
    };
    let outcome = cx.library.profile_create(&cx.flags, &request);
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        true,
        |parent, made| created(parent, &request, made),
        |made, out, _| {
            let mut parts = Vec::new();
            if !made.parents.is_empty() {
                parts.push(format!("parents: {}", made.parents.join(", ")));
            }
            if !made.bundles.is_empty() {
                parts.push(format!("bundles: {}", made.bundles.join(", ")));
            }
            let with = if parts.is_empty() {
                String::new()
            } else {
                format!(" with {}", parts.join("; "))
            };
            writeln!(out, "Created profile \"{}\"{with}", made.name)?;
            writeln!(out, "Saved to: {}", made.path)
        },
        ok,
    )
}

fn modify(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let name = text_arg(sub, "name");
    let edit = ProfileEdit {
        description: given(sub, "description"),
        add_parents: many(sub, "add-parent"),
        remove_parents: many(sub, "remove-parent"),
        add_bundles: many(sub, "add-bundle"),
        remove_bundles: many(sub, "remove-bundle"),
        exclude_fragments: many(sub, "exclude-fragment"),
        include_fragments: many(sub, "include-fragment"),
        exclude_mcp: many(sub, "exclude-mcp"),
        include_mcp: many(sub, "include-mcp"),
    };
    let outcome = cx.library.profile_modify(&cx.flags, &name, &edit);
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        true,
        |parent, done| modified(parent, &name, done),
        |done, out, _| {
            if done.changes.is_empty() {
                return nothing(out);
            }
            for change in &done.changes {
                writeln!(out, "{change}")?;
            }
            writeln!(out, "Modified profile \"{}\"", done.name)
        },
        ok,
    )
}

fn edit_in_editor(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let name = text_arg(sub, "name");
    let outcome = cx.library.profile_edit(&cx.flags, &name);
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        true,
        |parent, done| edited(parent, &name, done),
        |done, out, _| match done {
            ProfileEdited::Unchanged => nothing(out),
            ProfileEdited::Updated { path } => writeln!(out, "Updated profile: {path}"),
        },
        ok,
    )
}

fn export(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let name = text_arg(sub, "name");
    let outcome = cx
        .library
        .profile_export(&cx.flags, &name, &text_arg(sub, "dest"));
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        true,
        |parent, done| exported(parent, &name, done),
        |done, out, _| writeln!(out, "Exported: {} -> {}", done.source, done.dest),
        ok,
    )
}

fn import(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let target = text_arg(sub, "bundle");
    let outcome = cx.library.profile_import(
        &cx.flags,
        &text_arg(sub, "path"),
        flag(sub, "force"),
        &target,
    );
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        true,
        |parent, done| imported(parent, &target, done),
        |done, out, _| writeln!(out, "Imported: {} -> {}", done.source, done.dest),
        ok,
    )
}

/// `profile remove <name> [--yes]`: the daemon's `DeleteProfile` (step
/// 10b U3), previewed and confirmed at a terminal.
fn remove(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let name = text_arg(sub, "name");
    let under = library_api::names::profile_target(&name);
    crate::write::removal_served(
        cx,
        flag(sub, "yes"),
        |validate_only, answers, etag| {
            let request = v1::DeleteProfileRequest {
                validate_only,
                answers,
                etag,
                globals: cx.globals(),
                ..v1::DeleteProfileRequest::default()
            };
            cx.served.delete_profile(&under, request)
        },
        out,
        err,
        |_, out| writeln!(out, "Removed profile \"{name}\""),
    )
}

/// A profile named and nothing else, as a write answers it.
fn just_named(name: String) -> Option<v1::Profile> {
    Some(v1::Profile {
        name,
        ..<v1::Profile as Default>::default()
    })
}

/// `profile create --format json`: `CreateProfileResponse`, the profile in
/// the bundle it was written into.
pub(crate) fn created(
    parent: &str,
    request: &ProfileCreateRequest,
    made: &ProfileCreated,
) -> v1::CreateProfileResponse {
    v1::CreateProfileResponse {
        profile: Some(v1::Profile {
            name: profile_name(parent, &request.target, &made.name),
            description: request.description.clone(),
            parents: profile_parents(parent, &made.parents),
            bundles: profile_bundles(parent, &made.bundles),
            path: made.path.clone(),
            ..<v1::Profile as Default>::default()
        }),
        path: made.path.clone(),
        applied: Some(true),
        ..v1::CreateProfileResponse::default()
    }
}

/// `profile modify|edit <flags> --format json`: `UpdateProfileResponse`;
/// a modification that changed nothing applied nothing.
pub(crate) fn modified(
    parent: &str,
    asked: &str,
    done: &ProfileModified,
) -> v1::UpdateProfileResponse {
    v1::UpdateProfileResponse {
        profile: just_named(catalog::profile_ref(parent, asked)),
        applied: Some(!done.changes.is_empty()),
        changes: done.changes.clone(),
        ..v1::UpdateProfileResponse::default()
    }
}

/// `profile edit --format json` (the editor's round trip):
/// `UpdateProfileResponse`.
pub(crate) fn edited(parent: &str, asked: &str, done: &ProfileEdited) -> v1::UpdateProfileResponse {
    let (path, applied) = match done {
        ProfileEdited::Unchanged => (String::new(), false),
        ProfileEdited::Updated { path } => (path.clone(), true),
    };
    v1::UpdateProfileResponse {
        profile: just_named(catalog::profile_ref(parent, asked)),
        path,
        applied: Some(applied),
        ..v1::UpdateProfileResponse::default()
    }
}

/// `profile export --format json`: `ExportProfileResponse`.
pub(crate) fn exported(
    parent: &str,
    asked: &str,
    done: &ProfileExported,
) -> v1::ExportProfileResponse {
    v1::ExportProfileResponse {
        profile: catalog::profile_ref(parent, asked),
        source: done.source.clone(),
        destination: done.dest.clone(),
        applied: Some(true),
        ..v1::ExportProfileResponse::default()
    }
}

/// `profile import --format json`: `ImportProfileResponse`, the profile
/// named by the file it landed in.
pub(crate) fn imported(
    parent: &str,
    target: &str,
    done: &ProfileImported,
) -> v1::ImportProfileResponse {
    let file = done.dest.rsplit(['/', '\\']).next().unwrap_or_default();
    let id = file.strip_suffix(".yaml").unwrap_or(file);
    v1::ImportProfileResponse {
        profile: just_named(profile_name(parent, target, id)),
        path: done.dest.clone(),
        applied: Some(true),
        ..v1::ImportProfileResponse::default()
    }
}

/// Whether `profile edit` was given a flag to patch with (else it opens the
/// editor).
fn patch_asked(sub: &ArgMatches) -> bool {
    [
        "description",
        "add-parent",
        "remove-parent",
        "add-bundle",
        "remove-bundle",
        "exclude-fragment",
        "include-fragment",
        "exclude-mcp",
        "include-mcp",
    ]
    .iter()
    .any(|name| sub.value_source(name) == Some(clap::parser::ValueSource::CommandLine))
}

#[cfg(test)]
#[path = "profile.test.rs"]
mod tests;
