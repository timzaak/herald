// Account self-deletion (soft-delete) handler — BE-D07.
//
// DELETE /api/user, mounted on the existing `/api/user` route
// group (covered by `inject_token_identity` in server/mod.rs). The body
// carries the operation-bound reauthentication ticket.
//
// Error mapping (see design §4.2.2):
//   - 401 invalid / expired reauthentication ticket
//   - 409 account already deleted (CoreError::Conflict)
//   - 500 anything else
// The `From<CoreError> for ApiError` impl already covers this mapping, so the
// handler is a thin pass-through over the service.

use axum::{
    Json,
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::application::http::common::auth_utils::require_token_scope;
use crate::application::http::server::api_entities::{ApiError, ErrorResponse};
use crate::application::http::state::AppState;
use herald_api_auth::reauth::consume_reauth;
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_core::domain::audit::AuditContext;
use herald_core::domain::authentication::{
    CredentialScope, Identity, TargetOperation, TokenCredentialContext,
};

/// DELETE /api/user request body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAccountRequest {
    /// Single-use reauthentication ticket bound to account deletion.
    pub reauth_token: String,
}

/// Self-service account deletion (soft-delete).
///
/// Requires an authenticated browser token. Anonymizes PII,
/// marks the account `Deleted`, clears TOTP, cancels in-effect subscriptions,
/// revokes all sessions (incl. the caller's), and records a `user.delete`
/// audit event. One-time purchases are NOT refunded. Returns 204 on success —
/// the caller's token is invalidated by the session wipe.
#[utoipa::path(
    delete,
    path = "/api/user",
    tag = "user",
    request_body = DeleteAccountRequest,
    responses(
        (status = 204, description = "Account deleted (soft-delete); caller token revoked"),
        (status = 401, description = "Invalid or expired reauthentication", body = ErrorResponse),
        (status = 409, description = "Account is already deleted", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse)
    )
)]
pub async fn delete_account(
    State(app_state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<DeleteAccountRequest>,
) -> Result<StatusCode, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::DeleteAccount)?;
    consume_reauth(
        &app_state,
        &identity,
        &context,
        &req.reauth_token,
        TargetOperation::DeleteAccount,
    )
    .await?;
    let ctx = AuditContext::user(&identity, ip, user_agent_from_headers(&headers));
    app_state
        .self_delete_service
        .self_delete_account(&identity, &ctx)
        .await?;
    // No second revoke here: the service's Phase 3 already revoked every
    // browser token family of the user (including the caller's, fail-closed),
    // and a redundant post-delete revoke whose error propagates would answer
    // 500 for an account that was in fact deleted successfully.
    Ok(StatusCode::NO_CONTENT)
}
