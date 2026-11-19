//! The steps of the surfaces World's CLI contract features (`features/cli/`)
//! that no journey's module has, written as step 11 un-pended them (each
//! step names its feature).

use cucumber::gherkin::Step;
use cucumber::{given, then, when};

use super::World;
use crate::tests::files;
use crate::tests::gherkin_common::CommonWorld;

/// remote.feature: a command run from a subdirectory of the project (made
/// if it is not there), with no `CTXLOOM_ROOT`, so the project is found by
/// walking up.
#[when(expr = "I run {string} from the project subdirectory {string}")]
fn run_from_subdirectory(w: &mut World, line: String, sub: String) {
    let dir = w.common.project.dir().join(&sub);
    files::mkdirs(&dir);
    w.common.run_lines_at(Some(&dir), &line, "");
}

/// remote.feature: ctxloom has never run in this home.
#[given(expr = "the home has no {string} directory yet")]
fn home_lacks_dir(w: &mut World, rel: String) {
    assert!(
        !files::exists(w.common.project.home().join(&rel)),
        "the home already has {rel}"
    );
}

/// remote.feature: the directory's owner may read, write and search it,
/// where the host has permission bits at all.
#[then(expr = "the home directory {string} is readable, writable and searchable by its owner")]
fn home_dir_usable(w: &mut World, rel: String) {
    let dir = w.common.project.home().join(&rel);
    assert!(files::exists(&dir), "{rel} does not exist");
    if let Some(mode) = files::mode_of(&dir) {
        assert_eq!(mode & 0o700, 0o700, "{rel} is {mode:o}");
    }
}

#[then(expr = "the home file {string} contains {string}")]
fn home_holds(w: &mut World, rel: String, text: String) {
    let held = w.common.project.read_home(&rel).expect("a home file");
    assert!(held.contains(&text), "{rel}: {held}");
}

/// A Given or And after a Given that runs Alice's commands (the common
/// `Alice ...:` step is a When): deps.feature's "Alice has already let a
/// pull refresh the project:".
#[given(regex = r"^Alice .*:$")]
fn alice_already_ran(w: &mut World, step: &Step) {
    w.alice_runs(step);
}

/// The seeded remotes, once one was seeded.
fn remotes(w: &mut World) -> &mut super::super::deps::Remotes {
    w.remotes.as_mut().expect("a git remote was seeded")
}

/// deps.feature: Go's `the remote "<name>" changes fragment "<fragment>"
/// to "<content>"`, as a When (the surfaces World's Given is the same).
#[when(expr = "the remote {string} changes fragment {string} to {string}")]
fn remote_changes_fragment(w: &mut World, name: String, fragment: String, content: String) {
    remotes(w).changes_fragment(&name, &fragment, &content);
}

/// deps.feature: Go's `the remote "<name>" stops publishing bundle
/// "<bundle>"`.
#[given(expr = "the remote {string} stops publishing bundle {string}")]
fn remote_stops_publishing(w: &mut World, name: String, bundle: String) {
    remotes(w).stops_publishing(&name, &bundle);
}

/// deps.feature: Go's `BreakRemote`, its address left pointing at nothing.
#[given(expr = "the remote {string} becomes unreachable")]
fn remote_unreachable(w: &mut World, name: String) {
    remotes(w).becomes_unreachable(&name);
}

/// deps.feature: Go's `the remote "<name>" ships an MCP server ... and a
/// skill script printing ...`.
#[given(
    expr = "the remote {string} ships an MCP server {string} running {string} and a skill script printing {string}"
)]
fn remote_ships(w: &mut World, name: String, server: String, command: String, say: String) {
    remotes(w).ships(&name, &server, &command, &say);
}

/// deps.feature: the one cached clone of the remote, reset to its first
/// commit.
#[given(expr = "the remote {string}'s cached clone is forced back to its first commit")]
fn clone_forced_back(w: &mut World, _name: String) {
    super::super::deps::clone_forced_back_to_first_commit(&w.common.project);
}

/// profile.feature: an initialized project whose editor (the home config's
/// `editor`, through `sh`) rewrites the document's `description:` line to
/// `EDITED-BY-TEST`, keeping it valid YAML.
#[given("a ctxloom project with a description-rewriting editor")]
fn description_rewriting_editor(w: &mut World) {
    w.common.project = crate::tests::library::Project::initialized();
    w.common.project.write_home(
        ".ctxloom/config.yaml",
        "schema_version: 9\neditor:\n  command: sh\n  args:\n    - \"-c\"\n    - 'sed \"s/^description:.*/description: EDITED-BY-TEST/\" \"$1\" > \"$1.new\" && mv \"$1.new\" \"$1\"'\n    - editor\n",
    );
}

/// profile.feature: the engine file registers the MCP server `name` (the
/// surfaces World's negative step's reading).
#[then(expr = "the file {string} registers an MCP server named {string}")]
fn registers_mcp(w: &mut World, rel: String, name: String) {
    assert!(
        w.common.project.registers_mcp(&rel, "mcpServers", &name),
        "{rel} registers no MCP server {name}: {:?}",
        w.common.project.read(&rel)
    );
}

/// mcp.feature: an initialized project whose editor (the home config's
/// `editor`, through `sh`) rewrites the buffer's `command:` line to
/// `command: EDITED-BY-TEST`, keeping the rest of the entry.
#[given("a ctxloom project with a command-rewriting editor")]
fn command_rewriting_editor(w: &mut World) {
    w.common.project = crate::tests::library::Project::initialized();
    w.common.project.write_home(
        ".ctxloom/config.yaml",
        "schema_version: 9\neditor:\n  command: sh\n  args:\n    - \"-c\"\n    - 'sed \"s/^command:.*/command: EDITED-BY-TEST/\" \"$1\" > \"$1.new\" && mv \"$1.new\" \"$1\"'\n    - editor\n",
    );
}
