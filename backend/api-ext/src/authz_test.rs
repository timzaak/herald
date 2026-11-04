// Realm-membership authorization tests
//
// `require_realm_membership` is the ext API's cross-tenant boundary: it must
// enforce strict realm equality with no super-admin escape hatch, otherwise a
// leaked API key from any realm (including "admin") could read or mutate every
// other realm's users, client apps, and realms.

use crate::authz::require_realm_membership;
use axum::response::IntoResponse;
use herald_core::domain::authentication::Identity;
use herald_core::domain::user::entities::{User, UserStatus};

fn identity_in_realm(realm_id: &str) -> Identity {
    Identity::User(User {
        id: uuid::Uuid::nil(),
        realm_id: realm_id.to_string(),
        email: "key@example.com".to_string(),
        nickname: None,
        password_hash: None,
        provider_ids: vec![],
        status: UserStatus::Normal,
        phone: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    })
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn same_realm_identity_is_allowed() {
        assert!(
            require_realm_membership(&identity_in_realm("acme"), "acme", "user access").is_ok()
        );
    }

    #[test]
    fn cross_realm_identity_is_rejected() {
        let err = require_realm_membership(&identity_in_realm("acme"), "other", "user access")
            .expect_err("cross-realm access must be rejected");
        assert_eq!(
            err.into_response().status(),
            axum::http::StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn admin_realm_identity_cannot_access_other_realms() {
        // Regression: the "admin" realm used to bypass this check and could
        // reach any realm's ext endpoints. There is no cross-realm super-admin.
        let err = require_realm_membership(&identity_in_realm("admin"), "acme", "user access")
            .expect_err("admin-realm identity must not access other realms");
        assert_eq!(
            err.into_response().status(),
            axum::http::StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn admin_realm_identity_can_access_admin_realm_itself() {
        assert!(
            require_realm_membership(&identity_in_realm("admin"), "admin", "realm access").is_ok()
        );
    }
}
