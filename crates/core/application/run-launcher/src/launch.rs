//! THE one launch (T10): [`launch_of`] makes every engine process ctxloom
//! starts into a [`Launch`], in one order, for every kind alike (a one-shot's
//! turn, the terminal session, a hosted child):
//!
//! 1. the launch is set up ([`set_up_launch`]: its workspace, its session
//!    home, `CTXLOOM_ROOT`);
//! 2. the engine builds its command: the terminal's
//!    ([`Engine::drive_interactive`]), a turn's ([`Engine::drive_turn`]; a
//!    one-shot, owner ruling s4-Q8), or a child's ([`ChildEngine::command`]);
//! 3. the caller's permits are applied over it (rulings 78, 78b), never over
//!    a name ctxloom set;
//! 4. its program is found on its own `PATH` (PATHEXT's on Windows);
//! 5. a batch file's arguments are checked (std will not hand cmd.exe a line
//!    break);
//! 6. the complete command is measured against the platform's command line.
//!
//! A step that refuses ends the launch there: nothing after it is done, and
//! nothing is started. The launch's caller starts what it answers, exactly
//! as it is: the launcher a turn or the terminal (`RunnerLauncher::start`),
//! the daemon a child (`ChildEngine::start`).

use std::collections::BTreeMap;
use std::fmt;

use isolation_ports::Workspaces;
use run_ports::{
    ChildEngine, ChildPermissions, ChildSpec, Engine, EngineSettings, Ignored, Launch, LaunchAt,
    LaunchKind, LeftOut, ProgramNotFound, SessionRecords, Stdio, StopPolicy, TurnCommand,
};

use crate::home::{set_up_launch, LaunchSetup};
use crate::workspace::Placement;

/// What a launch is made from: which run it starts and how, its setup, and
/// what its engine's command is built with.
#[derive(Clone, PartialEq, Eq)]
pub struct LaunchRequest {
    pub at: LaunchAt,
    pub kind: LaunchKind,
    pub setup: LaunchSetup,
    /// A one-shot's prompt (on its turn's stdin), or the terminal's opening
    /// one (on its command line); a child's is none (it is sent messages).
    pub prompt: String,
    /// The engine's own session the launch resumes; none, it starts one.
    pub resume: Option<String>,
    /// A child's new conversation's native id (D17a); never with `resume`.
    pub session_id: Option<String>,
    /// The user's own home, as the composition root resolved it once.
    pub user_home: Option<String>,
    /// The model; empty names none.
    pub model: String,
    /// The effort level; empty names none.
    pub effort: String,
    /// How the config says the engine is run.
    pub settings: EngineSettings,
    /// How a child's permission requests are answered.
    pub child_permissions: ChildPermissions,
    /// The session's lock its starter takes; empty names none.
    pub session_lock: String,
}

impl fmt::Debug for LaunchRequest {
    /// The environment's names alone (the setup's own `Debug`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaunchRequest")
            .field("at", &self.at)
            .field("kind", &self.kind)
            .field("setup", &self.setup)
            .field("prompt", &self.prompt)
            .field("resume", &self.resume)
            .field("session_id", &self.session_id)
            .field("user_home", &self.user_home)
            .field("model", &self.model)
            .field("effort", &self.effort)
            .field("settings", &self.settings)
            .field("child_permissions", &self.child_permissions)
            .field("session_lock", &self.session_lock)
            .finish()
    }
}

/// The engine a launch is made for: its `Engine` (the home it prepares, a
/// turn's and the terminal's command) and, for a child, its child half.
#[derive(Clone, Copy)]
pub struct LaunchEngines<'a> {
    pub engine: &'a dyn Engine,
    pub child: Option<&'a dyn ChildEngine>,
}

/// Where a launch's program is looked for (`RunnerLauncher::find_program`):
/// the file it is spawned as, or why it is on no directory of its `PATH`.
pub type FindProgram<'a> = &'a dyn Fn(&TurnCommand) -> Result<String, ProgramNotFound>;

/// A launch made: the launch, where its engine was placed (the checkout its
/// run releases), and, for a child, the spec its engine converses by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchMade {
    pub launch: Launch,
    pub placed: Option<Placement>,
    pub child: Option<ChildSpec>,
}

/// Why a launch's making started nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotLaunched {
    /// Its words: a home that could not be written, a command the engine
    /// would not build.
    Failed(String),
    /// The program is on no directory of the command's PATH.
    ProgramNotFound(ProgramNotFound),
    /// A batch file's argument std will not pass.
    BatchArguments(run_ports::BatchArguments),
    /// The command is too long for the platform.
    TooLong(run_ports::CommandTooLong),
}

impl fmt::Display for NotLaunched {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotLaunched::Failed(detail) => f.write_str(detail),
            NotLaunched::ProgramNotFound(missing) => missing.fmt(f),
            NotLaunched::BatchArguments(refusal) => refusal.fmt(f),
            NotLaunched::TooLong(refusal) => refusal.fmt(f),
        }
    }
}

/// A launch that started nothing: why, and what its steps before the
/// refusal said (the placement its run still releases, the settings left
/// out, the permits ignored).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unprepared {
    pub why: NotLaunched,
    pub placed: Option<Placement>,
    pub left_out: Vec<LeftOut>,
    pub ignored: Vec<Ignored>,
}

impl Unprepared {
    fn at_setup(detail: String) -> Box<Unprepared> {
        Box::new(Unprepared {
            why: NotLaunched::Failed(detail),
            placed: None,
            left_out: Vec::new(),
            ignored: Vec::new(),
        })
    }
}

/// Make `request` into its launch on `engines`, in the one order (the
/// module's): set up, the engine's command, the permits, the program found
/// by `find`, the batch check and the length check for `os` (an empty `os`
/// measures nothing). Its error is why nothing can start.
pub fn launch_of(
    engines: LaunchEngines<'_>,
    find: FindProgram<'_>,
    records: &mut dyn SessionRecords,
    workspaces: Option<&dyn Workspaces>,
    request: &LaunchRequest,
    os: &str,
) -> Result<LaunchMade, Box<Unprepared>> {
    let _ = (engines, find, records, workspaces, request, os);
    let _ = (
        set_up_launch,
        StopPolicy::of,
        Stdio::Piped,
        BTreeMap::<u8, u8>::new,
    );
    Err(Unprepared::at_setup("not built".to_string()))
}

#[cfg(test)]
#[path = "launch.test.rs"]
mod tests;
