use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, State},
    http::HeaderMap,
};
use chrono::{Duration, Utc};
use herald_api_base::application::http::auth::util::{ClientIp, rate_limit_hit};
use herald_api_base::application::http::common::auth_utils::require_token_scope;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{
    CredentialScope, Identity, ReauthCredential, ReauthFactor, ReauthResult, TargetOperation,
    TokenCredentialContext,
};
use herald_core::domain::security_constants::{
    REAUTH_VERIFY_IP_RATE_LIMIT, REAUTH_VERIFY_USER_RATE_LIMIT,
};
use herald_core::domain::user_passkey::{
    PasskeyLoginState, UserPasskeyRepository, UserPasskeyService,
};
use herald_core::domain::user_totp::{UserTotpRepository, UserTotpService};
use herald_core::infrastructure::authentication::{
    REAUTH_TTL_SECONDS, ReauthConsumeError, RedisReauthStore,
};
use herald_core::infrastructure::user_passkey::{
    PostgresPasskeyRealmConfigReader, PostgresUserPasskeyRepository, RedisPasskeyChallengeStore,
};
use herald_core::infrastructure::user_totp::PostgresUserTotpRepository;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::passkey_rp::resolve_passkey_rp;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/reauth", axum::routing::post(handle_begin_reauth))
        .route("/reauth/verify", axum::routing::post(handle_verify_reauth))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReauthBeginRequest {
    pub target_operation: TargetOperation,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReauthBeginResponse {
    pub available_factors: Vec<ReauthFactor>,
    pub challenge: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReauthVerifyRequest {
    pub target_operation: TargetOperation,
    pub factor: ReauthFactor,
    pub password: Option<String>,
    pub totp_code: Option<String>,
    pub passkey_assertion: Option<PasskeyAssertion>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyAssertion {
    pub challenge_token: String,
    pub assertion: serde_json::Value,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReauthTicket {
    pub reauth_token: String,
    pub expires_in: u64,
}

#[utoipa::path(
    post,
    path = "/api/user/reauth",
    tag = "user",
    request_body = ReauthBeginRequest,
    responses((status = 200, body = ReauthBeginResponse), (status = 401)),
    security(("bearer_auth" = []))
)]
pub async fn handle_begin_reauth(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    headers: HeaderMap,
    Json(request): Json<ReauthBeginRequest>,
) -> Result<ApiResult<ReauthBeginResponse>, ApiError> {
    // The factor inventory (password/TOTP/passkey presence) is profile-level
    // data; custom-UI credentials need ProfileRead before reauth may start.
    require_token_scope(&identity, &context, CredentialScope::ProfileRead)?;
    Ok(ApiResult::ok(
        begin_reauth(
            &state,
            &identity,
            &context,
            request.target_operation,
            &headers,
        )
        .await?,
    ))
}
#[utoipa::path(
    post,
    path = "/api/user/reauth/verify",
    tag = "user",
    request_body = ReauthVerifyRequest,
    responses((status = 200, body = ReauthTicket), (status = 401), (status = 409)),
    security(("bearer_auth" = []))
)]
pub async fn handle_verify_reauth(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    ClientIp(client_ip): ClientIp,
    headers: HeaderMap,
    Json(request): Json<ReauthVerifyRequest>,
) -> Result<ApiResult<ReauthTicket>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::ProfileRead)?;
    let credential = match request.factor {
        ReauthFactor::Password => ReauthCredential::Password(
            request
                .password
                .ok_or_else(|| ApiError::bad_request("password is required"))?,
        ),
        ReauthFactor::Totp => ReauthCredential::Totp(
            request
                .totp_code
                .ok_or_else(|| ApiError::bad_request("totpCode is required"))?,
        ),
        ReauthFactor::Passkey => {
            let assertion = request
                .passkey_assertion
                .ok_or_else(|| ApiError::bad_request("passkeyAssertion is required"))?;
            ReauthCredential::Passkey {
                challenge_token: assertion.challenge_token,
                assertion: assertion.assertion,
            }
        }
    };
    Ok(ApiResult::ok(
        verify_reauth(
            &state,
            &identity,
            &context,
            request.target_operation,
            credential,
            &client_ip,
            &headers,
        )
        .await?,
    ))
}

pub async fn begin_reauth(
    state: &AppState,
    identity: &Identity,
    context: &TokenCredentialContext,
    _target: TargetOperation,
    headers: &HeaderMap,
) -> Result<ReauthBeginResponse, ApiError> {
    let user = identity
        .as_user()
        .ok_or_else(|| ApiError::forbidden("authenticated user token required"))?;
    let totp_repo = PostgresUserTotpRepository::new(state.db.clone());
    let has_totp = totp_repo
        .get_config_by_user_id(user.id)
        .await?
        .is_some_and(|config| config.enabled);

    let passkey_repo = Arc::new(PostgresUserPasskeyRepository::new(state.db.clone()));
    let relying_party =
        resolve_passkey_rp(state, &user.realm_id, headers, Some(context.client_app_id)).await?;
    let has_passkey = !passkey_repo
        .list_by_user_and_rp(&user.realm_id, user.id, &relying_party.id)
        .await?
        .is_empty();
    let factors = available_factors(user.password_hash.is_some(), has_totp, has_passkey);
    let challenge = if has_passkey {
        let service = passkey_service(state, passkey_repo)?;
        let login_state = PasskeyLoginState {
            realm_id: user.realm_id.clone(),
            client_id: context.client_app_id.to_string(),
            client_ip: String::new(),
            oauth_client_id: None,
            redirect_uri: None,
            state: None,
        };
        let (options, challenge_token) = service
            .begin_second_factor(&login_state, user.id, relying_party)
            .await
            .map_err(|_| ApiError::internal("Failed to begin Passkey reauthentication"))?;
        Some(serde_json::json!({
            "challengeToken": challenge_token,
            "options": options,
        }))
    } else {
        None
    };

    Ok(ReauthBeginResponse {
        available_factors: factors,
        challenge,
    })
}

pub async fn verify_reauth(
    state: &AppState,
    identity: &Identity,
    context: &TokenCredentialContext,
    target: TargetOperation,
    credential: ReauthCredential,
    client_ip: &str,
    headers: &HeaderMap,
) -> Result<ReauthTicket, ApiError> {
    let user = identity
        .as_user()
        .ok_or_else(|| ApiError::forbidden("authenticated user token required"))?;
    rate_limit_hit(
        state,
        format!("reauth:verify:user:{}", user.id),
        REAUTH_VERIFY_USER_RATE_LIMIT.0,
        REAUTH_VERIFY_USER_RATE_LIMIT.1,
    )
    .await?;
    rate_limit_hit(
        state,
        format!("reauth:verify:ip:{client_ip}"),
        REAUTH_VERIFY_IP_RATE_LIMIT.0,
        REAUTH_VERIFY_IP_RATE_LIMIT.1,
    )
    .await?;
    match credential {
        ReauthCredential::Password(password) => {
            let hash = user.password_hash.as_ref().ok_or_else(invalid_factor)?;
            if !bcrypt::verify(password, hash)
                .map_err(|_| ApiError::internal("Password verification failed"))?
            {
                return Err(ApiError::unauthorized("Invalid reauthentication factor"));
            }
        }
        ReauthCredential::Totp(code) => {
            let repo = PostgresUserTotpRepository::new(state.db.clone());
            let config = repo
                .get_config_by_user_id(user.id)
                .await?
                .filter(|config| config.enabled)
                .ok_or_else(invalid_factor)?;
            let secret = UserTotpService::decrypt_secret(&config.secret_hash)?;
            if !UserTotpService::verify_totp(&secret, &code)? {
                return Err(ApiError::unauthorized("Invalid reauthentication factor"));
            }
        }
        ReauthCredential::Passkey {
            challenge_token,
            assertion,
        } => {
            let relying_party =
                resolve_passkey_rp(state, &user.realm_id, headers, Some(context.client_app_id))
                    .await?;
            let repo = Arc::new(PostgresUserPasskeyRepository::new(state.db.clone()));
            let credential = passkey_service(state, repo)?
                .finish_second_factor(&challenge_token, &assertion)
                .await
                .map_err(|_| ApiError::unauthorized("Invalid reauthentication factor"))?;
            if credential.user_id != user.id || credential.rp_id != relying_party.id {
                return Err(ApiError::unauthorized("Invalid reauthentication factor"));
            }
        }
    }

    let result = ReauthResult {
        realm_id: user.realm_id.clone(),
        client_app_id: context.client_app_id,
        user_id: user.id.to_string(),
        target_operation: target,
        expires_at: Utc::now() + Duration::seconds(REAUTH_TTL_SECONDS as i64),
        consumed: false,
    };
    let token = RedisReauthStore::new(state.redis_manager.clone())
        .issue(result)
        .await?;
    Ok(ReauthTicket {
        reauth_token: token,
        expires_in: REAUTH_TTL_SECONDS,
    })
}

pub async fn consume_reauth(
    state: &AppState,
    identity: &Identity,
    context: &TokenCredentialContext,
    reauth_token: &str,
    target: TargetOperation,
) -> Result<(), ApiError> {
    let user = identity
        .as_user()
        .ok_or_else(|| ApiError::forbidden("authenticated user token required"))?;
    let result = RedisReauthStore::new(state.redis_manager.clone())
        .consume(
            reauth_token,
            &user.realm_id,
            context.client_app_id,
            &user.id.to_string(),
            target,
        )
        .await?;
    map_consume_result(result)
}

fn map_consume_result(result: Result<(), ReauthConsumeError>) -> Result<(), ApiError> {
    match result {
        Ok(()) => Ok(()),
        Err(ReauthConsumeError::Invalid) => Err(ApiError::unauthorized(
            "Invalid or expired reauthentication token",
        )),
        Err(ReauthConsumeError::Consumed) => Err(ApiError::conflict(
            "Reauthentication token already consumed",
        )),
        Err(ReauthConsumeError::TargetMismatch) => {
            Err(ApiError::conflict("Reauthentication target mismatch"))
        }
    }
}

fn invalid_factor() -> ApiError {
    ApiError::unauthorized("Reauthentication factor is unavailable")
}

fn available_factors(password: bool, totp: bool, passkey: bool) -> Vec<ReauthFactor> {
    [
        (password, ReauthFactor::Password),
        (totp, ReauthFactor::Totp),
        (passkey, ReauthFactor::Passkey),
    ]
    .into_iter()
    .filter_map(|(available, factor)| available.then_some(factor))
    .collect()
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
    UserPasskeyService::new(
        repo,
        Arc::new(RedisPasskeyChallengeStore::new(state.redis_manager.clone())),
        Arc::new(PostgresPasskeyRealmConfigReader::new(state.pool.clone())),
    )
    .map_err(|_| ApiError::internal("Failed to initialize Passkey reauthentication"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reauth_factor_list_only_exposes_bound_factors() {
        assert_eq!(
            available_factors(true, false, true),
            vec![ReauthFactor::Password, ReauthFactor::Passkey]
        );
        assert!(available_factors(false, false, false).is_empty());
    }

    #[test]
    fn self_service_reauth_consume_failures_remain_distinct() {
        use axum::response::IntoResponse;

        assert_eq!(
            map_consume_result(Err(ReauthConsumeError::Invalid))
                .unwrap_err()
                .into_response()
                .status(),
            axum::http::StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            map_consume_result(Err(ReauthConsumeError::Consumed))
                .unwrap_err()
                .into_response()
                .status(),
            axum::http::StatusCode::CONFLICT
        );
        assert_eq!(
            map_consume_result(Err(ReauthConsumeError::TargetMismatch))
                .unwrap_err()
                .into_response()
                .status(),
            axum::http::StatusCode::CONFLICT
        );
    }
}
