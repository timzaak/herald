//! Rate limiting utilities using Redis
//!
//! Provides thread-safe, atomic rate limiting using Redis Functions.
//! Redis Functions (Redis 7.0+) offer better performance and manageability
//! compared to traditional Lua scripts.
//!
//! Rate limiting can be disabled per-environment or per-request configuration.

use serde::{Deserialize, Serialize};

use crate::application::http::server::api_entities::ApiError;
use crate::application::http::state::AppState;

/// Rate limit configuration
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum number of requests allowed
    pub max_requests: i64,

    /// Time window in seconds
    pub window_secs: usize,

    /// Whether to enforce rate limiting in non-production environments
    /// When false (default), rate limiting is skipped in dev/test
    pub enforce_in_dev: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 10,
            window_secs: 60,
            enforce_in_dev: false,
        }
    }
}

impl RateLimitConfig {
    /// Create a new rate limit configuration
    pub fn new(max_requests: i64, window_secs: usize) -> Self {
        Self {
            max_requests,
            window_secs,
            enforce_in_dev: false,
        }
    }

    /// Enable enforcement in development/test environments
    pub fn with_enforce_in_dev(mut self) -> Self {
        self.enforce_in_dev = true;
        self
    }
}

/// Redis Function library name
const RATE_LIMIT_FUNCTION_LIBRARY: &str = "herald_rate_limit";

/// Redis Function for atomic increment and expiration
///
/// This function ensures that INCR and EXPIRE operations are atomic,
/// preventing race conditions where multiple requests might simultaneously
/// see count == 1 and compete to set expiration.
///
/// Redis Functions (introduced in Redis 7.0) offer several advantages over
/// traditional Lua scripts:
/// - Functions are loaded once and can be called multiple times
/// - Better performance due to persistent function library
/// - Built-in versioning and management via FUNCTION LIST/DELETE
/// - Easier to debug and maintain
const RATE_LIMIT_FUNCTION_CODE: &str = "#!lua name=herald_rate_limit\n\
\n\
local function rate_limit_check(keys, args)\n\
    local key = keys[1]\n\
    local limit = tonumber(args[1])\n\
    local window = tonumber(args[2])\n\
\n\
    local current = redis.call('incr', key)\n\
    if current == 1 then\n\
        redis.call('expire', key, window)\n\
    end\n\
\n\
    if current > limit then\n\
        return {0, current}\n\
    else\n\
        return {1, current}\n\
    end\n\
end\n\
\n\
redis.register_function('rate_limit_check', rate_limit_check)\n\
";

/// Initialize Redis Function library
///
/// This function loads the rate limiting function library into Redis.
/// It should be called during application startup.
///
/// # Returns
/// * `Ok(())` if the function library was loaded successfully
/// * `Err(ApiError)` if loading failed
///
/// # Note
/// This function is idempotent - calling it multiple times is safe.
/// Redis will replace the existing function library if it already exists.
pub async fn init_rate_limit_function(state: &AppState) -> Result<(), ApiError> {
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error"))?;

    // Load the function library using FUNCTION LOAD
    // The 'REPLACE' flag ensures we can update the function if it already exists
    redis::cmd("FUNCTION")
        .arg("LOAD")
        .arg("REPLACE")
        .arg(RATE_LIMIT_FUNCTION_CODE)
        .query_async::<String>(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!("Failed to load rate limit function library: {e}");
            ApiError::internal("Internal server error")
        })?;

    tracing::info!(
        "Redis Function library '{}' loaded successfully",
        RATE_LIMIT_FUNCTION_LIBRARY
    );

    Ok(())
}

/// Check if a rate limit should be enforced
///
/// Returns an error if the rate limit has been exceeded.
/// Rate limiting is always enforced when `app_env == "production"`; in
/// non-production environments it is skipped unless `config.enforce_in_dev`
/// is set to true.
///
/// This function uses Redis Functions (FCALL) for better performance
/// compared to traditional Lua scripts.
///
/// # Arguments
/// * `state` - Application state containing Redis client
/// * `key` - Unique key for rate limiting (e.g., "rl:login:ip:1.2.3.4")
/// * `config` - Rate limit configuration
///
/// # Example
/// ```no_run
/// use herald_api::application::http::rate_limit::{rate_limit, RateLimitConfig};
///
/// let config = RateLimitConfig::new(5, 60); // 5 requests per 60 seconds
/// rate_limit(&state, "rl:myfeature:user:123".to_string(), config).await?;
/// ```
pub async fn rate_limit(
    state: &AppState,
    key: String,
    config: RateLimitConfig,
) -> Result<(), ApiError> {
    // Production always enforces; non-production environments may skip for
    // local testing unless the call site opted in via `enforce_in_dev`.
    if state.app_env != "production" && !config.enforce_in_dev {
        return Ok(());
    }

    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error"))?;

    // Execute Redis Function using FCALL
    // FCALL function_name num_keys key1 [key2 ...] arg1 [arg2 ...]
    let result: (i64, i64) = redis::cmd("FCALL")
        .arg("rate_limit_check")
        .arg(1) // number of keys
        .arg(&key)
        .arg(config.max_requests)
        .arg(config.window_secs)
        .query_async(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!("Failed to execute rate limit function: {e}");
            ApiError::internal("Internal server error")
        })?;

    let (allowed, current_count) = result;

    if allowed == 0 {
        tracing::warn!(
            "Rate limit exceeded for key '{}': {} requests (limit: {})",
            key,
            current_count,
            config.max_requests
        );
        return Err(ApiError::too_many_requests(format!(
            "Rate limit exceeded: {} requests per {} seconds",
            config.max_requests, config.window_secs
        )));
    }

    tracing::debug!(
        "Rate limit check passed for key '{}': {}/{} requests",
        key,
        current_count,
        config.max_requests
    );

    Ok(())
}

/// Check if a rate limit should be enforced (simplified interface)
///
/// This is a simplified version that uses default config.
/// Use `rate_limit` for more control.
///
/// # Arguments
/// * `state` - Application state
/// * `key` - Unique key for rate limiting
/// * `limit` - Maximum number of requests
/// * `window_secs` - Time window in seconds
pub async fn rate_limit_hit(
    state: &AppState,
    key: String,
    limit: i64,
    window_secs: usize,
) -> Result<(), ApiError> {
    rate_limit(state, key, RateLimitConfig::new(limit, window_secs)).await
}

/// Rate limit function that enforces limits even in development/test environments
///
/// This is primarily intended for testing purposes.
///
/// # Arguments
/// * `state` - Application state
/// * `key` - Unique key for rate limiting
/// * `limit` - Maximum number of requests
/// * `window_secs` - Time window in seconds
pub async fn rate_limit_hit_forced(
    state: &AppState,
    key: String,
    limit: i64,
    window_secs: usize,
) -> Result<(), ApiError> {
    rate_limit(
        state,
        key,
        RateLimitConfig::new(limit, window_secs).with_enforce_in_dev(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use herald_core::infrastructure::redis::{ManagerConfig, RedisConnectionManager};
    use redis::AsyncCommands;
    use tokio::sync::OnceCell;

    static SHARED_MANAGER: OnceCell<Option<RedisConnectionManager>> = OnceCell::const_new();

    async fn get_shared_manager() -> Option<&'static RedisConnectionManager> {
        SHARED_MANAGER
            .get_or_init(|| async {
                let config = ManagerConfig::default();
                RedisConnectionManager::new(config).await.ok()
            })
            .await
            .as_ref()
    }

    /// Load the rate limit function library into Redis for tests.
    async fn load_function_library(
        conn: &mut redis::aio::ConnectionManager,
    ) -> Result<String, redis::RedisError> {
        redis::cmd("FUNCTION")
            .arg("LOAD")
            .arg("REPLACE")
            .arg(RATE_LIMIT_FUNCTION_CODE)
            .query_async::<String>(conn)
            .await
    }

    /// Call the rate_limit_check Redis function directly.
    async fn fcall_rate_limit_check(
        conn: &mut redis::aio::ConnectionManager,
        key: &str,
        limit: i64,
        window_secs: i64,
    ) -> Result<(i64, i64), redis::RedisError> {
        redis::cmd("FCALL")
            .arg("rate_limit_check")
            .arg(1) // num_keys
            .arg(key)
            .arg(limit)
            .arg(window_secs)
            .query_async::<(i64, i64)>(conn)
            .await
    }

    // ========================================================================
    // Redis-dependent tests (skip gracefully if unavailable)
    // ========================================================================
    // Redis-dependent tests (skip gracefully if unavailable)
    // ========================================================================

    #[tokio::test]
    async fn test_function_library_load_is_idempotent() {
        let manager = match get_shared_manager().await {
            Some(m) => m,
            None => {
                println!("Redis not available, skipping test");
                return;
            }
        };
        let mut conn = manager.get().await.unwrap();

        let first = load_function_library(&mut conn).await;
        let second = load_function_library(&mut conn).await;

        assert!(first.is_ok(), "First load failed: {first:?}");
        assert!(
            second.is_ok(),
            "Second load (idempotent) failed: {second:?}"
        );
    }

    #[tokio::test]
    async fn test_basic_rate_limiting() {
        let manager = match get_shared_manager().await {
            Some(m) => m,
            None => {
                println!("Redis not available, skipping test");
                return;
            }
        };
        let mut conn = manager.get().await.unwrap();

        // Ensure function library is loaded
        load_function_library(&mut conn).await.unwrap();

        let key = format!("test:rl:basic:{}", module_path!());

        // Clean up in case of prior failed run
        let _: () = conn.del(&key).await.unwrap_or(());

        // limit=3, window=60s
        let (allowed, count) = fcall_rate_limit_check(&mut conn, &key, 3, 60)
            .await
            .unwrap();
        assert_eq!(allowed, 1);
        assert_eq!(count, 1);

        let (allowed, count) = fcall_rate_limit_check(&mut conn, &key, 3, 60)
            .await
            .unwrap();
        assert_eq!(allowed, 1);
        assert_eq!(count, 2);

        let (allowed, count) = fcall_rate_limit_check(&mut conn, &key, 3, 60)
            .await
            .unwrap();
        assert_eq!(allowed, 1);
        assert_eq!(count, 3);

        // 4th call should exceed limit
        let (allowed, count) = fcall_rate_limit_check(&mut conn, &key, 3, 60)
            .await
            .unwrap();
        assert_eq!(allowed, 0);
        assert_eq!(count, 4);

        // 5th call continues exceeding
        let (allowed, count) = fcall_rate_limit_check(&mut conn, &key, 3, 60)
            .await
            .unwrap();
        assert_eq!(allowed, 0);
        assert_eq!(count, 5);

        // Cleanup
        let _: () = conn.del(&key).await.unwrap();
    }

    #[tokio::test]
    async fn test_different_keys_are_independent() {
        let manager = match get_shared_manager().await {
            Some(m) => m,
            None => {
                println!("Redis not available, skipping test");
                return;
            }
        };
        let mut conn = manager.get().await.unwrap();

        load_function_library(&mut conn).await.unwrap();

        let key_a = format!("test:rl:indep_a:{}", module_path!());
        let key_b = format!("test:rl:indep_b:{}", module_path!());

        // Clean up in case of prior failed run
        let _: () = conn.del(&key_a).await.unwrap_or(());
        let _: () = conn.del(&key_b).await.unwrap_or(());

        // Exhaust key_a (limit=2)
        let (a1, _) = fcall_rate_limit_check(&mut conn, &key_a, 2, 60)
            .await
            .unwrap();
        assert_eq!(a1, 1);
        let (a2, _) = fcall_rate_limit_check(&mut conn, &key_a, 2, 60)
            .await
            .unwrap();
        assert_eq!(a2, 1);
        let (a3, _) = fcall_rate_limit_check(&mut conn, &key_a, 2, 60)
            .await
            .unwrap();
        assert_eq!(a3, 0); // exceeded

        // key_b should still be allowed (independent counter)
        let (b1, count_b) = fcall_rate_limit_check(&mut conn, &key_b, 2, 60)
            .await
            .unwrap();
        assert_eq!(b1, 1);
        assert_eq!(count_b, 1);

        // Cleanup
        let _: () = conn.del(&key_a).await.unwrap_or(());
        let _: () = conn.del(&key_b).await.unwrap_or(());
    }

    #[tokio::test]
    async fn test_rate_limit_expire_set_on_first_increment() {
        let manager = match get_shared_manager().await {
            Some(m) => m,
            None => {
                println!("Redis not available, skipping test");
                return;
            }
        };
        let mut conn = manager.get().await.unwrap();

        load_function_library(&mut conn).await.unwrap();

        let key = format!("test:rl:expire:{}", module_path!());
        let _: () = conn.del(&key).await.unwrap_or(());

        // First call should set TTL
        let (allowed, _) = fcall_rate_limit_check(&mut conn, &key, 5, 120)
            .await
            .unwrap();
        assert_eq!(allowed, 1);

        // Verify TTL is set (should be <= 120 seconds, > 0)
        let ttl: i64 = redis::cmd("TTL")
            .arg(&key)
            .query_async(&mut conn)
            .await
            .unwrap();
        assert!(
            ttl > 0 && ttl <= 120,
            "TTL should be between 1 and 120, got {ttl}"
        );

        // Cleanup
        let _: () = conn.del(&key).await.unwrap();
    }
}
