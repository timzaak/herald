use std::sync::Arc;

use herald_core::domain::payment_attempt::services::PaymentAttemptService;
use herald_core::infrastructure::payment_attempt::PostgresPaymentAttemptRepository;

/// Background job that closes expired payment attempts ([US-PA-004]).
///
/// Delegates to `PaymentAttemptService::mark_expired_attempts`; the status
/// guard inside keeps a concurrently-succeeded attempt from being flipped to
/// expired.
pub struct PaymentAttemptExpiryJob {
    service: PaymentAttemptService<PostgresPaymentAttemptRepository>,
}

impl PaymentAttemptExpiryJob {
    pub fn new(repo: Arc<PostgresPaymentAttemptRepository>) -> Self {
        Self {
            service: PaymentAttemptService::new(repo),
        }
    }

    pub async fn run(&self) -> anyhow::Result<usize> {
        let expired = self
            .service
            .mark_expired_attempts(chrono::Utc::now())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to mark expired payment attempts: {}", e))?;
        Ok(expired.len())
    }
}
