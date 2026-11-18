//! `item`'s tests, beside its verbs.

mod read {
    use crate::tests::*;

    // fragment.feature, command.feature: list (filtered by --bundle), show
    // (text only), premises; a bare noun lists.
    #[test]
    fn test_fragment_and_command_list_show_and_premises() {
        // OD-40: `ListFragmentsResponse`; an empty list is `{}`.
        let fragments = json_of(&["fragment", "list"]);
        assert_eq!(fragments["fragments"].as_array().unwrap().len(), 4);
        assert_eq!(
            fragments["fragments"][0]["name"],
            "projects/calm-otter/bundles/demo/fragments/ctl"
        );
        assert_eq!(fragments["fragments"][0]["reference"], "demo#fragments/ctl");
        assert_eq!(json_of(&["fragment"]), fragments);
        assert_eq!(
            json_of(&["fragment", "list", "--bundle", "project"]),
            serde_json::json!({})
        );
        assert_eq!(
            go(&["fragment", "list", "--bundle", "nosuch"], JSON, BARE).refused(),
            "library-application-no-bundle-named:20"
        );
        assert!(text_of(&["fragment", "list"]).starts_with("Fragments (4):\n"));
        let commands = json_of(&["command", "list"]);
        assert_eq!(
            commands["commands"][0]["name"],
            "projects/calm-otter/bundles/demo/commands/review"
        );
        assert_eq!(commands["commands"][0]["reference"], "demo#commands/review");
        assert_eq!(json_of(&["command"]), commands);
        assert_eq!(
            text_of(&["fragment", "show", "demo#fragments/golang"]),
            "golang\n\nGO-RULES\n"
        );
        assert_eq!(
            text_of(&["command", "show", "demo#commands/review"]),
            "review\n\nREVIEW-BODY\n"
        );
        assert_eq!(
            text_of(&["fragment", "show", "demo#fragments/dist", "-d"]),
            "# (distilled version)\ndist\n\nshort form\n"
        );
        assert!(
            !go(&["fragment", "show", "demo#commands/review"], TEXT, BARE)
                .refused()
                .is_empty()
        );
        // The `PremiseIndex` singleton, each entry naming its fragment.
        let premises = json_of(&["fragment", "premises"]);
        assert_eq!(premises["name"], "projects/calm-otter/premiseIndex");
        assert_eq!(
            premises["fragments"][0]["fragment"],
            "projects/calm-otter/bundles/demo/fragments/golang"
        );
        assert_eq!(premises["fragments"][0]["premise"], "when editing Go code");
        assert!(text_of(&["fragment", "premises"])
            .ends_with("- demo#fragments/golang: when editing Go code\n"));
    }

    // OD-40 (design §5.2): `show` answers json when it is asked for by name,
    // the item's resource with its body; by default it stays the body's text
    // (an agent's shell runs it off a terminal).
    #[test]
    fn test_an_item_show_answers_an_asked_for_json_and_a_default_text() {
        let asked = go(&["fragment", "show", "demo#fragments/golang"], JSON, BARE);
        let fragment = asked.json();
        assert_eq!(
            fragment["name"],
            "projects/calm-otter/bundles/demo/fragments/golang"
        );
        assert_eq!(fragment["body"], "GO-RULES\n");
        let by_default = How {
            explicit: None,
            terminal: false,
            degraded: false,
        };
        let ran = go(
            &["fragment", "show", "demo#fragments/golang"],
            by_default,
            BARE,
        );
        assert_eq!(ran.code(), 0);
        assert_eq!(ran.out, "golang\n\nGO-RULES\n");
        let command = go(&["command", "show", "demo#commands/review"], JSON, BARE).json();
        assert_eq!(
            command["name"],
            "projects/calm-otter/bundles/demo/commands/review"
        );
        assert_eq!(command["body"], "REVIEW-BODY\n");
        let short = go(
            &["fragment", "show", "demo#fragments/dist", "-d"],
            JSON,
            BARE,
        )
        .json();
        assert_eq!(short["body_is_distilled"], true);
        assert_eq!(short["body"], "short form\n");
        // S51: the distilled form's state against its source, judged now.
        assert_eq!(short["distilled"], "FRESH");
        assert_eq!(fragment["distilled"], "NONE");
    }
}

mod distill {
    use crate::tests::*;

    // Plan step 5: a form that matches its source is kept without asking
    // (applied false, saying why); one forced reads the distill prompt
    // through the catalog (ruling 128), and this one holds no built-in
    // `ctxloom` bundle, so it is refused, as `bundle distill` is.
    #[test]
    fn test_distill_keeps_a_fresh_form_and_fails_without_a_distiller() {
        let kept = json_of(&["fragment", "distill", "demo#fragments/dist"]);
        assert_eq!(
            kept["fragment"]["name"],
            "projects/calm-otter/bundles/demo/fragments/dist"
        );
        assert_eq!(kept["applied"], false);
        assert_eq!(kept["detail"], serde_json::json!(["fresh"]));
        assert_eq!(kept["fragment"]["distilled"], "FRESH");
        assert!(text_of(&["fragment", "distill", "demo#fragments/dist"])
            .contains("its distilled form matches its source"));
        assert_eq!(
            go(
                &["fragment", "distill", "demo#fragments/dist", "-f"],
                JSON,
                BARE
            )
            .refused(),
            "library-catalog-bundle-not-found:20"
        );
        assert_eq!(
            go(&["command", "distill", "demo#commands/review"], JSON, BARE).refused(),
            "library-catalog-bundle-not-found:20"
        );
        assert_eq!(
            go(&["bundle", "distill", "demo", "-f"], JSON, BARE).refused(),
            "library-catalog-bundle-not-found:20"
        );
    }
}

mod write {
    use crate::tests::write_verbs::*;
    use crate::tests::TEXT;

    // j001900: `fragment create <ref>` writes what the editor wrote, and makes a
    // bundle nobody had, saying so.
    #[test]
    fn test_fragment_create_by_reference_says_it_made_the_bundle() {
        let (library, mem) = world();
        mem.set_editor(|_, content| Ok(format!("{content}\nNOTES")));
        assert_eq!(
            text(&library, &["fragment", "create", "runbook#fragments/deploy"]),
            "Created bundle \"runbook\"\nCreated fragment \"deploy\" in bundle \"runbook\"\nEdit with: ctxloom fragment edit runbook#fragments/deploy\n"
        );
        assert!(mem
            .text("runbook", "fragments/deploy.md")
            .unwrap()
            .ends_with("NOTES"));
        // OD-40: `CreateCommandResponse` and `CreateFragmentResponse`.
        let doc = json(&library, &["command", "create", "demo#commands/third"]);
        assert_eq!(
            doc["command"]["name"],
            "projects/calm-otter/bundles/demo/commands/third"
        );
        assert_eq!(doc["applied"], true);
        assert!(doc.get("bundle_created").is_none(), "{doc}");
        let doc = json(&library, &["fragment", "create", "fresh#fragments/one"]);
        assert_eq!(doc["bundle_created"], true, "{doc}");
        assert_eq!(
            doc["fragment"]["name"],
            "projects/calm-otter/bundles/fresh/fragments/one"
        );
    }

    // fragment.feature / command.feature: create, remove, edit
    #[test]
    fn test_fragment_and_command_create_remove_and_edit() {
        let (library, mem) = world();
        assert_eq!(
            text(&library, &["fragment", "create", "demo", "testing"]),
            "Created fragment \"testing\" in bundle \"demo\"\nEdit with: ctxloom fragment edit demo#fragments/testing\n"
        );
        assert_eq!(
            text(&library, &["command", "create", "demo", "second"]),
            "Created command \"second\" in bundle \"demo\"\nEdit with: ctxloom command edit demo#commands/second\n"
        );
        assert_eq!(
            json(&library, &["fragment", "create", "demo", "third"])["fragment"]["name"],
            "projects/calm-otter/bundles/demo/fragments/third"
        );
        // `DeleteFragmentResponse`: the preview names the fragment.
        let preview = json(&library, &["fragment", "remove", "demo#fragments/testing"]);
        assert_eq!(preview["applied"], false);
        assert_eq!(
            preview["fragment"]["name"],
            "projects/calm-otter/bundles/demo/fragments/testing"
        );
        assert_eq!(
            preview["apply_command"],
            "ctxloom fragment remove demo#fragments/testing --yes"
        );
        assert_eq!(
            text(&library, &["fragment", "remove", "demo#fragments/testing"]),
            "Would remove fragment \"testing\" from bundle \"demo\".\n\nNothing was removed. Re-run with --yes to apply:\n  ctxloom fragment remove demo#fragments/testing --yes\n"
        );
        assert_eq!(
            text(
                &library,
                &["fragment", "rm", "demo#fragments/testing", "--yes"]
            ),
            "Removed fragment \"testing\" from bundle \"demo\"\n"
        );
        assert_eq!(
            text(
                &library,
                &["command", "remove", "demo#commands/second", "-y"]
            ),
            "Removed command \"second\" from bundle \"demo\"\n"
        );
        mem.set_editor(|_, content| Ok(format!("{content}\nMORE")));
        assert_eq!(
            text(&library, &["fragment", "edit", "demo#fragments/example", "--no-distill"]),
            "Updated fragment \"example\" in bundle \"demo\"\n\nBundle modified. To publish changes:\n  ctxloom bundle push demo [remote]\n"
        );
        assert!(mem
            .text("demo", "fragments/example.md")
            .unwrap()
            .contains("MORE"));
        let ran = run_on(
            &library,
            &["command", "edit", "demo#commands/review", "--no-distill"],
            TEXT,
        );
        assert_eq!(ran.code(), 0);
        assert!(
            ran.err
                .contains("distilled form not refreshed (--no-distill)"),
            "{}",
            ran.err
        );
        mem.set_editor(|_, content| Ok(content.to_string()));
        assert_eq!(
            text(&library, &["fragment", "edit", "demo#fragments/example"]),
            "No changes made.\n"
        );
        // `UpdateFragmentResponse`: an edit that changed nothing applied
        // nothing.
        let unchanged = json(&library, &["fragment", "edit", "demo#fragments/example"]);
        assert_eq!(unchanged["applied"], false);
        assert_eq!(
            unchanged["fragment"]["name"],
            "projects/calm-otter/bundles/demo/fragments/example"
        );
        let removed = json(
            &library,
            &["fragment", "remove", "demo#fragments/third", "-y"],
        );
        assert_eq!(removed["applied"], true);
        assert_eq!(
            removed["fragment"]["name"],
            "projects/calm-otter/bundles/demo/fragments/third"
        );
    }
}

// Each view's mapping to its message, field by field (OD-40): what the
// binary's tests alone reached.
mod mapping {
    use super::super::*;
    use library_application::Preview;
    use serde_json::json;

    const PARENT: &str = "projects/p";

    fn json_of<M: serde::Serialize>(message: &M) -> serde_json::Value {
        let mut out = Vec::new();
        crate::api::print(&mut out, message).unwrap();
        serde_json::from_slice(&out).unwrap()
    }

    fn done(status: &str, kind: &str) -> ItemDistilled {
        ItemDistilled {
            status: status.to_string(),
            kind: kind.to_string(),
            bundle: "demo".to_string(),
            name: "review".to_string(),
            path: "x/review.distilled.md".to_string(),
        }
    }

    #[test]
    fn test_a_command_marked_no_distill_answers_its_whole_response() {
        assert_eq!(
            json_of(&distilled(PARENT, &done("no_distill", "command"))),
            json!({
                "command": {
                    "name": "projects/p/bundles/demo/commands/review",
                    "distilled": "NONE",
                    "no_distill": true,
                },
                "path": "x/review.distilled.md",
                "applied": false,
                "detail": ["no_distill"],
            })
        );
        let made = json_of(&distilled(PARENT, &done("distilled", "command")));
        assert!(made["command"].get("no_distill").is_none(), "{made}");
        assert_eq!(made["applied"], true);
    }

    #[test]
    fn test_a_fragment_marked_no_distill_says_so_with_its_path() {
        let kept = json_of(&distilled(PARENT, &done("no_distill", "fragment")));
        assert_eq!(kept["fragment"]["no_distill"], true, "{kept}");
        assert_eq!(kept["path"], "x/review.distilled.md");
        let made = json_of(&distilled(PARENT, &done("distilled", "fragment")));
        assert!(made["fragment"].get("no_distill").is_none(), "{made}");
    }

    #[test]
    fn test_a_no_distill_item_says_nothing_was_distilled() {
        let mut out = Vec::new();
        distilled_text(&done("no_distill", "command"), &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "command \"review\" in bundle \"demo\" is marked as no_distill: nothing distilled\n"
        );
    }

    fn made(kind: &str) -> ItemCreated {
        ItemCreated {
            status: "created".to_string(),
            bundle: "demo".to_string(),
            name: "one".to_string(),
            path: "x/one.md".to_string(),
            kind: kind.to_string(),
            bundle_created: true,
        }
    }

    #[test]
    fn test_a_created_item_answers_its_path_and_its_bundle_made() {
        assert_eq!(
            json_of(&created(PARENT, &made("command"))),
            json!({
                "command": {"name": "projects/p/bundles/demo/commands/one"},
                "path": "x/one.md",
                "applied": true,
                "bundle_created": true,
            })
        );
        assert_eq!(
            json_of(&created(PARENT, &made("fragment"))),
            json!({
                "fragment": {"name": "projects/p/bundles/demo/fragments/one"},
                "path": "x/one.md",
                "applied": true,
                "bundle_created": true,
            })
        );
    }

    fn edit(kind: &str) -> ItemEdited {
        ItemEdited {
            status: "updated".to_string(),
            bundle: "demo".to_string(),
            name: "one".to_string(),
            path: "x/one.md".to_string(),
            distilled: true,
            kind: kind.to_string(),
            reference: String::new(),
            distill_skipped: false,
        }
    }

    #[test]
    fn test_an_edited_item_answers_its_path_and_its_fresh_form() {
        assert_eq!(
            json_of(&edited(PARENT, &edit("command"))),
            json!({
                "command": {
                    "name": "projects/p/bundles/demo/commands/one",
                    "distilled": "FRESH",
                },
                "path": "x/one.md",
                "applied": true,
            })
        );
        assert_eq!(
            json_of(&edited(PARENT, &edit("fragment"))),
            json!({
                "fragment": {
                    "name": "projects/p/bundles/demo/fragments/one",
                    "distilled": "FRESH",
                },
                "path": "x/one.md",
                "applied": true,
            })
        );
    }

    fn gone() -> Removal<ItemRemoved> {
        Removal::Done(ItemRemoved {
            status: "removed".to_string(),
            bundle: "demo".to_string(),
            name: "one".to_string(),
            path: "x/one.md".to_string(),
            kind: String::new(),
        })
    }

    fn preview() -> Removal<ItemRemoved> {
        Removal::Preview(Preview::of(
            "one".to_string(),
            vec!["x/one.md".to_string()],
            "ctxloom remove it --yes".to_string(),
        ))
    }

    #[test]
    fn test_a_removed_command_answers_its_removal_or_its_preview() {
        assert_eq!(
            json_of(&removed(
                PARENT,
                ItemKind::Command,
                "demo#commands/one",
                &gone()
            )),
            json!({
                "command": {"name": "projects/p/bundles/demo/commands/one"},
                "path": "x/one.md",
                "applied": true,
            })
        );
        assert_eq!(
            json_of(&removed(
                PARENT,
                ItemKind::Command,
                "demo#commands/one",
                &preview()
            )),
            json!({
                "command": {"name": "projects/p/bundles/demo/commands/one"},
                "applied": false,
                "detail": ["x/one.md"],
                "apply_command": "ctxloom remove it --yes",
            })
        );
    }

    #[test]
    fn test_a_removed_fragment_answers_its_removal_or_its_preview() {
        assert_eq!(
            json_of(&removed(
                PARENT,
                ItemKind::Fragment,
                "demo#fragments/one",
                &gone()
            )),
            json!({
                "fragment": {"name": "projects/p/bundles/demo/fragments/one"},
                "path": "x/one.md",
                "applied": true,
            })
        );
        assert_eq!(
            json_of(&removed(
                PARENT,
                ItemKind::Fragment,
                "demo#fragments/one",
                &preview()
            )),
            json!({
                "fragment": {"name": "projects/p/bundles/demo/fragments/one"},
                "applied": false,
                "detail": ["x/one.md"],
                "apply_command": "ctxloom remove it --yes",
            })
        );
    }
}

// Step 10a U2: each read's request carries `--distilled` and the
// command's global flags.
mod asks {
    use crate::tests::{asked, asked_once, degraded};
    use daemon_proto::v1;

    #[test]
    fn test_item_reads_carry_distilled_and_the_globals() {
        let (_, command) = asked_once::<v1::GetCommandRequest>(&[
            "command",
            "show",
            "--distilled",
            "demo#commands/review",
        ]);
        assert!(command.prefer_distilled);
        assert_eq!(command.globals, degraded());
        let (_, fragment) = asked_once::<v1::GetFragmentRequest>(&[
            "fragment",
            "show",
            "--distilled",
            "demo#fragments/golang",
        ]);
        assert!(fragment.prefer_distilled);
        assert_eq!(fragment.globals, degraded());
        let (under, index) = asked_once::<v1::GetPremiseIndexRequest>(&["fragment", "premises"]);
        assert_eq!(under, "/premiseIndex");
        assert_eq!(index.globals, degraded());
        let commands = asked::<v1::ListCommandsRequest>(&["command", "list"]);
        assert!(!commands.is_empty());
        assert!(commands.iter().all(|(_, r)| r.globals == degraded()));
        let fragments = asked::<v1::ListFragmentsRequest>(&["fragment", "list"]);
        assert!(!fragments.is_empty());
        assert!(fragments.iter().all(|(_, r)| r.globals == degraded()));
    }
}
