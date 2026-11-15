//! What the daemon's launcher needs from outside, beyond `run-ports`' edge,
//! locks and tokens: each is the root's to implement.

use std::collections::BTreeMap;
use std::sync::Arc;

use library_application::Previewed;
use run_ports::{
    ChildEngine, ChildObservation, ChildSpec, Engine, GitUser, HeldLock, SessionRecords,
};

/// Where an engine keeps a child's transcripts, from the child's spec.
pub type Transcripts = Arc<dyn Fn(&ChildSpec) -> Option<String> + Send + Sync>;

/// The engine a launch sets its session home up and starts its child with,
/// composed for that launch from what the launcher said of its user's
/// settings (T21: her settings are read at every launch, where HER launcher
/// finds them: its `CLAUDE_CONFIG_DIR`, its XDG bases, S7).
pub type ForLaunch =
    Arc<dyn Fn(&BTreeMap<String, String>) -> (Arc<dyn Engine>, Arc<dyn ChildEngine>) + Send + Sync>;

/// One engine the daemon can host a child on, by its name: its `Engine` (the
/// session home it prepares, its credentials) beside its `ChildEngine` (the
/// process it starts), as the root's engine registry composes them.
#[derive(Clone)]
pub struct HostedEngine {
    pub engine: Arc<dyn Engine>,
    pub child: Arc<dyn ChildEngine>,
    /// Where the engine keeps a child's transcripts, if it says (a bind
    /// names the transcript of the native id there).
    pub transcripts: Transcripts,
    /// The engine each launch is composed with, from its launcher's places
    /// ([`HandOff::launcher`]); none: `engine` and `child` as they are.
    pub for_launch: Option<ForLaunch>,
}

impl HostedEngine {
    /// The engine's name.
    pub fn name(&self) -> &str {
        self.engine.name()
    }

    /// The engine and the child engine a launch whose launcher said
    /// `launcher` of its user's settings is set up and started with.
    pub fn launched(
        &self,
        launcher: &BTreeMap<String, String>,
    ) -> (Arc<dyn Engine>, Arc<dyn ChildEngine>) {
        match &self.for_launch {
            Some(compose) => compose(launcher),
            None => (Arc::clone(&self.engine), Arc::clone(&self.child)),
        }
    }
}

/// What a child's launcher handed the daemon (D-N45, S7): the environment
/// the child may be given, where the launcher finds its user's own settings
/// (the variables that place them: `CLAUDE_CONFIG_DIR`, the XDG bases),
/// which the launch reads her settings from and never hands the child, and
/// the agent it is planned for (empty: the project's default).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandOff {
    pub env: BTreeMap<String, String>,
    pub launcher: BTreeMap<String, String>,
    pub agent: String,
}

/// A hand-off file there is, by its child's stable id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    pub session: String,
    /// Whether it is marked in launch: its `CreateHandOff` marked it, and no
    /// `LaunchChild` has claimed it yet.
    pub marked: bool,
    /// Whether the creator of its launch is still there: it holds the
    /// launch's lock (the platform's file lock, never a pid or a timer). A
    /// mark whose creator is proven gone (a crash mid-launch) is cleared.
    pub creator_holds: bool,
}

/// A child's plan: its project's dry run, as a ctxloom process in that
/// project would make it, on `engine`, with `env` (its launcher's
/// environment). The error is the plan's code.
pub trait ChildPlans: Send + Sync {
    /// The plan for `agent` (empty: the project's default).
    fn preview(
        &self,
        engine: &str,
        agent: &str,
        project_dir: &str,
        env: &BTreeMap<String, String>,
    ) -> Result<Previewed, String>;

    /// The user's git identity in the project at `project_dir`, as git
    /// resolves it there in her own environment (`env`, her launcher's, and
    /// her home): what the child's session home is seeded with (owner
    /// ruling 112), as a CLI run's is. None when git knows neither part.
    fn git_user(&self, project_dir: &str, env: &BTreeMap<String, String>) -> Option<GitUser>;
}

/// The hand-off files a child's environment is kept in for its life (T6;
/// the 0600 files of `~/.ctxloom/daemon/launch/`, written by the daemon's
/// `CreateHandOff`): READ at each start of the child, a start after a
/// daemon restart included, deleted at its close, and swept when no open
/// session holds one.
pub trait HandOffs: Send + Sync {
    /// The hand-off of `session`; none when there is none, or it does not
    /// read.
    fn read(&self, session: &str) -> Option<HandOff>;

    /// The hand-off of `session` deleted, and its mark, if there are any.
    fn delete(&self, session: &str);

    /// The hand-off of `session` claimed by the `LaunchChild` it was made
    /// for: its mark cleared, the hand-off kept.
    fn claim(&self, session: &str);

    /// Every hand-off file there is.
    fn list(&self) -> Vec<Kept>;
}

/// The session directories of a child's project: the same port the one
/// launcher prepares every other session's home through
/// (`run_ports::SessionRecords`), so a hosted child's home is set up by the
/// same code.
pub trait ChildHomes: Send + Sync {
    /// The session directories of the project at `project_dir`.
    fn records(&self, project_dir: &str) -> Box<dyn SessionRecords>;

    /// Take the lock of session `harp` in the project at `project_dir`
    /// (`<project>/.ctxloom/sessions/<harp>.lock`, the platform's file lock,
    /// at once or not at all), held for the hosted child's life, so its
    /// session reads live and no sweep (a one-shot's launcher's included)
    /// takes it for lost while its host holds it. Why not, when it cannot
    /// be had (another holds it: the session runs elsewhere).
    fn hold_session(&self, project_dir: &str, harp: &str) -> Result<HeldLock, String>;
}

/// Where a hosted child works (owner s4-Q11): its agent's `workspace`, the
/// live project or a checkout of its session's own, placed by the one
/// launch through the project's workspaces (worktree-git's, under
/// ctxloom's home) and let go WIP-safe at the child's close and, for a
/// lost one, at the next start's sweep (OD-19).
pub trait ChildWorkspaces: Send + Sync {
    /// The workspaces of the project `project_id` at `project_dir`.
    fn of(
        &self,
        project_id: &str,
        project_dir: &str,
    ) -> Arc<dyn isolation_ports::Workspaces + Send + Sync>;
}

/// What hosted children say, heard off the launcher's thread: the root
/// pumps each child's observations (a blocking iterator) and hands them back
/// as they come, by run.
pub trait Ears: Send + Sync {
    /// Listen to `observations`, the child of `run`'s.
    fn listen(&self, run: String, observations: Box<dyn Iterator<Item = ChildObservation> + Send>);

    /// What was heard since last asked, in the order heard.
    fn heard(&self) -> Vec<(String, ChildObservation)>;
}
