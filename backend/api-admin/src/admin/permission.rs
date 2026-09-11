use axum::{Extension, Json, extract::State};
use axum_valid::Valid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use herald_api_base::application::http::common::auth_utils::require_token_scope;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{
    BrowserTokenService, CredentialScope, Identity, TokenCredentialContext,
};
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use validator::Validate;

pub use herald_api_base::application::http::server::api_entities::ErrorResponse;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct Rule {
    pub resource: String,
    pub action: String,
}

#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCheckRequest {
    #[validate(length(min = 1))]
    pub token: String,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[validate(length(min = 1, max = 36))]
    pub client_id: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCheckResponse {
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Uuid>,
}

/// Permission check
///
/// Spec: POST /api/permission/check
#[utoipa::path(
  post,
  path = "/api/permission/check",
  tag = "permission",
  request_body = PermissionCheckRequest,
  responses(
    (status = 200, description = "Permission check result", body = PermissionCheckResponse),
    (status = 400, description = "Bad request", body = ErrorResponse),
    (status = 500, description = "Internal server error", body = ErrorResponse)
  )
)]
#[tracing::instrument(
    // Governance: `payload` carries the access token and permission rules
    // bound to a user. All skipped; only the low-cardinality op type recorded.
    skip(state, payload),
    fields(db.operation = "check_permission")
)]
pub async fn check_permission(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(credential_context): Extension<TokenCredentialContext>,
    Valid(Json(payload)): Valid<Json<PermissionCheckRequest>>,
) -> Result<ApiResult<PermissionCheckResponse>, ApiError> {
    // Self-introspection only (RFC 7662-style): the caller must authenticate,
    // and the probed token must belong to the caller. Without this the endpoint
    // is an unauthenticated live-token + RBAC oracle for any stolen token.
    if !identity.is_user() {
        return Err(ApiError::forbidden(
            "Access denied: authenticated user token required",
        ));
    }

    // Scope gate for CustomUserUi credentials, matching GET /api/user/permissions
    // (which exposes the same self-permission matrix behind ProfileRead).
    require_token_scope(&identity, &credential_context, CredentialScope::ProfileRead)?;

    let token_data = RedisBrowserTokenService::new(state.redis_manager.clone())
        .lookup_access_token(&payload.token)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Browser token permission lookup failed");
            ApiError::internal("Internal server error")
        })?;
    let Some(token_data) = token_data else {
        return Ok(ApiResult::ok(PermissionCheckResponse {
            allowed: false,
            user_id: None,
        }));
    };

    if token_data.user_id != identity.user_id() || token_data.realm_id != identity.realm_id() {
        return Err(ApiError::forbidden(
            "Access denied: can only check a token that belongs to you",
        ));
    }

    let rules = match payload.rules {
        Some(rules) if !rules.is_empty() => rules,
        _ => {
            let user_id = Uuid::parse_str(&token_data.user_id)
                .map_err(|_| ApiError::internal("Token contains invalid user_id".to_string()))?;
            return Ok(ApiResult::ok(PermissionCheckResponse {
                allowed: true,
                user_id: Some(user_id),
            }));
        }
    };

    let mut allowed = false;

    let permission_checker = &state.permission_checker;

    for rule in rules {
        let auth_res = permission_checker
            .check_permission(
                &token_data.realm_id,
                &token_data.user_id,
                &rule.resource,
                &rule.action,
            )
            .await
            .unwrap_or(false);

        if auth_res {
            allowed = true;
            break;
        }
    }

    let user_id = Uuid::parse_str(&token_data.user_id)
        .map_err(|_| ApiError::internal("Token contains invalid user_id".to_string()))?;

    Ok(ApiResult::ok(PermissionCheckResponse {
        allowed,
        user_id: Some(user_id),
    }))
}

// Governance tests.
//
// Covers: admin `check_permission` instrument skip correctness.
//
// WHY: `check_permission` reads a session `token` (credential) from `payload`,
// plus rules that may reference resources bound to
// a user. If the `#[instrument]` macro ever stops skipping those, the token /
// user-bound data leaks into a span field. Source-scan baseline,
// anchored to `fn check_permission` and its immediately-preceding
// `#[tracing::instrument(...)]`.
#[cfg(test)]
mod instrument_skip_tests {
    const SRC: &str = include_str!("permission.rs");

    fn instrument_body_preceding(fn_name: &str) -> String {
        let needle = format!("fn {fn_name}");
        let fn_pos = SRC
            .find(&needle)
            .unwrap_or_else(|| panic!("fn {fn_name} not found in source"));
        let attr_start = SRC[..fn_pos]
            .rfind("#[tracing::instrument(")
            .unwrap_or_else(|| panic!("no #[tracing::instrument( preceding fn {fn_name}"));
        let body_start = attr_start + "#[tracing::instrument(".len();
        // Find the attribute close: the first line at/after body_start whose
        // trimmed content is exactly `)]`. This handles indented closes (e.g.
        // inside an `impl` block) and ignores inline `))]` sequences such as
        // `#[validate(length(...))]` that appear on struct fields.
        let tail = &SRC[body_start..];
        let mut consumed = 0usize;
        for line in tail.lines() {
            let prev = consumed;
            consumed += line.len() + 1; // +1 for the line separator
            if line.trim() == ")]" {
                return tail[..prev].to_string();
            }
        }
        panic!("unterminated #[tracing::instrument( for fn {fn_name}")
    }

    #[test]
    fn instrument_skip_admin_check_permission_excludes_token_and_payload() {
        let body = instrument_body_preceding("check_permission");
        for required in ["state", "payload"] {
            assert!(
                body.contains(required),
                "check_permission must skip `{required}`; body was:\n{body}"
            );
        }
        for banned in ["token", "password", "email", "secret", "code"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "check_permission span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }
}
