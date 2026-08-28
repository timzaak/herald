use serde::{Deserialize, Serialize};

/// High-level category for audit events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditCategory {
    UserManagement,
    Rbac,
    RealmManagement,
    Auth,
    Billing,
    OAuth,
    Compliance,
}

/// Specific action that was performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditAction {
    #[serde(rename = "user.create")]
    UserCreate,
    #[serde(rename = "user.update")]
    UserUpdate,
    #[serde(rename = "user.delete")]
    UserDelete,
    #[serde(rename = "role.create")]
    RoleCreate,
    #[serde(rename = "role.update")]
    RoleUpdate,
    #[serde(rename = "role.delete")]
    RoleDelete,
    #[serde(rename = "permission.create")]
    PermissionCreate,
    #[serde(rename = "permission.delete")]
    PermissionDelete,
    #[serde(rename = "role.assign")]
    RoleAssign,
    #[serde(rename = "role.unassign")]
    RoleUnassign,
    #[serde(rename = "permission.grant")]
    PermissionGrant,
    #[serde(rename = "permission.revoke")]
    PermissionRevoke,
    #[serde(rename = "realm.create")]
    RealmCreate,
    #[serde(rename = "realm.rbac_init")]
    RealmRbacInit,
    #[serde(rename = "auth.login")]
    AuthLogin,
    #[serde(rename = "auth.logout")]
    AuthLogout,
    #[serde(rename = "auth.client_switch")]
    AuthClientSwitch,
    #[serde(rename = "auth.login_failed")]
    AuthLoginFailed,
    #[serde(rename = "product.create")]
    ProductCreate,
    #[serde(rename = "product.update")]
    ProductUpdate,
    #[serde(rename = "product.delete")]
    ProductDelete,
    #[serde(rename = "payment_config.update")]
    PaymentConfigUpdate,
    #[serde(rename = "payment_config.delete")]
    PaymentConfigDelete,
    #[serde(rename = "payment.webhook")]
    PaymentWebhook,
    #[serde(rename = "payment.replay")]
    PaymentReplay,
    #[serde(rename = "oauth_config.create")]
    OAuthConfigCreate,
    #[serde(rename = "oauth_config.update")]
    OAuthConfigUpdate,
    #[serde(rename = "oauth_config.delete")]
    OAuthConfigDelete,
    #[serde(rename = "passkey_config.update")]
    PasskeyConfigUpdate,
    #[serde(rename = "passkey.register")]
    PasskeyRegister,
    #[serde(rename = "passkey.delete")]
    PasskeyDelete,
    #[serde(rename = "agreement.consent")]
    AgreementConsent,
    #[serde(rename = "agreement.published")]
    AgreementPublished,
    #[serde(rename = "agreement.reverted")]
    AgreementReverted,
}

/// Type of the target entity an audit event refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditTargetType {
    User,
    Role,
    Permission,
    Realm,
    Session,
    Product,
    OAuthConfig,
    Payment,
}

/// Outcome of the audited operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    Success,
    Failure,
}

/// What kind of actor performed the operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    User,
    Admin,
    System,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_action_serializes_correctly() {
        let pairs: Vec<(AuditAction, &str)> = vec![
            (AuditAction::UserCreate, "user.create"),
            (AuditAction::UserUpdate, "user.update"),
            (AuditAction::UserDelete, "user.delete"),
            (AuditAction::RoleCreate, "role.create"),
            (AuditAction::RoleUpdate, "role.update"),
            (AuditAction::RoleDelete, "role.delete"),
            (AuditAction::PermissionCreate, "permission.create"),
            (AuditAction::PermissionDelete, "permission.delete"),
            (AuditAction::RoleAssign, "role.assign"),
            (AuditAction::RoleUnassign, "role.unassign"),
            (AuditAction::PermissionGrant, "permission.grant"),
            (AuditAction::PermissionRevoke, "permission.revoke"),
            (AuditAction::RealmCreate, "realm.create"),
            (AuditAction::RealmRbacInit, "realm.rbac_init"),
            (AuditAction::AuthLogin, "auth.login"),
            (AuditAction::AuthLogout, "auth.logout"),
            (AuditAction::AuthClientSwitch, "auth.client_switch"),
            (AuditAction::AuthLoginFailed, "auth.login_failed"),
            (AuditAction::PaymentConfigUpdate, "payment_config.update"),
            (AuditAction::PaymentConfigDelete, "payment_config.delete"),
            (AuditAction::PaymentWebhook, "payment.webhook"),
            (AuditAction::PaymentReplay, "payment.replay"),
            (AuditAction::OAuthConfigCreate, "oauth_config.create"),
            (AuditAction::OAuthConfigUpdate, "oauth_config.update"),
            (AuditAction::OAuthConfigDelete, "oauth_config.delete"),
            (AuditAction::PasskeyConfigUpdate, "passkey_config.update"),
            (AuditAction::PasskeyRegister, "passkey.register"),
            (AuditAction::PasskeyDelete, "passkey.delete"),
            (AuditAction::AgreementConsent, "agreement.consent"),
            (AuditAction::AgreementPublished, "agreement.published"),
            (AuditAction::AgreementReverted, "agreement.reverted"),
        ];

        for (variant, expected) in pairs {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(
                json.trim_matches('"'),
                expected,
                "{:?} should serialize to {}",
                variant,
                expected
            );
        }
    }
}
