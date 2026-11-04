use herald_core::domain::authentication::Identity;

pub(crate) fn formal_payment_email(identity: &Identity) -> Option<String> {
    let email = identity.as_user()?.email.trim();
    if email.is_empty() || email.ends_with("@wechat.placeholder") {
        None
    } else {
        Some(email.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::formal_payment_email;
    use chrono::Utc;
    use herald_core::domain::{
        authentication::Identity,
        user::entities::{User, UserStatus},
    };
    use uuid::Uuid;

    fn user_identity(email: &str) -> Identity {
        Identity::User(User {
            id: Uuid::now_v7(),
            realm_id: "test-realm".to_string(),
            email: email.to_string(),
            nickname: None,
            password_hash: None,
            provider_ids: vec![],
            status: UserStatus::Normal,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            phone: None,
        })
    }

    #[test]
    fn rejects_wechat_placeholder_email_because_non_wechat_providers_require_formal_email() {
        let identity = user_identity("openid123@wechat.placeholder");

        assert_eq!(formal_payment_email(&identity), None);
    }

    #[test]
    fn trims_and_returns_formal_email_for_payment_provider_prefill() {
        let identity = user_identity(" buyer@example.com ");

        assert_eq!(
            formal_payment_email(&identity),
            Some("buyer@example.com".to_string())
        );
    }
}
