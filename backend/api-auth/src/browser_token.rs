use axum::{
    Json,
    extract::{Extension, State},
    http::HeaderMap,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::common::auth_utils::require_first_party_credential;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::{
    BrowserTokenService, Identity, RefreshError, TokenCredentialContext,
};
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::client::{
    ADMIN_WEB_CONSOLE_CLIENT_ID, USER_ACCOUNT_CENTER_CLIENT_ID, ports::ClientService,
};
use herald_core::infrastructure::authentication::RedisBrowserTokenService;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub refresh_expires_in: u64,
    pub token_type: String,
}

impl From<herald_core::domain::authentication::BrowserTokenSet> for BrowserTokenResponse {
    fn from(tokens: herald_core::domain::authentication::BrowserTokenSet) -> Self {
        Self {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_in: tokens.expires_in,
            refresh_expires_in: tokens.refresh_expires_in,
            token_type: tokens.token_type,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshBrowserTokenRequest {
    pub refresh_token: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SwitchClientRequest {
    pub target_client_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SwitchClientResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub refresh_expires_in: u64,
    pub token_type: String,
    pub client_id: String,
}

#[utoipa::path(
    post,
    path = "/api/auth/browser-token/refresh",
    tag = "auth",
    request_body = RefreshBrowserTokenRequest,
    responses((status = 200, body = BrowserTokenResponse), (status = 401))
)]
pub async fn refresh(
    State(state): State<AppState>,
    Json(request): Json<RefreshBrowserTokenRequest>,
) -> Result<ApiResult<BrowserTokenResponse>, ApiError> {
    let service = RedisBrowserTokenService::new(state.redis_manager.clone());
    let tokens = service
        .refresh(&request.refresh_token)
        .await
        .map_err(map_refresh_error)?;
    Ok(ApiResult::ok(tokens.into()))
}

const ADMIN_PERMISSIONS: &[&str] = &[
    "realm.view",
    "dashboard.view",
    "realm.manage",
    "users.view",
    "users.manage",
    "clients.view",
    "clients.manage",
    "roles.view",
    "roles.manage",
    "permissions.view",
    "permissions.manage",
    "policies.view",
    "policies.manage",
    "settings.view",
    "settings.manage",
    "audit.view",
    "api_keys.view",
    "api_keys.manage",
    "billing.view",
    "billing.manage",
    "points.manage",
];

#[utoipa::path(
    post,
    path = "/api/auth/browser-token/switch-client",
    tag = "auth",
    request_body = SwitchClientRequest,
    responses(
        (status = 200, body = SwitchClientResponse),
        (status = 400),
        (status = 401),
        (status = 403)
    ),
    security(("bearer_auth" = []))
)]
pub async fn switch_client(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(request): Json<SwitchClientRequest>,
) -> Result<ApiResult<SwitchClientResponse>, ApiError> {
    validate_switch_request(&context, &request.target_client_id)?;

    let user = identity
        .as_user()
        .ok_or_else(|| ApiError::forbidden("authenticated user token required"))?;
    if request.target_client_id == ADMIN_WEB_CONSOLE_CLIENT_ID {
        let permissions = state
            .permission_checker
            .get_user_permissions(&user.realm_id, &user.id.to_string())
            .await
            .map_err(|error| {
                tracing::error!(%error, "Failed to check admin-console eligibility");
                ApiError::internal("Failed to check permissions")
            })?;
        let eligible = permissions.iter().any(|permission| {
            let normalized = permission.replace(':', ".");
            ADMIN_PERMISSIONS.contains(&normalized.as_str())
        });
        if !eligible {
            return Err(ApiError::forbidden("admin console access denied"));
        }
    }

    let target = state
        .service
        .client_service()
        .get_client_app_by_client_id(&user.realm_id, &request.target_client_id)
        .await
        .map_err(|_| ApiError::bad_request("target client is unavailable"))?;
    if !target.enabled || !target.is_first_party {
        return Err(ApiError::bad_request("target client is unavailable"));
    }

    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());
    let tokens = token_service
        .create_first_party_token_family(
            user,
            &target,
            user_agent_from_headers(&headers),
            Some(ip.clone()),
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to create target product token family");
            ApiError::internal("Failed to switch client")
        })?;

    if let Err(error) = token_service.revoke_family(context.family_id).await {
        if let Ok(Some(created)) = token_service
            .lookup_access_token(&tokens.access_token)
            .await
        {
            let _ = token_service.revoke_family(created.family_id).await;
        }
        tracing::error!(%error, "Failed to revoke source product token family");
        return Err(ApiError::internal("Failed to switch client"));
    }

    let user_agent = user_agent_from_headers(&headers);
    if let Err(error) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: user.realm_id.clone(),
            category: AuditCategory::Auth,
            action: AuditAction::AuthClientSwitch,
            actor_id: user.id.to_string(),
            actor_type: Some(ActorType::User),
            actor_name: Some(user.email.clone()),
            target_type: AuditTargetType::Session,
            target_id: context.family_id.to_string(),
            target_name: None,
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "source_client_id": context.client_id,
                "target_client_id": request.target_client_id,
            })),
            ip_address: Some(ip),
            user_agent,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(%error, "Failed to record client-switch audit event");
    }

    Ok(ApiResult::ok(SwitchClientResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_in: tokens.expires_in,
        refresh_expires_in: tokens.refresh_expires_in,
        token_type: tokens.token_type,
        client_id: request.target_client_id,
    }))
}

fn validate_switch_request(
    context: &TokenCredentialContext,
    target_client_id: &str,
) -> Result<(), ApiError> {
    require_first_party_credential(context)?;
    if !matches!(
        target_client_id,
        ADMIN_WEB_CONSOLE_CLIENT_ID | USER_ACCOUNT_CENTER_CLIENT_ID
    ) {
        return Err(ApiError::bad_request("unsupported first-party client"));
    }
    if context.client_id == target_client_id {
        return Err(ApiError::bad_request("target client is already active"));
    }
    Ok(())
}

fn map_refresh_error(error: RefreshError) -> ApiError {
    match error {
        RefreshError::Invalid | RefreshError::ReuseDetected => {
            ApiError::unauthorized("invalid refresh token")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use herald_core::domain::authentication::CredentialClass;
    use std::collections::HashSet;
    use uuid::Uuid;

    fn credential_context(
        credential_class: CredentialClass,
        client_id: &str,
    ) -> TokenCredentialContext {
        TokenCredentialContext {
            client_app_id: Uuid::now_v7(),
            client_id: client_id.to_string(),
            family_id: Uuid::now_v7(),
            credential_class,
            allowed_scopes: HashSet::new(),
        }
    }

    #[test]
    fn browser_token_refresh_errors_are_all_unauthorized() {
        for error in [RefreshError::Invalid, RefreshError::ReuseDetected] {
            assert_eq!(
                map_refresh_error(error).into_response().status(),
                axum::http::StatusCode::UNAUTHORIZED
            );
        }
    }

    #[test]
    fn client_switch_only_accepts_cross_product_first_party_requests() {
        let personal =
            credential_context(CredentialClass::FirstParty, USER_ACCOUNT_CENTER_CLIENT_ID);
        assert!(validate_switch_request(&personal, ADMIN_WEB_CONSOLE_CLIENT_ID).is_ok());
        assert!(validate_switch_request(&personal, USER_ACCOUNT_CENTER_CLIENT_ID).is_err());
        assert!(validate_switch_request(&personal, "third-party-client").is_err());

        let custom = credential_context(CredentialClass::CustomUserUi, "custom-user-ui");
        assert!(validate_switch_request(&custom, ADMIN_WEB_CONSOLE_CLIENT_ID).is_err());
    }
}
