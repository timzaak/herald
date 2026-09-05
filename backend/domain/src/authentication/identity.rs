// Identity enum - represents authenticated caller (User or Client)

use crate::{
    authorization::{PrincipalRef, principal::principal_types},
    client::entities::ClientApp,
    client_api_keys::entities::ClientApiKey,
    user::entities::User,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialClass {
    FirstParty,
    CustomUserUi,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema, Hash, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialScope {
    FeatureRead,
    ProfileRead,
    ProfileWriteNickname,
    ChangePassword,
    ChangeEmail,
    DeleteAccount,
    TotpManage,
    PasskeyManage,
    Logout,
    PointsRead,
    PointsTransactionsRead,
    PurchaseRead,
    PurchaseInitiate,
    PurchaseStatusRead,
    InvoiceRead,
    InvoiceApply,
    SubscriptionRead,
    SubscriptionCancel,
}

#[derive(Debug, Clone)]
pub struct TokenCredentialContext {
    pub client_app_id: Uuid,
    pub client_id: String,
    pub family_id: Uuid,
    pub credential_class: CredentialClass,
    pub allowed_scopes: HashSet<CredentialScope>,
}

/// Authenticated identity representing the caller
///
/// This enum contains the full User, ClientApp, or ClientApiKey entity, providing
/// complete context for authorization decisions.
///
/// # Examples
///
/// ```rust,no_run
/// use herald_core::domain::authentication::Identity;
/// use herald_core::domain::user::entities::User;
///
/// let identity = Identity::User(user);
/// let realm_id = identity.realm_id();
/// assert!(identity.has_access_to_realm(&realm_id));
/// ```
#[derive(Debug, Clone)]
pub enum Identity {
    User(User),
    Client(ClientApp),
    ThirdParty(ClientApiKey),
}

impl Identity {
    /// Get the unique identifier of this identity
    pub fn id(&self) -> String {
        match self {
            Self::User(user) => user.id.to_string(),
            Self::Client(client) => client.id.to_string(),
            Self::ThirdParty(api_key) => api_key.id.clone(),
        }
    }

    /// Get the realm_id this identity belongs to
    pub fn realm_id(&self) -> String {
        match self {
            Self::User(user) => user.realm_id.clone(),
            Self::Client(client) => client.realm_id.clone(),
            Self::ThirdParty(api_key) => api_key.realm_id.clone(),
        }
    }

    /// Get the user_id (only valid for User identity)
    pub fn user_id(&self) -> String {
        match self {
            Self::User(user) => user.id.to_string(),
            Self::Client(_) => String::new(),
            Self::ThirdParty(_) => String::new(),
        }
    }

    /// Get the client_id (only valid for Client identity)
    pub fn client_id(&self) -> String {
        match self {
            Self::User(_) => String::new(),
            Self::Client(client) => client.client_id.clone(),
            Self::ThirdParty(_) => String::new(),
        }
    }

    /// Get the roles for this identity
    ///
    /// Returns an empty vector as roles are managed separately through the RBAC system.
    /// Role-based authorization should use PermissionChecker or RolePermissionRepository.
    pub fn roles(&self) -> Vec<String> {
        vec![]
    }

    /// Check if this identity has access to the specified realm
    ///
    /// **Business Rule**: Each user/client can ONLY access resources in their own realm.
    /// There is NO Super Admin cross-realm access.
    ///
    /// # Arguments
    /// * `target_realm_id` - The realm to check access for
    ///
    /// # Returns
    /// * `true` if the identity belongs to the target realm
    /// * `false` otherwise
    pub fn has_access_to_realm(&self, target_realm_id: &str) -> bool {
        self.realm_id() == target_realm_id
    }

    /// Get the User entity (if this is a User identity)
    pub fn as_user(&self) -> Option<&User> {
        match self {
            Self::User(user) => Some(user),
            _ => None,
        }
    }

    /// Get the ClientApp entity (if this is a Client identity)
    pub fn as_client(&self) -> Option<&ClientApp> {
        match self {
            Self::Client(client) => Some(client),
            _ => None,
        }
    }

    /// Get the ClientApiKey entity (if this is a ThirdParty identity)
    pub fn as_third_party(&self) -> Option<&ClientApiKey> {
        match self {
            Self::ThirdParty(api_key) => Some(api_key),
            _ => None,
        }
    }

    /// Check if this identity represents a user
    pub fn is_user(&self) -> bool {
        matches!(self, Self::User(_))
    }

    /// Check if this identity represents a client
    pub fn is_client(&self) -> bool {
        matches!(self, Self::Client(_))
    }

    /// Check if this identity represents a third-party API key
    pub fn is_third_party(&self) -> bool {
        matches!(self, Self::ThirdParty(_))
    }

    /// Derive the lightweight PrincipalRef for authorization checks.
    ///
    /// - User callers: `principal_type = principal_types::USER`, `principal_id = user_id`.
    /// - API-key callers: `principal_type = principal_types::API_KEY`, `principal_id = client_api_keys.id`.
    /// - OAuth Client callers: `principal_type = principal_types::CLIENT`, `principal_id = client_app.id`.
    pub fn principal_ref(&self) -> PrincipalRef {
        match self {
            Self::User(user) => PrincipalRef {
                principal_type: principal_types::USER,
                principal_id: user.id.to_string(),
                realm_id: user.realm_id.clone(),
            },
            Self::Client(client) => PrincipalRef {
                principal_type: principal_types::CLIENT,
                principal_id: client.id.to_string(),
                realm_id: client.realm_id.clone(),
            },
            Self::ThirdParty(api_key) => PrincipalRef {
                principal_type: principal_types::API_KEY,
                principal_id: api_key.id.clone(),
                realm_id: api_key.realm_id.clone(),
            },
        }
    }
}

impl fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User(user) => write!(
                f,
                "User(id={}, realm_id={}, email={})",
                user.id, user.realm_id, user.email
            ),
            Self::Client(client) => write!(
                f,
                "Client(id={}, realm_id={}, client_id={})",
                client.id, client.realm_id, client.client_id
            ),
            Self::ThirdParty(api_key) => write!(
                f,
                "ThirdParty(id={}, realm_id={}, name={})",
                api_key.id, api_key.realm_id, api_key.name
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::entities::generate_uuid_v7;
    use chrono::Utc;

    #[test]
    fn test_identity_user_realm_access() {
        let realm_id = "test-realm".to_string();
        let user = User {
            id: generate_uuid_v7(),
            realm_id: realm_id.clone(),
            email: "test@example.com".to_string(),
            nickname: None,
            password_hash: None,
            provider_ids: vec![],
            status: crate::user::entities::UserStatus::Normal,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let identity = Identity::User(user);

        assert_eq!(identity.realm_id(), realm_id);
        assert!(identity.has_access_to_realm(&realm_id));
        assert!(!identity.has_access_to_realm("other-realm"));
        assert!(identity.is_user());
        assert!(!identity.is_client());
        assert!(identity.as_user().is_some());
        assert!(identity.as_client().is_none());
    }

    #[test]
    fn test_identity_client_realm_access() {
        let realm_id = "test-realm".to_string();
        let client = ClientApp {
            id: generate_uuid_v7(),
            realm_id: realm_id.clone(),
            client_id: "test-client".to_string(),
            name: "Test Client".to_string(),
            description: None,
            redirect_uris: vec![],
            allowed_origins: vec![],
            email_verify_return_url: None,
            password_reset_return_url: None,
            browser_refresh_absolute_ttl_seconds: 2_592_000,
            is_first_party: false,
            enabled: true,
            icon_url: None,
            client_secret: None,
            device_code_grant_enabled: false,
            turnstile_enabled: false,
            turnstile_site_key: None,
            turnstile_secret_key: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let identity = Identity::Client(client);

        assert_eq!(identity.realm_id(), realm_id);
        assert!(identity.has_access_to_realm(&realm_id));
        assert!(!identity.has_access_to_realm("other-realm"));
        assert!(!identity.is_user());
        assert!(identity.is_client());
        assert!(identity.as_user().is_none());
        assert!(identity.as_client().is_some());
    }

    #[test]
    fn test_identity_third_party_realm_access() {
        use crate::client_api_keys::entities::ClientApiKey;

        let realm_id = "test-realm".to_string();
        let api_key = ClientApiKey {
            id: generate_uuid_v7().to_string(),
            name: "Test API Key".to_string(),
            api_key_hash: "$argon2id$test".to_string(),
            realm_id: realm_id.clone(),
            client_app_id: None,
            enabled: true,
            expires_at: None,
            created_at: Utc::now(),
            last_used_at: None,
        };

        let identity = Identity::ThirdParty(api_key);

        assert_eq!(identity.realm_id(), realm_id);
        assert!(identity.has_access_to_realm(&realm_id));
        assert!(!identity.has_access_to_realm("other-realm"));
        assert!(!identity.is_user());
        assert!(!identity.is_client());
        assert!(identity.is_third_party());
        assert!(identity.as_third_party().is_some());
    }
}
