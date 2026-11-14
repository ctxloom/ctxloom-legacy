//! The tools as an engine lists them, and their arguments as read.

use serde_json::json;

use super::{json_text, text, texts, tools, ToolArgs, TOOL_NAMES};

fn args(value: serde_json::Value) -> rmcp::model::JsonObject {
    match value {
        serde_json::Value::Object(map) => map,
        _ => panic!("an object"),
    }
}

// S46 less what is not built (OD-1): the names offered, each an object
// schema whose required arguments are its own properties.
#[test]
fn the_tools_offered_are_the_built_ones_each_with_an_object_schema() {
    let offered = tools();
    let names: Vec<&str> = offered.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(names, TOOL_NAMES);
    assert!(!names.contains(&"approval_prompt"));
    for tool in &offered {
        let schema = tool.input_schema.as_ref();
        assert_eq!(schema["type"], "object", "{}", tool.name);
        assert!(tool.description.is_some(), "{}", tool.name);
        for required in schema["required"].as_array().unwrap() {
            let name = required.as_str().unwrap();
            assert!(
                schema["properties"].get(name).is_some(),
                "{}: {name}",
                tool.name
            );
        }
    }
    let send = offered.iter().find(|t| t.name == "child_send").unwrap();
    assert_eq!(
        send.input_schema["required"],
        json!(["child", "urgency", "text"])
    );
    let assemble = offered
        .iter()
        .find(|t| t.name == "context_assemble")
        .unwrap();
    assert_eq!(assemble.input_schema["required"], json!(["bundles"]));
}

#[test]
fn a_text_argument_is_read_or_said_missing() {
    let given = args(json!({"child": "brisk-copper-moth", "n": 3}));
    assert_eq!(
        text("child_close", &given, "child"),
        Ok("brisk-copper-moth")
    );
    assert_eq!(
        text("child_close", &given, "n"),
        Err(ToolArgs::Missing {
            tool: "child_close".to_string(),
            argument: "n".to_string()
        })
    );
    assert!(text("child_close", &given, "absent").is_err());
}

#[test]
fn a_list_argument_is_its_texts_and_absent_only_where_optional() {
    let given = args(json!({"bundles": ["a", "b"], "bad": ["a", 1], "one": "a"}));
    assert_eq!(
        texts("t", &given, "bundles", true),
        Ok(vec!["a".to_string(), "b".to_string()])
    );
    assert_eq!(texts("t", &given, "tags", false), Ok(Vec::new()));
    assert!(texts("t", &given, "tags", true).is_err());
    assert!(texts("t", &given, "bad", false).is_err());
    assert!(texts("t", &given, "one", false).is_err());
}

#[test]
fn each_argument_error_reports_its_args() {
    use angzarr_diagnostic::Diagnostic;
    let missing = ToolArgs::Missing {
        tool: "child_send".to_string(),
        argument: "text".to_string(),
    };
    assert_eq!(
        missing.to_string(),
        "mcp-server-argument-missing tool=child_send argument=text"
    );
    assert_eq!(
        ToolArgs::Unknown {
            tool: "x".to_string()
        }
        .args()
        .len(),
        1
    );
    assert_eq!(
        ToolArgs::Urgency {
            given: "shout".to_string()
        }
        .to_string(),
        "mcp-server-urgency-invalid given=shout"
    );
}

// Every tool offered has a description, the owner's approved text
// (2026-11-14) byte for byte: embedded from `tools/<tool>.md`, without the
// file's closing newline. Written out here, so an edit to a file is seen.
#[test]
fn every_tool_offered_is_described_in_the_approved_words() {
    let approved = [
        ("session_list", "List this project's sessions (all: every project's), as the daemon's session index has them."),
        ("session_show", "Show one session as the daemon has it: its state, owner, children and pending messages."),
        ("transcript_show", "Show a session's transcript, its lines as its hooks have delivered them so far."),
        ("context_assemble", "Load the fragments you chose from this project's premise index, by the names it lists."),
        ("child_send", "Send one of your children a message: inform (read at its next turn), wake (start a turn) or steer (interrupt its turn)."),
        ("child_close", "Close one of your children: its engine is ended, and it takes no more messages."),
        ("child_clear", "Clear one of your children's conversation, as its engine's own /clear does."),
    ];
    let offered = tools();
    assert_eq!(offered.len(), approved.len());
    for tool in &offered {
        let said = tool.description.as_deref().unwrap_or_default();
        assert!(!said.trim().is_empty(), "{} is described", tool.name);
        assert_eq!(
            said,
            said.trim(),
            "{}: no surrounding whitespace",
            tool.name
        );
        let words = approved
            .iter()
            .find(|(name, _)| *name == tool.name)
            .map(|(_, words)| *words);
        assert_eq!(Some(said), words, "{}", tool.name);
    }
}

// A description file's closing newline, in either convention, is not part
// of the description; nothing else is taken off.
#[test]
fn a_description_is_its_file_without_the_closing_newline() {
    assert_eq!(super::describe("Words.\n"), "Words.");
    assert_eq!(super::describe("Words.\r\n"), "Words.");
    assert_eq!(super::describe("Words."), "Words.");
    assert_eq!(super::describe(" Words. \n"), " Words. ");
}

// A session argument is a string, said to be a harp or a full name.
#[test]
fn a_session_argument_is_a_string_naming_a_harp_or_a_full_name() {
    let offered = tools();
    let show = offered.iter().find(|t| t.name == "session_show").unwrap();
    let session = &show.input_schema["properties"]["session"];
    assert_eq!(session["type"], "string", "{session}");
    let description = session["description"].as_str().unwrap();
    assert!(
        description.starts_with("The session: a session's harp in this project"),
        "{description}"
    );
}

// A message's text is its JSON.
#[test]
fn a_messages_text_is_its_json() {
    let message = json!({"name": "x", "n": 1});
    let text = json_text(&message);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).ok(),
        Some(message),
        "{text}"
    );
}
