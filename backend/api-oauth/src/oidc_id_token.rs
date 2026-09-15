//! OIDC id_token issuance.
//!
//! Signs the minimal fixed claim set (PRD-pinned — nothing added, nothing
//! removed) with the platform's active RS256 key. Issuance happens at
//! authorization-code exchange time, so it inherits every gate the code
//! itself passed through: login consent, second factors, client enablement.

use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::security_constants::OIDC_ID_TOKEN_TTL_SECONDS;
use herald_core::domain::user::{User, UserRepository, UserStatus};
use herald_core::infrastructure::oidc_signing_key::ActiveSigningKey;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;

use crate::oidc_discovery::build_issuer;

#[derive(Debug, Serialize)]
pub struct OidcIdTokenClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub exp: i64,
    pub iat: i64,
    pub email: String,
    pub email_verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

/// Only Normal counts as a verified mailbox; WaitVerified users still
/// authenticate (they may be mid-verification) and must be reported honestly.
/// Forbidden/Deleted never reach issuance — the exchange rejects them first.
pub(crate) fn email_verified_from_status(status: &UserStatus) -> bool {
    matches!(status, UserStatus::Normal)
}

/// The user's nickname, shared by id_token issuance and userinfo so the two
/// identity surfaces cannot drift. The nickname lives on the profile row and
/// may be absent; a missing profile omits the claim rather than reporting an
/// empty nickname.
pub(crate) async fn profile_nickname(
    state: &AppState,
    user: &User,
) -> Result<Option<String>, ApiError> {
    match state.user_repository.get_profile(user.id).await {
        Ok(profile) => Ok(profile.nickname),
        Err(CoreError::NotFound) => Ok(None),
        Err(error) => {
            tracing::error!(%error, user_id = %user.id, "OIDC profile lookup failed");
            Err(ApiError::internal("Internal server error"))
        }
    }
}

/// Assemble the claims and sign the RS256 JWT with the key id in the header
/// so clients pick the right JWKS entry. Free of I/O by design: the caller
/// (`token.rs`) fetches the nickname/key/origin inputs concurrently before
/// the token family is written.
pub(crate) fn issue_id_token(
    realm_id: &str,
    client_id: &str,
    user: &User,
    nickname: Option<String>,
    nonce: Option<String>,
    active_key: &ActiveSigningKey,
    origin: &str,
) -> Result<String, ApiError> {
    let now = chrono::Utc::now().timestamp();
    let claims = OidcIdTokenClaims {
        iss: build_issuer(origin, realm_id),
        sub: user.id.to_string(),
        aud: client_id.to_string(),
        exp: now + OIDC_ID_TOKEN_TTL_SECONDS,
        iat: now,
        email: user.email.clone(),
        email_verified: email_verified_from_status(&user.status),
        nickname,
        nonce,
    };

    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(active_key.kid.clone());
    let encoding_key =
        EncodingKey::from_rsa_pem(active_key.private_key_pem.as_bytes()).map_err(|error| {
            tracing::error!(%error, "Stored OIDC signing key is not a valid RSA PEM");
            ApiError::internal("Internal server error")
        })?;
    encode(&header, &claims, &encoding_key).map_err(|error| {
        tracing::error!(%error, "Failed to sign OIDC id_token");
        ApiError::internal("Internal server error")
    })
}

/// Best-effort `oidc.id_token_issue` audit — never blocks the exchange.
pub(crate) async fn audit_id_token_issued(
    state: &AppState,
    realm_id: &str,
    user: &User,
    client_id: &str,
    ip: &str,
) {
    if let Err(error) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.to_string(),
            category: AuditCategory::OAuth,
            action: AuditAction::OidcIdTokenIssue,
            actor_id: user.id.to_string(),
            actor_type: Some(ActorType::User),
            actor_name: Some(user.email.clone()),
            target_type: AuditTargetType::User,
            target_id: user.id.to_string(),
            target_name: Some(user.email.clone()),
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "client_id": client_id,
            })),
            ip_address: Some(ip.to_string()),
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(%error, "Failed to record OIDC id_token issue audit event");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // WHY: email_verified is an identity assertion other systems trust; the
    // mapping from the account status machine must stay honest — only a
    // Normal (post-verification) account may report true.
    #[test]
    fn email_verified_is_true_only_for_normal_status() {
        assert!(email_verified_from_status(&UserStatus::Normal));
        assert!(!email_verified_from_status(&UserStatus::WaitVerified));
        assert!(!email_verified_from_status(&UserStatus::Forbidden));
        assert!(!email_verified_from_status(&UserStatus::Deleted));
    }
}
