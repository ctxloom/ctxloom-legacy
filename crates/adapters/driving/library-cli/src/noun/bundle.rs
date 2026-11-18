//! `bundle`: list, show and view, and its writes (B1), each json answer its
//! AIP resource or method response (OD-40, design §5.1).

use library_application::{
    BundleCreated, BundleEdited, BundleExported, BundleImported, BundleRemoved, BundleView, Removal,
};

use library_api::resources::count;

use crate::catalog::{self, bundle_name, ok};
use crate::prelude::*;

pub(crate) fn read(
    cx: &Context<'_>,
    (verb, sub): (&str, &ArgMatches),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    match verb {
        "show" => {
            // The daemon's read (step 10a U2), its text from the message (D10).
            let request = v1::GetBundleRequest {
                view: v1::bundle::View::Full as i32,
                globals: cx.globals(),
                ..v1::GetBundleRequest::default()
            };
            let under = library_api::names::bundle_target(&text_arg(sub, "name"));
            let read = cx.served.get_bundle(&under, request);
            served(cx, read, out, err, false, |bundle, out, notes| {
                let mut said = text::Notes(Vec::new());
                let written =
                    text::bundle_show(out, &mut said, &crate::read_back::bundle_show(bundle));
                notes.extend(said.0);
                written
            })
        }
        "view" => viewed_by_the_daemon(cx, sub, out, err),
        _ => listed_by_the_daemon(cx, out, err),
    }
}

/// `bundle list`, the daemon's `ListBundles` (step 10a U2), its text
/// rendered from the message alone (D10).
fn listed_by_the_daemon(
    cx: &Context<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let request = v1::ListBundlesRequest {
        globals: cx.globals(),
        ..v1::ListBundlesRequest::default()
    };
    let in_project = cx.served.in_project();
    served(
        cx,
        cx.served.list_bundles("", request),
        out,
        err,
        false,
        |listed, out, _| text::bundles(out, listed, in_project),
    )
}

/// `bundle view <bundle>#<path>`, one item, the daemon's read of its kind
/// (step 10a U2, owner ruling 2026-11-17): a fragment's or a command's
/// body, or a skill's, an MCP server's or a profile's fields as YAML, keys
/// sorted and references short, each rendered from the item's message
/// alone. A whole bundle (no `#`), or a path no item kind serves, is the
/// bundle's CONTENT read of it, which the daemon refuses as the view
/// refuses a path it does not hold.
fn viewed_by_the_daemon(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let ask = text_arg(sub, "name");
    let prefer_distilled = flag(sub, "distilled");
    let under = library_api::names::reference_target(&ask);
    let kind = ask
        .split_once('#')
        .and_then(|(_, path)| path.split_once('/'))
        .map_or("", |(kind, _)| kind);
    let render = |view: BundleView, out: &mut dyn Write, notes: &mut Vec<String>| {
        let mut said = text::Notes(Vec::new());
        let written = text::bundle_view(out, &mut said, &view, &ask);
        notes.extend(said.0);
        written
    };
    match kind {
        "fragments" => {
            let request = v1::GetFragmentRequest {
                prefer_distilled,
                globals: cx.globals(),
                ..v1::GetFragmentRequest::default()
            };
            let read = cx.served.get_fragment(&under, request);
            served(cx, read, out, err, false, |f, out, notes| {
                render(
                    crate::read_back::body_view(&ask, &f.body, f.body_is_distilled),
                    out,
                    notes,
                )
            })
        }
        "commands" | "prompts" => {
            let request = v1::GetCommandRequest {
                prefer_distilled,
                globals: cx.globals(),
                ..v1::GetCommandRequest::default()
            };
            let read = cx.served.get_command(&under, request);
            served(cx, read, out, err, false, |c, out, notes| {
                render(
                    crate::read_back::body_view(&ask, &c.body, c.body_is_distilled),
                    out,
                    notes,
                )
            })
        }
        "skills" => {
            let request = v1::GetSkillRequest {
                globals: cx.globals(),
                ..v1::GetSkillRequest::default()
            };
            let read = cx.served.get_skill(&under, request);
            served(cx, read, out, err, false, |skill, out, notes| {
                render(crate::read_back::skill_view(&ask, skill), out, notes)
            })
        }
        "mcp" => {
            let request = v1::GetMcpServerRequest {
                globals: cx.globals(),
                ..v1::GetMcpServerRequest::default()
            };
            let read = cx.served.get_mcp_server(&under, request);
            served(cx, read, out, err, false, |server, out, notes| {
                render(crate::read_back::mcp_view(&ask, server), out, notes)
            })
        }
        "profiles" => {
            let request = v1::GetProfileRequest {
                globals: cx.globals(),
                ..v1::GetProfileRequest::default()
            };
            let read = cx
                .served
                .get_profile(&library_api::names::profile_target(&ask), request);
            served(cx, read, out, err, false, |profile, out, notes| {
                render(crate::read_back::profile_view(&ask, profile), out, notes)
            })
        }
        _ => {
            let request = v1::GetBundleRequest {
                view: v1::bundle::View::Content as i32,
                prefer_distilled,
                globals: cx.globals(),
                ..v1::GetBundleRequest::default()
            };
            let read = cx.served.get_bundle(&under, request);
            served(cx, read, out, err, false, |bundle, out, notes| {
                render(crate::read_back::bundle_view(bundle), out, notes)
            })
        }
    }
}

pub(crate) fn write(
    cx: &Context<'_>,
    (verb, sub): (&str, &ArgMatches),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Option<Exit>, Failure> {
    let code = match verb {
        "create" => create(cx, sub, out, err)?,
        "edit" => edit(cx, sub, out, err)?,
        "remove" => remove(cx, sub, out, err)?,
        "export" => export(cx, sub, out, err)?,
        "import" => import(cx, sub, out, err)?,
        "distill" => distill(cx, sub, out, err)?,
        _ => return Ok(None),
    };
    Ok(Some(code))
}

/// `bundle distill <bundle> [-f]` (OD-39: by name): each item's line in
/// text; in json the `Bundle`, each of its items with the distilled state
/// it was left in.
fn distill(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let outcome = cx
        .library
        .bundle_distill(&cx.flags, &text_arg(sub, "name"), flag(sub, "force"));
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        false,
        |parent, done| {
            let entry = |i: &library_application::ItemDistilled, dir: &str| v1::BundleItem {
                item: catalog::item_name(parent, &done.bundle, dir, &i.name),
                distilled: catalog::distilled(i.status != "no_distill"),
                no_distill: Some(i.status == "no_distill"),
                ..v1::BundleItem::default()
            };
            v1::Bundle {
                name: catalog::bundle_name(parent, &done.bundle),
                fragments: done
                    .items
                    .iter()
                    .filter(|i| i.kind == "fragment")
                    .map(|i| entry(i, "fragments"))
                    .collect(),
                commands: done
                    .items
                    .iter()
                    .filter(|i| i.kind == "command")
                    .map(|i| entry(i, "commands"))
                    .collect(),
                ..v1::Bundle::default()
            }
        },
        |done, out, _| {
            for item in &done.items {
                super::item::distilled_text(item, out)?;
            }
            Ok(())
        },
        ok,
    )
}

fn create(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let name = text_arg(sub, "name");
    let outcome = cx
        .library
        .bundle_create(&cx.flags, &name, &text_arg(sub, "description"));
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        false,
        created,
        |made, out, _| {
            writeln!(out, "Created bundle: {}", made.path)?;
            writeln!(
                out,
                "Edit its item files to add your fragments and prompts."
            )
        },
        catalog::ok,
    )
}

fn edit(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let name = text_arg(sub, "name");
    let request = BundleEditRequest {
        name,
        description: given(sub, "description"),
        version: given(sub, "version"),
        add_tags: many(sub, "add-tag"),
        remove_tags: many(sub, "remove-tag"),
        add_fragments: many(sub, "add-fragment"),
        remove_fragments: many(sub, "remove-fragment"),
        add_prompts: many(sub, "add-prompt"),
        remove_prompts: many(sub, "remove-prompt"),
        add_mcp: many(sub, "add-mcp"),
        remove_mcp: many(sub, "remove-mcp"),
    };
    let outcome = cx.library.bundle_edit(&cx.flags, &request);
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        false,
        edited,
        |edited, out, _| {
            if edited.changes.is_empty() {
                return nothing(out);
            }
            for change in &edited.changes {
                writeln!(out, "{change}")?;
            }
            writeln!(out, "Updated bundle: {}", edited.path)
        },
        catalog::ok,
    )
}

fn remove(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let name = text_arg(sub, "name");
    let outcome = cx.library.bundle_remove(&cx.flags, &name, flag(sub, "yes"));
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        false,
        |parent, gone| removed(parent, &name, gone),
        |gone, out, _| {
            catalog::removal_text(gone, out, |gone, out| {
                writeln!(out, "Removed bundle: {}", gone.path)
            })
        },
        crate::write::removal_exit,
    )
}

fn export(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let name = text_arg(sub, "name");
    let outcome = cx.library.bundle_export(
        &cx.flags,
        &name,
        &text_arg(sub, "dest"),
        &text_arg(sub, "output"),
    );
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        false,
        exported,
        |done, out, _| writeln!(out, "Exported: {} -> {}", done.source, done.dest),
        catalog::ok,
    )
}

fn import(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let outcome = cx
        .library
        .bundle_import(&cx.flags, &text_arg(sub, "path"), flag(sub, "force"));
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        false,
        imported,
        |done, out, _| {
            writeln!(out, "Imported: {} -> {}", done.source, done.dest)?;
            writeln!(out, "  Version: {}", done.version)?;
            writeln!(
                out,
                "  Fragments: {}, Commands: {}, MCP: {}",
                done.fragments, done.commands, done.mcp
            )
        },
        catalog::ok,
    )
}

/// A bundle named and nothing else, as a write answers it.
fn just_named(parent: &str, catalog: &str) -> Option<v1::Bundle> {
    Some(v1::Bundle {
        name: bundle_name(parent, catalog),
        ..v1::Bundle::default()
    })
}

/// `bundle create`: `CreateBundleResponse`.
pub(crate) fn created(parent: &str, made: &BundleCreated) -> v1::CreateBundleResponse {
    v1::CreateBundleResponse {
        bundle: just_named(parent, &made.name),
        path: made.path.clone(),
        applied: Some(true),
        ..v1::CreateBundleResponse::default()
    }
}

/// `bundle edit`: `UpdateBundleResponse`; an edit that changed nothing
/// applied nothing and lists no change.
pub(crate) fn edited(parent: &str, edited: &BundleEdited) -> v1::UpdateBundleResponse {
    v1::UpdateBundleResponse {
        bundle: just_named(parent, &edited.name),
        path: edited.path.clone(),
        applied: Some(!edited.changes.is_empty()),
        changes: edited.changes.clone(),
        ..v1::UpdateBundleResponse::default()
    }
}

/// `bundle remove`: `DeleteBundleResponse`, the preview's or the removal's.
pub(crate) fn removed(
    parent: &str,
    asked: &str,
    gone: &Removal<BundleRemoved>,
) -> v1::DeleteBundleResponse {
    catalog::removal(
        gone,
        |preview| v1::DeleteBundleResponse {
            bundle: just_named(parent, asked),
            applied: Some(false),
            detail: preview.detail,
            apply_command: preview.apply_command,
            ..v1::DeleteBundleResponse::default()
        },
        |done| v1::DeleteBundleResponse {
            bundle: just_named(parent, &done.name),
            path: done.path.clone(),
            applied: Some(true),
            ..v1::DeleteBundleResponse::default()
        },
    )
}

/// `bundle export`: `ExportBundleResponse`.
pub(crate) fn exported(parent: &str, done: &BundleExported) -> v1::ExportBundleResponse {
    v1::ExportBundleResponse {
        bundle: bundle_name(parent, &done.name),
        source: done.source.clone(),
        destination: done.dest.clone(),
        applied: Some(true),
        ..v1::ExportBundleResponse::default()
    }
}

/// `bundle import`: `ImportBundleResponse`, the bundle with its version and
/// counts, named by the directory it landed in.
pub(crate) fn imported(parent: &str, done: &BundleImported) -> v1::ImportBundleResponse {
    let name = done
        .dest
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    v1::ImportBundleResponse {
        bundle: Some(v1::Bundle {
            name: bundle_name(parent, name),
            version: done.version.clone(),
            fragment_count: count(done.fragments),
            command_count: count(done.commands),
            mcp_server_count: count(done.mcp),
            ..v1::Bundle::default()
        }),
        path: done.dest.clone(),
        applied: Some(true),
        ..v1::ImportBundleResponse::default()
    }
}

#[cfg(test)]
#[path = "bundle.test.rs"]
mod tests;
