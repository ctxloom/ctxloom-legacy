//! The library aggregate on angzarr-embedded's markup: one stream per tree.
//!
//! Commands are operation-shaped (L17) and decided on evidence (L14): the
//! application looks, `decide` judges what it found against the state and
//! the token the caller saw (G4 superseded), and records an INTENT. Facts
//! are what already happened (an observation, a step done or failed) and
//! are never refused: one the tree cannot use records nothing.

// The `#[aggregate]` macro generates the facts' enum from its handlers, and
// every fact is `Record*` (the naming rule), so its variants share the
// prefix by design.
#![allow(clippy::enum_variant_names)]

use angzarr_embedded::markup::{
    pack_message, Bug, Context, DomainMessage, FactEmit, Failed, PageContext, Refuses,
};
use angzarr_embedded_macros::{aggregate, applies, handles_cmd, handles_fact};

use crate::refusal::refuse;
use crate::{
    cause_of, ChangeFailed, ChangeIntended, ChangeStatus, CreateDirectory, Evidence, External,
    Intent, IntentSuperseded, Library, LibraryCommand, LibraryEvent, LibraryTreeRejection,
    MovePath, RecordStepDone, RecordStepFailed, RecordTreeObserved, ReplaceFile, RunSequence, Step,
    StepApplied, TrashPath, TreeObserved,
};
use crate::{
    BuiltinReadOnly, LibraryIgnored, NoSuchResource, NotInGit, PlanRejected, PlanStale,
    ResourceExists, TreeInMotion,
};

/// The code a plan's rejection takes when the tree no longer reads and the
/// plan named no diagnostic of its own.
pub const TREE_UNREADABLE: &str = "library-tree-tree-unreadable";

/// The library aggregate: one tree, folded from its stream.
#[derive(Clone, Debug, Default)]
pub struct LibraryAggregate {
    library: Library,
}

impl LibraryAggregate {
    /// The folded tree.
    pub fn state(&self) -> &Library {
        &self.library
    }
}

/// Which of the evidence's checks a command answers to: each command
/// judges only what its `refuses` lists.
#[derive(Clone, Copy, Default)]
struct Checks {
    plan: bool,
    builtin: bool,
}

/// The bundle a library path is in (`content/bundles/v2/<bundle>/...`),
/// else the path itself: what `BuiltinReadOnly` names.
fn bundle_of(path: &str) -> String {
    path.strip_prefix("content/bundles/v2/")
        .and_then(|rest| rest.split('/').next())
        .filter(|bundle| !bundle.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// The evidence's verdict, in order: git, motion, the built-in bundle, the
/// plan. None when it all holds.
fn judge(evidence: &Evidence, path: &str, checks: Checks) -> Result<(), LibraryTreeRejection> {
    if !evidence.in_git {
        return Err(match &evidence.ignored {
            Some(ignored) => LibraryTreeRejection::LibraryIgnored {
                source: ignored.source.clone(),
                line: ignored.line,
                pattern: ignored.pattern.clone(),
            },
            None => LibraryTreeRejection::NotInGit {
                root: evidence.root.clone(),
            },
        });
    }
    if let Some(motion) = evidence.in_motion {
        return Err(LibraryTreeRejection::TreeInMotion {
            motion,
            root: evidence.root.clone(),
        });
    }
    if checks.builtin && evidence.builtin {
        return Err(LibraryTreeRejection::BuiltinReadOnly {
            bundle: bundle_of(path),
        });
    }
    if checks.plan {
        if let Some(rejected) = &evidence.plan_rejection {
            return Err(LibraryTreeRejection::PlanRejected {
                code: rejected.code.clone(),
                args: rejected.args.clone(),
            });
        }
        if !evidence.tree_reads {
            return Err(LibraryTreeRejection::PlanRejected {
                code: TREE_UNREADABLE.to_string(),
                args: Default::default(),
            });
        }
    }
    Ok(())
}

impl LibraryAggregate {
    /// `path`'s token now, refused stale when the caller saw another.
    fn fresh(&self, path: &str, seen: u64) -> Result<(), LibraryTreeRejection> {
        let now = self.library.token(path);
        if seen == now {
            return Ok(());
        }
        Err(LibraryTreeRejection::PlanStale {
            path: path.to_string(),
            seen,
            now,
        })
    }

    fn present(&self, path: &str) -> Result<(), LibraryTreeRejection> {
        if self.library.known_absent(path) {
            return Err(LibraryTreeRejection::NoSuchResource {
                path: path.to_string(),
            });
        }
        Ok(())
    }

    fn absent(&self, path: &str) -> Result<(), LibraryTreeRejection> {
        if self.library.known_present(path) {
            return Err(LibraryTreeRejection::ResourceExists {
                path: path.to_string(),
            });
        }
        Ok(())
    }

    /// One step of a sequence judged on the token its caller saw. Not on
    /// existence: a sequence refuses only what its `refuses` lists, and a
    /// retry resumes steps whose paths an earlier try already made or took
    /// away (T28), so the operation's own NOREPLACE is the word on that
    /// (10b, U8).
    fn step(&self, step: &Step) -> Result<(), LibraryTreeRejection> {
        if let Some(replace) = &step.replace {
            return self.fresh(&replace.path, replace.seen_sequence);
        }
        if let Some(trash) = &step.trash {
            return self.fresh(&trash.path, trash.seen_sequence);
        }
        if let Some(moved) = &step.r#move {
            return self.fresh(&moved.from, moved.seen_sequence);
        }
        Ok(())
    }

    /// The intent of the command `ctx` carries, its steps `steps`.
    fn intent(&self, verb: String, steps: Vec<Step>, ctx: &Context) -> ChangeIntended {
        let request_id = ctx
            .command
            .command_id
            .as_ref()
            .and_then(|id| uuid::Uuid::from_slice(&id.value).ok())
            .map(|id| id.to_string())
            .unwrap_or_default();
        ChangeIntended {
            intent: Intent {
                request_id,
                verb,
                steps,
                base_cause: self.library.last_cause.clone(),
                done: 0,
            },
        }
    }

    /// What an observation adds to the state: its changes less those the
    /// record already holds at the same oid (an observation made against
    /// an older state, whose base another has recorded since, or one that
    /// says again what was said). None when it adds nothing.
    fn observation(&self, fact: RecordTreeObserved) -> Option<TreeObserved> {
        let library = &self.library;
        if !library.name.is_empty() && fact.name != library.name {
            return None;
        }
        let stale = fact.base_cause != library.last_cause;
        let changes: Vec<_> = fact
            .changes
            .into_iter()
            .filter(|change| {
                library.resources.get(&change.path).map(|r| r.oid.as_str())
                    != Some(change.to_oid.as_str())
                    || (change.status == ChangeStatus::Renamed
                        && library
                            .resources
                            .get(&change.from_path)
                            .is_some_and(|r| !r.oid.is_empty()))
            })
            .collect();
        let dirty_after: std::collections::BTreeMap<_, _> = fact
            .dirty_after
            .iter()
            .map(|dirty| (dirty.path.clone(), dirty.oid.clone()))
            .collect();
        let first = !library.baselined;
        let baseline = fact.baseline || first;
        // HEAD alone moving is nothing (QUESTIONS A), and nor is a motion
        // alone: a read during a merge that sees what the last one saw
        // records nothing more.
        let same =
            changes.is_empty() && dirty_after == library.dirty && fact.tree_after == library.tree;
        if same && !baseline {
            return None;
        }
        if same && fact.baseline && stale {
            // A baseline someone else recorded since this one was made.
            return None;
        }
        Some(TreeObserved {
            name: fact.name,
            external: Some(External {
                head_before: fact.head_before,
                head_after: fact.head_after,
            }),
            intended: None,
            changes,
            dirty_after: fact.dirty_after,
            tree_after: fact.tree_after,
            baseline,
            in_motion: fact.in_motion,
        })
    }
}

/// An event a fact records, or none.
fn recorded<E: DomainMessage>(event: Option<E>) -> Result<FactEmit<LibraryAggregate>, Bug> {
    Ok(FactEmit::of(event.map(|event| pack_message(&event))))
}

#[aggregate(domain = "library", command = LibraryCommand, event = LibraryEvent)]
impl LibraryAggregate {
    /// Replace (or create) one file: judged on git, motion, the built-in
    /// bundle and the plan, then on the path's token.
    #[handles_cmd(
        ReplaceFile,
        refuses(
            PlanStale,
            BuiltinReadOnly,
            PlanRejected,
            TreeInMotion,
            NotInGit,
            LibraryIgnored
        )
    )]
    fn replace(
        &self,
        cmd: ReplaceFile,
        ctx: &Context,
    ) -> Result<Option<ChangeIntended>, Failed<Refuses<Self, ReplaceFile>>> {
        let checks = Checks {
            plan: true,
            builtin: true,
        };
        judge(&cmd.evidence, &cmd.path, checks)
            .and_then(|()| self.fresh(&cmd.path, cmd.seen_sequence))
            .map_err(|rejection| {
                refuse!(
                    LibraryTreeRejection,
                    rejection,
                    Refuses<Self, ReplaceFile>,
                    [
                        PlanStale { path, seen, now } => PlanStale,
                        BuiltinReadOnly { bundle } => BuiltinReadOnly,
                        PlanRejected { code, args } => PlanRejected,
                        TreeInMotion { motion, root } => TreeInMotion,
                        NotInGit { root } => NotInGit,
                        LibraryIgnored { source, line, pattern } => LibraryIgnored,
                    ]
                )
            })?;
        let verb = cmd.verb.clone();
        let step = Step {
            replace: Some(cmd),
            create: None,
            trash: None,
            r#move: None,
        };
        Ok(Some(self.intent(verb, vec![step], ctx)))
    }

    /// A new directory, never over something the record knows is there.
    #[handles_cmd(
        CreateDirectory,
        refuses(
            ResourceExists,
            BuiltinReadOnly,
            PlanRejected,
            TreeInMotion,
            NotInGit,
            LibraryIgnored
        )
    )]
    fn create(
        &self,
        cmd: CreateDirectory,
        ctx: &Context,
    ) -> Result<Option<ChangeIntended>, Failed<Refuses<Self, CreateDirectory>>> {
        let checks = Checks {
            plan: true,
            builtin: true,
        };
        judge(&cmd.evidence, &cmd.path, checks)
            .and_then(|()| self.absent(&cmd.path))
            .map_err(|rejection| {
                refuse!(
                    LibraryTreeRejection,
                    rejection,
                    Refuses<Self, CreateDirectory>,
                    [
                        ResourceExists { path } => ResourceExists,
                        BuiltinReadOnly { bundle } => BuiltinReadOnly,
                        PlanRejected { code, args } => PlanRejected,
                        TreeInMotion { motion, root } => TreeInMotion,
                        NotInGit { root } => NotInGit,
                        LibraryIgnored { source, line, pattern } => LibraryIgnored,
                    ]
                )
            })?;
        let verb = cmd.verb.clone();
        let step = Step {
            replace: None,
            create: Some(cmd),
            trash: None,
            r#move: None,
        };
        Ok(Some(self.intent(verb, vec![step], ctx)))
    }

    /// A path into the trash: it must not be known gone, and its token
    /// must be the one the caller saw.
    #[handles_cmd(
        TrashPath,
        refuses(
            NoSuchResource,
            PlanStale,
            BuiltinReadOnly,
            TreeInMotion,
            NotInGit,
            LibraryIgnored
        )
    )]
    fn trash(
        &self,
        cmd: TrashPath,
        ctx: &Context,
    ) -> Result<Option<ChangeIntended>, Failed<Refuses<Self, TrashPath>>> {
        let checks = Checks {
            plan: false,
            builtin: true,
        };
        judge(&cmd.evidence, &cmd.path, checks)
            .and_then(|()| self.present(&cmd.path))
            .and_then(|()| self.fresh(&cmd.path, cmd.seen_sequence))
            .map_err(|rejection| {
                refuse!(
                    LibraryTreeRejection,
                    rejection,
                    Refuses<Self, TrashPath>,
                    [
                        NoSuchResource { path } => NoSuchResource,
                        PlanStale { path, seen, now } => PlanStale,
                        BuiltinReadOnly { bundle } => BuiltinReadOnly,
                        TreeInMotion { motion, root } => TreeInMotion,
                        NotInGit { root } => NotInGit,
                        LibraryIgnored { source, line, pattern } => LibraryIgnored,
                    ]
                )
            })?;
        let verb = cmd.verb.clone();
        let step = Step {
            replace: None,
            create: None,
            trash: Some(cmd),
            r#move: None,
        };
        Ok(Some(self.intent(verb, vec![step], ctx)))
    }

    /// ctxloom's own move: from a path not known gone, to one not known
    /// there, `from`'s token the one the caller saw.
    #[handles_cmd(
        MovePath,
        refuses(
            NoSuchResource,
            ResourceExists,
            PlanStale,
            TreeInMotion,
            NotInGit,
            LibraryIgnored
        )
    )]
    fn move_path(
        &self,
        cmd: MovePath,
        ctx: &Context,
    ) -> Result<Option<ChangeIntended>, Failed<Refuses<Self, MovePath>>> {
        judge(&cmd.evidence, &cmd.from, Checks::default())
            .and_then(|()| self.present(&cmd.from))
            .and_then(|()| self.absent(&cmd.to))
            .and_then(|()| self.fresh(&cmd.from, cmd.seen_sequence))
            .map_err(|rejection| {
                refuse!(
                    LibraryTreeRejection,
                    rejection,
                    Refuses<Self, MovePath>,
                    [
                        NoSuchResource { path } => NoSuchResource,
                        ResourceExists { path } => ResourceExists,
                        PlanStale { path, seen, now } => PlanStale,
                        TreeInMotion { motion, root } => TreeInMotion,
                        NotInGit { root } => NotInGit,
                        LibraryIgnored { source, line, pattern } => LibraryIgnored,
                    ]
                )
            })?;
        let verb = cmd.verb.clone();
        let step = Step {
            replace: None,
            create: None,
            trash: None,
            r#move: Some(cmd),
        };
        Ok(Some(self.intent(verb, vec![step], ctx)))
    }

    /// A retryable sequence (T28): its evidence judged once, each step's
    /// token judged as its own command's; the steps kept in the intent.
    #[handles_cmd(
        RunSequence,
        refuses(PlanStale, PlanRejected, TreeInMotion, NotInGit, LibraryIgnored)
    )]
    fn sequence(
        &self,
        cmd: RunSequence,
        ctx: &Context,
    ) -> Result<Option<ChangeIntended>, Failed<Refuses<Self, RunSequence>>> {
        let checks = Checks {
            plan: true,
            builtin: false,
        };
        judge(&cmd.evidence, "", checks)
            .and_then(|()| cmd.steps.iter().try_for_each(|step| self.step(step)))
            .map_err(|rejection| {
                refuse!(
                    LibraryTreeRejection,
                    rejection,
                    Refuses<Self, RunSequence>,
                    [
                        PlanStale { path, seen, now } => PlanStale,
                        PlanRejected { code, args } => PlanRejected,
                        TreeInMotion { motion, root } => TreeInMotion,
                        NotInGit { root } => NotInGit,
                        LibraryIgnored { source, line, pattern } => LibraryIgnored,
                    ]
                )
            })?;
        Ok(Some(self.intent(cmd.verb, cmd.steps, ctx)))
    }

    /// An observation (L5): recorded when it adds anything to the state, a
    /// tree's first one always (its baseline, L16); another tree's, or one
    /// that adds nothing, records nothing.
    #[handles_fact(RecordTreeObserved)]
    fn observed(&self, fact: RecordTreeObserved, ctx: &Context) -> Result<FactEmit<Self>, Bug> {
        let _ = ctx;
        recorded(self.observation(fact))
    }

    /// A pending intent's next step done: what it left becomes its paths'
    /// record at this event's sequence. A step not pending records nothing.
    #[handles_fact(RecordStepDone)]
    fn step_done(&self, fact: RecordStepDone, ctx: &Context) -> Result<FactEmit<Self>, Bug> {
        let _ = ctx;
        let pending = self
            .library
            .pending
            .get(&fact.request_id)
            .is_some_and(|intent| fact.step == intent.done);
        recorded(pending.then_some(StepApplied {
            request_id: fact.request_id,
            step: fact.step,
            observed: fact.observed,
        }))
    }

    /// A pending intent's step failed: final.
    #[handles_fact(RecordStepFailed)]
    fn step_failed(&self, fact: RecordStepFailed, ctx: &Context) -> Result<FactEmit<Self>, Bug> {
        let _ = ctx;
        let pending = self.library.pending.contains_key(&fact.request_id);
        recorded(pending.then_some(ChangeFailed {
            request_id: fact.request_id,
            step: fact.step,
            ext_err_txt: fact.ext_err_txt,
        }))
    }

    #[applies(TreeObserved)]
    fn tree_observed(&mut self, event: TreeObserved, page: &PageContext) {
        self.library.apply(
            &LibraryEvent::TreeObserved(event),
            page.sequence,
            cause_of(page.meta),
        );
    }

    #[applies(ChangeIntended)]
    fn change_intended(&mut self, event: ChangeIntended, page: &PageContext) {
        self.library.apply(
            &LibraryEvent::ChangeIntended(event),
            page.sequence,
            cause_of(page.meta),
        );
    }

    #[applies(StepApplied)]
    fn step_applied(&mut self, event: StepApplied, page: &PageContext) {
        self.library.apply(
            &LibraryEvent::StepApplied(event),
            page.sequence,
            cause_of(page.meta),
        );
    }

    #[applies(ChangeFailed)]
    fn change_failed(&mut self, event: ChangeFailed, page: &PageContext) {
        self.library.apply(
            &LibraryEvent::ChangeFailed(event),
            page.sequence,
            cause_of(page.meta),
        );
    }

    #[applies(IntentSuperseded)]
    fn intent_superseded(&mut self, event: IntentSuperseded, page: &PageContext) {
        self.library.apply(
            &LibraryEvent::IntentSuperseded(event),
            page.sequence,
            cause_of(page.meta),
        );
    }
}

#[cfg(test)]
#[path = "aggregate.test.rs"]
mod tests;
