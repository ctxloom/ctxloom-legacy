//! The library's read refusals (step 10a U2, E5, owner 2026-11-17): each
//! decline of a library use case (`library_application::AppError`) is its
//! own typed refusal message (`ctxloom.v1.Library*`, library.proto), under
//! the status its `(refusal_status)` declares; a client maps it back to the
//! diagnostic a person is told by the message's TYPE alone
//! ([`rejection`]): the use case's own code and args, rendered from its
//! own catalog. A cause one wraps rides as its code and flattened args.
//! Its three tables (to the wire, back, and the tests' samples) list the
//! same variants in the same order, library.proto's.

use angzarr_diagnostic::{Arg, Diagnostic, Report};
use daemon_ports::Any;
use daemon_proto::v1;
use library_application::AppError;
use run_application::client::{flat_args, report_of};

use crate::refusal::ApiRefusal;

/// A wrapped diagnostic as the wire's `Cause`.
fn wire_cause(d: &dyn Diagnostic) -> Option<v1::Cause> {
    Some(v1::Cause {
        code: d.code().to_string(),
        args: flat_args(d),
    })
}

/// A `Cause` back as the diagnostic it was.
fn nested(cause: Option<v1::Cause>) -> Arg {
    let cause = cause.unwrap_or_default();
    Arg::Nested(Box::new(report_of(&cause.code, &cause.args)))
}

/// The code a library use case's diagnostic `suffix` is under.
fn code(suffix: &str) -> String {
    format!("library-application-{suffix}")
}

/// `message`, decoded from `refusal` when it is that type.
fn decode<T: prost::Message + prost::Name + Default>(refusal: &Any) -> Option<T> {
    refusal.to_msg::<T>().ok()
}

/// `error`'s typed refusal, under its message's status; none for what is
/// no decline (an adapter's failure, a distill's, fatal findings).
// A table of one arm per cause: its length is its variant count.
#[allow(clippy::too_many_lines)]
pub(crate) fn typed(error: &AppError) -> Option<ApiRefusal> {
    Some(match error {
        AppError::ConfigUnparsable { path, cause } => {
            ApiRefusal::of(&v1::LibraryConfigUnparsable {
                path: path.clone(),
                cause: wire_cause(cause.as_ref()),
            })
        }
        AppError::ConfigNewer {
            path,
            found,
            current,
        } => ApiRefusal::of(&v1::LibraryConfigNewer {
            path: path.clone(),
            found: *found,
            current: *current,
        }),
        AppError::ConfigOlder {
            path,
            found,
            oldest,
        } => ApiRefusal::of(&v1::LibraryConfigOlder {
            path: path.clone(),
            found: *found,
            oldest: *oldest,
        }),
        AppError::ConfigVersionUnreadable { path } => {
            ApiRefusal::of(&v1::LibraryConfigVersionUnreadable { path: path.clone() })
        }
        AppError::ConfigUnknownKey { violations } => ApiRefusal::of(&v1::LibraryConfigUnknownKey {
            violations: violations.clone(),
        }),
        AppError::ConfigBreaksSchema { violations } => {
            ApiRefusal::of(&v1::LibraryConfigBreaksSchema {
                violations: violations.clone(),
            })
        }
        AppError::UnknownSection { section, available } => {
            ApiRefusal::of(&v1::LibraryUnknownSection {
                section: section.clone(),
                available: available.clone(),
            })
        }
        AppError::BundleUnreadable { ask, cause } => ApiRefusal::of(&v1::LibraryBundleUnreadable {
            ask: ask.clone(),
            cause: wire_cause(cause.as_ref()),
        }),
        AppError::NoBundlesDir => ApiRefusal::of(&v1::LibraryNoBundlesDir {}),
        AppError::ViewPath { cause } => ApiRefusal::of(&v1::LibraryViewPath {
            cause: wire_cause(cause.as_ref()),
        }),
        AppError::ViewMissing { kind, name } => ApiRefusal::of(&v1::LibraryViewMissing {
            kind: kind.clone(),
            name: name.clone(),
        }),
        AppError::NotViewable { dir, path } => ApiRefusal::of(&v1::LibraryNotViewable {
            dir: dir.clone(),
            path: path.clone(),
        }),
        AppError::ItemMissing {
            kind,
            name,
            available,
        } => ApiRefusal::of(&v1::LibraryItemMissing {
            kind: kind.clone(),
            name: name.clone(),
            available: available.clone(),
        }),
        AppError::OperationalPromptMissing {
            bundle,
            prompt,
            dir,
        } => ApiRefusal::of(&v1::LibraryOperationalPromptMissing {
            bundle: bundle.clone(),
            prompt: prompt.clone(),
            dir: dir.clone(),
        }),
        AppError::BadReference { expected, got } => ApiRefusal::of(&v1::LibraryBadReference {
            expected: expected.clone(),
            got: got.clone(),
        }),
        AppError::WrongKind {
            ask,
            got,
            want,
            expected,
        } => ApiRefusal::of(&v1::LibraryWrongKind {
            ask: ask.clone(),
            got: got.clone(),
            want: want.clone(),
            expected: expected.clone(),
        }),
        AppError::UnsupportedFormat { command, format } => {
            ApiRefusal::of(&v1::LibraryUnsupportedFormat {
                command: command.clone(),
                format: format.clone(),
            })
        }
        AppError::SkillNotFound { name, bundle } => ApiRefusal::of(&v1::LibrarySkillNotFound {
            name: name.clone(),
            bundle: bundle.clone(),
        }),
        AppError::SkillNameNotFound { name } => {
            ApiRefusal::of(&v1::LibrarySkillNameNotFound { name: name.clone() })
        }
        AppError::NoBundleNamed { name, known } => ApiRefusal::of(&v1::LibraryNoBundleNamed {
            name: name.clone(),
            known: known.clone(),
        }),
        AppError::SearchNothing => ApiRefusal::of(&v1::LibrarySearchNothing {}),
        AppError::SearchType { value } => ApiRefusal::of(&v1::LibrarySearchType {
            value: value.clone(),
        }),
        AppError::SearchRemote => ApiRefusal::of(&v1::LibrarySearchRemote {}),
        AppError::AgentNotFound { name } => {
            ApiRefusal::of(&v1::LibraryAgentNotFound { name: name.clone() })
        }
        AppError::LaunchNoAgent { name } => {
            ApiRefusal::of(&v1::LibraryLaunchNoAgent { name: name.clone() })
        }
        AppError::LaunchNoProject => ApiRefusal::of(&v1::LibraryLaunchNoProject {}),
        AppError::LaunchNoDefault => ApiRefusal::of(&v1::LibraryLaunchNoDefault {}),
        AppError::LaunchNoAgents => ApiRefusal::of(&v1::LibraryLaunchNoAgents {}),
        AppError::McpNotFound { name } => {
            ApiRefusal::of(&v1::LibraryMcpNotFound { name: name.clone() })
        }
        AppError::UnknownEngine { engine, known } => ApiRefusal::of(&v1::LibraryUnknownEngine {
            engine: engine.clone(),
            known: known.clone(),
        }),
        AppError::EngineAuth { engine, why } => ApiRefusal::of(&v1::LibraryEngineAuth {
            engine: engine.clone(),
            why: why.clone(),
        }),
        AppError::Compose { profile, cause } => ApiRefusal::of(&v1::LibraryCompose {
            profile: profile.clone(),
            cause: wire_cause(cause.as_ref()),
        }),
        AppError::NoFragments { detail } => ApiRefusal::of(&v1::LibraryNoFragments {
            detail: detail.clone(),
        }),
        AppError::AgentExclusive { other } => ApiRefusal::of(&v1::LibraryAgentExclusive {
            other: other.clone(),
        }),
        AppError::NoContextExclusive { other } => ApiRefusal::of(&v1::LibraryNoContextExclusive {
            other: other.clone(),
        }),
        AppError::SavedCommand { name } => {
            ApiRefusal::of(&v1::LibrarySavedCommand { name: name.clone() })
        }
        AppError::Workspace { value } => ApiRefusal::of(&v1::LibraryWorkspace {
            value: value.clone(),
        }),
        AppError::RunNeedsDryRun => ApiRefusal::of(&v1::LibraryRunNeedsDryRun {}),
        AppError::Declined(declined) => ApiRefusal::of(&v1::LibraryDeclined {
            cause: wire_cause(declined.as_ref()),
        }),
        AppError::Cause(_)
        | AppError::DistillFailed { .. }
        | AppError::DistillTruncated { .. }
        | AppError::StartupAborted { .. } => return None,
    })
}

/// The diagnostic a library read's typed `refusal` is, by its type; none
/// for any other message.
// A table of one arm per cause: its length is its variant count.
#[allow(clippy::too_many_lines)]
pub fn rejection(refusal: &Any) -> Option<Report> {
    let name = refusal.type_url.rsplit('/').next().unwrap_or_default();
    Some(match name {
        "ctxloom.v1.LibraryConfigUnparsable" => {
            let r = decode::<v1::LibraryConfigUnparsable>(refusal)?;
            Report::new(code("config-unparsable"))
                .arg("path", Arg::Str(r.path))
                .arg("cause", nested(r.cause))
        }
        "ctxloom.v1.LibraryConfigNewer" => {
            let r = decode::<v1::LibraryConfigNewer>(refusal)?;
            Report::new(code("config-newer"))
                .arg("path", Arg::Str(r.path))
                .arg("found", Arg::Uint(r.found))
                .arg("current", Arg::Uint(r.current))
        }
        "ctxloom.v1.LibraryConfigOlder" => {
            let r = decode::<v1::LibraryConfigOlder>(refusal)?;
            Report::new(code("config-older"))
                .arg("path", Arg::Str(r.path))
                .arg("found", Arg::Uint(r.found))
                .arg("oldest", Arg::Uint(r.oldest))
        }
        "ctxloom.v1.LibraryConfigVersionUnreadable" => {
            let r = decode::<v1::LibraryConfigVersionUnreadable>(refusal)?;
            Report::new(code("config-version-unreadable")).arg("path", Arg::Str(r.path))
        }
        "ctxloom.v1.LibraryConfigUnknownKey" => {
            let r = decode::<v1::LibraryConfigUnknownKey>(refusal)?;
            Report::new(code("config-unknown-key")).arg("violations", Arg::Str(r.violations))
        }
        "ctxloom.v1.LibraryConfigBreaksSchema" => {
            let r = decode::<v1::LibraryConfigBreaksSchema>(refusal)?;
            Report::new(code("config-breaks-schema")).arg("violations", Arg::Str(r.violations))
        }
        "ctxloom.v1.LibraryUnknownSection" => {
            let r = decode::<v1::LibraryUnknownSection>(refusal)?;
            Report::new(code("unknown-section"))
                .arg("section", Arg::Str(r.section))
                .arg("available", Arg::Str(r.available))
        }
        "ctxloom.v1.LibraryBundleUnreadable" => {
            let r = decode::<v1::LibraryBundleUnreadable>(refusal)?;
            Report::new(code("bundle-unreadable"))
                .arg("ask", Arg::Str(r.ask))
                .arg("cause", nested(r.cause))
        }
        "ctxloom.v1.LibraryNoBundlesDir" => {
            decode::<v1::LibraryNoBundlesDir>(refusal)?;
            Report::new(code("no-bundles-dir"))
        }
        "ctxloom.v1.LibraryViewPath" => {
            let r = decode::<v1::LibraryViewPath>(refusal)?;
            Report::new(code("view-path")).arg("cause", nested(r.cause))
        }
        "ctxloom.v1.LibraryViewMissing" => {
            let r = decode::<v1::LibraryViewMissing>(refusal)?;
            Report::new(code("view-missing"))
                .arg("kind", Arg::Str(r.kind))
                .arg("name", Arg::Str(r.name))
        }
        "ctxloom.v1.LibraryNotViewable" => {
            let r = decode::<v1::LibraryNotViewable>(refusal)?;
            Report::new(code("not-viewable"))
                .arg("dir", Arg::Str(r.dir))
                .arg("path", Arg::Str(r.path))
        }
        "ctxloom.v1.LibraryItemMissing" => {
            let r = decode::<v1::LibraryItemMissing>(refusal)?;
            Report::new(code("item-missing"))
                .arg("kind", Arg::Str(r.kind))
                .arg("name", Arg::Str(r.name))
                .arg("available", Arg::Str(r.available))
        }
        "ctxloom.v1.LibraryOperationalPromptMissing" => {
            let r = decode::<v1::LibraryOperationalPromptMissing>(refusal)?;
            Report::new(code("operational-prompt-missing"))
                .arg("bundle", Arg::Str(r.bundle))
                .arg("prompt", Arg::Str(r.prompt))
                .arg("dir", Arg::Str(r.dir))
        }
        "ctxloom.v1.LibraryBadReference" => {
            let r = decode::<v1::LibraryBadReference>(refusal)?;
            Report::new(code("bad-reference"))
                .arg("expected", Arg::Str(r.expected))
                .arg("got", Arg::Str(r.got))
        }
        "ctxloom.v1.LibraryWrongKind" => {
            let r = decode::<v1::LibraryWrongKind>(refusal)?;
            Report::new(code("wrong-kind"))
                .arg("ask", Arg::Str(r.ask))
                .arg("got", Arg::Str(r.got))
                .arg("want", Arg::Str(r.want))
                .arg("expected", Arg::Str(r.expected))
        }
        "ctxloom.v1.LibraryUnsupportedFormat" => {
            let r = decode::<v1::LibraryUnsupportedFormat>(refusal)?;
            Report::new(code("unsupported-format"))
                .arg("command", Arg::Str(r.command))
                .arg("format", Arg::Str(r.format))
        }
        "ctxloom.v1.LibrarySkillNotFound" => {
            let r = decode::<v1::LibrarySkillNotFound>(refusal)?;
            Report::new(code("skill-not-found"))
                .arg("name", Arg::Str(r.name))
                .arg("bundle", Arg::Str(r.bundle))
        }
        "ctxloom.v1.LibrarySkillNameNotFound" => {
            let r = decode::<v1::LibrarySkillNameNotFound>(refusal)?;
            Report::new(code("skill-name-not-found")).arg("name", Arg::Str(r.name))
        }
        "ctxloom.v1.LibraryNoBundleNamed" => {
            let r = decode::<v1::LibraryNoBundleNamed>(refusal)?;
            Report::new(code("no-bundle-named"))
                .arg("name", Arg::Str(r.name))
                .arg("known", Arg::Str(r.known))
        }
        "ctxloom.v1.LibrarySearchNothing" => {
            decode::<v1::LibrarySearchNothing>(refusal)?;
            Report::new(code("search-nothing"))
        }
        "ctxloom.v1.LibrarySearchType" => {
            let r = decode::<v1::LibrarySearchType>(refusal)?;
            Report::new(code("search-type")).arg("value", Arg::Str(r.value))
        }
        "ctxloom.v1.LibrarySearchRemote" => {
            decode::<v1::LibrarySearchRemote>(refusal)?;
            Report::new(code("search-remote"))
        }
        "ctxloom.v1.LibraryAgentNotFound" => {
            let r = decode::<v1::LibraryAgentNotFound>(refusal)?;
            Report::new(code("agent-not-found")).arg("name", Arg::Str(r.name))
        }
        "ctxloom.v1.LibraryLaunchNoAgent" => {
            let r = decode::<v1::LibraryLaunchNoAgent>(refusal)?;
            Report::new(code("launch-no-agent")).arg("name", Arg::Str(r.name))
        }
        "ctxloom.v1.LibraryLaunchNoProject" => {
            decode::<v1::LibraryLaunchNoProject>(refusal)?;
            Report::new(code("launch-no-project"))
        }
        "ctxloom.v1.LibraryLaunchNoDefault" => {
            decode::<v1::LibraryLaunchNoDefault>(refusal)?;
            Report::new(code("launch-no-default"))
        }
        "ctxloom.v1.LibraryLaunchNoAgents" => {
            decode::<v1::LibraryLaunchNoAgents>(refusal)?;
            Report::new(code("launch-no-agents"))
        }
        "ctxloom.v1.LibraryMcpNotFound" => {
            let r = decode::<v1::LibraryMcpNotFound>(refusal)?;
            Report::new(code("mcp-not-found")).arg("name", Arg::Str(r.name))
        }
        "ctxloom.v1.LibraryUnknownEngine" => {
            let r = decode::<v1::LibraryUnknownEngine>(refusal)?;
            Report::new(code("unknown-engine"))
                .arg("engine", Arg::Str(r.engine))
                .arg("known", Arg::Str(r.known))
        }
        "ctxloom.v1.LibraryEngineAuth" => {
            let r = decode::<v1::LibraryEngineAuth>(refusal)?;
            Report::new(code("engine-auth"))
                .arg("engine", Arg::Str(r.engine))
                .arg("why", Arg::Str(r.why))
        }
        "ctxloom.v1.LibraryCompose" => {
            let r = decode::<v1::LibraryCompose>(refusal)?;
            Report::new(code("compose"))
                .arg("profile", Arg::Str(r.profile))
                .arg("cause", nested(r.cause))
        }
        "ctxloom.v1.LibraryNoFragments" => {
            let r = decode::<v1::LibraryNoFragments>(refusal)?;
            Report::new(code("no-fragments")).arg("detail", Arg::Str(r.detail))
        }
        "ctxloom.v1.LibraryAgentExclusive" => {
            let r = decode::<v1::LibraryAgentExclusive>(refusal)?;
            Report::new(code("agent-exclusive")).arg("other", Arg::Str(r.other))
        }
        "ctxloom.v1.LibraryNoContextExclusive" => {
            let r = decode::<v1::LibraryNoContextExclusive>(refusal)?;
            Report::new(code("no-context-exclusive")).arg("other", Arg::Str(r.other))
        }
        "ctxloom.v1.LibrarySavedCommand" => {
            let r = decode::<v1::LibrarySavedCommand>(refusal)?;
            Report::new(code("saved-command")).arg("name", Arg::Str(r.name))
        }
        "ctxloom.v1.LibraryWorkspace" => {
            let r = decode::<v1::LibraryWorkspace>(refusal)?;
            Report::new(code("workspace")).arg("value", Arg::Str(r.value))
        }
        "ctxloom.v1.LibraryRunNeedsDryRun" => {
            decode::<v1::LibraryRunNeedsDryRun>(refusal)?;
            Report::new(code("run-needs-dry-run"))
        }
        "ctxloom.v1.LibraryDeclined" => {
            let cause = decode::<v1::LibraryDeclined>(refusal)?
                .cause
                .unwrap_or_default();
            report_of(&cause.code, &cause.args)
        }
        _ => return None,
    })
}

/// One of every typed refusal, for the tests.
#[cfg(test)]
// A table of one arm per cause: its length is its variant count.
#[allow(clippy::too_many_lines)]
pub(crate) fn every() -> Vec<AppError> {
    let yaml = || angzarr_yaml::parse("a: [b\n").expect_err("an unterminated flow sequence");
    vec![
        AppError::ConfigUnparsable {
            path: "path-x".to_string(),
            cause: Box::new(yaml()),
        },
        AppError::ConfigNewer {
            path: "path-x".to_string(),
            found: 7,
            current: 7,
        },
        AppError::ConfigOlder {
            path: "path-x".to_string(),
            found: 7,
            oldest: 7,
        },
        AppError::ConfigVersionUnreadable {
            path: "path-x".to_string(),
        },
        AppError::ConfigUnknownKey {
            violations: "violations-x".to_string(),
        },
        AppError::ConfigBreaksSchema {
            violations: "violations-x".to_string(),
        },
        AppError::UnknownSection {
            section: "section-x".to_string(),
            available: "available-x".to_string(),
        },
        AppError::BundleUnreadable {
            ask: "ask-x".to_string(),
            cause: Box::new(library::BundleError::NoManifest {
                bundle: "demo".to_string(),
            }),
        },
        AppError::NoBundlesDir,
        AppError::ViewPath {
            cause: Box::new(library::RefError::Selector {
                selector: "fragments".to_string(),
            }),
        },
        AppError::ViewMissing {
            kind: "kind-x".to_string(),
            name: "name-x".to_string(),
        },
        AppError::NotViewable {
            dir: "dir-x".to_string(),
            path: "path-x".to_string(),
        },
        AppError::ItemMissing {
            kind: "kind-x".to_string(),
            name: "name-x".to_string(),
            available: "available-x".to_string(),
        },
        AppError::OperationalPromptMissing {
            bundle: "bundle-x".to_string(),
            prompt: "prompt-x".to_string(),
            dir: "dir-x".to_string(),
        },
        AppError::BadReference {
            expected: "expected-x".to_string(),
            got: "got-x".to_string(),
        },
        AppError::WrongKind {
            ask: "ask-x".to_string(),
            got: "got-x".to_string(),
            want: "want-x".to_string(),
            expected: "expected-x".to_string(),
        },
        AppError::UnsupportedFormat {
            command: "command-x".to_string(),
            format: "format-x".to_string(),
        },
        AppError::SkillNotFound {
            name: "name-x".to_string(),
            bundle: "bundle-x".to_string(),
        },
        AppError::SkillNameNotFound {
            name: "name-x".to_string(),
        },
        AppError::NoBundleNamed {
            name: "name-x".to_string(),
            known: "known-x".to_string(),
        },
        AppError::SearchNothing,
        AppError::SearchType {
            value: "value-x".to_string(),
        },
        AppError::SearchRemote,
        AppError::AgentNotFound {
            name: "name-x".to_string(),
        },
        AppError::LaunchNoAgent {
            name: "name-x".to_string(),
        },
        AppError::LaunchNoProject,
        AppError::LaunchNoDefault,
        AppError::LaunchNoAgents,
        AppError::McpNotFound {
            name: "name-x".to_string(),
        },
        AppError::UnknownEngine {
            engine: "engine-x".to_string(),
            known: "known-x".to_string(),
        },
        AppError::EngineAuth {
            engine: "engine-x".to_string(),
            why: "why-x".to_string(),
        },
        AppError::Compose {
            profile: "profile-x".to_string(),
            cause: Box::new(library::ProfileError::Circular {
                name: "dev".to_string(),
            }),
        },
        AppError::NoFragments {
            detail: "detail-x".to_string(),
        },
        AppError::AgentExclusive {
            other: "other-x".to_string(),
        },
        AppError::NoContextExclusive {
            other: "other-x".to_string(),
        },
        AppError::SavedCommand {
            name: "name-x".to_string(),
        },
        AppError::Workspace {
            value: "value-x".to_string(),
        },
        AppError::RunNeedsDryRun,
        AppError::Declined(Box::new(AppError::NoBundlesDir)),
    ]
}

#[cfg(test)]
#[path = "library_refusals.test.rs"]
mod tests;
