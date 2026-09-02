// OAuth helper functions for login and callback handlers

use herald_api_base::application::http::auth::error::AuthError;
use herald_api_base::application::http::auth::util::is_registration_enabled;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::oauth::{
    ports::{OAuthProviderHandler, OAuthRepository},
    value_objects::{OAuthConfig, OAuthUserInfo},
};
use herald_core::domain::security_constants::{
    DEFAULT_JWT_EXPIRATION_SECONDS, OAUTH_STATE_TTL_SECONDS, OAUTH_STATE_VALIDATION_TIMEOUT_SECONDS,
};
use herald_core::domain::user::{UserRepository, UserService};
use herald_core::infrastructure::oauth::providers::{
    apple::AppleOAuthProvider, facebook::FacebookOAuthProvider, github::GitHubOAuthProvider,
    google::GoogleOAuthProvider, wechat::WeChatOAuthProvider,
    wechat_miniprogram::WeChatMiniProgramProvider,
};
use herald_core::infrastructure::redis::RedisConnectionManager;
use jsonwebtoken::{EncodingKey, Header, encode};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

/// OAuth state data stored in Redis
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OAuthStateData {
    realm_id: String,
    client_id: String,
    provider_type: String,
    redirect_uri: Option<String>,
    #[serde(default)]
    downstream_state: Option<String>,
    created_at: i64,
}

#[derive(Debug, Deserialize)]
struct DownstreamAuthorizationState {
    client_id: String,
    realm_id: String,
    redirect_uri: String,
    code_challenge: String,
}

pub struct OAuthCallbackResult {
    pub user_id: Uuid,
    pub client_id: String,
    pub downstream_redirect_uri: Option<String>,
}

async fn realm_public_origin_for_oauth(
    state: &AppState,
    realm_id: &str,
) -> Result<String, AuthError> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT hostname FROM custom_domain_mapping
         WHERE realm_id = $1 AND enabled = true
         ORDER BY updated_at DESC
         LIMIT 1",
    )
    .bind(realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(
            realm_id = %realm_id,
            error = %e,
            "Failed to query custom-domain mapping for OAuth URL"
        );
        AuthError::InternalServerError("Failed to build public realm URL".to_string())
    })?;

    if let Some((hostname,)) = row {
        Ok(format!("https://{hostname}"))
    } else {
        Ok(state.public_base_url.trim_end_matches('/').to_string())
    }
}

/// Provider handler enum
pub enum ProviderHandler {
    Google(GoogleOAuthProvider),
    GitHub(GitHubOAuthProvider),
    Facebook(FacebookOAuthProvider),
    Apple(AppleOAuthProvider),
    WeChat(WeChatOAuthProvider),
    WeChatMiniProgram(WeChatMiniProgramProvider),
}

impl ProviderHandler {
    fn from_str(s: &str) -> Result<Self, AuthError> {
        match s {
            "google" => Ok(ProviderHandler::Google(GoogleOAuthProvider)),
            "github" => Ok(ProviderHandler::GitHub(GitHubOAuthProvider)),
            "facebook" => Ok(ProviderHandler::Facebook(FacebookOAuthProvider)),
            "apple" => Ok(ProviderHandler::Apple(AppleOAuthProvider)),
            "wechat" => Ok(ProviderHandler::WeChat(WeChatOAuthProvider)),
            "wechat_miniprogram" => Ok(ProviderHandler::WeChatMiniProgram(
                WeChatMiniProgramProvider,
            )),
            _ => Err(AuthError::BadRequest(format!(
                "Unsupported provider: {}",
                s
            ))),
        }
    }

    /// Get authorization URL for this provider
    fn get_auth_url(&self, state_token: &str, config: &OAuthConfig) -> Result<String, AuthError> {
        match self {
            ProviderHandler::Google(p) => p.get_auth_url(state_token, config).map_err(|e| {
                AuthError::InternalServerError(format!("Failed to generate auth URL: {}", e))
            }),
            ProviderHandler::GitHub(p) => p.get_auth_url(state_token, config).map_err(|e| {
                AuthError::InternalServerError(format!("Failed to generate auth URL: {}", e))
            }),
            ProviderHandler::Facebook(p) => p.get_auth_url(state_token, config).map_err(|e| {
                AuthError::InternalServerError(format!("Failed to generate auth URL: {}", e))
            }),
            ProviderHandler::Apple(p) => p.get_auth_url(state_token, config).map_err(|e| {
                AuthError::InternalServerError(format!("Failed to generate auth URL: {}", e))
            }),
            ProviderHandler::WeChat(p) => p.get_auth_url(state_token, config).map_err(|e| {
                AuthError::InternalServerError(format!("Failed to generate auth URL: {}", e))
            }),
            ProviderHandler::WeChatMiniProgram(_) => Err(AuthError::BadRequest(
                "WeChat Mini Program does not use authorization URL".to_string(),
            )),
        }
    }
}

/// JWT claims for session
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JwtClaims {
    sub: String, // user_id
    realm_id: String,
    exp: i64,
    iat: i64,
}

/// Generate OAuth authorization URL
///
/// # Arguments
/// * `state` - AppState containing database and Redis connections
/// * `realm_id` - Realm ID
/// * `provider_type` - OAuth provider type (google, github, facebook, apple)
/// * `redirect_uri` - Optional redirect URI after login
///
/// # Returns
/// * (auth_url, state_token) - Authorization URL and state token
pub async fn generate_oauth_auth_url(
    state: &AppState,
    realm_id: String,
    provider_type: String,
    client_id: String,
    redirect_uri: Option<String>,
    downstream_state: Option<String>,
) -> Result<(String, String), AuthError> {
    // Get provider config from database
    let config = state
        .service
        .oauth_config_service()
        .list_enabled_providers(&realm_id)
        .await
        .map_err(|e| {
            AuthError::InternalServerError(format!("Failed to list OAuth providers: {}", e))
        })?
        .into_iter()
        .find(|c| c.provider_type.as_str() == provider_type)
        .ok_or_else(|| {
            AuthError::NotFound("OAuth Provider not configured or disabled".to_string())
        })?;

    if let Some(ref downstream_state) = downstream_state {
        validate_downstream_state_reference(state, &realm_id, downstream_state).await?;
    }

    // Generate state token (UUID v7)
    let state_token = Uuid::now_v7().to_string();

    // Store state data in Redis (5 minutes TTL)
    let state_data = OAuthStateData {
        realm_id: realm_id.clone(),
        client_id,
        provider_type: provider_type.clone(),
        redirect_uri: redirect_uri.clone(),
        downstream_state,
        created_at: chrono::Utc::now().timestamp(),
    };

    let state_json = serde_json::to_string(&state_data)
        .map_err(|e| AuthError::InternalServerError(format!("Failed to serialize state: {}", e)))?;

    let mut redis_conn: redis::aio::ConnectionManager =
        state.redis_manager.get().await.map_err(|e| {
            AuthError::InternalServerError(format!("Redis connection error: {}", e))
        })?;

    redis_conn
        .set_ex::<String, String, ()>(
            format!("oauth:state:{}", state_token),
            state_json,
            OAUTH_STATE_TTL_SECONDS,
        )
        .await
        .map_err(|e| AuthError::InternalServerError(format!("Failed to store state: {}", e)))?;

    // Build OAuth config for provider handler
    let redirect_uri_value = match redirect_uri {
        Some(uri) => uri,
        None => {
            let public_origin = realm_public_origin_for_oauth(state, &realm_id).await?;
            format!(
                "{}/api/oauth/{}/{}/callback",
                public_origin, realm_id, provider_type
            )
        }
    };

    let oauth_config = OAuthConfig {
        client_id: config.client_id.clone(),
        client_secret: config.client_secret.clone(),
        redirect_uri: redirect_uri_value,
        scopes: config.scopes.clone(),
    };

    // Get provider handler
    let provider = ProviderHandler::from_str(&provider_type)?;

    // Generate authorization URL
    let auth_url = provider.get_auth_url(&state_token, &oauth_config)?;

    Ok((auth_url, state_token))
}

async fn validate_downstream_state_reference(
    state: &AppState,
    realm_id: &str,
    downstream_state: &str,
) -> Result<(), AuthError> {
    let mut redis_conn: redis::aio::ConnectionManager = state
        .redis_manager
        .get()
        .await
        .map_err(|e| AuthError::InternalServerError(format!("Redis connection error: {e}")))?;
    let state_json: Option<String> = redis_conn
        .get(format!("oauth:state:{downstream_state}"))
        .await
        .map_err(|e| {
            AuthError::InternalServerError(format!("Failed to read downstream state: {e}"))
        })?;
    let state_json = state_json
        .ok_or_else(|| AuthError::BadRequest("Invalid or expired downstream state".to_string()))?;
    let downstream: DownstreamAuthorizationState = serde_json::from_str(&state_json)
        .map_err(|_| AuthError::BadRequest("Invalid downstream authorization state".to_string()))?;

    if downstream.realm_id != realm_id {
        return Err(AuthError::BadRequest(
            "Downstream state realm mismatch".to_string(),
        ));
    }
    if downstream.client_id.is_empty()
        || downstream.redirect_uri.is_empty()
        || downstream.code_challenge.is_empty()
    {
        return Err(AuthError::BadRequest(
            "Incomplete downstream authorization state".to_string(),
        ));
    }

    Ok(())
}

/// Validate state token from Redis
///
/// # Arguments
/// * `redis_manager` - Redis connection manager
/// * `state_token` - State token to validate
///
/// # Returns
/// * OAuthStateData if valid
async fn validate_state_token(
    redis_manager: &RedisConnectionManager,
    state_token: &str,
) -> Result<OAuthStateData, AuthError> {
    let mut redis_conn: redis::aio::ConnectionManager = redis_manager
        .get()
        .await
        .map_err(|e| AuthError::InternalServerError(format!("Redis connection error: {}", e)))?;

    // Get and delete state token (one-time use)
    let state_json: Option<String> = redis_conn
        .get_del(format!("oauth:state:{}", state_token))
        .await
        .map_err(|e| {
            if e.to_string().contains("not found") || e.to_string().contains("nil") {
                AuthError::BadRequest("Invalid or expired state token".to_string())
            } else {
                AuthError::InternalServerError(format!("Failed to validate state: {}", e))
            }
        })?;

    let state_json = state_json
        .ok_or_else(|| AuthError::BadRequest("Invalid or expired state token".to_string()))?;

    let state_data: OAuthStateData = serde_json::from_str(&state_json).map_err(|e| {
        AuthError::InternalServerError(format!("Failed to parse state data: {}", e))
    })?;

    // Check expiration (5 minutes)
    let now = chrono::Utc::now().timestamp();
    if now - state_data.created_at > OAUTH_STATE_VALIDATION_TIMEOUT_SECONDS {
        return Err(AuthError::BadRequest("State token expired".to_string()));
    }

    Ok(state_data)
}

/// Exchange authorization code for user info
///
/// # Arguments
/// * `provider_type` - OAuth provider type
/// * `code` - Authorization code from OAuth provider
/// * `config` - OAuth provider config
///
/// # Returns
/// * OAuthUserInfo
pub async fn exchange_code_for_user_info(
    provider_type: &str,
    code: String,
    config: OAuthConfig,
) -> Result<OAuthUserInfo, AuthError> {
    let provider = ProviderHandler::from_str(provider_type)?;

    // Create HTTP client for OAuth providers
    let http_client =
        herald_core::infrastructure::oauth::ReqwestHttpClient::new().map_err(|e| {
            AuthError::InternalServerError(format!("Failed to create HTTP client: {}", e))
        })?;

    // Exchange code for user info using the provider handler
    // Note: The exchange_code_and_get_user method returns impl Future, so we need to use the provider directly
    let user_info = match provider {
        ProviderHandler::Google(p) => p
            .exchange_code_and_get_user(code, &config, &http_client)
            .await
            .map_err(|e| {
                AuthError::InternalServerError(format!("Failed to get user info: {}", e))
            })?,
        ProviderHandler::GitHub(p) => p
            .exchange_code_and_get_user(code, &config, &http_client)
            .await
            .map_err(|e| {
                AuthError::InternalServerError(format!("Failed to get user info: {}", e))
            })?,
        ProviderHandler::Facebook(p) => p
            .exchange_code_and_get_user(code, &config, &http_client)
            .await
            .map_err(|e| {
                AuthError::InternalServerError(format!("Failed to get user info: {}", e))
            })?,
        ProviderHandler::Apple(p) => p
            .exchange_code_and_get_user(code, &config, &http_client)
            .await
            .map_err(|e| {
                AuthError::InternalServerError(format!("Failed to get user info: {}", e))
            })?,
        ProviderHandler::WeChat(p) => p
            .exchange_code_and_get_user(code, &config, &http_client)
            .await
            .map_err(|e| match e {
                herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                    AuthError::Unauthorized(msg)
                }
                _ => AuthError::InternalServerError(format!("Failed to get user info: {}", e)),
            })?,
        ProviderHandler::WeChatMiniProgram(p) => p
            .exchange_code_and_get_user(code, &config, &http_client)
            .await
            .map_err(|e| match e {
                herald_core::domain::common::entities::app_errors::CoreError::BadRequest(msg) => {
                    AuthError::BadRequest(msg)
                }
                _ => AuthError::InternalServerError(format!("Failed to get user info: {}", e)),
            })?,
    };

    Ok(user_info)
}

/// Find or create user from OAuth user info
///
/// # Arguments
/// * `state` - AppState
/// * `realm_id` - Realm ID
/// * `user_info` - OAuth user info
///
/// # Returns
/// * User ID
pub async fn find_or_create_user(
    state: &AppState,
    realm_id: &str,
    user_info: &OAuthUserInfo,
) -> Result<Uuid, AuthError> {
    // Four-level matching strategy: union_id -> open_id -> email -> create.
    // Google providers set union_id = None, so they effectively traverse
    // open_id -> email -> create.

    // Priority 1: Match by union_id if available (cross-app matching)
    if let Some(union_id) = &user_info.union_id {
        match state
            .service
            .oauth_provider_repository()
            .find_by_union_id(realm_id, union_id)
            .await
        {
            Ok(provider) => {
                tracing::info!("Found user via union_id: {}", union_id);
                if let Some(user_id) = provider.user_id {
                    ensure_oauth_provider_linked(state, realm_id, user_id, user_info).await?;
                    return Ok(user_id);
                }
            }
            Err(herald_core::domain::common::entities::app_errors::CoreError::NotFound) => {
                tracing::debug!("No user found via union_id, trying open_id matching");
            }
            Err(e) => {
                return Err(AuthError::InternalServerError(format!(
                    "Failed to lookup user by union_id: {}",
                    e
                )));
            }
        }
    }

    // Priority 2: Match by open_id (direct provider matching)
    let provider_open_id = user_info
        .open_id
        .as_deref()
        .unwrap_or(&user_info.provider_user_id);

    match state
        .service
        .oauth_provider_repository()
        .find_by_provider_and_open_id(realm_id, user_info.provider_type.as_str(), provider_open_id)
        .await
    {
        Ok(provider) => {
            if let Some(user_id) = provider.user_id {
                return Ok(user_id);
            }
        }
        Err(herald_core::domain::common::entities::app_errors::CoreError::NotFound) => {}
        Err(e) => {
            return Err(AuthError::InternalServerError(format!(
                "Failed to lookup oauth provider by open_id: {}",
                e
            )));
        }
    }

    // Priority 3: Match by email
    let user_id = find_or_create_user_by_email(state, realm_id, user_info).await?;
    ensure_oauth_provider_linked(state, realm_id, user_id, user_info).await?;
    Ok(user_id)
}

async fn ensure_oauth_provider_linked(
    state: &AppState,
    realm_id: &str,
    user_id: Uuid,
    user_info: &OAuthUserInfo,
) -> Result<(), AuthError> {
    use herald_core::domain::oauth::entities::{CreateOAuthProviderConfig, OAuthProvider};

    let provider_open_id = user_info
        .open_id
        .as_deref()
        .unwrap_or(&user_info.provider_user_id);

    match state
        .service
        .oauth_provider_repository()
        .find_by_provider_and_open_id(realm_id, user_info.provider_type.as_str(), provider_open_id)
        .await
    {
        Ok(provider) => {
            if provider.user_id != Some(user_id) {
                state
                    .service
                    .oauth_provider_repository()
                    .link_provider_to_user(user_id, provider.id)
                    .await
                    .map_err(|e| {
                        AuthError::InternalServerError(format!(
                            "Failed to link oauth provider: {}",
                            e
                        ))
                    })?;
            }
            Ok(())
        }
        Err(herald_core::domain::common::entities::app_errors::CoreError::NotFound) => {
            let provider = OAuthProvider::new(CreateOAuthProviderConfig {
                realm_id: realm_id.to_string(),
                provider_type: user_info.provider_type.clone(),
                open_id: provider_open_id.to_string(),
                union_id: user_info.union_id.clone(),
                email: Some(user_info.email.clone()),
                user_id: Some(user_id),
            });

            state
                .service
                .oauth_provider_repository()
                .create_provider(provider)
                .await
                .map(|_| ())
                .map_err(|e| {
                    AuthError::InternalServerError(format!(
                        "Failed to create oauth provider link: {}",
                        e
                    ))
                })
        }
        Err(e) => Err(AuthError::InternalServerError(format!(
            "Failed to lookup oauth provider: {}",
            e
        ))),
    }
}

/// Find or create user by email
///
/// Helper function for email-based user matching
async fn find_or_create_user_by_email(
    state: &AppState,
    realm_id: &str,
    user_info: &OAuthUserInfo,
) -> Result<Uuid, AuthError> {
    use herald_core::domain::user::value_objects::CreateUserRequest;

    // Try to find user by email
    match state
        .user_repository
        .get_user_by_email(realm_id, &user_info.email)
        .await
    {
        Ok(user) => {
            // Email-based linking grants access to an EXISTING account, so it
            // must require a provider-verified email. Some providers hand out
            // addresses the user never confirmed (e.g. GitHub non-primary
            // emails); linking on those would let an attacker take over a
            // password account by registering the victim's email upstream.
            if !user_info.verified {
                tracing::warn!(
                    realm_id = %realm_id,
                    provider = %user_info.provider_type.as_str(),
                    "Blocked OAuth login into existing account: provider email not verified"
                );
                return Err(AuthError::Forbidden(
                    "Provider email is not verified; cannot sign in to an existing account with it"
                        .to_string(),
                ));
            }
            Ok(user.id)
        }
        Err(herald_core::domain::common::entities::app_errors::CoreError::NotFound) => {
            // Account creation is gated by the realm's registration policy.
            // Registration-disabled realms must not auto-provision accounts via
            // OAuth (mirrors the gate in email-otp/register handlers; PRD:
            // 注册政策优先 — auto-register must not bypass realm policy).
            let registration_enabled =
                is_registration_enabled(state, realm_id)
                    .await
                    .map_err(|_| {
                        AuthError::InternalServerError(
                            "Failed to query registration config".to_string(),
                        )
                    })?;
            if !registration_enabled {
                tracing::debug!(
                    realm_id = %realm_id,
                    "OAuth auto-register blocked: registration not enabled for realm"
                );
                return Err(AuthError::Conflict(
                    "Registration is not enabled for this realm".to_string(),
                ));
            }

            // User not found, create new user
            let create_request = CreateUserRequest {
                realm_id: realm_id.to_string(),
                email: user_info.email.clone(),
                password: None, // OAuth users don't have passwords
                provider_ids: None,
            };

            let user = state
                .service
                .user_service()
                .create_user_without_identity_check(create_request)
                .await
                .map_err(|e| match e {
                    herald_core::domain::common::entities::app_errors::CoreError::Conflict(msg) => {
                        AuthError::Conflict(msg)
                    }
                    _ => AuthError::InternalServerError(format!("Failed to create user: {}", e)),
                })?;

            Ok(user.id)
        }
        Err(e) => Err(AuthError::InternalServerError(format!(
            "Failed to lookup user: {}",
            e
        ))),
    }
}

/// Generate JWT session token
///
/// # Arguments
/// * `user_id` - User ID
/// * `realm_id` - Realm ID
/// * `jwt_secret` - JWT secret key
///
/// # Returns
/// * JWT token string
pub fn generate_jwt_token(
    user_id: &str,
    realm_id: &str,
    jwt_secret: &str,
) -> Result<String, AuthError> {
    let now = chrono::Utc::now().timestamp();
    let exp = now + jwt_expiration_seconds()?;

    let claims = JwtClaims {
        sub: user_id.to_string(),
        realm_id: realm_id.to_string(),
        exp,
        iat: now,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_ref()),
    )
    .map_err(|e| AuthError::InternalServerError(format!("Failed to generate JWT: {}", e)))
}

pub fn jwt_secret(state: &AppState) -> Result<&str, AuthError> {
    if state.jwt_secret.is_empty() {
        return Err(AuthError::InternalServerError(
            "JWT secret is not configured".to_string(),
        ));
    }
    Ok(&state.jwt_secret)
}

pub fn jwt_expiration_seconds() -> Result<i64, AuthError> {
    match std::env::var("JWT_EXPIRATION_SECONDS") {
        Ok(value) => value.parse::<i64>().map_err(|_| {
            AuthError::InternalServerError("JWT_EXPIRATION_SECONDS must be an integer".to_string())
        }),
        Err(_) => Ok(DEFAULT_JWT_EXPIRATION_SECONDS),
    }
}

/// Handle OAuth callback
///
/// # Arguments
/// * `state` - AppState
/// * `realm_id` - Realm ID
/// * `provider_type` - OAuth provider type
/// * `code` - Authorization code
/// * `state_token` - State token from query params
///
/// # Returns
/// * (user_id, jwt_token)
#[tracing::instrument(
    // Governance: code is the provider authorization code,
    // state_token is the CSRF state — both secrets. state holds handles;
    // realm_id conservatively skipped. Only the operation type is recorded.
    skip(state, realm_id, code, state_token),
    fields(db.operation = "oauth_callback")
)]
pub async fn handle_oauth_callback(
    state: &AppState,
    realm_id: String,
    provider_type: String,
    code: String,
    state_token: String,
) -> Result<OAuthCallbackResult, AuthError> {
    // Validate state token
    let state_data = validate_state_token(&state.redis_manager, &state_token).await?;

    // Verify realm_id matches
    if state_data.realm_id != realm_id {
        return Err(AuthError::BadRequest("Realm mismatch".to_string()));
    }

    // Verify provider_type matches
    if state_data.provider_type != provider_type {
        return Err(AuthError::BadRequest("Provider mismatch".to_string()));
    }

    // Get provider config from database
    let config = state
        .service
        .oauth_config_service()
        .list_enabled_providers(&realm_id)
        .await
        .map_err(|e| {
            AuthError::InternalServerError(format!("Failed to list OAuth providers: {}", e))
        })?
        .into_iter()
        .find(|c| c.provider_type.as_str() == provider_type)
        .ok_or_else(|| AuthError::NotFound("OAuth provider not configured".to_string()))?;

    // Build OAuth config
    let redirect_uri = match state_data.redirect_uri {
        Some(uri) => uri,
        None => {
            let public_origin = realm_public_origin_for_oauth(state, &realm_id).await?;
            format!(
                "{}/api/oauth/{}/{}/callback",
                public_origin, realm_id, provider_type
            )
        }
    };

    let oauth_config = OAuthConfig {
        client_id: config.client_id.clone(),
        client_secret: config.client_secret.clone(),
        redirect_uri,
        scopes: config.scopes.clone(),
    };

    // Exchange code for user info
    let user_info = exchange_code_for_user_info(&provider_type, code, oauth_config).await?;

    // Find or create user
    let user_id = find_or_create_user(state, &realm_id, &user_info).await?;

    let downstream_redirect_uri = match state_data.downstream_state {
        Some(downstream_state) => Some(
            issue_downstream_authorization_code(state, &realm_id, user_id, &downstream_state)
                .await?,
        ),
        None => None,
    };

    Ok(OAuthCallbackResult {
        user_id,
        client_id: state_data.client_id,
        downstream_redirect_uri,
    })
}

pub async fn issue_downstream_authorization_code(
    state: &AppState,
    realm_id: &str,
    user_id: Uuid,
    downstream_state: &str,
) -> Result<String, AuthError> {
    let mut redis_conn: redis::aio::ConnectionManager = state
        .redis_manager
        .get()
        .await
        .map_err(|e| AuthError::InternalServerError(format!("Redis connection error: {e}")))?;
    let state_json: Option<String> = redis_conn
        .get_del(format!("oauth:state:{downstream_state}"))
        .await
        .map_err(|e| {
            AuthError::InternalServerError(format!("Failed to consume downstream state: {e}"))
        })?;
    let state_json = state_json.ok_or_else(|| {
        AuthError::BadRequest(
            "Downstream state not found or already used; restart authorization".to_string(),
        )
    })?;
    let downstream: DownstreamAuthorizationState = serde_json::from_str(&state_json)
        .map_err(|_| AuthError::BadRequest("Invalid downstream authorization state".to_string()))?;

    if downstream.realm_id != realm_id {
        return Err(AuthError::BadRequest(
            "Downstream state realm mismatch".to_string(),
        ));
    }
    if downstream.client_id.is_empty()
        || downstream.redirect_uri.is_empty()
        || downstream.code_challenge.is_empty()
    {
        return Err(AuthError::BadRequest(
            "Incomplete downstream authorization state".to_string(),
        ));
    }

    let auth_code = format!("ac_{}", Uuid::now_v7());
    let code_value = serde_json::json!({
        "code_challenge": downstream.code_challenge,
        "client_id": downstream.client_id,
        "redirect_uri": downstream.redirect_uri,
        "user_id": user_id.to_string(),
        "realm_id": downstream.realm_id,
    })
    .to_string();
    redis_conn
        .set_ex::<String, String, ()>(
            format!("oauth:code:{auth_code}"),
            code_value,
            OAUTH_STATE_TTL_SECONDS,
        )
        .await
        .map_err(|e| {
            AuthError::InternalServerError(format!("Failed to store authorization code: {e}"))
        })?;

    build_downstream_redirect_uri(&downstream.redirect_uri, &auth_code, downstream_state)
}

fn build_downstream_redirect_uri(
    redirect_uri: &str,
    auth_code: &str,
    downstream_state: &str,
) -> Result<String, AuthError> {
    let mut redirect_url = Url::parse(redirect_uri)
        .map_err(|_| AuthError::BadRequest("Invalid downstream redirect URI".to_string()))?;
    redirect_url
        .query_pairs_mut()
        .append_pair("code", auth_code)
        .append_pair("state", downstream_state);
    Ok(redirect_url.into())
}

#[cfg(test)]
mod downstream_redirect_tests {
    use super::*;

    #[test]
    fn preserves_existing_query_and_encodes_state_when_returning_to_client() {
        // WHY: downstream redirect URIs may already contain application query
        // parameters, and state is opaque client data that must survive exactly.
        let redirect = build_downstream_redirect_uri(
            "https://client.example/callback?source=herald",
            "ac_test",
            "opaque state&value",
        )
        .expect("valid redirect URI");
        let parsed = Url::parse(&redirect).expect("generated redirect URI");
        let pairs: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(pairs.get("source").map(String::as_str), Some("herald"));
        assert_eq!(pairs.get("code").map(String::as_str), Some("ac_test"));
        assert_eq!(
            pairs.get("state").map(String::as_str),
            Some("opaque state&value")
        );
    }

    #[test]
    fn legacy_broker_state_without_downstream_reference_remains_readable() {
        // WHY: broker states created just before deployment must still complete
        // as direct Herald logins after the optional field is introduced.
        let state: OAuthStateData = serde_json::from_str(
            r#"{"realm_id":"realm","client_id":"console","provider_type":"google","redirect_uri":null,"created_at":1}"#,
        )
        .expect("legacy state should deserialize");

        assert_eq!(state.downstream_state, None);
    }
}
