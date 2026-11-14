//! What the daemon's launcher needs from outside, beyond `run-ports`' edge,
//! locks and tokens: each is the root's to implement.

use std::collections::BTreeMap;
use std::sync::Arc;

use library_application::Previewed;
use run_ports::{ChildEngine, ChildObservation, ChildSpec, Engine, GitUser, SessionRecords};

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
    /// Whether it was written longer ago than a launch takes to record its
    /// child (the root's grace): one no session holds is swept only then,
    /// so a launch between its hand-off and its `LaunchChild` keeps it.
    pub settled: bool,
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

    /// The hand-off of `session` deleted, if there is one.
    fn delete(&self, session: &str);

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
