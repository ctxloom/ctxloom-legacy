//! The library commands (S1) wired: the content and config files over the
//! process's filesystem, the application over them, and the driving adapter
//! joined to the command line as an extension.

use cli_support::Exit;
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use angzarr_presentation::Catalog;
use companion_probe::ProcessProbe;
use config_files::{ConfigFilesFs, ScaffoldFs};
use content_fs::{ContentFs, UserFilesFs};
use ctxloom_cli::{Call, Extension, Failure};
use ctxloom_ports::Executables;
use deps_ports::RemoteLibrary;
use editor_process::ProcessEditor;
use file_system::{Fs, Kind};
use library_application::{Library, WritePorts};
use process_env::EnvSnapshot;

use crate::{LibError, Wiring};

/// Where a program is on the `PATH`, by the filesystem seam, by the file
/// names the host looks a command up by (`git`; on Windows `git.exe` and the
/// rest of `PATHEXT`'s).
pub(crate) struct PathSearch {
    pub(crate) fs: Arc<dyn Fs>,
    pub(crate) dirs: Vec<PathBuf>,
    pub(crate) flavor: os::Flavor,
    pub(crate) pathext: Option<String>,
}

impl PathSearch {
    fn runnable(&self, candidate: &std::path::Path) -> Option<String> {
        let meta = self.fs.metadata(candidate).ok()?;
        (meta.kind == Kind::File && meta.executable).then(|| candidate.display().to_string())
    }
}

impl Executables for PathSearch {
    fn find(&self, name: &str) -> Option<String> {
        // A name with a separator is the file it names, not a name to search
        // the directories for.
        if name.contains(['/', '\\']) {
            return self.runnable(std::path::Path::new(name));
        }
        let names = self.flavor.candidates(name, self.pathext.as_deref());
        self.dirs
            .iter()
            .find_map(|dir| names.iter().find_map(|file| self.runnable(&dir.join(file))))
    }
}

/// The library's commands as an extension of the command line. The library is
/// wired when a command first needs it, so a command that needs nothing of the
/// home (`version`) does not.
pub struct LibraryCommands {
    /// The environment the process was launched in.
    env: Arc<EnvSnapshot>,
    library: Option<Library>,
    /// What the library reads of the remotes, where the program has them.
    remotes: Option<Rc<dyn RemoteLibrary>>,
    /// This binary's stamp, which the daemon that names the project must
    /// share (M3).
    version: String,
}

impl LibraryCommands {
    pub fn new(env: Arc<EnvSnapshot>) -> LibraryCommands {
        LibraryCommands {
            env,
            library: None,
            remotes: None,
            version: String::new(),
        }
    }

    /// The library naming its resources under the project the daemon of
    /// this stamp names (OD-40, OQ-2).
    pub fn with_version(mut self, version: &str) -> LibraryCommands {
        self.version = version.to_string();
        self
    }

    /// The library reading the remotes too.
    pub fn with_remotes(mut self, remotes: Rc<dyn RemoteLibrary>) -> LibraryCommands {
        self.remotes = Some(remotes);
        self
    }

    fn wired(&mut self) -> Result<&Library, LibError> {
        if self.library.is_none() {
            let wiring = Wiring::new(Arc::clone(&self.env))?;
            // Distill's model: a one-shot on no context (plan step 5,
            // OD-38), hosted by a daemon of this binary's stamp (U4).
            let library = build(&wiring, self.remotes.clone())?
                .with_distiller(Rc::new(crate::distiller::OneShotDistiller {
                    env: Arc::clone(wiring.env()),
                    stamp: self.version.clone(),
                }))
                // A project `ctxloom` bundle replaces distill's prompt only
                // once the user trusts the project: her daemon is asked,
                // once, where it decides (step 8 U2, D4).
                .with_trust(crate::daemon::trust_ask(&wiring, &self.version))
                // `config create` makes the directory a git repository as
                // init does (step 10, G1, QUESTIONS.md E).
                .with_repository_step(repository_step(&wiring));
            self.library = Some(library);
        }
        Ok(self
            .library
            .as_ref()
            .unwrap_or_else(|| unreachable!("just built")))
    }
}

/// The git observer of `wiring`'s process: git on its `PATH`, in its
/// environment (step 10, U12).
pub(crate) fn observer(wiring: &Wiring) -> library_git::GitTrees {
    library_git::GitTrees::new(wiring.fs(), Arc::clone(wiring.env()))
}

/// init's git step over the directory the command was run in, for `config
/// create`.
fn repository_step(wiring: &Wiring) -> library_application::RepositoryStep {
    let observer = observer(wiring);
    let dir = wiring.surroundings().cwd.display().to_string();
    Rc::new(move || {
        init_application::ensure_repository(&observer, &dir)
            .map(|made| made.map(|notice| Box::new(notice) as run_ports::Advice))
    })
}

/// What every library command takes first (step 10, U12; QUESTIONS.md B):
/// the home's library made a git repository at its first read (G2), which
/// also asks git its version, so git missing or too old fails the command
/// (exit 10) before it reads anything. A home with no library yet has
/// nothing to make one of. (The project's own tree is the read's, which the
/// library domain observes and refuses outside git: U0, U2.)
pub(crate) fn home_repository(wiring: &Wiring) -> Result<(), Failure> {
    let app = wiring.home().join(".ctxloom");
    let is_dir = wiring
        .fs()
        .metadata(&app)
        .is_ok_and(|meta| meta.kind == Kind::Dir);
    if !is_dir {
        return Ok(());
    }
    library_application::ensure_home_repository(&observer(wiring), &app.display().to_string())
        .map(|_| ())
        .map_err(|e| Failure::boxed(e.into_failure(), Exit::FAILED))
}

/// The program's words for a library read's diagnostics: the wiring's
/// catalogs and the library's own, read once, at the first diagnostic said;
/// a catalog that does not read says each by its code and args.
fn words() -> library_application::Words {
    static CATALOG: std::sync::OnceLock<Option<angzarr_presentation::Catalog>> =
        std::sync::OnceLock::new();
    Arc::new(|d: &dyn angzarr_diagnostic::Diagnostic| {
        let catalog = CATALOG.get_or_init(|| {
            let mut sources = crate::catalog_sources();
            sources.extend([
                angzarr_yaml::EN_US,
                library::EN_US,
                library_application::EN_US,
                deps::EN_US,
                deps_application::EN_US,
            ]);
            angzarr_presentation::Catalog::en_us(&sources).ok()
        });
        match catalog {
            Some(catalog) => catalog.message(d),
            None => d.to_string(),
        }
    })
}

pub(crate) fn build(
    wiring: &Wiring,
    remotes: Option<Rc<dyn RemoteLibrary>>,
) -> Result<Library, LibError> {
    let fs = wiring.fs();
    let home = wiring.home().to_path_buf();
    let (around, app) = wiring.locate();
    let content = ContentFs::new(Arc::clone(&fs), &app.path);
    let files = ConfigFilesFs::new(Arc::clone(&fs), app, &home);
    // Overrides are text: a variable that is not UTF-8 is none (it is never
    // an error, as `std::env::vars` made it).
    let environ = wiring.env().vars_utf8().into_iter().collect();
    let ports = WritePorts {
        user: Arc::new(UserFilesFs::new(Arc::clone(&fs))),
        scaffold: Arc::new(ScaffoldFs::new(Arc::clone(&fs), &around.cwd)),
        editor: Arc::new(ProcessEditor::new(Arc::clone(&fs), &around.temp_root)),
    };
    let library = Library::new(Arc::new(content), Arc::new(files), environ)
        .with_write_ports(ports)
        // The one companion probe (OD-8): it holds no environment or
        // directory; each call names its site (D1), the caller's environment
        // and the project's root, else the working directory here.
        .with_companions(Arc::new(ProcessProbe::standard(
            Arc::clone(&fs),
            wiring.env().flavor(),
        )))
        .with_probe_cwd(around.cwd.display().to_string())
        // The engines a plan is shown to, for what they say of it (owner ruling
        // 168): the real ones, as the run drives them.
        .with_engines(advising_engines())
        // What each engine cannot carry, in its own word (its surface half): the
        // capability-loss report and the swap preview read it.
        .with_hook_losses(hook_losses())
        // A read the daemon answers says a diagnostic (an agent's resolution
        // error) in the program's words (step 10a U2).
        .with_words(words());
    // ctxloom's default content, built in (owner rulings 110, 111).
    let library = if builtins_withheld(wiring.env()) {
        library
    } else {
        library.with_builtins()
    };
    Ok(match remotes {
        Some(remotes) => library.with_remotes(remotes),
        None => library,
    })
}

/// The engines whose word on a run's plan is said (`Engine::advise`): those
/// the run drives. A test engine advises nothing, and is not asked.
fn advising_engines() -> Vec<Arc<dyn run_ports::Engine>> {
    let program = &crate::program::running().name;
    vec![
        Arc::new(engine_claude::Claude::calling(program)),
        Arc::new(engine_opencode::Opencode::calling(program)),
    ]
}

/// Each composed engine's hook events it has no native form for, with why
/// (`EngineSurfaces::hook_losses`), by its name.
fn hook_losses() -> library_application::HookLosses {
    let engines = crate::commands::engines();
    engines
        .names()
        .into_iter()
        .filter_map(|name| {
            let losses = engines.get(&name)?.hook_losses();
            Some((name, losses))
        })
        .collect()
}

/// Under `test-support`, `CTXLOOM_TEST_NO_BUILTINS` withholds the built-in
/// bundles, so the Go-parity tests hold the catalog Go had; it is compiled
/// out of the shipped binary.
#[cfg(feature = "test-support")]
fn builtins_withheld(env: &EnvSnapshot) -> bool {
    env.non_empty("CTXLOOM_TEST_NO_BUILTINS").is_some()
}

/// The shipped binary always has its built-in bundles.
#[cfg(not(feature = "test-support"))]
fn builtins_withheld(_: &EnvSnapshot) -> bool {
    false
}

/// The library's names (and the remotes' and dependencies', B4), rooted
/// where the daemon's `ResolveProject` says (OD-40, OQ-2): asked only when a
/// json answer prints a name.
pub(crate) struct DaemonNames {
    pub(crate) env: Arc<EnvSnapshot>,
    pub(crate) version: String,
    /// Whether naming the project may mint its id ([`minting`]).
    pub(crate) minting: crate::Minting,
}

/// Whether `noun`'s names may mint the project's id (owner ruling
/// 2026-11-14): every command that names its project may, at its root, but
/// the library's `run`, which is the dry run alone (a launch is the run use
/// case's), never does: "dry run should *not* mint".
pub(crate) fn minting(noun: &str) -> crate::Minting {
    if noun == "run" {
        crate::Minting::Never
    } else {
        crate::Minting::AtRoot
    }
}

impl DaemonNames {
    fn wiring(&self) -> Result<Wiring, Failure> {
        Wiring::new(Arc::clone(&self.env)).map_err(|e| Failure::new(e, Exit::FAILED))
    }

    fn parent(&self) -> Result<String, Failure> {
        crate::daemon::library_parent(&self.wiring()?, &self.version, self.minting)
    }
}

impl library_cli::Names for DaemonNames {
    fn parent(&self) -> Result<String, Failure> {
        DaemonNames::parent(self)
    }

    /// A dry run's: the daemon asked, but a project nothing has named is
    /// not named for it (owner rulings 2026-11-14).
    fn named_parent(&self) -> Result<Option<String>, Failure> {
        crate::daemon::library_parent_if_named(&self.wiring()?, &self.version, self.minting)
    }
}

impl deps_cli::Names for DaemonNames {
    fn parent(&self) -> Result<String, Failure> {
        DaemonNames::parent(self)
    }
}

/// The library reads the daemon answers (step 10a U2), as this command
/// asks them: its project's (registered at its root, minted there if it
/// has no identity yet), at its working directory, with its environment.
pub(crate) struct DaemonServed {
    pub(crate) env: Arc<EnvSnapshot>,
    pub(crate) version: String,
}

impl DaemonServed {
    fn wiring(&self) -> Result<Wiring, Failure> {
        Wiring::new(Arc::clone(&self.env)).map_err(|e| Failure::new(e, Exit::FAILED))
    }
}

impl library_cli::Served for DaemonServed {
    fn in_project(&self) -> bool {
        self.wiring()
            .is_ok_and(|wiring| wiring.ctxloom_project().is_some())
    }

    library_cli::served_reads!(served_read);

    library_cli::served_writes!(served_write);

    /// The Edit need's EDITOR launch, run here on the user's terminal
    /// (step 10b U3): the program and arguments the daemon resolved, on a
    /// file of the document's text, what it left read back.
    fn edit_document(&self, edit: &daemon_proto::v1::Edit) -> Result<String, Failure> {
        use ctxloom_ports::Editor as _;
        let launch = edit.launch.as_ref().ok_or_else(|| {
            Failure::new(
                library_application::AppError::from(library_application::WriteError::NotWired {}),
                Exit::FAILED,
            )
        })?;
        let wiring = self.wiring()?;
        let (around, _) = wiring.locate();
        let editor = ProcessEditor::new(wiring.fs(), &around.temp_root);
        let command = ctxloom_ports::EditorCommand {
            program: launch.program.clone(),
            args: launch.args.clone(),
        };
        editor
            .edit(&command, &edit.file_name, &edit.text)
            .map_err(|cause| {
                Failure::new(library_application::AppError::Cause(cause), Exit::FAILED)
            })
    }

    fn assemble_context(
        &self,
        request: daemon_proto::v1::AssembleContextRequest,
    ) -> Result<daemon_proto::v1::AssembledContext, Failure> {
        use daemon_api::LibraryReads;
        let wiring = self.wiring()?;
        let client = crate::daemon::library_client(&wiring, &self.version)?;
        crate::daemon::answered(client.assemble_context(request))
    }

    fn preview_run(
        &self,
        request: daemon_proto::v1::CreateRunRequest,
    ) -> Result<daemon_proto::v1::Run, Failure> {
        use daemon_api::LibraryReads;
        let wiring = self.wiring()?;
        let client = crate::daemon::preview_client(&wiring, &self.version)?;
        crate::daemon::answered(client.preview_run(request))
    }

    fn own_loadout(
        &self,
        request: daemon_proto::v1::GetLoadoutRequest,
    ) -> Result<(daemon_proto::v1::Loadout, Vec<daemon_proto::v1::Notice>), Failure> {
        use daemon_api::LibraryReads;
        let wiring = self.wiring()?;
        let client = crate::daemon::home_client(&wiring, &self.version);
        let read = crate::daemon::answered(client.get_loadout("/loadout", request))?;
        Ok((read.answer, read.notices))
    }
}

/// [`DaemonServed`]'s reads, from `library_cli::served_reads!`: each the
/// daemon's (reached, or started) of this binary's stamp.
macro_rules! served_read {
    ($($method:ident ($rpc:ident) $request:ident . $target:ident => $response:ident),* $(,)?) => {
        $(
            fn $method(
                &self,
                under: &str,
                request: daemon_proto::v1::$request,
            ) -> Result<
                (
                    daemon_proto::v1::$response,
                    Vec<daemon_proto::v1::Notice>,
                ),
                Failure,
            > {
                use daemon_api::LibraryReads;
                let wiring = self.wiring()?;
                let client = crate::daemon::library_client(&wiring, &self.version)?;
                let read = crate::daemon::answered(client.$method(under, request))?;
                Ok((read.answer, read.notices))
            }
        )*
    };
}
use served_read;

/// [`DaemonServed`]'s writes, from `library_cli::served_writes!`: each the
/// daemon's (reached, or started) of this binary's stamp, under one
/// request id, sent once more to a new daemon if this one could not be
/// reached (S8).
macro_rules! served_write {
    ($($method:ident ($rpc:ident) $request:ident => $response:ident),* $(,)?) => {
        $(
            fn $method(
                &self,
                under: &str,
                request: daemon_proto::v1::$request,
            ) -> Result<daemon_proto::v1::$response, Failure> {
                use daemon_api::LibraryWrites;
                let wiring = self.wiring()?;
                let client = crate::daemon::library_client(&wiring, &self.version)?;
                crate::daemon::answered(client.$method(under, request))
            }
        )*
    };
}
use served_write;

impl deps_cli::Served for DaemonServed {
    deps_cli::served_reads!(served_read);
}

/// Whether `noun`'s verb in `call` is a read the daemon answers (step 10a
/// U2, `library_cli::is_served_read`), whose home step is the daemon's own; every
/// other verb still runs in process, and takes it here first.
fn served(noun: &str, call: &Call<'_>) -> bool {
    call.matches.subcommand_matches(noun).is_some_and(|sub| {
        library_cli::is_served_read(noun, sub) || library_cli::is_served_write(noun, sub)
    })
}

impl Extension for LibraryCommands {
    fn nouns(&self) -> Vec<clap::Command> {
        library_cli::nouns()
    }

    fn run(
        &mut self,
        noun: &str,
        call: &Call<'_>,
        catalog: &Catalog,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let names = DaemonNames {
            env: Arc::clone(&self.env),
            version: self.version.clone(),
            minting: minting(noun),
        };
        if !served(noun, call) {
            home_repository(
                &Wiring::new(Arc::clone(&self.env)).map_err(|e| Failure::new(e, Exit::FAILED))?,
            )?;
        }
        let served = DaemonServed {
            env: Arc::clone(&self.env),
            version: self.version.clone(),
        };
        let library = self.wired().map_err(|e| Failure::new(e, Exit::FAILED))?;
        library_cli::run(library, &names, &served, noun, call, catalog, out, err)
    }
}

#[cfg(test)]
#[path = "library.test.rs"]
mod tests;
