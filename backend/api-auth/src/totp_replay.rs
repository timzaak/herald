// Shared TOTP one-time-code replay tracking.
//
// The `totp:last_code:{user}` Redis record implements the one-time TOTP
// contract ACROSS ceremonies: a code consumed at login is rejected at
// re-authentication and vice versa, so the key format, value format
// (`"{code}:{epoch}"`) and TTL below are the single definition every
// ceremony must use.
//
// Consumption is a single atomic check-and-set (the Lua script below), in the
// spirit of the `reauth_consume` FCALL: the replay decision and the record
// write happen in one Redis call, so two concurrent submissions of one
// still-valid code cannot both be accepted.

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

/// Outcome of atomically consuming a TOTP code.
pub(crate) enum CodeConsume {
    /// This call recorded the code as consumed.
    Consumed,
    /// The exact code is already recorded within the replay window — a
    /// sequential or concurrent replay that must be rejected.
    Replay,
}

static CONSUME_CODE_SCRIPT: std::sync::LazyLock<redis::Script> = std::sync::LazyLock::new(|| {
    redis::Script::new(
        r"
local raw = redis.call('GET', KEYS[1])
if raw then
  local last = raw:match('^(.-):')
  if last == ARGV[1] then return 'REPLAY' end
end
redis.call('SET', KEYS[1], ARGV[1] .. ':' .. ARGV[2], 'EX', tonumber(ARGV[3]))
return 'OK'
",
    )
});

/// Atomically record `code` as consumed for the user.
///
/// The replay check and the record write are one Redis script invocation, so
/// the acceptance decision cannot interleave: of two racing submissions of the
/// same code, exactly one sees the empty (or different-code) record and wins.
pub(crate) async fn consume_last_code(
    conn: &mut redis::aio::ConnectionManager,
    user_id: &impl std::fmt::Display,
    code: &str,
) -> Result<CodeConsume, ApiError> {
    let key = last_code_key(user_id);
    let result: String = CONSUME_CODE_SCRIPT
        .key(&key)
        .arg(code)
        .arg(epoch_seconds())
        .arg(TOTP_REPLAY_WINDOW_SECONDS)
        .invoke_async(conn)
        .await
        .map_err(|e| {
            tracing::error!("Failed to consume TOTP code in Redis: {}", e);
            ApiError::internal("Redis operation error".to_string())
        })?;
    match result.as_str() {
        "OK" => Ok(CodeConsume::Consumed),
        "REPLAY" => Ok(CodeConsume::Replay),
        other => {
            tracing::error!("Unexpected TOTP consume script result: {other}");
            Err(ApiError::internal("Redis operation error".to_string()))
        }
    }
}
