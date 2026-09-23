use axum::{
    Form, Json,
    extract::{Path, State},
};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::auth::util::{ClientIp, rate_limit_hit};
use herald_api_base::application::http::common::public_helper::realm_public_url;
use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::security_constants::{
    DEVICE_AUTHORIZE_IP_RATE_LIMIT, DEVICE_CODE_DEFAULT_INTERVAL_SECONDS, DEVICE_CODE_TTL_SECONDS,
    DEVICE_CODE_USER_CODE_ALPHABET, DEVICE_CODE_USER_CODE_LENGTH,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeviceAuthorizationRequest {
    pub client_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct DeviceAuthorizationResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: i64,
    pub interval: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeviceAuthorizationErrorResponse {
    pub error: String,
    pub error_description: String,
}

fn generate_user_code() -> String {
    let alphabet = DEVICE_CODE_USER_CODE_ALPHABET.as_bytes();
    let len = DEVICE_CODE_USER_CODE_LENGTH;
    // The user_code is a bearer secret for the device grant: it must come from
    // the OS RNG (UUIDv4), never from UUIDv7 whose leading bytes are the
    // millisecond timestamp and therefore predictable.
    let uuid_bytes = uuid::Uuid::new_v4();
    let bytes = uuid_bytes.as_bytes();
    let mut chars = String::with_capacity(len + 1);
    for i in 0..len {
        if i == 4 {
            chars.push('-');
        }
        let byte = bytes[i % bytes.len()] as usize;
        chars.push(alphabet[byte % alphabet.len()] as char);
    }
    chars
}

#[utoipa::path(
    post,
    path = "/api/device/{realmId}/authorize",
    tag = "device",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
    ),
    request_body(content = DeviceAuthorizationRequest, content_type = "application/x-www-form-urlencoded"),
    responses(
        (status = 200, description = "Device authorization created", body = DeviceAuthorizationResponse),
        (status = 400, description = "Bad request", body = DeviceAuthorizationErrorResponse),
        (status = 401, description = "Invalid client", body = DeviceAuthorizationErrorResponse),
        (status = 403, description = "Forbidden", body = DeviceAuthorizationErrorResponse),
    )
)]
pub async fn device_authorize(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Form(payload): Form<DeviceAuthorizationRequest>,
) -> Result<Json<DeviceAuthorizationResponse>, ApiError> {
    if payload.client_id.is_empty() {
        return Err(ApiError::bad_request_json(
            DeviceAuthorizationErrorResponse {
                error: "invalid_request".to_string(),
                error_description: "client_id is required".to_string(),
            },
        ));
    }

    // Per-IP cap: each request writes device_code/user_code state to Redis,
    // so an unauthenticated flood can fill Redis.
    rate_limit_hit(
        &state,
        format!("rl:device-authorize:ip:{ip}"),
        DEVICE_AUTHORIZE_IP_RATE_LIMIT.0,
        DEVICE_AUTHORIZE_IP_RATE_LIMIT.1,
    )
    .await?;

    let client_row = sqlx::query_as::<_, (bool, bool)>(
        "SELECT enabled, device_code_grant_enabled FROM client_app WHERE realm_id = $1 AND client_id = $2",
    )
    .bind(&realm_id)
    .bind(&payload.client_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(
            realm_id = %realm_id,
            client_id = %payload.client_id,
            error = %e,
            "Database query failed: client_app lookup"
        );
        ApiError::internal("Database query failed".to_string())
    })?;

    let Some((enabled, device_code_grant_enabled)) = client_row else {
        return Err(ApiError::with_json(
            axum::http::StatusCode::UNAUTHORIZED,
            DeviceAuthorizationErrorResponse {
                error: "invalid_client".to_string(),
                error_description: format!(
                    "Client app with client_id '{}' not found in realm '{}'",
                    payload.client_id, realm_id
                ),
            },
        ));
    };

    if !enabled {
        return Err(ApiError::with_json(
            axum::http::StatusCode::FORBIDDEN,
            DeviceAuthorizationErrorResponse {
                error: "client_app_disabled".to_string(),
                error_description: "Client app is disabled".to_string(),
            },
        ));
    }

    if !device_code_grant_enabled {
        return Err(ApiError::with_json(
            axum::http::StatusCode::FORBIDDEN,
            DeviceAuthorizationErrorResponse {
                error: "unauthorized_client".to_string(),
                error_description: "Device code grant is not enabled for this client".to_string(),
            },
        ));
    }

    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error".to_string()))?;

    // device_code is the secret the device polls with — use the fully random
    // UUIDv4 rather than the time-ordered v7.
    let device_code = uuid::Uuid::new_v4().to_string();
    let mut user_code = generate_user_code();
    for _ in 0..5 {
        let exists: bool = conn
            .exists(format!("deviceUserCode:{}", user_code))
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Redis EXISTS failed: user code collision check");
                ApiError::internal("Internal server error".to_string())
            })?;
        if !exists {
            break;
        }
        user_code = generate_user_code();
    }

    let user_code_key = format!("deviceUserCode:{}", user_code);
    let exists: bool = conn.exists(&user_code_key).await.map_err(|e| {
        tracing::error!(error = %e, "Redis EXISTS failed: final user code collision check");
        ApiError::internal("Internal server error".to_string())
    })?;
    if exists {
        return Err(ApiError::internal(
            "Failed to allocate unique device user code".to_string(),
        ));
    }

    let now = chrono::Utc::now().timestamp();
    let state_json = serde_json::json!({
        "user_code": user_code,
        "client_id": payload.client_id,
        "realm_id": realm_id,
        "status": "pending",
        "user_id": null,
        "interval": DEVICE_CODE_DEFAULT_INTERVAL_SECONDS,
        "last_poll_at": 0,
        "created_at": now,
    })
    .to_string();

    let _: Vec<()> = redis::pipe()
        .atomic()
        .cmd("SET")
        .arg(format!("device:{}", device_code))
        .arg(&state_json)
        .arg("EX")
        .arg(DEVICE_CODE_TTL_SECONDS)
        .cmd("SET")
        .arg(user_code_key)
        .arg(&device_code)
        .arg("EX")
        .arg(DEVICE_CODE_TTL_SECONDS)
        .query_async(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Redis pipeline failed: device code storage");
            ApiError::internal("Internal server error".to_string())
        })?;

    let verification_uri = realm_public_url(&state, &realm_id, "device").await?;
    let verification_uri_complete =
        realm_public_url(&state, &realm_id, &format!("device/{user_code}")).await?;

    Ok(Json(DeviceAuthorizationResponse {
        device_code,
        user_code,
        verification_uri,
        verification_uri_complete,
        expires_in: DEVICE_CODE_TTL_SECONDS as i64,
        interval: DEVICE_CODE_DEFAULT_INTERVAL_SECONDS,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_user_code_format() {
        let code = generate_user_code();
        // Format: XXXX-XXXX (8 chars + 1 dash)
        assert_eq!(code.len(), DEVICE_CODE_USER_CODE_LENGTH + 1);
        assert_eq!(code.chars().filter(|c| *c == '-').count(), 1);
        assert!(code.contains('-'));
    }

    #[test]
    fn test_generate_user_code_uses_valid_alphabet() {
        let code = generate_user_code();
        let valid_chars: std::collections::HashSet<char> =
            DEVICE_CODE_USER_CODE_ALPHABET.chars().collect();
        for c in code.chars() {
            if c != '-' {
                assert!(
                    valid_chars.contains(&c),
                    "Invalid char '{}' in user code",
                    c
                );
            }
        }
    }

    #[test]
    fn test_generate_user_code_dash_position() {
        let code = generate_user_code();
        // Dash should be at position 4 (between the two groups of 4)
        assert_eq!(code.chars().nth(4), Some('-'));
    }

    #[test]
    fn test_generate_user_code_is_random_not_timestamp_derived() {
        // WHY: the user_code is a bearer secret for the device grant. When it
        // was derived from UUIDv7 bytes, the leading (timestamp) bytes made
        // most of the code predictable. OS-random UUIDv4 codes generated in a
        // tight loop (same millisecond) must still be unique — timestamp-
        // derived codes collapse to identical values here.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(generate_user_code()));
        }
    }
}
