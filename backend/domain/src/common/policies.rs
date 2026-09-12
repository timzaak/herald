// Policy traits and AllowAll stub implementations
// PermissionBased* implementations moved to infrastructure/authorization/policies.rs

#![allow(clippy::manual_async_fn)]

use crate::authentication::Identity;

/// Resource types that can be accessed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    User,
    Realm,
    Client,
    OAuthConfig,
    RealmConfig,
    // Future resources can be added here
}

/// Actions that can be performed on resources
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Create,
    Read,
    Update,
    Delete,
    List,
    // Future actions can be added here
}

impl Resource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Resource::User => "users",
            Resource::Realm => "realms",
            Resource::Client => "clients",
            Resource::OAuthConfig => "oauth_configs",
            Resource::RealmConfig => "realm_configs",
        }
    }
}

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Create => "create",
            Action::Read => "read",
            Action::Update => "update",
            Action::Delete => "delete",
            Action::List => "list",
        }
    }
}

pub trait RealmPolicy: Send + Sync {
    fn can_create_realm(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_update_own_realm_settings(
        &self,
        identity: Identity,
    ) -> impl Future<Output = bool> + Send;
    fn can_list_realms(&self, identity: Identity) -> impl Future<Output = bool> + Send;
}

pub trait ClientPolicy: Send + Sync {
    fn can_create_client(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_read_client(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_update_client(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_delete_client(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_list_clients(&self, identity: Identity) -> impl Future<Output = bool> + Send;
}

pub trait UserPolicy: Send + Sync {
    fn can_create_user(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_read_user(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_update_user(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_delete_user(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_list_users(&self, identity: Identity) -> impl Future<Output = bool> + Send;
}

pub trait RealmConfigPolicy: Send + Sync {
    fn can_create_config(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_read_config(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_update_config(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_delete_config(&self, identity: Identity) -> impl Future<Output = bool> + Send;
}

pub trait OAuthConfigPolicy: Send + Sync {
    fn can_create_config(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_read_config(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_update_config(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_delete_config(&self, identity: Identity) -> impl Future<Output = bool> + Send;
    fn can_list_configs(&self, identity: Identity) -> impl Future<Output = bool> + Send;
}

/// Realm 允许所有策略（开发/测试用）
#[derive(Debug, Clone)]
pub struct AllowAllRealmPolicy;

impl RealmPolicy for AllowAllRealmPolicy {
    fn can_create_realm(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_update_own_realm_settings(
        &self,
        _identity: Identity,
    ) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_list_realms(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
}

/// Client 允许所有策略（开发/测试用）
#[derive(Debug, Clone)]
pub struct AllowAllClientPolicy;

impl ClientPolicy for AllowAllClientPolicy {
    fn can_create_client(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_read_client(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_update_client(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_delete_client(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_list_clients(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
}

/// User 允许所有策略（开发/测试用）
#[derive(Debug, Clone)]
pub struct AllowAllUserPolicy;

impl UserPolicy for AllowAllUserPolicy {
    fn can_create_user(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_read_user(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_update_user(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_delete_user(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_list_users(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
}

/// `RealmConfig` 允许所有策略（开发/测试用）
#[derive(Debug, Clone)]
pub struct AllowAllRealmConfigPolicy;

impl RealmConfigPolicy for AllowAllRealmConfigPolicy {
    fn can_create_config(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_read_config(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_update_config(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_delete_config(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
}

/// `OAuthConfig` 允许所有策略（开发/测试用）
#[derive(Debug, Clone)]
pub struct AllowAllOAuthConfigPolicy;

impl OAuthConfigPolicy for AllowAllOAuthConfigPolicy {
    fn can_create_config(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_read_config(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_update_config(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_delete_config(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
    fn can_list_configs(&self, _identity: Identity) -> impl Future<Output = bool> + Send {
        async move { true }
    }
}

use crate::common::entities::app_errors::CoreError;

/// 确保策略检查通过,如果失败则返回 Forbidden 错误
pub fn ensure_policy(condition: bool, message: &str) -> Result<(), CoreError> {
    if condition {
        Ok(())
    } else {
        Err(CoreError::Forbidden(message.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authentication::Identity;
    use crate::common::entities::generate_uuid_v7;
    use crate::user::entities::{User, UserStatus};
    use chrono::Utc;

    fn create_test_identity(user_id: &str, realm_id: &str) -> Identity {
        let user = User {
            id: generate_uuid_v7(),
            realm_id: realm_id.to_string(),
            email: format!("{}@test.com", user_id),
            nickname: None,
            password_hash: None,
            provider_ids: vec![],
            status: UserStatus::Normal,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        Identity::User(user)
    }

    #[tokio::test]
    async fn test_allow_all_realm_policy_allows_create() {
        let policy = AllowAllRealmPolicy;
        let identity = create_test_identity("user123", "admin");

        let can_create = policy.can_create_realm(identity).await;

        assert!(
            can_create,
            "AllowAllRealmPolicy should allow realm creation"
        );
    }

    #[tokio::test]
    async fn test_allow_all_realm_policy_allows_list() {
        let policy = AllowAllRealmPolicy;
        let identity = create_test_identity("user002", "test-realm");

        let can_list = policy.can_list_realms(identity).await;

        assert!(can_list, "AllowAllRealmPolicy should allow realm listing");
    }

    #[tokio::test]
    async fn test_ensure_policy_passes_when_true() {
        let result = ensure_policy(true, "Test policy");
        assert!(result.is_ok(), "Policy should pass when condition is true");
    }

    #[tokio::test]
    async fn test_ensure_policy_fails_when_false() {
        let result = ensure_policy(false, "Test policy failure");
        assert!(
            result.is_err(),
            "Policy should fail when condition is false"
        );
        if let Err(CoreError::Forbidden(msg)) = result {
            assert_eq!(msg, "Test policy failure");
        } else {
            panic!("Expected CoreError::Forbidden");
        }
    }
}
