//! `agent`: list and show, and its writes (B2), each as its API resource or
//! its method's response (OD-40, design §3.7, §5.6). `agent default` still
//! answers in text only.

use library_api::resources::{agent, agent_name};

use crate::prelude::*;

pub(crate) fn read(
    cx: &Context<'_>,
    (verb, sub): (&str, &ArgMatches),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    // The daemon's reads (step 10a U2), their text from the message (D10).
    if verb == "show" {
        let request = v1::GetAgentRequest {
            globals: cx.globals(),
            ..v1::GetAgentRequest::default()
        };
        let under = library_api::names::named_target("agents", &text_arg(sub, "name"));
        let read = cx.served.get_agent(&under, request);
        return served(cx, read, out, err, false, |agent, out, _| {
            text::agent_show(out, &crate::read_back::agent_show(agent))
        });
    }
    let request = v1::ListAgentsRequest {
        globals: cx.globals(),
        ..v1::ListAgentsRequest::default()
    };
    let read = cx.served.list_agents("", request);
    served(cx, read, out, err, false, |listed, out, _| {
        text::agent_list(out, &crate::read_back::agent_rows(listed))
    })
}

pub(crate) fn write(
    cx: &Context<'_>,
    (verb, sub): (&str, &ArgMatches),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Option<Exit>, Failure> {
    let name = text_arg(sub, "name");
    let code = match verb {
        "create" | "edit" => {
            let request = agent_request(sub)?;
            if verb == "create" {
                let outcome = cx.library.agent_write(&cx.flags, &name, &request, false);
                api::written(
                    cx,
                    outcome,
                    out,
                    err,
                    |parent, row| v1::CreateAgentResponse {
                        agent: Some(agent(parent, row, &default_agent(cx))),
                        applied: Some(true),
                        ..v1::CreateAgentResponse::default()
                    },
                    |row, out, _| agent_written(row, "Created", out),
                    |_| Exit::OK,
                )?
            } else {
                let outcome = cx
                    .library
                    .agent_edit(&cx.flags, &name, &request, flag(sub, "yes"));
                api::written(
                    cx,
                    outcome,
                    out,
                    err,
                    |parent, value| edited(parent, value, &default_agent(cx)),
                    |value, out, _| match value {
                        AgentWritten::Preview(preview) => swap_preview_text(out, preview),
                        AgentWritten::Done(row) => agent_written(row, "Updated", out),
                    },
                    // A preview would change the binding (ruling 153).
                    |value| match value {
                        AgentWritten::Preview(_) => Exit::WOULD_CHANGE,
                        AgentWritten::Done(_) => Exit::OK,
                    },
                )?
            }
        }
        "default" => {
            text_only(cx, "agent default")?;
            let asked = given(sub, "name");
            let outcome = cx.library.agent_default(&cx.flags, asked.as_deref());
            finish_text(cx, outcome, out, err, |done, out, notes| match done {
                DefaultAgent::Current(current) if current.is_empty() => {
                    writeln!(out, "No default agent set.")
                }
                DefaultAgent::Current(current) => writeln!(out, "Default agent: {current}"),
                DefaultAgent::Set { name, defined } => {
                    if !defined {
                        notes.push(format!(
                            "agent \"{name}\" is not defined yet; define it with `ctxloom agent create {name}`"
                        ));
                    }
                    writeln!(out, "Set default agent to \"{name}\".")
                }
            })?
        }
        "remove" => {
            let under = library_api::names::named_target("agents", &name);
            crate::write::removal_served(
                cx,
                flag(sub, "yes"),
                |validate_only, answers, etag| {
                    let request = v1::DeleteAgentRequest {
                        validate_only,
                        answers,
                        etag,
                        globals: cx.globals(),
                        ..v1::DeleteAgentRequest::default()
                    };
                    cx.served.delete_agent(&under, request)
                },
                out,
                err,
                |_, out| writeln!(out, "Removed agent \"{name}\""),
            )?
        }
        _ => return Ok(None),
    };
    Ok(Some(code))
}

/// The agent a bare run binds, empty when none: what each agent's `default`
/// says (OQ-7). Asked only for a json answer, after the answer itself was
/// had, so its notices are the answer's own and are not said again.
fn default_agent(cx: &Context<'_>) -> String {
    match cx.library.agent_default(&cx.flags, None).result {
        Ok(DefaultAgent::Current(name)) => name,
        _ => String::new(),
    }
}

/// An agent known by its name alone (a preview's, a removal's).
fn named(parent: &str, agent: &str) -> v1::Agent {
    v1::Agent {
        name: agent_name(parent, agent),
        ..Default::default()
    }
}

/// `agent edit`: the binding written, or the swap's preview (its engines and
/// what the new one gives up; nothing written).
fn edited(parent: &str, value: &AgentWritten, default: &str) -> v1::UpdateAgentResponse {
    match value {
        AgentWritten::Preview(preview) => v1::UpdateAgentResponse {
            agent: Some(named(parent, &preview.agent)),
            applied: Some(false),
            apply_command: preview.apply.clone(),
            from: preview.from.clone(),
            to: preview.to.clone(),
            not_carried: preview.not_carried.iter().map(api::not_carried).collect(),
            ..v1::UpdateAgentResponse::default()
        },
        AgentWritten::Done(row) => v1::UpdateAgentResponse {
            agent: Some(agent(parent, row, default)),
            applied: Some(true),
            ..v1::UpdateAgentResponse::default()
        },
    }
}

fn agent_request(sub: &ArgMatches) -> Result<AgentRequest, Failure> {
    Ok(AgentRequest {
        engine: given(sub, "engine"),
        model: given(sub, "model"),
        effort: given(sub, "effort"),
        profiles: given_list(sub, "profiles"),
        runtime: given(sub, "runtime"),
        surfaces: given_pairs(sub, "surface")?,
        roots: given_pairs(sub, "root")?,
        env: given_list(sub, "env"),
        session_home: given(sub, "session-home"),
        home: flag(sub, "home"),
    })
}

/// The binding a create or an edit wrote, in the config's shape.
fn agent_written(row: &AgentRow, verb: &str, out: &mut dyn Write) -> io::Result<()> {
    let mut parts = Vec::new();
    if !row.engine.is_empty() {
        parts.push(format!("engine: {}", row.engine));
    }
    if !row.profiles.is_empty() {
        parts.push(format!("profiles: {}", row.profiles.join(", ")));
    }
    if !row.runtime.is_empty() {
        parts.push(format!("runtime: {}", row.runtime));
    }
    // Ruling 67: what is shown is what is written; ruling 139:
    // permissions are said to be passed on unchecked.
    for line in &row.engine_lines {
        parts.push(line.line.clone());
    }
    for line in row.engine_lines.iter().filter(|l| l.permissions) {
        parts.push(format!(
            "permissions {}",
            library_application::passed_verbatim(&line.engine)
        ));
    }
    if !row.env.is_empty() {
        parts.push(format!("env: {}", row.env.join(", ")));
    }
    if !row.session_home.is_empty() {
        parts.push(format!("session_home: {}", row.session_home));
    }
    writeln!(out, "{verb} agent \"{}\" ({})", row.name, parts.join(", "))
}

/// The swap's preview: what the new engine cannot carry, and how to apply it.
fn swap_preview_text(out: &mut dyn Write, preview: &SwapPreview) -> io::Result<()> {
    writeln!(
        out,
        "Moving agent \"{}\" from {} to {} gives up what {} cannot carry:",
        preview.agent, preview.from, preview.to, preview.to
    )?;
    for lost in &preview.not_carried {
        writeln!(out, "  {}", lost.line())?;
    }
    writeln!(out)?;
    writeln!(out, "Nothing was changed. Re-run with --yes to apply:")?;
    writeln!(out, "  {}", preview.apply)
}

#[cfg(test)]
#[path = "agent.test.rs"]
mod tests;
