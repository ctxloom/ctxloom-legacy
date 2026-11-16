use std::sync::{Arc, Mutex};

use crate::{ChildPermissions, PermissionAnswer, PermissionAsk, PermissionAsks};

/// An asker that answers every ask with its first option, and keeps what
/// it was told.
#[derive(Default)]
struct First {
    withdrawn: Mutex<Vec<(String, String)>>,
}

impl PermissionAsks for First {
    fn ask(&self, ask: &PermissionAsk) -> PermissionAnswer {
        ask.options
            .first()
            .map_or(PermissionAnswer::Cancelled, |o| {
                PermissionAnswer::Selected(o.option_id.clone())
            })
    }

    fn withdraw(&self, request_id: &str, reason: &str) {
        self.withdrawn
            .lock()
            .unwrap()
            .push((request_id.to_string(), reason.to_string()));
    }
}

#[test]
fn a_routed_posture_is_said_by_its_name_alone_and_is_its_own_asker() {
    let asks: Arc<dyn PermissionAsks> = Arc::new(First::default());
    let routed = ChildPermissions::Ask(Arc::clone(&asks));
    assert_eq!(format!("{routed:?}"), "Ask");
    assert_eq!(format!("{:?}", ChildPermissions::Allow), "Allow");
    assert_eq!(format!("{:?}", ChildPermissions::Refuse), "Refuse");
    assert_eq!(routed, ChildPermissions::Ask(Arc::clone(&asks)));
    let other: Arc<dyn PermissionAsks> = Arc::new(First::default());
    assert_ne!(routed, ChildPermissions::Ask(other));
    assert_ne!(routed, ChildPermissions::Refuse);
    assert_ne!(ChildPermissions::Allow, ChildPermissions::Refuse);
    assert_eq!(ChildPermissions::Allow, ChildPermissions::Allow);
}
