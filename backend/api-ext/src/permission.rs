// Permission Check API for Third-Party Integration
//
// Allows third-party apps to check user permissions using API Key authentication.

use axum::{
    Json,
    extract::{Extension, State},
};
use herald_core::domain::authentication::{BrowserTokenService, Identity};
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::user::UserRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::client_app_scope::{bound_client_identifier, is_admin_api_key};
use herald_api_base::application::http::common::auth_utils::{
    TokenClientAppLookup, lookup_token_client_app,
};
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;

/// Create a denied permission check response
fn denied_response(error: Option<String>) -> Json<PermissionCheckResponse> {
    Json(PermissionCheckResponse {
        allowed: false,
        user_id: None,
        error,
    })
}

/// Structured 400 body for a missing/empty rules array (SDK-documented
/// contract; mirrors the `grant_bucket_required` shape in points.rs).
#[derive(Debug, Serialize, ToSchema)]
pub struct RulesRequiredBody {
    pub code: &'static str,
    pub message: &'static str,
}

/// Permission check request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCheckRequest {
    /// User's browser access token.
    pub access_token: String,

    /// Permission rules to check. Deserialized as `Option` so a missing or
    /// empty array yields a structured 400 `rules_required` body instead of a
    /// vacuous allowed=true (an empty rule set must not read as "all
    /// granted"), matching the SDK-documented contract.
    pub rules: Option<Vec<PermissionRule>>,
}

/// Single permission rule
#[derive(Debug, Deserialize, ToSchema, Clone)]
pub struct PermissionRule {
    /// Resource identifier (e.g., "article", "user")
    pub resource: String,

    /// Action identifier (e.g., "read", "write", "delete")
    pub action: String,
}

/// Permission check response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCheckResponse {
    /// Whether all permissions are granted
    pub allowed: bool,

    /// User ID (only present if allowed=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Error message (only present if error occurred)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Check user permissions
///
/// This endpoint allows third-party applications to check if a user
/// has specific permissions using their browser access token.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Example
/// ```json
/// {
///   "accessToken": "user-access-token",
///   "rules": [
///     {"resource": "article", "action": "read"},
///     {"resource": "article", "action": "write"}
///   ]
/// }
/// ```
#[utoipa::path(
    post,
    path = "/api/ext/permission/check",
    tag = "ext",
    request_body = PermissionCheckRequest,
    responses(
        (status = 200, description = "Permission check completed", body = PermissionCheckResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn check_permission(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Json(req): Json<PermissionCheckRequest>,
) -> Result<Json<PermissionCheckResponse>, ApiError> {
    // A missing or empty rules array is a request-contract error, not a
    // vacuous "all allowed": SDK callers that fail to populate rules must get
    // a 400 `rules_required`, never allowed=true with the caller's user id.
    let rules = req.rules.filter(|rules| !rules.is_empty()).ok_or_else(|| {
        ApiError::bad_request_json(RulesRequiredBody {
            code: "rules_required",
            message: "Permission check requires a non-empty rules array",
        })
    })?;

    tracing::info!(
        access_token_length = req.access_token.len(),
        rules_count = rules.len(),
        "Permission check requested"
    );

    let token_data = match RedisBrowserTokenService::new(state.redis_manager.clone())
        .lookup_access_token(&req.access_token)
        .await
    {
        Ok(Some(data)) => data,
        Ok(None) => {
            tracing::warn!("Browser access token not found or expired");
            return Ok(denied_response(Some("token_not_found".to_string())));
        }
        Err(e) => {
            tracing::error!("Failed to load session: {:?}", e);
            return Ok(denied_response(Some("internal_error".to_string())));
        }
    };

    if !identity.has_access_to_realm(&token_data.realm_id) {
        tracing::warn!(
            api_key_realm_id = %identity.realm_id(),
            token_realm_id = %token_data.realm_id,
            "Cross-realm permission check blocked"
        );
        return Ok(denied_response(Some(
            "cross_realm_access_forbidden".to_string(),
        )));
    }

    let admin_api_key = match is_admin_api_key(&state, &identity).await {
        Ok(value) => value,
        Err(_) => return Ok(denied_response(Some("internal_error".to_string()))),
    };
    let bound_client_id = match bound_client_identifier(&state, &identity).await {
        Ok(value) => value,
        Err(_) => return Ok(denied_response(Some("internal_error".to_string()))),
    };
    // The principal recheck runs through the same lookup identity_middleware
    // uses on every /api/user/* Bearer route, so the two surfaces cannot
    // drift: a disabled or deleted Client App row rejects the token outright.
    // Disabling/deleting also revokes the app's browser token families
    // (api-admin client_apps update/delete), but that revocation is
    // after-persist best-effort — on its failure tokens survive to natural
    // expiry, so this recheck must not assume they are gone. The enabled
    // state must be evaluated
    // for admin/unbound keys too, not only inside the bound-key comparison.
    let token_client_id =
        match lookup_token_client_app(&state, token_data.client_app_id, &token_data.realm_id)
            .await?
        {
            TokenClientAppLookup::Active { client_id } => Some(client_id),
            TokenClientAppLookup::Missing => {
                tracing::warn!(
                    token_client_app_id = %token_data.client_app_id,
                    "Browser token Client App no longer exists"
                );
                return Ok(denied_response(Some("invalid_token".to_string())));
            }
            TokenClientAppLookup::Disabled => {
                tracing::warn!(
                    token_client_app_id = %token_data.client_app_id,
                    "Browser token Client App is disabled"
                );
                return Ok(denied_response(Some("invalid_token".to_string())));
            }
        };
    if !admin_api_key && bound_client_id != token_client_id {
        tracing::warn!(
            api_key_id = %identity.id(),
            bound_client_id = ?bound_client_id,
            token_client_app_id = %token_data.client_app_id,
            "Client App scoped API key attempted permission check for another Client App"
        );
        return Ok(denied_response(Some("forbidden".to_string())));
    }

    let user_id = match Uuid::parse_str(&token_data.user_id) {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("Invalid user_id in browser token: {}", e);
            return Ok(denied_response(Some("invalid_token".to_string())));
        }
    };

    let user = match state.user_repository.get_user_by_id(user_id).await {
        Ok(user) if user.realm_id == token_data.realm_id => user,
        Ok(_) => {
            tracing::warn!(
                token_user_id = %token_data.user_id,
                token_realm_id = %token_data.realm_id,
                "User realm does not match token realm"
            );
            return Ok(denied_response(Some("invalid_token".to_string())));
        }
        Err(CoreError::NotFound) => {
            tracing::warn!(
                token_user_id = %token_data.user_id,
                "User referenced by browser token no longer exists"
            );
            return Ok(denied_response(Some("invalid_token".to_string())));
        }
        Err(e) => {
            tracing::error!("Failed to load user for permission check: {:?}", e);
            return Ok(denied_response(Some("internal_error".to_string())));
        }
    };

    // Defense in depth, mirroring identity_middleware: a Forbidden/Deleted
    // user's token families are revoked on the normal status-transition path,
    // but a direct-DB edit or a failed/lagged revocation must not leave the
    // introspection endpoint answering allowed=true (with the userId) for a
    // credential every Bearer route rejects. WaitVerified keeps access so
    // users can complete email verification.
    if user.status.is_disabled() {
        tracing::warn!(
            token_user_id = %token_data.user_id,
            "Browser token user is disabled; refusing introspection"
        );
        return Ok(denied_response(Some("invalid_token".to_string())));
    }

    // 3. Check all permission rules
    let permission_checker = &state.permission_checker;

    let mut all_allowed = true;
    for rule in &rules {
        match permission_checker
            .check_permission(
                &token_data.realm_id,
                &user_id.to_string(),
                &rule.resource,
                &rule.action,
            )
            .await
        {
            Ok(allowed) => {
                if !allowed {
                    all_allowed = false;
                    tracing::debug!(
                        realm_id = %token_data.realm_id,
                        user_id = %user_id,
                        resource = %rule.resource,
                        action = %rule.action,
                        "Permission denied"
                    );
                    break; // Early exit on first denial
                }
            }
            Err(e) => {
                tracing::error!("Failed to check permission: {:?}", e);
                return Ok(denied_response(Some("internal_error".to_string())));
            }
        }
    }

    tracing::info!(
        realm_id = %token_data.realm_id,
        user_id = %user_id,
        allowed = all_allowed,
        "Permission check completed"
    );

    Ok(Json(PermissionCheckResponse {
        allowed: all_allowed,
        user_id: all_allowed.then(|| user_id.to_string()),
        error: None,
    }))
}
