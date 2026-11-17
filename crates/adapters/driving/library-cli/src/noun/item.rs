//! `fragment` and `command`: list, show and `fragment premises`, and their
//! writes (B1), each json answer its AIP resource or method response
//! (OD-40, design §5.2).

use library_application::{ItemCreated, ItemDistilled, ItemEdited, ItemRemoved, ItemShow, Removal};

use crate::catalog::{self, ok};
use crate::prelude::*;
use crate::read_back;

pub(crate) fn read(
    cx: &Context<'_>,
    kind: &str,
    (verb, sub): (&str, &ArgMatches),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let command = kind == "command";
    match verb {
        // Text unless json is asked for by name (an agent's shell runs it
        // off a terminal for the body); json is the item's resource. Each
        // the daemon's read (step 10a U2), its text from the message (D10).
        "show" => {
            let under = library_api::names::reference_target(&text_arg(sub, "ref"));
            let prefer_distilled = flag(sub, "distilled");
            let show = |reference: &str, body: &str, distilled: bool, state: i32| {
                read_back::item_show(reference, body, distilled, state)
            };
            let text = |show: ItemShow, out: &mut dyn Write, notes: &mut Vec<String>| {
                let mut said = text::Notes(Vec::new());
                let written = text::item_show(out, &mut said, &show);
                notes.extend(said.0);
                written
            };
            if command {
                let request = v1::GetCommandRequest {
                    prefer_distilled,
                    globals: cx.globals(),
                    ..v1::GetCommandRequest::default()
                };
                let read = cx.served.get_command(&under, request);
                return served(cx, read, out, err, true, |c, out, notes| {
                    text(
                        show(&c.reference, &c.body, c.body_is_distilled, c.distilled),
                        out,
                        notes,
                    )
                });
            }
            let request = v1::GetFragmentRequest {
                prefer_distilled,
                globals: cx.globals(),
                ..v1::GetFragmentRequest::default()
            };
            let read = cx.served.get_fragment(&under, request);
            served(cx, read, out, err, true, |f, out, notes| {
                text(
                    show(&f.reference, &f.body, f.body_is_distilled, f.distilled),
                    out,
                    notes,
                )
            })
        }
        "premises" => {
            let request = v1::GetPremiseIndexRequest {
                globals: cx.globals(),
                ..v1::GetPremiseIndexRequest::default()
            };
            let read = cx.served.get_premise_index("/premiseIndex", request);
            served(cx, read, out, err, false, |index, out, _| {
                text::premises(out, &read_back::premises(index))
            })
        }
        _ => listed_by_the_daemon(cx, kind, &text_arg(sub, "bundle"), out, err),
    }
}

/// `fragment|command list`, the daemon's `ListFragments` or `ListCommands`
/// (step 10a U2), its text from the message (D10). A listing of one bundle
/// that holds none asks the whole catalog's too, to say how many the other
/// bundles hold.
fn listed_by_the_daemon(
    cx: &Context<'_>,
    kind: &str,
    bundle: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let under = |bundle: &str| match bundle {
        "" => String::new(),
        bundle => library_api::names::bundle_target(bundle),
    };
    let in_project = cx.served.in_project();
    let total = |filtered: usize, count: &dyn Fn() -> usize| match (bundle, filtered) {
        ("", n) => n,
        (_, 0) => count(),
        (_, n) => n,
    };
    let text = |items: Vec<(&str, &[String])>, total: usize, out: &mut dyn Write| {
        let listed = read_back::Listed {
            kind,
            items,
            bundle_filter: bundle,
            total,
            in_project,
        };
        text::item_list(out, &read_back::item_list(&listed))
    };
    if kind == "command" {
        let request = || v1::ListCommandsRequest {
            globals: cx.globals(),
            ..v1::ListCommandsRequest::default()
        };
        let every = || {
            cx.served
                .list_commands("", request())
                .map_or(0, |(listed, _)| listed.commands.len())
        };
        let read = cx.served.list_commands(&under(bundle), request());
        return served(cx, read, out, err, false, |listed, out, _| {
            let items = listed
                .commands
                .iter()
                .map(|c| (c.reference.as_str(), c.tags.as_slice()))
                .collect();
            text(items, total(listed.commands.len(), &every), out)
        });
    }
    let request = || v1::ListFragmentsRequest {
        globals: cx.globals(),
        ..v1::ListFragmentsRequest::default()
    };
    let every = || {
        cx.served
            .list_fragments("", request())
            .map_or(0, |(listed, _)| listed.fragments.len())
    };
    let read = cx.served.list_fragments(&under(bundle), request());
    served(cx, read, out, err, false, |listed, out, _| {
        let items = listed
            .fragments
            .iter()
            .map(|f| (f.reference.as_str(), f.tags.as_slice()))
            .collect();
        text(items, total(listed.fragments.len(), &every), out)
    })
}

pub(crate) fn write(
    cx: &Context<'_>,
    kind: ItemKind,
    (verb, sub): (&str, &ArgMatches),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Option<Exit>, Failure> {
    let reference = text_arg(sub, "ref");
    let code = match verb {
        "create" => {
            let outcome = match given(sub, "name") {
                Some(name) => {
                    cx.library
                        .item_create(&cx.flags, kind, &text_arg(sub, "bundle"), &name)
                }
                None => cx
                    .library
                    .item_create_at(&cx.flags, kind, &text_arg(sub, "bundle")),
            };
            catalog::answer(
                cx,
                outcome,
                out,
                err,
                false,
                created,
                |made, out, _| {
                    if made.bundle_created {
                        writeln!(out, "Created bundle \"{}\"", made.bundle)?;
                    }
                    writeln!(
                        out,
                        "Created {} \"{}\" in bundle \"{}\"",
                        made.kind, made.name, made.bundle
                    )?;
                    writeln!(
                        out,
                        "Edit with: ctxloom {} edit {}#{}/{}",
                        made.kind,
                        made.bundle,
                        dir_of(&made.kind),
                        made.name
                    )
                },
                ok,
            )?
        }
        "remove" => {
            let outcome = cx
                .library
                .item_remove(&cx.flags, kind, &reference, flag(sub, "yes"));
            catalog::answer(
                cx,
                outcome,
                out,
                err,
                false,
                |parent, gone| removed(parent, kind, &reference, gone),
                |gone, out, _| {
                    catalog::removal_text(gone, out, |gone, out| {
                        writeln!(
                            out,
                            "Removed {} \"{}\" from bundle \"{}\"",
                            gone.kind, gone.name, gone.bundle
                        )
                    })
                },
                crate::write::removal_exit,
            )?
        }
        "edit" => {
            let no_distill = flag(sub, "no-distill");
            let outcome = cx
                .library
                .item_edit(&cx.flags, kind, &reference, no_distill);
            catalog::answer(
                cx,
                outcome,
                out,
                err,
                false,
                edited,
                |edited, out, notes| {
                    if edited.status == "unchanged" {
                        return nothing(out);
                    }
                    if edited.distill_skipped {
                        notes.push("distilled form not refreshed (--no-distill)".to_string());
                    }
                    writeln!(
                        out,
                        "Updated {} \"{}\" in bundle \"{}\"",
                        edited.kind, edited.name, edited.bundle
                    )?;
                    writeln!(out)?;
                    writeln!(out, "Bundle modified. To publish changes:")?;
                    writeln!(out, "  ctxloom bundle push {} [remote]", edited.bundle)
                },
                ok,
            )?
        }
        "distill" => distill(cx, kind, &reference, flag(sub, "force"), out, err)?,
        _ => return Ok(None),
    };
    Ok(Some(code))
}

/// `fragment|command distill <ref> [-f]` (plan step 5).
fn distill(
    cx: &Context<'_>,
    kind: ItemKind,
    reference: &str,
    force: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let outcome = cx.library.item_distill(&cx.flags, kind, reference, force);
    catalog::answer(
        cx,
        outcome,
        out,
        err,
        false,
        distilled,
        |done, out, _| distilled_text(done, out),
        ok,
    )
}

/// What one item's distillation said, in text.
pub(crate) fn distilled_text(done: &ItemDistilled, out: &mut dyn Write) -> std::io::Result<()> {
    match done.status.as_str() {
        "no_distill" => writeln!(
            out,
            "{} \"{}\" in bundle \"{}\" is marked as no_distill: nothing distilled",
            done.kind, done.name, done.bundle
        ),
        "fresh" => writeln!(
            out,
            "{} \"{}\" in bundle \"{}\": its distilled form matches its source (-f distills it again)",
            done.kind, done.name, done.bundle
        ),
        _ => writeln!(
            out,
            "Distilled {} \"{}\" in bundle \"{}\" ({})",
            done.kind, done.name, done.bundle, done.path
        ),
    }
}

/// `fragment|command distill`: `DistillFragmentResponse` or
/// `DistillCommandResponse`; what was not asked (fresh, or no_distill)
/// applied nothing, and says why in `detail`.
pub(crate) fn distilled(
    parent: &str,
    done: &ItemDistilled,
) -> Either<v1::DistillFragmentResponse, v1::DistillCommandResponse> {
    let name = catalog::item_name(parent, &done.bundle, dir_of(&done.kind), &done.name);
    let applied = Some(done.status == "distilled");
    let state = if done.status == "no_distill" {
        library_application::DistilledState::None
    } else {
        library_application::DistilledState::Fresh
    };
    let distilled = catalog::distilled_state(state);
    let detail = if done.status == "distilled" {
        Vec::new()
    } else {
        vec![done.status.clone()]
    };
    if is_command(&done.kind) {
        return Either::Command(v1::DistillCommandResponse {
            command: Some(v1::Command {
                name,
                distilled,
                no_distill: (done.status == "no_distill").then_some(true),
                ..v1::Command::default()
            }),
            path: done.path.clone(),
            applied,
            detail,
            ..v1::DistillCommandResponse::default()
        });
    }
    Either::Fragment(v1::DistillFragmentResponse {
        fragment: Some(v1::Fragment {
            name,
            distilled,
            no_distill: (done.status == "no_distill").then_some(true),
            ..v1::Fragment::default()
        }),
        path: done.path.clone(),
        applied,
        detail,
        ..v1::DistillFragmentResponse::default()
    })
}

/// A fragment's message or a command's, as the kind is.
#[derive(serde::Serialize)]
#[serde(untagged)]
pub(crate) enum Either<F, C> {
    Fragment(F),
    Command(C),
}

/// Whether `kind` (the use case's word or its directory) is a command's.
fn is_command(kind: &str) -> bool {
    matches!(kind, "command" | "commands" | "prompt" | "prompts")
}

/// `fragment|command create`: `CreateFragmentResponse` or
/// `CreateCommandResponse`.
pub(crate) fn created(
    parent: &str,
    made: &ItemCreated,
) -> Either<v1::CreateFragmentResponse, v1::CreateCommandResponse> {
    let name = catalog::item_name(parent, &made.bundle, dir_of(&made.kind), &made.name);
    if is_command(&made.kind) {
        return Either::Command(v1::CreateCommandResponse {
            command: Some(v1::Command {
                name,
                ..v1::Command::default()
            }),
            path: made.path.clone(),
            applied: Some(true),
            bundle_created: made.bundle_created,
            ..v1::CreateCommandResponse::default()
        });
    }
    Either::Fragment(v1::CreateFragmentResponse {
        fragment: Some(v1::Fragment {
            name,
            ..v1::Fragment::default()
        }),
        path: made.path.clone(),
        applied: Some(true),
        bundle_created: made.bundle_created,
        ..v1::CreateFragmentResponse::default()
    })
}

/// `fragment|command edit`: `UpdateFragmentResponse` or
/// `UpdateCommandResponse`; an edit that changed nothing applied nothing.
/// Its distilled form refreshed is `FRESH`; one not refreshed is no claim.
pub(crate) fn edited(
    parent: &str,
    edited: &ItemEdited,
) -> Either<v1::UpdateFragmentResponse, v1::UpdateCommandResponse> {
    let name = catalog::item_name(parent, &edited.bundle, dir_of(&edited.kind), &edited.name);
    let applied = Some(edited.status != "unchanged");
    let distilled = if edited.distilled {
        v1::DistilledState::Fresh as i32
    } else {
        v1::DistilledState::Unspecified as i32
    };
    if is_command(&edited.kind) {
        return Either::Command(v1::UpdateCommandResponse {
            command: Some(v1::Command {
                name,
                distilled,
                ..v1::Command::default()
            }),
            path: edited.path.clone(),
            applied,
            ..v1::UpdateCommandResponse::default()
        });
    }
    Either::Fragment(v1::UpdateFragmentResponse {
        fragment: Some(v1::Fragment {
            name,
            distilled,
            ..v1::Fragment::default()
        }),
        path: edited.path.clone(),
        applied,
        ..v1::UpdateFragmentResponse::default()
    })
}

/// `fragment|command remove`: `DeleteFragmentResponse` or
/// `DeleteCommandResponse`, the preview's (named by the reference asked) or
/// the removal's.
pub(crate) fn removed(
    parent: &str,
    kind: ItemKind,
    asked: &str,
    gone: &Removal<ItemRemoved>,
) -> Either<v1::DeleteFragmentResponse, v1::DeleteCommandResponse> {
    let command = kind == ItemKind::Command;
    let dir = if command { "commands" } else { "fragments" };
    let name = match gone {
        Removal::Done(done) => catalog::item_name(parent, &done.bundle, dir, &done.name),
        Removal::Preview(_) => {
            catalog::reference_name(parent, asked).unwrap_or_else(|| asked.to_string())
        }
    };
    let (path, applied, detail, apply_command) = catalog::removal(
        gone,
        |preview| (String::new(), false, preview.detail, preview.apply_command),
        |done| (done.path.clone(), true, Vec::new(), String::new()),
    );
    if command {
        return Either::Command(v1::DeleteCommandResponse {
            command: Some(v1::Command {
                name,
                ..v1::Command::default()
            }),
            path,
            applied: Some(applied),
            detail,
            apply_command,
            ..v1::DeleteCommandResponse::default()
        });
    }
    Either::Fragment(v1::DeleteFragmentResponse {
        fragment: Some(v1::Fragment {
            name,
            ..v1::Fragment::default()
        }),
        path,
        applied: Some(applied),
        detail,
        apply_command,
        ..v1::DeleteFragmentResponse::default()
    })
}

/// The directory a kind's items live under in a reference.
fn dir_of(kind: &str) -> &'static str {
    if is_command(kind) {
        "commands"
    } else {
        "fragments"
    }
}

#[cfg(test)]
#[path = "item.test.rs"]
mod tests;
