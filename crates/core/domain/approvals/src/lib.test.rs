use crate::{diagnostic_samples, APPROVALS_DOMAIN, APPROVAL_NAMES_DOMAIN, EN_US};

#[test]
fn the_domains_are_approvals_and_approval_names() {
    assert_eq!(APPROVALS_DOMAIN, "approvals");
    assert_eq!(APPROVAL_NAMES_DOMAIN, "approval-names");
}

#[test]
fn every_diagnostic_has_a_message_and_its_code_is_prefixed() {
    let (name, catalog) = EN_US;
    assert_eq!(name, "approvals");
    for sample in diagnostic_samples() {
        let code = sample.code();
        assert!(code.starts_with("approvals-"), "{code} is prefixed");
        assert!(
            catalog.contains(&format!("{code} =")),
            "{code} has no message"
        );
    }
    assert_eq!(diagnostic_samples().len(), 5);
}
