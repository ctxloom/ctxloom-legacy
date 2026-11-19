//! init.feature's companion scenarios at the application port (D2: the
//! questions out, the answers in, as data).

use std::cell::RefCell;

use super::*;

/// The programs on her machine.
struct Machine {
    there: Vec<&'static str>,
    added: RefCell<Vec<String>>,
}

impl Companions for Machine {
    fn add(&self, name: &str) -> Result<Added, NotAdded> {
        if !self.there.contains(&name) {
            return Err(NotAdded::Missing {
                program: name.to_string(),
            });
        }
        let mut added = self.added.borrow_mut();
        if added.iter().any(|n| n == name) {
            return Ok(Added::Already);
        }
        added.push(name.to_string());
        Ok(Added::New)
    }
}

fn machine(there: &[&'static str]) -> Machine {
    Machine {
        there: there.to_vec(),
        added: RefCell::default(),
    }
}

fn answer(id: &str, given: Option<&str>) -> Answer {
    Answer {
        id: id.to_string(),
        given: given.map(ToString::to_string),
    }
}

#[test]
fn an_offer_answered_yes_and_the_names_typed_are_accepted_each_once() {
    let answers = [
        answer("companion:ltk", Some("yes")),
        answer("companion:reprise", Some("no")),
        answer("companion:taskloom", Some("YES")),
        answer("companions", Some(" acme, ltk  beta ")),
        answer("output", Some("anything")),
    ];
    assert_eq!(accepted(&answers), ["ltk", "taskloom", "acme", "beta"]);
    assert!(accepted(&[answer("companions", None), answer("companion:ltk", None)]).is_empty());
}

// "A first init adds the companions she names, and refuses one she does not
// have": the one she has is added, the other refused naming its program,
// and nothing stops for it.
#[test]
fn each_accepted_companion_is_added_and_one_she_does_not_have_is_refused() {
    let there = machine(&["ltk"]);
    let done = register(&there, &[answer("companions", Some("reprise ltk ltk"))]);
    assert_eq!(done.added, [("ltk".to_string(), Added::New)]);
    assert_eq!(
        done.missing,
        [("reprise".to_string(), "reprise".to_string())]
    );
    assert!(done.failed.is_empty());
    assert_eq!(*there.added.borrow(), ["ltk"]);
}
