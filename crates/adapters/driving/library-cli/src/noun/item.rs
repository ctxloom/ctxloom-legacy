//! `fragment` and `command`: list, show and `fragment premises`, and their
//! writes (B1), each json answer its AIP resource or method response
//! (OD-40, design §5.2).

use library_application::{ItemCreated, ItemDistilled, ItemShow};

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
        "create" => match given(sub, "name") {
            Some(name) => {
                let outcome =
                    cx.library
                        .item_create(&cx.flags, kind, &text_arg(sub, "bundle"), &name);
                catalog::answer(
                    cx,
                    outcome,
                    out,
                    err,
                    false,
                    created,
                    |made, out, _| {
                        created_text(
                            &made.kind,
                            &made.bundle,
                            &made.name,
                            made.bundle_created,
                            out,
                        )
                    },
                    ok,
                )?
            }
            None => create_served(
                cx,
                kind,
                &text_arg(sub, "bundle"),
                given(sub, "body"),
                out,
                err,
            )?,
        },
        "remove" => remove_served(cx, kind, &reference, flag(sub, "yes"), out, err)?,
        "edit" => edit_served(
            cx,
            kind,
            &reference,
            (given(sub, "body"), flag(sub, "no-distill")),
            out,
            err,
        )?,
        "distill" => distill(cx, kind, &reference, flag(sub, "force"), out, err)?,
        _ => return Ok(None),
    };
    Ok(Some(code))
}

/// What a create said, in text (Go's words).
fn created_text(
    kind: &str,
    bundle: &str,
    name: &str,
    bundle_created: bool,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    if bundle_created {
        writeln!(out, "Created bundle \"{bundle}\"")?;
    }
    writeln!(out, "Created {kind} \"{name}\" in bundle \"{bundle}\"")?;
    writeln!(
        out,
        "Edit with: ctxloom {kind} edit {bundle}#{}/{name}",
        dir_of(kind)
    )
}

/// The bundle and the item a resource name names
/// (`.../bundles/{b}/{dir}/{id}`).
fn bundle_and_item(name: &str) -> (String, String) {
    let rest = name.split_once("/bundles/").map_or(name, |(_, rest)| rest);
    let mut parts = rest.splitn(3, '/');
    let bundle = parts.next().unwrap_or_default().to_string();
    let item = parts.nth(1).unwrap_or_default().to_string();
    (bundle, item)
}

/// What the CLI can present of a need (step 10b U3): it launches the
/// editor on the user's terminal, and asks nothing else here.
fn presentable() -> Option<v1::Presentable> {
    Some(v1::Presentable {
        can_ask: false,
        can_launch: true,
    })
}

/// The answer to `needs`' Edit need, the editor launched on its document
/// (step 10b U3: the preview's Edit need, presented by the CLI); none
/// when it asks none.
fn edit_answers(cx: &Context<'_>, needs: &[v1::Need]) -> Result<Vec<v1::Answer>, Failure> {
    let mut answers = Vec::new();
    for need in needs {
        if let Some(v1::need::Kind::Edit(edit)) = &need.kind {
            let edited = cx.served.edit_document(edit)?;
            answers.push(v1::Answer {
                need_id: need.id.clone(),
                answer: Some(v1::answer::Answer::Edited(edited)),
            });
        }
    }
    Ok(answers)
}

/// `fragment|command create <bundle>#<dir>/<id> [--body]`: the daemon's
/// `CreateFragment` or `CreateCommand` (step 10b U3). With no body, its
/// preview's Edit need is presented (the editor launched here), and the
/// apply sends what the editor left.
fn create_served(
    cx: &Context<'_>,
    kind: ItemKind,
    reference: &str,
    body: Option<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let (bundle, path) = reference.split_once('#').unwrap_or((reference, ""));
    let id = path.split_once('/').map_or(path, |(_, id)| id).to_string();
    let under = library_api::names::bundle_target(bundle);
    let previewed = body.is_none();
    let word = if kind == ItemKind::Command {
        "command"
    } else {
        "fragment"
    };
    let text = |name: &str, bundle_created: bool, out: &mut dyn Write| {
        let (bundle, item) = bundle_and_item(name);
        created_text(word, &bundle, &item, bundle_created, out)
    };
    if kind == ItemKind::Command {
        let ask = |validate_only: bool, answers: Vec<v1::Answer>| v1::CreateCommandRequest {
            command_id: id.clone(),
            command: Some(v1::Command {
                body: body.clone().unwrap_or_default(),
                ..v1::Command::default()
            }),
            validate_only,
            answers,
            presentable: presentable(),
            globals: cx.globals(),
            ..v1::CreateCommandRequest::default()
        };
        let mut answers = Vec::new();
        if previewed {
            let preview = cx.served.create_command(&under, ask(true, Vec::new()))?;
            answers = edit_answers(cx, &preview.needs)?;
        }
        let made = cx.served.create_command(&under, ask(false, answers));
        return crate::answer_written(
            cx,
            made,
            |m| std::mem::take(&mut m.notices),
            out,
            err,
            |m, out| {
                let name = m.command.as_ref().map_or("", |c| c.name.as_str());
                text(name, m.bundle_created, out)
            },
            |_| Exit::OK,
        );
    }
    let ask = |validate_only: bool, answers: Vec<v1::Answer>| v1::CreateFragmentRequest {
        fragment_id: id.clone(),
        fragment: Some(v1::Fragment {
            body: body.clone().unwrap_or_default(),
            ..v1::Fragment::default()
        }),
        validate_only,
        answers,
        presentable: presentable(),
        globals: cx.globals(),
        ..v1::CreateFragmentRequest::default()
    };
    let mut answers = Vec::new();
    if previewed {
        let preview = cx.served.create_fragment(&under, ask(true, Vec::new()))?;
        answers = edit_answers(cx, &preview.needs)?;
    }
    let made = cx.served.create_fragment(&under, ask(false, answers));
    crate::answer_written(
        cx,
        made,
        |m| std::mem::take(&mut m.notices),
        out,
        err,
        |m, out| {
            let name = m.fragment.as_ref().map_or("", |f| f.name.as_str());
            text(name, m.bundle_created, out)
        },
        |_| Exit::OK,
    )
}

/// What an edit said, in text (Go's words): nothing changed, or the item
/// updated and how to publish it.
fn edited_text(
    word: &str,
    name: &str,
    applied: bool,
    no_distill: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> std::io::Result<()> {
    if !applied {
        return nothing(out);
    }
    let (bundle, item) = bundle_and_item(name);
    writeln!(out, "Updated {word} \"{item}\" in bundle \"{bundle}\"")?;
    writeln!(out)?;
    writeln!(out, "Bundle modified. To publish changes:")?;
    writeln!(out, "  ctxloom bundle push {bundle} [remote]")?;
    if no_distill {
        writeln!(err, "{PROG}: distilled form not refreshed (--no-distill)")?;
    }
    Ok(())
}

/// `fragment|command edit <ref> [--body] [--no-distill]`: the daemon's
/// `UpdateFragment` or `UpdateCommand` (step 10b U3). With no body, its
/// preview's Edit need is presented (the editor launched here, on the
/// body as it is), and the apply sends what the editor left.
fn edit_served(
    cx: &Context<'_>,
    kind: ItemKind,
    reference: &str,
    (body, no_distill): (Option<String>, bool),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let under = library_api::names::reference_target(reference);
    let previewed = body.is_none();
    let word = if kind == ItemKind::Command {
        "command"
    } else {
        "fragment"
    };
    let mut said = Vec::new();
    let code = if kind == ItemKind::Command {
        let ask = |validate_only: bool, answers: Vec<v1::Answer>| v1::UpdateCommandRequest {
            command: Some(v1::Command {
                body: body.clone().unwrap_or_default(),
                no_distill: no_distill.then_some(true),
                ..v1::Command::default()
            }),
            validate_only,
            answers,
            presentable: presentable(),
            globals: cx.globals(),
            ..v1::UpdateCommandRequest::default()
        };
        let mut answers = Vec::new();
        if previewed {
            let preview = cx.served.update_command(&under, ask(true, Vec::new()))?;
            answers = edit_answers(cx, &preview.needs)?;
        }
        let edited = cx.served.update_command(&under, ask(false, answers));
        crate::answer_written(
            cx,
            edited,
            |m| std::mem::take(&mut m.notices),
            out,
            err,
            |m, out| {
                let name = m.command.as_ref().map_or("", |c| c.name.as_str());
                let applied = m.applied.unwrap_or(false);
                edited_text(word, name, applied, no_distill && applied, out, &mut said)
            },
            |_| Exit::OK,
        )?
    } else {
        let ask = |validate_only: bool, answers: Vec<v1::Answer>| v1::UpdateFragmentRequest {
            fragment: Some(v1::Fragment {
                body: body.clone().unwrap_or_default(),
                no_distill: no_distill.then_some(true),
                ..v1::Fragment::default()
            }),
            validate_only,
            answers,
            presentable: presentable(),
            globals: cx.globals(),
            ..v1::UpdateFragmentRequest::default()
        };
        let mut answers = Vec::new();
        if previewed {
            let preview = cx.served.update_fragment(&under, ask(true, Vec::new()))?;
            answers = edit_answers(cx, &preview.needs)?;
        }
        let edited = cx.served.update_fragment(&under, ask(false, answers));
        crate::answer_written(
            cx,
            edited,
            |m| std::mem::take(&mut m.notices),
            out,
            err,
            |m, out| {
                let name = m.fragment.as_ref().map_or("", |f| f.name.as_str());
                let applied = m.applied.unwrap_or(false);
                edited_text(word, name, applied, no_distill && applied, out, &mut said)
            },
            |_| Exit::OK,
        )?
    };
    err.write_all(&said).map_err(crate::output_failure)?;
    Ok(code)
}

/// `fragment|command remove <ref> [--yes]`: the daemon's `DeleteFragment`
/// or `DeleteCommand` (step 10b U3), previewed and confirmed at a terminal.
fn remove_served(
    cx: &Context<'_>,
    kind: ItemKind,
    reference: &str,
    yes: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let under = library_api::names::reference_target(reference);
    let done = |name: &str, word: &str, out: &mut dyn Write| {
        let (bundle, item) = bundle_and_item(name);
        writeln!(out, "Removed {word} \"{item}\" from bundle \"{bundle}\"")
    };
    if kind == ItemKind::Command {
        return crate::write::removal_served(
            cx,
            yes,
            |validate_only, answers, etag| {
                let request = v1::DeleteCommandRequest {
                    validate_only,
                    answers,
                    etag,
                    globals: cx.globals(),
                    ..v1::DeleteCommandRequest::default()
                };
                cx.served.delete_command(&under, request)
            },
            out,
            err,
            |gone, out| {
                let name = gone.command.as_ref().map_or("", |c| c.name.as_str());
                done(name, "command", out)
            },
        );
    }
    crate::write::removal_served(
        cx,
        yes,
        |validate_only, answers, etag| {
            let request = v1::DeleteFragmentRequest {
                validate_only,
                answers,
                etag,
                globals: cx.globals(),
                ..v1::DeleteFragmentRequest::default()
            };
            cx.served.delete_fragment(&under, request)
        },
        out,
        err,
        |gone, out| {
            let name = gone.fragment.as_ref().map_or("", |f| f.name.as_str());
            done(name, "fragment", out)
        },
    )
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
