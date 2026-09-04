// =============================================================================
// Authorization Utilities
// =============================================================================
//
// Shared authorization helper functions to reduce code duplication across
// HTTP handlers. These utilities provide consistent authorization checks
// with proper error handling and logging.
//
// =============================================================================

use crate::application::http::server::api_entities::ApiError;
use crate::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::{
    CredentialClass, CredentialScope, Identity, TokenCredentialContext,
};
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::client::ADMIN_WEB_CONSOLE_CLIENT_ID;
#[cfg(test)]
use herald_core::domain::client::USER_ACCOUNT_CENTER_CLIENT_ID;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SelfIdentity {
    identity: Identity,
    user_id: Uuid,
}

impl SelfIdentity {
    pub fn require(identity: Identity) -> Result<Self, ApiError> {
        if !identity.is_user() {
            return Err(ApiError::forbidden(
                "Access denied: authenticated user token required",
            ));
        }

        let user_id = Uuid::parse_str(&identity.user_id())
            .map_err(|_| ApiError::internal("Invalid user ID"))?;

        Ok(Self { identity, user_id })
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn into_identity(self) -> Identity {
        self.identity
    }

    pub fn realm_id(&self) -> String {
        self.identity.realm_id()
    }

    pub fn user_id(&self) -> Uuid {
        self.user_id
    }

    pub fn user_id_string(&self) -> String {
        self.user_id.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct AdminIdentity {
    identity: Identity,
    realm_id: String,
    user_id: Uuid,
}

impl AdminIdentity {
    pub fn require(identity: Identity, realm_id: &str, context: &str) -> Result<Self, ApiError> {
        if !identity.is_user() {
            return Err(ApiError::forbidden(format!(
                "Access denied: authenticated user token required for {}",
                context
            )));
        }

        if identity.realm_id() != realm_id {
            return Err(ApiError::forbidden(format!(
                "Access denied: cannot access {} from a different realm",
                context
            )));
        }

        let user_id = Uuid::parse_str(&identity.user_id())
            .map_err(|_| ApiError::internal("Invalid user ID"))?;

        Ok(Self {
            identity,
            realm_id: realm_id.to_string(),
            user_id,
        })
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn into_identity(self) -> Identity {
        self.identity
    }

    pub fn realm_id(&self) -> &str {
        &self.realm_id
    }

    pub fn user_id(&self) -> Uuid {
        self.user_id
    }

    pub fn user_id_string(&self) -> String {
        self.user_id.to_string()
    }

    pub async fn require_permission(
        &self,
        state: &AppState,
        resource: &str,
        action: &str,
    ) -> Result<(), ApiError> {
        let allowed = state
            .permission_checker
            .check_permission(&self.realm_id, &self.user_id.to_string(), resource, action)
            .await
            .map_err(|e| {
                tracing::error!(
                    error = %e,
                    realm_id = %self.realm_id,
                    user_id = %self.user_id,
                    resource,
                    action,
                    "Permission check failed"
                );
                ApiError::internal("Failed to check permission")
            })?;

        if !allowed {
            record_permission_denied_audit(
                state,
                &self.realm_id,
                &self.user_id.to_string(),
                self.identity.as_user().map(|u| u.email.clone()),
                resource,
                action,
            )
            .await;
            return Err(ApiError::forbidden(format!(
                "Insufficient permissions: requires {resource}.{action}"
            )));
        }

        Ok(())
    }
}

/// Record the PermissionDenied audit event for a rejected permission check
/// (Audit PRD §4.2: denied 403 operations must also produce a failed audit
/// event). Best-effort — the 403 is already decided and must not fail
/// because of the audit write.
pub async fn record_permission_denied_audit(
    state: &AppState,
    realm_id: &str,
    user_id: &str,
    actor_name: Option<String>,
    resource: &str,
    action: &str,
) {
    let permission = format!("{resource}.{action}");
    if let Err(audit_err) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.to_string(),
            category: AuditCategory::Rbac,
            action: AuditAction::PermissionDenied,
            actor_id: user_id.to_string(),
            actor_type: Some(ActorType::User),
            actor_name,
            target_type: AuditTargetType::Permission,
            target_id: permission.clone(),
            target_name: Some(permission),
            result: AuditResult::Failure,
            details: Some(serde_json::json!({
                "resource": resource,
                "action": action,
                "reason": "permission_denied",
            })),
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record permission-denied audit event");
    }
}

pub fn require_token_scope(
    identity: &Identity,
    credential_context: &TokenCredentialContext,
    scope: CredentialScope,
) -> Result<(), ApiError> {
    if !identity.is_user() {
        return Err(ApiError::forbidden(
            "Access denied: authenticated user token required",
        ));
    }
    match credential_context.credential_class {
        CredentialClass::FirstParty => Ok(()),
        CredentialClass::CustomUserUi if credential_context.allowed_scopes.contains(&scope) => {
            Ok(())
        }
        CredentialClass::CustomUserUi => Err(ApiError::forbidden("token scope denied")),
    }
}

pub fn require_first_party_credential(
    credential_context: &TokenCredentialContext,
) -> Result<(), ApiError> {
    if credential_context.credential_class != CredentialClass::FirstParty {
        return Err(ApiError::forbidden(
            "Access denied: first-party credential required",
        ));
    }
    Ok(())
}

pub fn require_admin_console_credential(
    credential_context: &TokenCredentialContext,
) -> Result<(), ApiError> {
    require_first_party_credential(credential_context)?;
    if credential_context.client_id != ADMIN_WEB_CONSOLE_CLIENT_ID {
        return Err(ApiError::forbidden(
            "Access denied: admin console credential required",
        ));
    }
    Ok(())
}

pub fn require_authenticated_user_in_realm_with_token(
    identity: &Identity,
    credential_context: &TokenCredentialContext,
    realm_id: &str,
    context: &str,
) -> Result<Uuid, ApiError> {
    match credential_context.credential_class {
        CredentialClass::FirstParty | CredentialClass::CustomUserUi => {
            require_authenticated_user_in_realm(identity, realm_id, context)
        }
    }
}

/// Require an authenticated user identity and verify realm access.
///
/// This helper ensures that:
/// - The identity represents an authenticated user (not a client/API key)
/// - The user has access to the specified realm
///
/// # Arguments
/// * `identity` - The authenticated identity
/// * `realm_id` - The realm ID to verify access to
/// * `context` - Context description for error messages (e.g., "purchase APIs", "WeChat orders")
///
/// # Returns
/// * `Ok(Uuid)` - The parsed user ID if authorized
/// * `Err(ApiError)` - Forbidden error if unauthorized
pub fn require_authenticated_user_in_realm(
    identity: &Identity,
    realm_id: &str,
    context: &str,
) -> Result<Uuid, ApiError> {
    if !identity.is_user() {
        return Err(ApiError::forbidden(format!(
            "Access denied: authenticated user token required for {}",
            context
        )));
    }

    if !identity.has_access_to_realm(realm_id) {
        return Err(ApiError::forbidden(format!(
            "Access denied: cannot access {} from a different realm",
            context
        )));
    }

    Uuid::parse_str(&identity.user_id()).map_err(|_| ApiError::internal("Invalid user ID"))
}

/// Require realm access for the current identity.
///
/// This helper verifies that the identity has access to the specified realm,
/// preventing cross-realm access attempts.
///
/// # Arguments
/// * `identity` - The authenticated identity
/// * `realm_id` - The realm ID to verify access to
/// * `action` - The action being performed (for error messages)
///
/// # Returns
/// * `Ok(())` - If the identity has access to the realm
/// * `Err(ApiError)` - Forbidden error if cross-realm access attempted
pub fn require_realm_access(
    identity: &Identity,
    realm_id: &str,
    action: &str,
) -> Result<(), ApiError> {
    if identity.realm_id() != realm_id {
        return Err(ApiError::forbidden(format!(
            "Access denied: cannot {} resources in a different realm",
            action
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use herald_core::domain::{
        authentication::{CredentialClass, CredentialScope, Identity, TokenCredentialContext},
        client::entities::ClientApp,
        common::entities::generate_uuid_v7,
        user::entities::User,
    };
    use std::collections::HashSet;

    fn create_test_user(user_id: &str, realm_id: &str) -> User {
        User {
            id: generate_uuid_v7(),
            realm_id: realm_id.to_string(),
            email: format!("{}@example.com", user_id),
            nickname: None,
            password_hash: None,
            provider_ids: vec![],
            status: herald_core::domain::user::entities::UserStatus::Normal,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn create_test_client(client_id: &str, realm_id: &str) -> ClientApp {
        ClientApp {
            id: generate_uuid_v7(),
            realm_id: realm_id.to_string(),
            client_id: client_id.to_string(),
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
        }
    }

    fn token_context(
        credential_class: CredentialClass,
        allowed_scopes: impl IntoIterator<Item = CredentialScope>,
    ) -> TokenCredentialContext {
        let client_id = match credential_class {
            CredentialClass::FirstParty => ADMIN_WEB_CONSOLE_CLIENT_ID,
            CredentialClass::CustomUserUi => "custom-user-ui",
        };
        TokenCredentialContext {
            client_app_id: generate_uuid_v7(),
            client_id: client_id.to_string(),
            family_id: generate_uuid_v7(),
            credential_class,
            allowed_scopes: HashSet::from_iter(allowed_scopes),
        }
    }

    #[test]
    fn token_scope_allows_custom_user_ui_whitelisted_scope() {
        let identity = Identity::User(create_test_user("user123", "realm1"));
        let context = token_context(
            CredentialClass::CustomUserUi,
            [CredentialScope::ProfileRead],
        );

        assert!(require_token_scope(&identity, &context, CredentialScope::ProfileRead).is_ok());
    }

    #[test]
    fn token_scope_rejects_custom_user_ui_unlisted_scope_fail_closed() {
        let identity = Identity::User(create_test_user("user123", "realm1"));
        let context = token_context(CredentialClass::CustomUserUi, []);

        let error = require_token_scope(&identity, &context, CredentialScope::ProfileRead)
            .expect_err("an unlisted self-service capability must be denied");
        assert!(error.to_string().contains("token scope denied"));
    }

    #[test]
    fn token_scope_rejects_custom_user_ui_admin_capability() {
        let context = token_context(
            CredentialClass::CustomUserUi,
            [CredentialScope::ProfileRead],
        );

        let error = require_first_party_credential(&context)
            .expect_err("custom UI credentials must never enter admin RBAC");
        assert!(
            error
                .to_string()
                .contains("first-party credential required")
        );
    }

    #[test]
    fn token_scope_first_party_bypasses_scope_but_not_realm_authorization() {
        let identity = Identity::User(create_test_user("user123", "realm1"));
        let context = token_context(CredentialClass::FirstParty, []);

        assert!(require_token_scope(&identity, &context, CredentialScope::InvoiceApply).is_ok());
        require_first_party_credential(&context).unwrap();
        let error = AdminIdentity::require(identity, "realm2", "admin")
            .expect_err("scope bypass must not bypass the existing realm/RBAC entry gate");
        assert!(error.to_string().contains("different realm"));
    }

    #[test]
    fn admin_console_rejects_personal_center_first_party_credential() {
        let mut context = token_context(CredentialClass::FirstParty, []);
        context.client_id = USER_ACCOUNT_CENTER_CLIENT_ID.to_string();

        let error = require_admin_console_credential(&context)
            .expect_err("personal-center credentials must not enter admin routes");
        assert!(
            error
                .to_string()
                .contains("admin console credential required")
        );
    }

    #[test]
    fn test_require_realm_access_allows_same_realm() {
        let user = create_test_user("user123", "realm1");
        let identity = Identity::User(user);
        assert!(require_realm_access(&identity, "realm1", "view").is_ok());
    }

    #[test]
    fn test_require_realm_access_blocks_cross_realm() {
        let user = create_test_user("user123", "realm1");
        let identity = Identity::User(user);
        let result = require_realm_access(&identity, "realm2", "view");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("different realm"));
    }

    #[test]
    fn test_require_authenticated_user_in_realm_blocks_non_user() {
        let client = create_test_client("client123", "realm1");
        let identity = Identity::Client(client);
        let result = require_authenticated_user_in_realm(&identity, "realm1", "test action");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("authenticated user token required")
        );
    }

    #[test]
    fn test_require_authenticated_user_in_realm_blocks_cross_realm() {
        let user = create_test_user("user123", "realm1");
        let identity = Identity::User(user);
        let result = require_authenticated_user_in_realm(&identity, "realm2", "test action");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("different realm"));
    }

    #[test]
    fn test_require_authenticated_user_in_realm_returns_user_id() {
        let user_id = "123e4567-e89b-12d3-a456-426614174000";
        let user = create_test_user(user_id, "realm1");
        let expected_id = user.id; // Save the ID before moving user
        let identity = Identity::User(user);
        let result = require_authenticated_user_in_realm(&identity, "realm1", "test action");
        assert!(result.is_ok());
        // Verify we get back the same UUID that was assigned to the user
        assert_eq!(result.unwrap(), expected_id);
    }

    #[test]
    fn test_self_identity_requires_user() {
        let client = create_test_client("client123", "realm1");
        let result = SelfIdentity::require(Identity::Client(client));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("authenticated user token required")
        );
    }

    #[test]
    fn test_self_identity_wraps_current_user() {
        let user = create_test_user("user123", "realm1");
        let expected_id = user.id;
        let result = SelfIdentity::require(Identity::User(user)).unwrap();

        assert_eq!(result.realm_id(), "realm1");
        assert_eq!(result.user_id(), expected_id);
    }

    #[test]
    fn test_admin_identity_requires_same_realm() {
        let user = create_test_user("user123", "realm1");
        let result = AdminIdentity::require(Identity::User(user), "realm2", "admin test");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("different realm"));
    }

    #[test]
    fn test_admin_identity_wraps_same_realm_user() {
        let user = create_test_user("user123", "realm1");
        let expected_id = user.id;
        let result = AdminIdentity::require(Identity::User(user), "realm1", "admin test").unwrap();

        assert_eq!(result.realm_id(), "realm1");
        assert_eq!(result.user_id(), expected_id);
    }
}
