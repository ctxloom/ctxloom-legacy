//! A library view as its API message (OD-40; design §6), the ONE mapping
//! (step 10a U2): what the daemon answers a read with, and what the CLI
//! prints for a read still answered in process, named under the parent the
//! read was asked under (`projects/{harp}`, or `users/me` outside a
//! project).
//!
//! Moved here from library-cli (U2) for the reads the daemon serves: the
//! agents, the profiles and the skills (the hooks' reads, O3).

use std::collections::BTreeMap;

use daemon_proto::google::protobuf::{value::Kind, ListValue, Struct, Value};
use daemon_proto::v1;
use library_application::{AgentRow, BundleList, BundleRow, ProfileList, ProfileRow, SkillList};

use crate::names::{bundle_name, bundle_ref, item_name, origin, profile_ref};

/// The bundle a project's own profiles are named under.
pub const PROJECT_BUNDLE: &str = "project";

/// A free-form document as a `google.protobuf.Value` (OQ-11, owner ruling
/// 2026-11-13): whatever was written, an object or not, for display.
pub fn document(json: &serde_json::Value) -> Value {
    let kind = match json {
        serde_json::Value::Null => Kind::NullValue(0),
        serde_json::Value::Bool(b) => Kind::BoolValue(*b),
        serde_json::Value::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or_default()),
        serde_json::Value::String(s) => Kind::StringValue(s.clone()),
        serde_json::Value::Array(items) => Kind::ListValue(ListValue {
            values: items.iter().map(document).collect(),
        }),
        serde_json::Value::Object(fields) => Kind::StructValue(Struct {
            fields: fields
                .iter()
                .map(|(key, field)| (key.clone(), document(field)))
                .collect::<BTreeMap<_, _>>(),
        }),
    };
    Value { kind: Some(kind) }
}

/// `bundle list`: `ListBundlesResponse`, the whole catalog (design §1.2).
pub fn bundles(parent: &str, list: &BundleList) -> v1::ListBundlesResponse {
    v1::ListBundlesResponse {
        bundles: list.rows.iter().map(|row| bundle(parent, row)).collect(),
        next_page_token: String::new(),
    }
}

/// A `bundle list` row as its `Bundle` (BASIC).
pub fn bundle(parent: &str, row: &BundleRow) -> v1::Bundle {
    v1::Bundle {
        name: bundle_name(parent, &row.name),
        origin: origin(&row.name, row.origin == "ctxloom/builtin") as i32,
        reference: row.reference.clone(),
        commit: row.commit.clone(),
        path: row.path.clone(),
        version: row.version.clone(),
        description: row.description.clone(),
        tags: row.tags.clone(),
        fragment_count: count(row.fragment_count),
        command_count: count(row.command_count),
        mcp_server_count: count(row.mcp_count),
        profile_count: count(row.profile_count),
        held: row.held.then_some(true),
        ..v1::Bundle::default()
    }
}

/// An engine's resource name under `parent`.
pub fn engine_name(parent: &str, engine: &str) -> String {
    format!("{parent}/engines/{engine}")
}

/// An agent's resource name under `parent`.
pub fn agent_name(parent: &str, agent: &str) -> String {
    format!("{parent}/agents/{agent}")
}

/// `agent list`: `ListAgentsResponse`, every agent under `parent`, the one
/// a bare run binds being `default`.
pub fn agents(parent: &str, rows: &[AgentRow], default: &str) -> v1::ListAgentsResponse {
    v1::ListAgentsResponse {
        agents: rows.iter().map(|row| agent(parent, row, default)).collect(),
        next_page_token: String::new(),
    }
}

/// The engine an agent references, by its resource name (design §2); an
/// agent that names none names none.
pub fn engine_ref(parent: &str, engine: &str) -> String {
    if engine.is_empty() {
        return String::new();
    }
    engine_name(parent, engine)
}

/// An agent as its binding declares it (the BASIC view): its engine blocks
/// keyed by engine, so `engines.<e>.model` keeps its path; its engine and
/// profiles by their resource names (design §2: one that names no place
/// keeps its configured text, OQ-16).
pub fn agent(parent: &str, row: &AgentRow, default: &str) -> v1::Agent {
    v1::Agent {
        name: agent_name(parent, &row.name),
        engine: engine_ref(parent, &row.engine),
        profiles: row
            .profiles
            .iter()
            .map(|profile| profile_ref(parent, profile))
            .collect(),
        runtime: row.runtime.clone(),
        driving: row.driving.clone(),
        env: row.env.clone(),
        session_home: row.session_home.clone(),
        engines: blocks(&row.engines),
        default: Some(row.name == default),
        ..Default::default()
    }
}

/// `agents.<n>.engines` as the config holds it: each engine's model, effort
/// and permissions document, the document as written (rulings 133-139).
fn blocks(engines: &serde_json::Value) -> BTreeMap<String, v1::AgentEngine> {
    engines
        .as_object()
        .into_iter()
        .flatten()
        .map(|(engine, block)| {
            let word = |key: &str| {
                block
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let lined = v1::AgentEngine {
                model: word("model"),
                effort: word("effort"),
                permissions: block.get("permissions").map(document),
            };
            (engine.clone(), lined)
        })
        .collect()
}

/// The profile's own name within its key (`<bundle>#profiles/<name>`).
pub fn profile_id(name: &str) -> &str {
    name.split_once("#profiles/").map_or(name, |(_, id)| id)
}

/// A profile's resource name, below the bundle that ships it (the
/// project's own, `project`, for none).
pub fn profile_name(parent: &str, entry: &str, id: &str) -> String {
    let entry = if entry.is_empty() {
        PROJECT_BUNDLE
    } else {
        entry
    };
    item_name(parent, entry, "profiles", id)
}

/// `profile list`: `ListProfilesResponse`.
pub fn profiles(parent: &str, list: &ProfileList) -> v1::ListProfilesResponse {
    v1::ListProfilesResponse {
        profiles: list
            .rows
            .iter()
            .map(|row| profile_row(parent, row))
            .collect(),
        next_page_token: String::new(),
    }
}

fn profile_row(parent: &str, row: &ProfileRow) -> v1::Profile {
    v1::Profile {
        name: profile_name(parent, &row.entry, &row.display_name),
        description: row.description.clone(),
        parents: profile_parents(parent, &row.parents),
        tags: row.tags.clone(),
        bundles: profile_bundles(parent, &row.bundles),
        default: Some(row.default),
        path: row.path.clone(),
        ..<v1::Profile as Default>::default()
    }
}

/// A profile's parents as Profile names (OQ-16: one with no place keeps
/// its text).
pub fn profile_parents(parent: &str, asks: &[String]) -> Vec<String> {
    asks.iter().map(|ask| profile_ref(parent, ask)).collect()
}

/// A profile's bundles as Bundle (or item) names.
pub fn profile_bundles(parent: &str, asks: &[String]) -> Vec<String> {
    asks.iter().map(|ask| bundle_ref(parent, ask)).collect()
}

/// A skill's resource name.
pub fn skill_name(parent: &str, bundle: &str, id: &str) -> String {
    item_name(parent, bundle, "skills", id)
}

/// A count as the API's uint32 (OQ-12), saturating.
pub fn count(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `skill list`: `ListSkillsResponse`.
pub fn skills(parent: &str, list: &SkillList) -> v1::ListSkillsResponse {
    v1::ListSkillsResponse {
        skills: list
            .rows
            .iter()
            .map(|row| {
                // A row's name is `<bundle>/<skill>`, its source the bundle.
                let id = row
                    .name
                    .strip_prefix(&format!("{}/", row.source))
                    .unwrap_or(&row.name);
                v1::Skill {
                    name: skill_name(parent, &row.source, id),
                    description: row.description.clone(),
                    tags: row.tags.clone(),
                    file_count: count(row.file_count),
                    ..v1::Skill::default()
                }
            })
            .collect(),
        next_page_token: String::new(),
    }
}

#[cfg(test)]
#[path = "resources.test.rs"]
mod tests;
