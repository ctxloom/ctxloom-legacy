//! `run --dry-run` (B2): its json the `Run` it would create (OD-40, design
//! §5.9: `CreateRun` with `validate_only`), named under the project the
//! daemon names (owner ruling 2026-11-14: "dry run *absolutely* calls (and
//! spawns, if necessary) a daemon"), its references resource names (design
//! §2). It never mints the project's id: in a project nothing has named,
//! its name prints as null and its references keep their configured text.

use library_application::{DryRun, FindingRow};

use crate::prelude::*;

pub(crate) fn read(
    cx: &Context<'_>,
    sub: &ArgMatches,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    if !flag(sub, "dry-run") {
        return Err(AppError::RunNeedsDryRun.into_failure());
    }
    let mut prompt = text_arg(sub, "prompt");
    if prompt.is_empty() {
        prompt = many(sub, "prompt_words").join(" ");
    }
    let request = RunRequest {
        agent: text_arg(sub, "agent"),
        profile: text_arg(sub, "profile"),
        fragments: many(sub, "fragment"),
        tags: many(sub, "tag"),
        engine: text_arg(sub, "engine"),
        prompt,
        saved_command: text_arg(sub, "command"),
        workspace: text_arg(sub, "workspace"),
        no_context: flag(sub, "no-context"),
    };
    let outcome = cx.library.dry_run(&cx.flags, &request);
    say_notices(&outcome.notices, PROG, cx.format, cx.catalog, err);
    let previewed = outcome.result.map_err(failed)?;
    if cx.format == Format::Json {
        let parent = cx.names.named_parent()?;
        let run = planned(parent.as_deref(), &previewed.plan);
        // Unresolved, the name is null, never left out (owner 2026-11-14).
        daemon_proto::with_unresolved_names(&run, &["name"])
            .map_err(io::Error::other)
            .and_then(|printed| api::print(out, &printed))
    } else {
        text::dry_run(out, &previewed.plan)
    }
    .map_err(output_failure)?;
    // A dry run that found nothing fatal would launch (ruling 153): the
    // distinguishing success 1, said only under `--detailed-exitcode`.
    if previewed.fatal.is_empty() {
        return Ok(Exit::WOULD_CHANGE);
    }
    Err(AppError::aborted(&previewed.fatal).into_failure())
}

/// The `Run` a dry run would create, under `parent` (none: the project is
/// not named): the plan's agent, engine, workspace, runtime, profiles,
/// fragments, context and its token estimate, the prompt, the findings, and
/// the agent's settings on the engine. Its name is `<parent>/runs/-`, as
/// nothing is created and no run ID is given; its agent, engine and
/// profiles are resource names (design §2), their configured text where
/// the project has no name.
pub(crate) fn planned(parent: Option<&str>, plan: &DryRun) -> v1::Run {
    let named = |name: &dyn Fn(&str, &str) -> String, text: &str| match parent {
        Some(parent) if !text.is_empty() => name(parent, text),
        _ => text.to_string(),
    };
    v1::Run {
        name: parent.map_or_else(String::new, |parent| format!("{parent}/runs/-")),
        agent: named(&|p, a| format!("{p}/agents/{a}"), &plan.agent),
        engine: named(&crate::noun::engine::engine_name, &plan.engine),
        profiles: plan
            .profiles
            .iter()
            .map(|profile| named(&crate::catalog::profile_ref, profile))
            .collect(),
        context: plan.context.clone(),
        // A count (OQ-12); no context holds 2^32 tokens.
        tokens: u32::try_from(plan.tokens).unwrap_or(u32::MAX),
        findings: plan.findings.iter().map(finding).collect(),
        settings: plan.settings.as_ref().map(api::settings),
        workspace: plan.workspace.clone(),
        runtime: plan.runtime.clone(),
        prompt: plan.prompt.clone(),
        fragments: plan.fragments.clone(),
        ..v1::Run::default()
    }
}

fn finding(found: &FindingRow) -> v1::Finding {
    v1::Finding {
        kind: found.kind.clone(),
        text: found.text.clone(),
        remedy: found.remedy.clone(),
        fatal: found.fatal,
    }
}

#[cfg(test)]
#[path = "run.test.rs"]
mod tests;
