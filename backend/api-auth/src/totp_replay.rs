// Shared TOTP one-time-code replay tracking.
//
// The `totp:last_code:{user}` Redis record implements the one-time TOTP
// contract ACROSS ceremonies: a code consumed at login is rejected at
// re-authentication and vice versa, so the key format, value format
// (`"{code}:{epoch}"`) and TTL below are the single definition every
// ceremony must use.

use herald_api_base::application::http::auth::util::epoch_seconds;
use herald_api_base::application::http::server::api_entities::ApiError;
use herald_core::domain::security_constants::TOTP_REPLAY_WINDOW_SECONDS;
use redis::AsyncCommands;

pub(crate) fn last_code_key(user_id: &impl std::fmt::Display) -> String {
    format!("totp:last_code:{user_id}")
}

/// Load the last consumed code record (`"{code}:{epoch}"`), if any.
pub(crate) async fn load_last_code(
    conn: &mut redis::aio::ConnectionManager,
    user_id: &impl std::fmt::Display,
) -> Result<Option<String>, ApiError> {
    let key = last_code_key(user_id);
    conn.get(&key).await.map_err(|e| {
        tracing::error!("Failed to get last TOTP code from Redis: {}", e);
        ApiError::internal("Redis operation error".to_string())
    })
}

/// Record a freshly consumed code under the shared replay window.
pub(crate) async fn record_last_code(
    conn: &mut redis::aio::ConnectionManager,
    user_id: &impl std::fmt::Display,
    code: &str,
) -> Result<(), ApiError> {
    let key = last_code_key(user_id);
    let code_data = format!("{}:{}", code, epoch_seconds());
    let _: () = conn
        .set_ex(&key, code_data, TOTP_REPLAY_WINDOW_SECONDS)
        .await
        .map_err(|e| {
            tracing::error!("Failed to store last TOTP code in Redis: {}", e);
            ApiError::internal("Redis operation error".to_string())
        })?;
    Ok(())
}
