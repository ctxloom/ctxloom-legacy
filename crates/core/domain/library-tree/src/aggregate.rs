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
    BuiltinReadOnly, LibraryIgnored, NoSuchResource, NoSuchSlot, NotInGit, OneChangePerCall,
    PlanRejected, PlanStale, ResourceExists, SlotNotDue, TreeInMotion,
};
use crate::{
    ChangeFailed, ChangeIntended, ChangeStatus, CreateDirectory, DirtyPath, Evidence, External,
    HomeRepositoryMade, Intent, IntentSuperseded, Library, LibraryCommand, LibraryEvent,
    LibraryTreeRejection, MovePath, PurgeSlot, PurgeTrash, RecordHomeRepositoryMade,
    RecordStepDone, RecordStepFailed, RecordTreeObserved, ReplaceFile, RunSequence, Step,
    StepApplied, TrashPath, TreeObserved, KEEP_DAYS,
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

    /// `path` as ONE file (T29): refused when the record knows several
    /// files under it (a directory), each named below it.
    fn one_file(&self, path: &str) -> Result<(), LibraryTreeRejection> {
        let under = format!("{}/", path.trim_end_matches('/'));
        let paths: Vec<String> = self
            .library
            .resources
            .iter()
            .filter(|(key, resource)| key.starts_with(&under) && !resource.oid.is_empty())
            .map(|(key, _)| key[under.len()..].to_string())
            .collect();
        if paths.len() < 2 {
            return Ok(());
        }
        Err(LibraryTreeRejection::OneChangePerCall { paths })
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

/// What a step leaves, as an observation would see it: each path it
/// writes with the id it writes there; for a trash, `under` the path whose
/// every change is a deletion.
enum Leaves {
    Ids(Vec<(String, String)>),
    Gone(String),
    /// A move: not attributed by oid (U9's).
    Unknown,
}

fn leaves(step: &Step) -> Leaves {
    if let Some(replace) = &step.replace {
        return Leaves::Ids(vec![(replace.path.clone(), replace.new_oid.clone())]);
    }
    if let Some(create) = &step.create {
        let dir = create.path.trim_end_matches('/');
        return Leaves::Ids(
            create
                .entries
                .iter()
                .map(|entry| (format!("{dir}/{}", entry.path), entry.oid.clone()))
                .collect(),
        );
    }
    if let Some(trash) = &step.trash {
        return Leaves::Gone(trash.path.clone());
    }
    Leaves::Unknown
}

impl LibraryAggregate {
    /// The pending intent whose next step's new ids `changes` show (the
    /// first by request id), that step, and what the changes say of its
    /// paths.
    fn attributed(&self, changes: &[crate::Change]) -> Option<(String, u32, Vec<DirtyPath>)> {
        self.library.pending.values().find_map(|intent| {
            let step = intent.steps.get(intent.done as usize)?;
            let seen = match leaves(step) {
                Leaves::Ids(ids) if !ids.is_empty() => ids
                    .iter()
                    .map(|(path, oid)| {
                        changes
                            .iter()
                            .find(|c| &c.path == path && &c.to_oid == oid)
                            .map(|_| DirtyPath {
                                path: path.clone(),
                                oid: oid.clone(),
                            })
                    })
                    .collect::<Option<Vec<_>>>()?,
                Leaves::Gone(path) => {
                    let under = format!("{}/", path.trim_end_matches('/'));
                    let mine: Vec<_> = changes
                        .iter()
                        .filter(|c| c.path == path || c.path.starts_with(&under))
                        .collect();
                    if mine.is_empty() || mine.iter().any(|c| !c.to_oid.is_empty()) {
                        return None;
                    }
                    mine.iter()
                        .map(|c| DirtyPath {
                            path: c.path.clone(),
                            oid: String::new(),
                        })
                        .collect()
                }
                _ => return None,
            };
            Some((intent.request_id.clone(), intent.done, seen))
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
            LibraryIgnored,
            OneChangePerCall
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
            .and_then(|()| self.one_file(&cmd.path))
            .and_then(|()| self.fresh(&cmd.path, cmd.seen_sequence))
            .map_err(|rejection| {
                refuse!(
                    LibraryTreeRejection,
                    rejection,
                    Refuses<Self, ReplaceFile>,
                    [
                        OneChangePerCall { paths } => OneChangePerCall,
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
            purge: None,
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
            purge: None,
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
            purge: None,
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
        // Into another tree (QUESTIONS 1(a)), where it goes is that tree's
        // record, never this one's: its NOREPLACE rename judges it.
        let here = cmd.to_tree.is_empty();
        judge(&cmd.evidence, &cmd.from, Checks::default())
            .and_then(|()| self.present(&cmd.from))
            .and_then(|()| if here { self.absent(&cmd.to) } else { Ok(()) })
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
            purge: None,
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

    /// The purge of a kept trash slot (G12, L12): the slot must be in the
    /// state and, by the daemon's clock (its age the EVIDENCE: decide reads
    /// no clock), at least [`KEEP_DAYS`] old. Its one step removes the
    /// slot; its `StepApplied` drops it from the state.
    #[handles_cmd(PurgeTrash, refuses(NoSuchSlot, SlotNotDue))]
    fn purge(
        &self,
        cmd: PurgeTrash,
        ctx: &Context,
    ) -> Result<Option<ChangeIntended>, Failed<Refuses<Self, PurgeTrash>>> {
        let refused = if !self.library.trashed.contains_key(&cmd.request_id) {
            Some(LibraryTreeRejection::NoSuchSlot {
                request_id: cmd.request_id.clone(),
            })
        } else if cmd.age_days < KEEP_DAYS {
            Some(LibraryTreeRejection::SlotNotDue {
                request_id: cmd.request_id.clone(),
                age_days: cmd.age_days,
            })
        } else {
            None
        };
        if let Some(rejection) = refused {
            return Err(refuse!(
                LibraryTreeRejection,
                rejection,
                Refuses<Self, PurgeTrash>,
                [
                    NoSuchSlot { request_id } => NoSuchSlot,
                    SlotNotDue { request_id, age_days } => SlotNotDue,
                ]
            ));
        }
        let step = Step {
            replace: None,
            create: None,
            trash: None,
            r#move: None,
            purge: Some(PurgeSlot {
                request_id: cmd.request_id,
            }),
        };
        Ok(Some(self.intent(
            "trash purge".to_string(),
            vec![step],
            ctx,
        )))
    }

    /// An observation (L5): recorded when it adds anything to the state, a
    /// tree's first one always (its baseline, L16); another tree's, or one
    /// that adds nothing, records nothing.
    ///
    /// An observation that sees a pending intent's own new ids is the
    /// intent's (U3's attribution, by oid): its step done (`StepApplied`)
    /// when that is all it sees and the dirty set is what the step leaves,
    /// else a `TreeObserved` caused by the intent.
    #[handles_fact(RecordTreeObserved)]
    fn observed(&self, fact: RecordTreeObserved, ctx: &Context) -> Result<FactEmit<Self>, Bug> {
        let _ = ctx;
        let Some(mut observed) = self.observation(fact) else {
            return recorded(None::<TreeObserved>);
        };
        // A baseline, or a sight past a moved HEAD (a commit, a checkout),
        // is the outside's: its cause is the commits.
        let moved = observed
            .external
            .as_ref()
            .is_some_and(|external| external.head_after != self.library.head);
        if observed.baseline || moved {
            return recorded(Some(observed));
        }
        let Some((request_id, step, seen)) = self.attributed(&observed.changes) else {
            return recorded(Some(observed));
        };
        let whole = seen.len() == observed.changes.len()
            && observed.tree_after == self.library.tree
            && self.library.dirty_after(&seen)
                == observed
                    .dirty_after
                    .iter()
                    .map(|d| (d.path.clone(), d.oid.clone()))
                    .collect();
        if whole {
            return recorded(Some(StepApplied {
                request_id,
                step,
                observed: seen,
            }));
        }
        observed.external = None;
        observed.intended = Some(request_id);
        recorded(Some(observed))
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

    /// The home step made this tree a repository of ctxloom's own (G3):
    /// recorded once; said again, it adds nothing.
    #[handles_fact(RecordHomeRepositoryMade)]
    fn home_repository_made(
        &self,
        fact: RecordHomeRepositoryMade,
        ctx: &Context,
    ) -> Result<FactEmit<Self>, Bug> {
        let _ = ctx;
        recorded(
            (!self.library.home_repository_made).then_some(HomeRepositoryMade { name: fact.name }),
        )
    }

    #[applies(HomeRepositoryMade)]
    fn home_repository_was_made(&mut self, event: HomeRepositoryMade, page: &PageContext) {
        self.library.apply_page(
            &LibraryEvent::HomeRepositoryMade(event),
            page.sequence,
            page.meta,
        );
    }

    #[applies(TreeObserved)]
    fn tree_observed(&mut self, event: TreeObserved, page: &PageContext) {
        self.library
            .apply_page(&LibraryEvent::TreeObserved(event), page.sequence, page.meta);
    }

    #[applies(ChangeIntended)]
    fn change_intended(&mut self, event: ChangeIntended, page: &PageContext) {
        self.library.apply_page(
            &LibraryEvent::ChangeIntended(event),
            page.sequence,
            page.meta,
        );
    }

    #[applies(StepApplied)]
    fn step_applied(&mut self, event: StepApplied, page: &PageContext) {
        self.library
            .apply_page(&LibraryEvent::StepApplied(event), page.sequence, page.meta);
    }

    #[applies(ChangeFailed)]
    fn change_failed(&mut self, event: ChangeFailed, page: &PageContext) {
        self.library
            .apply_page(&LibraryEvent::ChangeFailed(event), page.sequence, page.meta);
    }

    #[applies(IntentSuperseded)]
    fn intent_superseded(&mut self, event: IntentSuperseded, page: &PageContext) {
        self.library.apply_page(
            &LibraryEvent::IntentSuperseded(event),
            page.sequence,
            page.meta,
        );
    }
}

#[cfg(test)]
#[path = "aggregate.test.rs"]
mod tests;
