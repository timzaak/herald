// Redis Idempotency Store - Infrastructure Layer
//
// Redis implementation of the IdempotencyStore trait.
// This provides the concrete storage backend for idempotency keys.

use std::sync::Arc;
use std::time::Duration;

use crate::redis::RedisConnectionManager;
use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::points::entities::{IdempotencyStatus, PointsTransaction};
use herald_domain::points::idempotency_service::IdempotencyStore;

const IDEMPOTENCY_TTL: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours
const LOCK_TTL: Duration = Duration::from_secs(60); // 1 minute lock timeout

// ---------------------------------------------------------------------------
// Redis Function
// ---------------------------------------------------------------------------

/// Redis Function library name
const IDEMPOTENCY_FUNCTION_LIBRARY: &str = "herald_idempotency";

/// Redis Function for atomic lock acquisition.
///
/// Atomically sets the main key (NX + TTL) and the status key in a single
/// FCALL invocation, preventing race conditions where only one of the two
/// keys is written if the process crashes mid-operation.
///
/// KEYS[1] = main cache key
/// KEYS[2] = status key (main_key + ":status")
/// ARGV[1] = request data (value for main key)
/// ARGV[2] = idempotency TTL (seconds, for main key EX)
/// ARGV[3] = lock TTL (seconds, for status key SETEX)
/// ARGV[4] = status value (e.g. "processing")
const IDEMPOTENCY_FUNCTION_CODE: &str = "#!lua name=herald_idempotency\n\
\n\
local function idempotency_acquire_lock(keys, args)\n\
    if redis.call('SET', keys[1], args[1], 'NX', 'EX', args[2]) then\n\
        redis.call('SETEX', keys[2], args[3], args[4])\n\
        return 1\n\
    else\n\
        return 0\n\
    end\n\
end\n\
\n\
local function idempotency_claim_failed_retry(keys, args)\n\
    if redis.call('GET', keys[1]) == args[2] then\n\
        redis.call('SETEX', keys[1], args[1], args[3])\n\
        return 1\n\
    else\n\
        return 0\n\
    end\n\
end\n\
\n\
redis.register_function('idempotency_acquire_lock', idempotency_acquire_lock)\n\
redis.register_function('idempotency_claim_failed_retry', idempotency_claim_failed_retry)\n\
";

/// Load the idempotency Redis Function library.
///
/// Idempotent -- safe to call multiple times (REPLACE semantics).
pub async fn init_idempotency_function(redis: &RedisConnectionManager) -> Result<(), CoreError> {
    let mut conn = redis
        .get()
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to get Redis connection: {}", e)))?;

    redis::cmd("FUNCTION")
        .arg("LOAD")
        .arg("REPLACE")
        .arg(IDEMPOTENCY_FUNCTION_CODE)
        .query_async::<String>(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!("Failed to load idempotency function library: {e}");
            CoreError::DatabaseError(format!(
                "Failed to load idempotency function library: {}",
                e
            ))
        })?;

    tracing::info!(
        "Redis Function library '{}' loaded successfully",
        IDEMPOTENCY_FUNCTION_LIBRARY
    );

    Ok(())
}

/// Redis implementation of IdempotencyStore
pub struct RedisIdempotencyStore {
    redis: Arc<RedisConnectionManager>,
}

impl RedisIdempotencyStore {
    pub fn new(redis: Arc<RedisConnectionManager>) -> Self {
        Self { redis }
    }
}

impl IdempotencyStore for RedisIdempotencyStore {
    fn get_from_cache(
        &self,
        cache_key: &str,
    ) -> impl Future<Output = Option<PointsTransaction>> + Send {
        let redis = self.redis.clone();
        let cache_key = cache_key.to_string();

        async move {
            let mut conn = redis.get().await.ok()?;

            let result: Option<String> = redis::cmd("GET")
                .arg(&cache_key)
                .query_async(&mut conn)
                .await
                .ok()?;

            result.and_then(|data| serde_json::from_str(&data).ok())
        }
    }

    fn get_status_from_cache(
        &self,
        cache_key: &str,
    ) -> impl Future<Output = Option<IdempotencyStatus>> + Send {
        let redis = self.redis.clone();
        let cache_key = cache_key.to_string();

        async move {
            let mut conn = redis.get().await.ok()?;

            let result: Option<String> = redis::cmd("GET")
                .arg(format!("{}:status", cache_key))
                .query_async(&mut conn)
                .await
                .ok()?;

            result.and_then(|status| status.parse().ok())
        }
    }

    fn try_create_lock(
        &self,
        cache_key: &str,
        request_data: &str,
    ) -> impl Future<Output = Result<bool, CoreError>> + Send {
        let redis = self.redis.clone();
        let cache_key = cache_key.to_string();
        let request_data = request_data.to_string();

        async move {
            let mut conn = redis.get().await.map_err(|e| {
                CoreError::DatabaseError(format!("Failed to get Redis connection: {}", e))
            })?;

            // Atomically SET main key (NX+EX) and status key via Redis Function.
            // If the process crashes between the two keys, no inconsistent state
            // is left -- either both keys are written or neither.
            let status_key = format!("{}:status", cache_key);
            let status = IdempotencyStatus::Processing.as_str();
            let result: i32 = redis::cmd("FCALL")
                .arg("idempotency_acquire_lock")
                .arg(2) // number of keys
                .arg(&cache_key)
                .arg(&status_key)
                .arg(&request_data)
                .arg(IDEMPOTENCY_TTL.as_secs())
                .arg(LOCK_TTL.as_secs())
                .arg(status)
                .query_async(&mut conn)
                .await
                .map_err(|e| CoreError::DatabaseError(format!("Failed to create lock: {}", e)))?;

            let result = result == 1;

            Ok(result)
        }
    }

    fn claim_failed_for_retry(
        &self,
        cache_key: &str,
    ) -> impl Future<Output = Result<bool, CoreError>> + Send {
        let redis = self.redis.clone();
        let cache_key = cache_key.to_string();

        async move {
            let mut conn = redis.get().await.map_err(|e| {
                CoreError::DatabaseError(format!("Failed to get Redis connection: {}", e))
            })?;

            // Atomic failed→processing compare-and-set via the same Redis
            // Function library as the initial lock: the GET and the SETEX are
            // one FCALL, so of two concurrent retries only the one that still
            // observes "failed" flips the marker and proceeds.
            let status_key = format!("{}:status", cache_key);
            let result: i32 = redis::cmd("FCALL")
                .arg("idempotency_claim_failed_retry")
                .arg(1) // number of keys
                .arg(&status_key)
                .arg(LOCK_TTL.as_secs())
                .arg(IdempotencyStatus::Failed.as_str())
                .arg(IdempotencyStatus::Processing.as_str())
                .query_async(&mut conn)
                .await
                .map_err(|e| {
                    CoreError::DatabaseError(format!("Failed to claim failed retry: {}", e))
                })?;

            Ok(result == 1)
        }
    }

    fn save_to_cache(
        &self,
        cache_key: &str,
        transaction: &PointsTransaction,
    ) -> impl Future<Output = Result<(), CoreError>> + Send {
        let redis = self.redis.clone();
        let cache_key = cache_key.to_string();
        let transaction = transaction.clone();

        async move {
            let mut conn = redis.get().await.map_err(|e| {
                CoreError::DatabaseError(format!("Failed to get Redis connection: {}", e))
            })?;

            let data = serde_json::to_string(&transaction).map_err(|e| {
                CoreError::BadRequest(format!("Failed to serialize transaction: {}", e))
            })?;

            // Cache the transaction data
            redis::cmd("SETEX")
                .arg(&cache_key)
                .arg(IDEMPOTENCY_TTL.as_secs())
                .arg(&data)
                .query_async::<()>(&mut conn)
                .await
                .map_err(|e| CoreError::DatabaseError(format!("Failed to save to cache: {}", e)))?;

            // Update status to completed
            let status = IdempotencyStatus::Completed.as_str();
            redis::cmd("SETEX")
                .arg(format!("{}:status", cache_key))
                .arg(IDEMPOTENCY_TTL.as_secs())
                .arg(status)
                .query_async::<()>(&mut conn)
                .await
                .map_err(|e| CoreError::DatabaseError(format!("Failed to update status: {}", e)))?;

            Ok(())
        }
    }

    fn mark_failed(&self, cache_key: &str) -> impl Future<Output = Result<(), CoreError>> + Send {
        let redis = self.redis.clone();
        let cache_key = cache_key.to_string();

        async move {
            let mut conn = redis.get().await.map_err(|e| {
                CoreError::DatabaseError(format!("Failed to get Redis connection: {}", e))
            })?;

            let status = IdempotencyStatus::Failed.as_str();
            let key = format!("{}:status", cache_key);

            // Use same TTL as main key so failure state persists and retries
            // within the 24h window are properly rejected
            redis::cmd("SETEX")
                .arg(key)
                .arg(IDEMPOTENCY_TTL.as_secs())
                .arg(status)
                .query_async::<()>(&mut conn)
                .await
                .map_err(|e| CoreError::DatabaseError(format!("Failed to mark failed: {}", e)))?;

            Ok(())
        }
    }
}
