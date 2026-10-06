use axum::extract::{Extension, State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{
    CredentialClass, CredentialScope, Identity, TokenCredentialContext,
};
use herald_core::domain::authorization::permission_service::PermissionService;

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub authenticated: bool,
    pub realm_id: Option<String>,
    pub user_id: Option<String>,
    /// Retained in the response shape for clients that display RBAC grants;
    /// browser-token authorization itself is governed by `scopes`.
    pub permissions: Option<Vec<String>>,
    pub client_app_id: uuid::Uuid,
    pub client_id: String,
    pub credential_class: CredentialClass,
    pub scopes: Vec<CredentialScope>,
}

#[utoipa::path(
  get,
  path = "/api/auth/status",
  tag = "auth",
  responses((status = 200, body = StatusResponse), (status = 401)),
  security(("bearer_auth" = []))
)]
pub async fn status(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
) -> Result<ApiResult<StatusResponse>, ApiError> {
    let user = identity
        .as_user()
        .ok_or_else(|| ApiError::forbidden("authenticated user token required"))?;
    // The realm RBAC grant list is profile-level data: custom-UI credentials
    // only see it when ProfileRead was granted. First-party tokens always do.
    let may_read_profile_grants = match context.credential_class {
        CredentialClass::FirstParty => true,
        CredentialClass::CustomUserUi => context
            .allowed_scopes
            .contains(&CredentialScope::ProfileRead),
        // MCP credentials never reach this browser surface; fail closed if
        // a future mount lets one through.
        CredentialClass::Mcp => false,
    };
    let mut scopes: Vec<_> = context.allowed_scopes.into_iter().collect();
    scopes.sort_by_key(|scope| format!("{scope:?}"));
    let permissions = if may_read_profile_grants {
        Some(
            state
                .permission_checker
                .get_user_permissions(&user.realm_id, &user.id.to_string())
                .await
                .map_err(|e| {
                    tracing::error!(
                        error = %e,
                        user_id = %user.id,
                        realm_id = user.realm_id,
                        "Failed to fetch user permissions for status"
                    );
                    ApiError::internal("Failed to fetch permissions")
                })?,
        )
    } else {
        None
    };
    Ok(ApiResult::ok(StatusResponse {
        authenticated: true,
        realm_id: Some(user.realm_id.clone()),
        user_id: Some(user.id.to_string()),
        permissions,
        client_app_id: context.client_app_id,
        client_id: context.client_id,
        credential_class: context.credential_class,
        scopes,
    }))
}
