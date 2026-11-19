//! `config create` and `config edit` (Go's `cli/config.go`,
//! `operations.InitializeProject`). They work on the `.ctxloom` under the
//! working directory and do not load the configuration first: a config that
//! does not load is the very thing `config edit` is for.

use ctxloom_ports::Put;
use library::write::envelope::new_envelope;
use library::write::profile::{profile_path, PROJECT_BUNDLE};
use library::write::scaffold::{self, DEFAULT_REMOTES, SEED_NAME, SEED_PROFILE};

use crate::agents::ENGINES;
use crate::error::AppError;
use crate::project::{Library, Outcome};
use crate::write::editor::editor_from_env;
use crate::write::views::{ConfigCreated, ConfigEdited, GitignoreInstalled};
use crate::write::{WriteError, WritePorts};

impl Library {
    fn write_side(&self) -> Result<&WritePorts, AppError> {
        self.ports
            .as_ref()
            .ok_or_else(|| WriteError::NotWired {}.into())
    }

    /// `config create [--engine E]`: the config, the default remotes registry,
    /// the seeded default profile and the nested `.gitignore` of ctxloom's
    /// private state; it refuses an existing config.
    ///
    /// Before anything is written, the directory is made a git repository
    /// when it is in none (step 10, G1, [`Library::with_repository_step`]),
    /// and the notice says so.
    /// `manage gitignore install` (step 10c U5): `.ctxloom/.gitignore`
    /// written whole through the scaffold when it is not what ctxloom
    /// writes; whether it was, and the `.ctxloom` it is in.
    pub fn gitignore_install(&self) -> Outcome<GitignoreInstalled> {
        let result = (|| {
            let scaffold = &self.write_side()?.scaffold;
            let want = scaffold::nested_gitignore();
            let held = scaffold.read(".gitignore").map_err(AppError::Cause)?;
            let written = held.as_deref() != Some(want.as_bytes());
            if written {
                scaffold
                    .write(&[Put::file(".gitignore", want)])
                    .map_err(AppError::Cause)?;
            }
            Ok(GitignoreInstalled {
                app_dir: scaffold.app_dir(),
                written,
            })
        })();
        Outcome {
            result,
            notices: Vec::new(),
        }
    }

    pub fn config_create(&self, engine: &str) -> Outcome<ConfigCreated> {
        let mut notices = Vec::new();
        let result = self.created(engine, &mut notices);
        Outcome { result, notices }
    }

    fn created(
        &self,
        engine: &str,
        notices: &mut Vec<crate::Notice>,
    ) -> Result<ConfigCreated, AppError> {
        let scaffold = &self.write_side()?.scaffold;
        let path = format!("{}/config.yaml", scaffold.app_dir());
        if scaffold.exists("config.yaml").map_err(AppError::Cause)? {
            return Err(WriteError::ConfigExists { path }.into());
        }
        let engine = if engine.is_empty() {
            "claude-code"
        } else {
            engine
        };
        if !ENGINES.contains(&engine) {
            return Err(WriteError::ConfigEngine {
                engine: engine.to_string(),
                valid: ENGINES.join(", "),
            }
            .into());
        }
        if let Some(step) = &self.repository {
            let made = step().map_err(|e| AppError::Cause(e.into_failure()))?;
            notices.extend(made.map(crate::Notice::Git));
        }
        let bundle = format!("content/bundles/v2/{PROJECT_BUNDLE}");
        let mut files = vec![
            Put::file("config.yaml", scaffold::initial_config(engine)),
            Put::file("remotes.yaml", DEFAULT_REMOTES),
        ];
        let manifest = format!("{bundle}/bundle.yaml");
        if !scaffold.exists(&manifest).map_err(AppError::Cause)? {
            files.push(Put::file(manifest, new_envelope("1.0.0", "")));
        }
        // ctxloom's private state (sessions, cache, the project id) out of
        // git from the start; one that is there is the project's.
        if !scaffold.exists(".gitignore").map_err(AppError::Cause)? {
            files.push(Put::file(".gitignore", scaffold::nested_gitignore()));
        }
        let seed = format!("{bundle}/{}", profile_path(SEED_NAME));
        if !scaffold.exists(&seed).map_err(AppError::Cause)? {
            files.push(Put::file(seed, SEED_PROFILE));
        }
        scaffold.write(&files).map_err(AppError::Cause)?;
        Ok(ConfigCreated { path })
    }

    /// `config edit`, served (step 10b U3): the config of where it is run
    /// (the project's, else the home's) handed to the editor of the
    /// environment as `config.yaml` (an editor round trip, so the daemon's
    /// write is the Edit need's answer), and what the editor left written
    /// back whole, unparsed: a config that does not load is the very thing
    /// it is for. Left as it was, nothing is written; none to edit is
    /// refused before any editor is asked.
    pub fn config_edited(&self) -> Outcome<ConfigEdited> {
        Outcome {
            result: self.round_tripped(),
            notices: Vec::new(),
        }
    }

    fn round_tripped(&self) -> Result<ConfigEdited, AppError> {
        let ports = self.write_side()?;
        let scope = match self.files.source() {
            ctxloom_ports::Source::Project => ctxloom_ports::Scope::Project,
            _ => ctxloom_ports::Scope::Home,
        };
        let path = self.files.layer_path(scope);
        let Some(layer) = self.files.layer(scope).map_err(AppError::Cause)? else {
            return Err(WriteError::ConfigMissing { path }.into());
        };
        let edited = ports
            .editor
            .edit(&editor_from_env(&self.environ), "config.yaml", &layer.text)
            .map_err(AppError::Cause)?;
        let changed = edited != layer.text;
        if changed {
            self.files
                .write_layer(scope, &edited)
                .map_err(AppError::Cause)?;
        }
        Ok(ConfigEdited { path, changed })
    }
}

#[cfg(test)]
#[path = "configs.test.rs"]
mod tests;
