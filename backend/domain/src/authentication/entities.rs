use crate::authentication::identity::{CredentialClass, CredentialScope};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct BrowserAccessTokenData {
    pub realm_id: String,
    pub client_app_id: Uuid,
    pub user_id: String,
    pub family_id: Uuid,
    pub credential_class: CredentialClass,
    /// Carries `CredentialScope::Openid` when the OAuth exchange that minted
    /// this token's family requested the `openid` scope (an id_token was
    /// issued); gates the OIDC userinfo endpoint (OIDC Core §5.3.1).
    pub allowed_scopes: HashSet<CredentialScope>,
    pub expires_at: DateTime<Utc>,
}

/// Ownership + lifecycle status of a single browser-token family, surfaced for
/// the admin "revoke one session" guard (design kickoff-user §4.2.2).
///
/// Unlike `list_user_sessions` (which filters out revoked/expired families and
/// is keyed by user_id), this is read directly from the family record
/// (`bt:fam:{familyId}`) so the caller can distinguish:
/// - a family that does **not** belong to the target user/realm (→ 404, prevent
///   cross-realm existence leakage), from
/// - a family that belongs to the target user/realm but is already revoked or
///   past its absolute expiry (→ 204 idempotent no-op).
///
/// `expired` is computed by the infra layer at read time from
/// `absolute_expires_at_ts <= now`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyLifecycle {
    pub user_id: String,
    pub realm_id: String,
    pub revoked: bool,
    pub expired: bool,
}

/// Snapshot of a single active session for a user, assembled from the browser
/// token family record (`bt:fam:{familyId}`) plus the independent session
/// metadata index (`bt:meta:{familyId}`, written at login). Legacy sessions
/// created before the meta index existed surface `client_app_name` /
/// `user_agent` / `client_ip` / `created_at` as `None`.
//
// `absolute_expires_at` is non-optional because it is always derivable from the
// family record's `absolute_expires_at_ts`; the meta-dependent fields above it
// are optional to tolerate legacy families without meta.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserSessionSummary {
    pub family_id: Uuid,
    pub realm_id: String,
    pub client_app_id: Uuid,
    pub client_app_name: Option<String>,
    pub credential_class: CredentialClass,
    pub user_agent: Option<String>,
    pub client_ip: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub absolute_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct BrowserRefreshTokenData {
    pub realm_id: String,
    pub client_app_id: Uuid,
    pub user_id: String,
    pub family_id: Uuid,
    pub successor_digest: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub absolute_expires_at: DateTime<Utc>,
    pub revoked: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTokenSet {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub refresh_expires_in: u64,
    pub token_type: String,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RefreshError {
    #[error("refresh token invalid or expired")]
    Invalid,
    #[error("refresh token reuse detected; family revoked")]
    ReuseDetected,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetOperation {
    ChangePassword,
    ChangeEmail,
    BindAuthenticator,
    RemoveAuthenticator,
    DeleteAccount,
}

impl TargetOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChangePassword => "change_password",
            Self::ChangeEmail => "change_email",
            Self::BindAuthenticator => "bind_authenticator",
            Self::RemoveAuthenticator => "remove_authenticator",
            Self::DeleteAccount => "delete_account",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReauthFactor {
    Password,
    Totp,
    Passkey,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "factor", content = "value")]
pub enum ReauthCredential {
    Password(String),
    Totp(String),
    Passkey {
        challenge_token: String,
        assertion: serde_json::Value,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ReauthResult {
    pub realm_id: String,
    pub client_app_id: Uuid,
    pub user_id: String,
    pub target_operation: TargetOperation,
    pub expires_at: DateTime<Utc>,
    pub consumed: bool,
}
