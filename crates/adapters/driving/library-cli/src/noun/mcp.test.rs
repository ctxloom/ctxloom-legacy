//! `mcp`'s tests, beside its verbs.

mod read {
    use crate::tests::*;

    // mcp.feature (moved from the four-noun test of lib.test.rs, unchanged)
    #[test]
    fn test_mcp_server_list_and_show_and_the_bare_noun() {
        // OD-40: `ListMcpServersResponse`, each server below its bundle; a
        // show is the list filtered by the ID (design §3.5).
        let servers = json_of(&["mcp", "server", "list"]);
        assert_eq!(
            servers["mcp_servers"][0]["name"],
            "projects/calm-otter/bundles/demo/mcpServers/tree-sitter"
        );
        assert_eq!(servers["mcp_servers"][0]["command"], "tree-sitter");
        assert_eq!(json_of(&["mcp", "server"]), servers);
        let shown = json_of(&["mcp", "server", "show", "tree-sitter"]);
        assert_eq!(
            shown["mcp_servers"][0]["name"],
            "projects/calm-otter/bundles/demo/mcpServers/tree-sitter"
        );
        assert_eq!(shown["mcp_servers"][0]["args"][0], "--stdio");
        assert!(shown.get("found").is_none());
        assert!(text_of(&["mcp", "server", "show", "tree-sitter"])
            .starts_with("MCP Server: tree-sitter\n"));
        // plan supersedes Go: mcp.feature (S2) refuses the bare noun, which
        // listed the servers in S1.
        assert!(go(&["mcp"], TEXT, BARE).refused().contains("mcp-bare"));
        assert!(go(&["mcp"], JSON, BARE).refused().contains("mcp-bare-json"));
    }
}

mod write {
    use crate::tests::write_verbs::*;
    use crate::tests::{How, JSON, TEXT};

    // mcp.feature
    #[test]
    fn test_mcp_set_edit_and_the_bare_noun() {
        let (library, mem) = world();
        assert_eq!(
            text(
                &library,
                &[
                    "mcp",
                    "server",
                    "set",
                    "demo#mcp/tools",
                    "--command",
                    "run",
                    "--arg",
                    "-x",
                    "--arg",
                    "y",
                    "--env",
                    "A=1",
                    "--env",
                    "B=2",
                    "--tag",
                    "t1",
                    "--notes",
                    "n",
                    "--installation",
                    "i",
                ]
            ),
            "Set MCP server \"tools\" in bundle \"demo\"\n"
        );
        let file = mem.text("demo", "mcp/tools.yaml").unwrap();
        assert!(file.contains("command: run"), "{file}");
        assert!(file.contains("- -x") && file.contains("- y"), "{file}");
        assert!(
            file.contains("A: 1") && file.contains("B: \"2\"") || file.contains("B: 2"),
            "{file}"
        );
        let doc = json(
            &library,
            &[
                "mcp",
                "server",
                "set",
                "demo#mcp/tools",
                "--command",
                "",
                "--arg",
                "",
                "--env",
                "",
                "--url",
                "https://x.example/v1",
                "--header",
                "H=v",
            ],
        );
        // `UpdateMcpServerResponse`: the server set, not made (it was there).
        assert_eq!(
            doc["mcp_server"]["name"],
            "projects/calm-otter/bundles/demo/mcpServers/tools"
        );
        assert_eq!(doc["applied"], true);
        assert!(doc.get("created").is_none(), "{doc}");
        let made = json(
            &library,
            &["mcp", "server", "set", "demo#mcp/fresh", "--command", "f"],
        );
        assert_eq!(made["created"], true, "{made}");
        let file = mem.text("demo", "mcp/tools.yaml").unwrap();
        assert!(
            file.contains("url: https://x.example/v1") && file.contains("H: v"),
            "{file}"
        );
    }

    // mcp.feature: a bad pair is refused, the edit's round trip, and the
    // bare noun.
    #[test]
    fn test_mcp_edit_and_the_bare_noun() {
        let (library, mem) = world();
        for flags in [
            &["--command", "run"][..],
            &["--command", "", "--url", "https://x.example/v1"],
        ] {
            let mut line = vec!["mcp", "server", "set", "demo#mcp/tools"];
            line.extend(flags);
            let _ = text(&library, &line);
        }
        let bad = refused_code(
            &library,
            &["mcp", "server", "set", "demo#mcp/tools", "--env", "nope"],
            TEXT,
        );
        assert!(bad.contains("flag-pair"), "{bad}");
        assert_eq!(
            text(&library, &["mcp", "server", "edit", "demo#mcp/tools"]),
            "No changes made.\n"
        );
        mem.set_editor(|_, content| {
            Ok(content.replace("https://x.example/v1", "https://y.example/v1"))
        });
        assert_eq!(
            text(&library, &["mcp", "server", "edit", "demo#mcp/tools"]),
            "Updated MCP server \"tools\" in bundle \"demo\"\n"
        );
        // Asked for by name, the edit answers `UpdateMcpServerResponse`.
        mem.set_editor(|_, content| Ok(content.to_string()));
        let ran = run_on(
            &library,
            &["mcp", "server", "edit", "demo#mcp/tools"],
            JSON_EXPLICIT,
        );
        let edited = ran.json();
        assert_eq!(edited["applied"], false);
        assert_eq!(
            edited["mcp_server"]["name"],
            "projects/calm-otter/bundles/demo/mcpServers/tools"
        );
        // json that nobody asked for (the default, off a terminal) is not json
        let unasked = How {
            explicit: None,
            terminal: false,
            degraded: false,
        };
        let said = refused_code(&library, &["mcp"], unasked);
        assert!(
            said.contains("mcp-bare") && !said.contains("mcp-bare-json"),
            "{said}"
        );
        let said = refused_code(&library, &["mcp"], TEXT);
        assert!(
            said.contains("mcp-bare") && !said.contains("mcp-bare-json"),
            "{said}"
        );
        assert!(refused_code(&library, &["mcp"], JSON).contains("mcp-bare-json"));
    }
}

// Each view's mapping to its message, field by field (OD-40): what the
// binary's tests alone reached.
mod mapping {
    use super::super::*;
    use library_application::McpListRow;
    use serde_json::json;

    const PARENT: &str = "projects/p";

    fn json_of<M: serde::Serialize>(message: &M) -> serde_json::Value {
        let mut out = Vec::new();
        crate::api::print(&mut out, message).unwrap();
        serde_json::from_slice(&out).unwrap()
    }

    #[test]
    fn test_a_shown_server_is_its_whole_resource() {
        let show = McpShow {
            name: "tree".to_string(),
            found: true,
            entries: vec![McpRow {
                name: "tree".to_string(),
                command: "tree-sitter".to_string(),
                args: vec!["--stdio".to_string()],
                env: [("K".to_string(), "v".to_string())].into(),
                url: "https://mcp.example".to_string(),
                served_by: "acme".to_string(),
                source: "demo".to_string(),
                notes: "a note".to_string(),
                installation: "npm i tree".to_string(),
            }],
        };
        assert_eq!(
            json_of(&shown(PARENT, &show)),
            json!({"mcp_servers": [{
                "name": "projects/p/bundles/demo/mcpServers/tree",
                "command": "tree-sitter",
                "args": ["--stdio"],
                "env": {"K": "v"},
                "url": "https://mcp.example",
                "served_by": "acme",
                "notes": "a note",
                "installation": "npm i tree",
            }]})
        );
    }

    #[test]
    fn test_a_listed_server_is_its_row() {
        let list = McpList {
            servers: vec![McpListRow {
                name: "tree".to_string(),
                command: "tree-sitter".to_string(),
                args: vec!["--stdio".to_string()],
                url: "https://mcp.example".to_string(),
                served_by: "acme".to_string(),
                source: "demo".to_string(),
            }],
        };
        assert_eq!(
            json_of(&listed(PARENT, &list)),
            json!({"mcp_servers": [{
                "name": "projects/p/bundles/demo/mcpServers/tree",
                "command": "tree-sitter",
                "args": ["--stdio"],
                "url": "https://mcp.example",
                "served_by": "acme",
            }]})
        );
    }

    #[test]
    fn test_a_set_server_answers_its_path() {
        let set = McpSet {
            status: "created".to_string(),
            bundle: "demo".to_string(),
            name: "tree".to_string(),
            path: "mcp/tree.yaml".to_string(),
        };
        assert_eq!(
            json_of(&set_of(PARENT, &set)),
            json!({
                "mcp_server": {"name": "projects/p/bundles/demo/mcpServers/tree"},
                "path": "mcp/tree.yaml",
                "applied": true,
                "created": true,
            })
        );
    }
}
