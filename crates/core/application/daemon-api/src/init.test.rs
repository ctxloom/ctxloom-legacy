//! `InitProject`'s pieces (step 10c U6): its needs as the preview asks them,
//! its answers as the apply reads them, the checkout's harp staged with the
//! scaffold, and a retry's tree found in its intent.

use daemon_proto::v1;
use init_application::needs::{self, Given};
use library_api::writes::Shape;
use library_tree::TreeName;

use super::{given_of, installed_of, need_of, tree_of, with_checkout};

#[test]
fn test_every_need_init_asks_is_optional_and_keeps_its_kind() {
    let asked: Vec<v1::Need> = needs::needs(&["mempalace".to_string()])
        .iter()
        .map(need_of)
        .collect();
    assert!(asked.iter().all(|need| need.optional), "{asked:?}");
    assert!(matches!(
        &asked[0].kind,
        Some(v1::need::Kind::Ask(ask)) if ask.suggestion == "ctxloom/" && ask.text == init_application::OUTPUT_DIR_QUESTION
    ));
    assert!(matches!(
        &asked[1].kind,
        Some(v1::need::Kind::Choose(choose)) if choose.choices == ["on-demand", "always"]
    ));
    assert!(matches!(
        &asked[2].kind,
        Some(v1::need::Kind::Confirm(confirm)) if confirm.text == "Add the mempalace companion?"
    ));
    assert_eq!(asked[2].id, "companion:mempalace");
}

#[test]
fn test_the_answers_are_read_by_their_need_and_kind() {
    let answer = |id: &str, answer: v1::answer::Answer| v1::Answer {
        need_id: id.to_string(),
        answer: Some(answer),
    };
    let given = given_of(&[
        answer("output-dir", v1::answer::Answer::Text("out/".to_string())),
        answer("premised", v1::answer::Answer::Chosen("always".to_string())),
        answer("companion:x", v1::answer::Answer::Confirmed(true)),
        v1::Answer {
            need_id: "unanswered".to_string(),
            answer: None,
        },
    ]);
    assert_eq!(
        given,
        [
            ("output-dir".to_string(), Given::Text("out/".to_string())),
            ("premised".to_string(), Given::Chosen("always".to_string())),
            ("companion:x".to_string(), Given::Confirmed(true)),
        ]
    );
}

#[test]
fn test_the_checkout_harp_is_staged_with_the_scaffold() {
    let shape = with_checkout(
        Shape::Create {
            path: String::new(),
            files: vec![ctxloom_ports::Put::file("config.yaml", "a")],
        },
        "brisk-copper-moth",
    );
    let Shape::Create { path, files } = shape else {
        panic!("{shape:?}")
    };
    assert_eq!(path, "");
    let staged: Vec<(&str, &[u8])> = files
        .iter()
        .map(|put| (put.path.as_str(), put.bytes.as_slice()))
        .collect();
    assert_eq!(
        staged,
        [
            ("config.yaml", b"a".as_slice()),
            ("state/checkout", b"brisk-copper-moth\n".as_slice()),
        ]
    );
}

#[test]
fn test_a_retry_finds_its_tree_in_its_intent() {
    let name = "projects/calm-otter/checkouts/brisk-copper-moth/library";
    let known = library_checkout::Known {
        root: Vec::new(),
        intent: intent(vec![library_tree::Step {
            create: Some(library_tree::CreateDirectory {
                name: name.to_string(),
                verb: "init".to_string(),
                path: String::new(),
                entries: Vec::new(),
                evidence: library_tree::Evidence {
                    tree_reads: true,
                    plan_rejection: None,
                    builtin: false,
                },
            }),
            replace: None,
            trash: None,
            r#move: None,
            purge: None,
        }]),
        end: library_checkout::End::Pending { done: 0 },
    };
    assert_eq!(
        tree_of(&known).map(|tree| tree.to_string()).as_deref(),
        Some(name)
    );
    assert_eq!(
        tree_of(&known).as_ref().map(TreeName::checkout_harp),
        Some("brisk-copper-moth")
    );
    let none = library_checkout::Known {
        intent: intent(Vec::new()),
        ..known
    };
    assert_eq!(tree_of(&none), None);
}

fn intent(steps: Vec<library_tree::Step>) -> library_tree::Intent {
    library_tree::Intent {
        request_id: "r".to_string(),
        verb: "init".to_string(),
        steps,
        base_cause: String::new(),
        done: 0,
        foreign: Vec::new(),
    }
}

// `manage install` answers the project's own `.gitignore` beside its
// `.ctxloom`, and what it did.
#[test]
fn test_manage_install_answers_the_project_s_gitignore_and_what_it_did() {
    let made = installed_of(
        "/work/calm/.ctxloom",
        (true, "unchanged".to_string()),
        "calm-otter".to_string(),
        Vec::new(),
    );
    assert_eq!(made.gitignore, "/work/calm/.gitignore");
    assert_eq!(made.app_dir, "/work/calm/.ctxloom");
    assert!(made.initialized);
    assert_eq!(made.gitignore_status, "unchanged");
    assert_eq!(made.project_id, "calm-otter");
}
