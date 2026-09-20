use redis::AsyncCommands;

use crate::redis::RedisConnectionManager;
use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::user_passkey::ports::PasskeyChallengeStore;

pub struct RedisPasskeyChallengeStore {
    manager: RedisConnectionManager,
}

impl RedisPasskeyChallengeStore {
    pub fn new(manager: RedisConnectionManager) -> Self {
        Self { manager }
    }

    async fn get_connection(&self) -> Result<redis::aio::ConnectionManager, CoreError> {
        self.manager.get().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to get Redis connection");
            CoreError::InternalServerError(format!("Redis connection error: {}", e))
        })
    }
}

impl PasskeyChallengeStore for RedisPasskeyChallengeStore {
    async fn store(&self, token: &str, payload: &[u8], ttl_secs: u64) -> Result<(), CoreError> {
        let mut conn = self.get_connection().await?;

        let _: () = conn.set_ex(token, payload, ttl_secs).await?;
        Ok(())
    }

    async fn load(&self, token: &str) -> Result<Option<Vec<u8>>, CoreError> {
        let mut conn = self.get_connection().await?;

        let payload = conn.get(token).await?;
        Ok(payload)
    }

    async fn consume(&self, token: &str) -> Result<Option<Vec<u8>>, CoreError> {
        let mut conn = self.get_connection().await?;

        // GETDEL is atomic: of concurrent consumers of one challenge token,
        // exactly one receives the payload and the rest receive None.
        let payload: Option<Vec<u8>> = redis::cmd("GETDEL")
            .arg(token)
            .query_async(&mut conn)
            .await?;
        Ok(payload)
    }

    async fn delete(&self, token: &str) -> Result<(), CoreError> {
        let mut conn = self.get_connection().await?;

        let _: () = conn.del(token).await?;
        Ok(())
    }
}
