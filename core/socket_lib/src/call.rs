use crate::CallId;

#[derive(Debug, PartialEq, Eq)]
pub enum CallEndAction {
    TearDown(CallId),
    Acknowledge(CallId),
    Ignore,
}

pub fn is_current_call(call_id: &str, current: Option<&str>) -> bool {
    current == Some(call_id)
}

pub fn resolve_call_end(requested: Option<&str>, current: Option<&str>) -> CallEndAction {
    match (requested, current) {
        (Some(requested), Some(current)) if requested == current => {
            CallEndAction::TearDown(current.into())
        }
        (Some(requested), _) => CallEndAction::Acknowledge(requested.into()),
        (None, Some(current)) => CallEndAction::TearDown(current.into()),
        (None, None) => CallEndAction::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_actions_require_their_originating_call_to_still_be_active() {
        assert!(!is_current_call("A", None));
        assert!(!is_current_call("A", Some("B")));
        assert!(is_current_call("B", Some("B")));
    }

    #[test]
    fn hang_up_only_tears_down_its_own_call() {
        assert_eq!(
            resolve_call_end(Some("A"), Some("B")),
            CallEndAction::Acknowledge("A".into())
        );
        assert_eq!(
            resolve_call_end(Some("A"), None),
            CallEndAction::Acknowledge("A".into())
        );
        assert_eq!(
            resolve_call_end(Some("B"), Some("B")),
            CallEndAction::TearDown("B".into())
        );
        assert_eq!(
            resolve_call_end(None, Some("B")),
            CallEndAction::TearDown("B".into())
        );
        assert_eq!(resolve_call_end(None, None), CallEndAction::Ignore);
    }
}
