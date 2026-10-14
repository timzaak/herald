//! Single source of the `resource.action` policy-matching semantics shared by
//! the flat RBAC check path (infra `RedisPermissionChecker`) and the
//! organization authorization layer (tier evaluation and in-org self-hold
//! comparison). Both consumers must read these items so the two surfaces can
//! never drift apart.

/// Actions a `manage` grant conveys on the same resource. Single source of
/// the manage→view/create implication (permissions PRD §4.1): both the
/// policy-check path (`action_matches_hierarchy`) and the listing path
/// (`expand_permission_hierarchy`) read this constant, so the two cannot
/// drift apart.
pub const MANAGE_IMPLIES: &[&str] = &["view", "create"];

/// Check if a granted action covers the requested action (with hierarchy)
///
/// Permission hierarchy: manage > create > view — `manage` is the only action
/// with downward implication, covering `view` and `create` on the same
/// resource (permissions PRD §4.1). Both the policy-check path
/// (`matches_policy`) and the effective-permission expansion
/// (`expand_permission_hierarchy`) derive from `MANAGE_IMPLIES`.
pub fn action_matches_hierarchy(granted_action: &str, requested_action: &str) -> bool {
    // Exact match always works
    if granted_action == requested_action {
        return true;
    }

    // Hierarchical checks
    match granted_action {
        "manage" => MANAGE_IMPLIES.contains(&requested_action),
        "view" => false,
        _ => false,
    }
}

// Hierarchy unit tests.
//
// WHY: the hierarchy is a published permission contract — `manage` conveys
// `view`/`create` and nothing else conveys anything. If these semantics drift
// (e.g. `create` starting to imply `view`), flat checks, listed effective
// permissions, and organization tier evaluation all change meaning at once.
#[cfg(test)]
mod hierarchy_tests {
    use super::{MANAGE_IMPLIES, action_matches_hierarchy};

    #[test]
    fn exact_match_always_grants() {
        for action in ["manage", "view", "create", "delete", "publish"] {
            assert!(action_matches_hierarchy(action, action));
        }
    }

    #[test]
    fn manage_conveys_view_and_create() {
        assert!(action_matches_hierarchy("manage", "view"));
        assert!(action_matches_hierarchy("manage", "create"));
    }

    #[test]
    fn downward_only_no_action_implies_manage() {
        assert!(!action_matches_hierarchy("view", "manage"));
        assert!(!action_matches_hierarchy("create", "manage"));
        // `create` does not imply `view` (permissions PRD §4.1).
        assert!(!action_matches_hierarchy("create", "view"));
    }

    #[test]
    fn view_conveys_nothing_beyond_itself() {
        assert!(!action_matches_hierarchy("view", "create"));
        assert!(!action_matches_hierarchy("view", "delete"));
    }

    #[test]
    fn unknown_actions_convey_nothing() {
        assert!(!action_matches_hierarchy("delete", "view"));
        assert!(!action_matches_hierarchy("admin", "manage"));
    }

    #[test]
    fn manage_implies_pinned_to_view_and_create() {
        assert_eq!(MANAGE_IMPLIES, &["view", "create"]);
    }
}
