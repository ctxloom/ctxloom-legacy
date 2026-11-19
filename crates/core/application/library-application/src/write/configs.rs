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
use crate::write::views::{ConfigCreated, ConfigEdited};
use crate::write::{WriteError, WritePorts};

impl Library {
    fn write_side(&self) -> Result<&WritePorts, AppError> {
        self.ports
            .as_ref()
            .ok_or_else(|| WriteError::NotWired {}.into())
    }

    /// `config create [--engine E]`: the config, the default remotes registry,
    /// the seeded default profile and the nested `.gitignore` of ctxloom's
    /// private state; it refuses an existing config. It makes no git
    /// repository (the library is git-free, owner 2026-11-18).
    pub fn config_create(&self, engine: &str) -> Outcome<ConfigCreated> {
        Outcome {
            result: self.created(engine),
            notices: Vec::new(),
        }
    }

    fn created(&self, engine: &str) -> Result<ConfigCreated, AppError> {
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
