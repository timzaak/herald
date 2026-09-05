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
    #[serde(rename = "permission.update")]
    PermissionUpdate,
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
    /// An authenticated caller was denied an operation by the permission
    /// gate (403). Recorded best-effort by the `require_permission` choke
    /// points so failed authorization attempts are auditable (audit PRD
    /// §4.2: 权限不足等失败操作同样产生审计事件).
    #[serde(rename = "rbac.permission_denied")]
    PermissionDenied,
    #[serde(rename = "realm.create")]
    RealmCreate,
    #[serde(rename = "realm.rbac_init")]
    RealmRbacInit,
    /// Non-payment realm config row written via the generic configs API
    /// (SMTP, LDAP, Turnstile, registration policy, white-label, ...).
    /// Payment providers keep their dedicated `payment_config.*` actions.
    #[serde(rename = "realm_config.update")]
    RealmConfigUpdate,
    #[serde(rename = "realm_config.delete")]
    RealmConfigDelete,
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
    /// IAP client receipt submission (Apple jwsRepresentation / Google
    /// purchaseToken) verified and fulfilled.
    #[serde(rename = "iap.receipt_submit")]
    IapReceiptSubmit,
    /// IAP server-driven lifecycle event processed (Apple notification or
    /// Google reconciliation replay): purchase / renewal / expiration /
    /// refund / status change.
    #[serde(rename = "iap.notification")]
    IapNotification,
    #[serde(rename = "oauth_config.create")]
    OAuthConfigCreate,
    #[serde(rename = "oauth_config.update")]
    OAuthConfigUpdate,
    #[serde(rename = "oauth_config.delete")]
    OAuthConfigDelete,
    #[serde(rename = "passkey_config.update")]
    PasskeyConfigUpdate,
    #[serde(rename = "totp_config.update")]
    TotpConfigUpdate,
    #[serde(rename = "email_otp_config.update")]
    EmailOtpConfigUpdate,
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
