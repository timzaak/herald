use axum::{
    Json,
    extract::{Extension, Path, State},
    http::HeaderMap,
};
use axum_valid::Valid;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use herald_api_base::application::http::auth::util::{
    ClientIp, rate_limit_hit, user_agent_from_headers,
};
use herald_api_base::application::http::common::auth_utils::require_token_scope;
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::{
    CredentialScope, Identity, TargetOperation, TokenCredentialContext,
};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::user::ports::UserRepository;
use herald_core::domain::user_passkey::{
    PasskeyCredentialView, PasskeyError, UserPasskeyRepository, UserPasskeyService,
};
use herald_core::infrastructure::user::repositories::PostgresUserRepository;
use herald_core::infrastructure::user_passkey::{
    PostgresPasskeyRealmConfigReader, PostgresUserPasskeyRepository, RedisPasskeyChallengeStore,
};

use crate::passkey_rp::{ensure_passkey_enabled, resolve_passkey_rp};
use crate::reauth::consume_reauth;

const PASSKEY_USER_RATE_LIMIT: (i64, usize) = (5, 60);
const PASSKEY_CHALLENGE_TTL_SECONDS: u64 = 300;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/passkey/registration/begin",
            axum::routing::post(handle_begin_passkey_registration),
        )
        .route(
            "/passkey/registration/finish",
            axum::routing::post(handle_finish_passkey_registration),
        )
        .route(
            "/passkey/credentials",
            axum::routing::get(handle_list_passkey_credentials),
        )
        .route(
            "/passkey/credentials/{credentialId}",
            axum::routing::patch(handle_rename_passkey_credential)
                .delete(handle_delete_passkey_credential),
        )
}

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct BeginRegistrationRequest {
    pub reauth_token: String,
    pub nickname: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BeginRegistrationResponse {
    pub reg_token: String,
    pub options: serde_json::Value,
}

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct FinishRegistrationRequest {
    pub reauth_token: String,
    pub reg_token: String,
    pub attestation: serde_json::Value,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FinishRegistrationResponse {
    pub credential_id: String,
    pub nickname: Option<String>,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyCredentialViewResponse {
    pub credential_id: String,
    pub nickname: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub backup_eligible: bool,
    pub backup_state: bool,
    pub transports: Vec<String>,
    pub aaguid: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListPasskeysResponse {
    pub credentials: Vec<PasskeyCredentialViewResponse>,
}

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RenamePasskeyRequest {
    #[validate(length(min = 1, max = 128))]
    pub nickname: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeletePasskeyRequest {
    pub reauth_token: String,
}

#[utoipa::path(
    post,
    path = "/api/user/passkey/registration/begin",
    tag = "user",
    request_body = BeginRegistrationRequest,
    responses(
        (status = 200, description = "Passkey registration challenge created", body = BeginRegistrationResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Passkey is not enabled for this realm", body = ErrorResponse),
        (status = 429, description = "Too many requests", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_begin_passkey_registration(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<BeginRegistrationRequest>>,
) -> Result<ApiResult<BeginRegistrationResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PasskeyManage)?;
    let user_id = identity_user_id(&identity)?;
    rate_limit_passkey_user(&state, user_id).await?;

    let user_repo = PostgresUserRepository::new(state.db.clone());
    let user = user_repo.get_user_by_id(user_id).await?;
    ensure_passkey_enabled(&state, &user.realm_id).await?;

    let repo = Arc::new(PostgresUserPasskeyRepository::new(state.db.clone()));
    let relying_party = resolve_passkey_rp(
        &state,
        &user.realm_id,
        &headers,
        Some(context.client_app_id),
    )
    .await?;
    let existing = repo
        .list_by_user_and_rp(&user.realm_id, user_id, &relying_party.id)
        .await?;
    let exclude = existing
        .iter()
        .map(|credential| credential.credential_id.clone())
        .collect::<Vec<_>>();
    let service = passkey_service(&state, repo)?;

    // Consume the single-use reauth ticket only after all prerequisites have
    // been validated and just before the state-mutating operation.
    consume_reauth(
        &state,
        &identity,
        &context,
        &req.reauth_token,
        TargetOperation::BindAuthenticator,
    )
    .await?;

    let (options, reg_token) = service
        .begin_registration(&user.realm_id, &user, &exclude, relying_party)
        .await
        .map_err(map_registration_begin_error)?;
    store_registration_nickname(&state, &reg_token, req.nickname.as_deref()).await?;
    let options =
        serde_json::to_value(options).map_err(|_| ApiError::internal("Internal server error"))?;

    Ok(ApiResult::ok(BeginRegistrationResponse {
        reg_token,
        options,
    }))
}

#[utoipa::path(
    post,
    path = "/api/user/passkey/registration/finish",
    tag = "user",
    request_body = FinishRegistrationRequest,
    responses(
        (status = 200, description = "Passkey registration finished", body = FinishRegistrationResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Invalid or expired registration token", body = ErrorResponse),
        (status = 409, description = "Credential already exists", body = ErrorResponse),
        (status = 422, description = "Attestation verification failed", body = ErrorResponse),
        (status = 429, description = "Too many requests", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_finish_passkey_registration(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<FinishRegistrationRequest>>,
) -> Result<ApiResult<FinishRegistrationResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PasskeyManage)?;
    let user_id = identity_user_id(&identity)?;
    rate_limit_passkey_user(&state, user_id).await?;

    let repo = Arc::new(PostgresUserPasskeyRepository::new(state.db.clone()));
    let service = passkey_service(&state, repo)?;
    let nickname = load_registration_nickname(&state, &req.reg_token).await?;

    // Consume the single-use reauth ticket only after validating the registration
    // token and just before the state-mutating operation.
    consume_reauth(
        &state,
        &identity,
        &context,
        &req.reauth_token,
        TargetOperation::BindAuthenticator,
    )
    .await?;

    let credential = service
        .finish_registration(
            &req.reg_token,
            &req.attestation,
            nickname.as_deref(),
            user_id,
            &identity.realm_id(),
        )
        .await
        .map_err(map_registration_finish_error)?;

    // Audit passkey credential registration (PRD §4.1 audit rule).
    if let Err(audit_err) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: identity.realm_id(),
            category: AuditCategory::Auth,
            action: AuditAction::PasskeyRegister,
            actor_id: user_id.to_string(),
            actor_type: Some(ActorType::User),
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::User,
            target_id: credential.id.to_string(),
            target_name: credential.nickname.clone(),
            result: AuditResult::Success,
            details: None,
            ip_address: Some(ip),
            user_agent: user_agent_from_headers(&headers),
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record passkey register audit event");
    }

    Ok(ApiResult::ok(FinishRegistrationResponse {
        credential_id: credential.id.to_string(),
        nickname: credential.nickname,
        created_at: credential.created_at.to_rfc3339(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/user/passkey/credentials",
    tag = "user",
    responses(
        (status = 200, description = "Passkey credentials retrieved", body = ListPasskeysResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Passkey is not enabled for this realm", body = ErrorResponse),
        (status = 429, description = "Too many requests", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_list_passkey_credentials(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    headers: HeaderMap,
) -> Result<ApiResult<ListPasskeysResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PasskeyManage)?;
    let user_id = identity_user_id(&identity)?;
    rate_limit_passkey_user(&state, user_id).await?;
    ensure_passkey_enabled(&state, &identity.realm_id()).await?;

    let repo = PostgresUserPasskeyRepository::new(state.db.clone());
    let relying_party = resolve_passkey_rp(
        &state,
        &identity.realm_id(),
        &headers,
        Some(context.client_app_id),
    )
    .await?;
    let credentials = repo
        .list_by_user_and_rp(&identity.realm_id(), user_id, &relying_party.id)
        .await?
        .into_iter()
        .map(|credential| PasskeyCredentialViewResponse::from(credential.to_view()))
        .collect();

    Ok(ApiResult::ok(ListPasskeysResponse { credentials }))
}

#[utoipa::path(
    patch,
    path = "/api/user/passkey/credentials/{credentialId}",
    tag = "user",
    params(
        ("credentialId" = String, Path, description = "Passkey credential UUID")
    ),
    request_body = RenamePasskeyRequest,
    responses(
        (status = 204, description = "Passkey credential renamed"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Credential not found, or Passkey is not enabled for this realm", body = ErrorResponse),
        (status = 429, description = "Too many requests", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_rename_passkey_credential(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    headers: HeaderMap,
    Path(credential_id): Path<String>,
    Valid(Json(req)): Valid<Json<RenamePasskeyRequest>>,
) -> Result<ApiResult<()>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PasskeyManage)?;
    let user_id = identity_user_id(&identity)?;
    rate_limit_passkey_user(&state, user_id).await?;
    ensure_passkey_enabled(&state, &identity.realm_id()).await?;
    let credential_id = Uuid::parse_str(&credential_id)
        .map_err(|_| ApiError::bad_request("Invalid credentialId"))?;

    let repo = PostgresUserPasskeyRepository::new(state.db.clone());
    let relying_party = resolve_passkey_rp(
        &state,
        &identity.realm_id(),
        &headers,
        Some(context.client_app_id),
    )
    .await?;
    repo.rename(
        &identity.realm_id(),
        user_id,
        &relying_party.id,
        credential_id,
        &req.nickname,
    )
    .await
    .map_err(map_repository_error)?;

    Ok(ApiResult::no_content())
}

#[utoipa::path(
    delete,
    path = "/api/user/passkey/credentials/{credentialId}",
    tag = "user",
    params(
        ("credentialId" = String, Path, description = "Passkey credential UUID")
    ),
    request_body = DeletePasskeyRequest,
    responses(
        (status = 204, description = "Passkey credential deleted"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Credential not found, or Passkey is not enabled for this realm", body = ErrorResponse),
        (status = 429, description = "Too many requests", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn handle_delete_passkey_credential(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(credential_id): Path<String>,
    Json(req): Json<DeletePasskeyRequest>,
) -> Result<ApiResult<()>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PasskeyManage)?;
    let user_id = identity_user_id(&identity)?;
    rate_limit_passkey_user(&state, user_id).await?;
    ensure_passkey_enabled(&state, &identity.realm_id()).await?;
    let credential_id = Uuid::parse_str(&credential_id)
        .map_err(|_| ApiError::bad_request("Invalid credentialId"))?;

    let repo = PostgresUserPasskeyRepository::new(state.db.clone());
    let relying_party = resolve_passkey_rp(
        &state,
        &identity.realm_id(),
        &headers,
        Some(context.client_app_id),
    )
    .await?;

    // Consume the single-use reauth ticket only after validating the credential
    // id and origin, and just before the state-mutating delete.
    consume_reauth(
        &state,
        &identity,
        &context,
        &req.reauth_token,
        TargetOperation::RemoveAuthenticator,
    )
    .await?;

    repo.delete(
        &identity.realm_id(),
        user_id,
        &relying_party.id,
        credential_id,
    )
    .await
    .map_err(map_repository_error)?;

    // Audit passkey credential deletion (PRD §4.1 audit rule).
    if let Err(audit_err) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: identity.realm_id(),
            category: AuditCategory::Auth,
            action: AuditAction::PasskeyDelete,
            actor_id: user_id.to_string(),
            actor_type: Some(ActorType::User),
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::User,
            target_id: credential_id.to_string(),
            target_name: None,
            result: AuditResult::Success,
            details: None,
            ip_address: Some(ip),
            user_agent: user_agent_from_headers(&headers),
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %audit_err, "Failed to record passkey delete audit event");
    }

    Ok(ApiResult::no_content())
}

impl From<PasskeyCredentialView> for PasskeyCredentialViewResponse {
    fn from(view: PasskeyCredentialView) -> Self {
        Self {
            credential_id: view.id.to_string(),
            nickname: view.nickname,
            created_at: view.created_at.to_rfc3339(),
            last_used_at: view.last_used_at.map(|dt| dt.to_rfc3339()),
            backup_eligible: view.backup_eligible,
            backup_state: view.backup_state,
            transports: view.transports,
            aaguid: view.aaguid.map(|id| id.to_string()),
        }
    }
}

fn identity_user_id(identity: &Identity) -> Result<Uuid, ApiError> {
    Uuid::parse_str(&identity.user_id()).map_err(|e| {
        tracing::error!("Invalid user_id format in identity: {}", e);
        ApiError::internal("Invalid user_id format")
    })
}

async fn rate_limit_passkey_user(state: &AppState, user_id: Uuid) -> Result<(), ApiError> {
    rate_limit_hit(
        state,
        format!("rl:passkey:user:{user_id}"),
        PASSKEY_USER_RATE_LIMIT.0,
        PASSKEY_USER_RATE_LIMIT.1,
    )
    .await
}

async fn store_registration_nickname(
    state: &AppState,
    reg_token: &str,
    nickname: Option<&str>,
) -> Result<(), ApiError> {
    let Some(nickname) = nickname else {
        return Ok(());
    };
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error"))?;
    let _: () = conn
        .set_ex(
            registration_nickname_key(reg_token),
            nickname,
            PASSKEY_CHALLENGE_TTL_SECONDS,
        )
        .await
        .map_err(|e| {
            tracing::error!("Failed to store passkey registration nickname: {}", e);
            ApiError::internal("Internal server error")
        })?;

    Ok(())
}

async fn load_registration_nickname(
    state: &AppState,
    reg_token: &str,
) -> Result<Option<String>, ApiError> {
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error"))?;
    let key = registration_nickname_key(reg_token);
    let nickname: Option<String> = conn.get(&key).await.map_err(|e| {
        tracing::error!("Failed to load passkey registration nickname: {}", e);
        ApiError::internal("Internal server error")
    })?;
    let _: () = conn.del(&key).await.map_err(|e| {
        tracing::error!("Failed to delete passkey registration nickname: {}", e);
        ApiError::internal("Internal server error")
    })?;

    Ok(nickname)
}

fn registration_nickname_key(reg_token: &str) -> String {
    format!("passkey:reg:nickname:{reg_token}")
}

fn passkey_service(
    state: &AppState,
    repo: Arc<PostgresUserPasskeyRepository>,
) -> Result<
    UserPasskeyService<
        PostgresUserPasskeyRepository,
        RedisPasskeyChallengeStore,
        PostgresPasskeyRealmConfigReader,
    >,
    ApiError,
> {
    let challenge_store = Arc::new(RedisPasskeyChallengeStore::new(state.redis_manager.clone()));
    let config_reader = Arc::new(PostgresPasskeyRealmConfigReader::new(state.pool.clone()));

    UserPasskeyService::new(repo, challenge_store, config_reader).map_err(map_passkey_error)
}

fn map_registration_begin_error(err: PasskeyError) -> ApiError {
    match err {
        PasskeyError::Disabled => ApiError::not_found("Passkey is not enabled for this realm"),
        other => map_passkey_error(other),
    }
}

fn map_registration_finish_error(err: PasskeyError) -> ApiError {
    match err {
        PasskeyError::ChallengeExpired => {
            ApiError::unauthorized("Invalid or expired registration token")
        }
        PasskeyError::VerificationFailed | PasskeyError::Unsupported => {
            ApiError::unprocessable_entity("Passkey verification failed")
        }
        PasskeyError::OwnerMismatch => {
            ApiError::forbidden("passkey credential does not belong to user in realm")
        }
        PasskeyError::Repo(CoreError::Conflict(_)) => {
            ApiError::conflict("Passkey credential already exists")
        }
        PasskeyError::Repo(CoreError::DatabaseError(msg))
            if msg.to_ascii_lowercase().contains("unique") =>
        {
            ApiError::conflict("Passkey credential already exists")
        }
        other => map_passkey_error(other),
    }
}

fn map_passkey_error(err: PasskeyError) -> ApiError {
    match err {
        PasskeyError::Disabled => ApiError::not_found("Passkey is not enabled for this realm"),
        PasskeyError::VerificationFailed => ApiError::unauthorized("Passkey verification failed"),
        PasskeyError::NotFound => ApiError::not_found("Passkey credential not found"),
        PasskeyError::ChallengeExpired => ApiError::unauthorized("Challenge expired"),
        PasskeyError::Unsupported => ApiError::unprocessable_entity("Passkey is unsupported"),
        PasskeyError::OwnerMismatch => ApiError::forbidden("Challenge does not belong to user"),
        PasskeyError::Repo(err) => map_repository_error(err),
    }
}

fn map_repository_error(err: CoreError) -> ApiError {
    match err {
        CoreError::Forbidden(msg) => ApiError::forbidden(msg),
        CoreError::NotFound => ApiError::not_found("Passkey credential not found"),
        other => ApiError::from(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use herald_core::domain::authentication::CredentialClass;
    use herald_core::domain::user::entities::{User, UserStatus};
    use std::collections::HashSet;

    #[test]
    fn self_service_passkey_scope_rejects_custom_ui_token_without_grant() {
        let user_id = Uuid::now_v7();
        let identity = Identity::User(User {
            id: user_id,
            realm_id: "realm".to_string(),
            email: "user@example.com".to_string(),
            nickname: None,
            password_hash: None,
            provider_ids: vec![],
            status: UserStatus::Normal,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        let context = TokenCredentialContext {
            client_app_id: Uuid::now_v7(),
            client_id: "custom-user-ui".to_string(),
            family_id: Uuid::now_v7(),
            credential_class: CredentialClass::CustomUserUi,
            allowed_scopes: HashSet::new(),
        };

        assert!(require_token_scope(&identity, &context, CredentialScope::PasskeyManage).is_err());
    }
}
