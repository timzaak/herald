use std::sync::Arc;

use sqlx::PgPool;
use tracing::{error, info, warn};
use uuid::Uuid;

use herald_core::domain::billing::compensation::WebhookEventProcessor;

/// Result of a payment-event retry sweep.
#[derive(Debug, Default)]
pub struct RetryStats {
    pub scanned: usize,
    pub reprocessed: usize,
    pub failed: usize,
}

/// Sweeps `payment_event WHERE processed = false` and re-runs each missed
/// event through the same `WebhookEventProcessor` used by
/// `WebhookCompensationJob` (design §5.5.1 / tech-research §3.2).
///
/// This is the reliability backstop for the subscription ImmediateCancel role
/// revoke (BE-D05): it guarantees a webhook the API layer failed to process is
/// eventually re-run, so a cancel/expire/refund can never permanently miss its
/// role revoke. Safe to run alongside `WebhookCompensationJob` — both call
/// `reprocess_event`, which is idempotent at the webhook + business layers
/// (design §5.5 three-layer guarantee).
pub struct PaymentEventRetryJob {
    pg_pool: PgPool,
    processor: Arc<dyn WebhookEventProcessor>,
    batch_size: i64,
    backoff_secs: i64,
}

impl PaymentEventRetryJob {
    pub fn new(
        pg_pool: PgPool,
        processor: Arc<dyn WebhookEventProcessor>,
        batch_size: i64,
        backoff_secs: i64,
    ) -> Self {
        Self {
            pg_pool,
            processor,
            batch_size,
            backoff_secs,
        }
    }

    #[tracing::instrument(
        // Governance: root span — no inbound request context.
        // `self` carries the DB pool and the WebhookEventProcessor trait object
        // (which holds AppState handles), so it is skipped. Only the low
        // cardinality job name is recorded.
        skip(self),
        fields(job.name = "payment_event_retry")
    )]
    pub async fn run(&self) -> anyhow::Result<RetryStats> {
        let mut stats = RetryStats::default();

        // NULL next_retry_at = eligible for immediate retry (design §7 risk note
        // for the nullable BE-D01 column). A row with a future next_retry_at is
        // still cooling down after a prior failure.
        //
        // `iap_%` rows are the client receipt-submission dedup rows (event_type
        // `iap_one_time` / `iap_recurring` / `iap_non_renewing`): their payload
        // carries no provider-replayable data (no signedPayload / purchaseToken),
        // so reprocessing them is impossible by construction. Their recovery is
        // user-resubmission-driven — the submit handler's conflict branch re-runs
        // failed/dead rows — and sweeping them would only produce an eternal
        // BadRequest backoff loop.
        let rows: Vec<(Uuid, String, String, String, serde_json::Value)> = sqlx::query_as(
            r#"
            SELECT id, realm_id, payment_provider, event_type, payload
            FROM payment_event
            WHERE processed = false
              AND event_type NOT LIKE 'iap\_%'
              AND (next_retry_at IS NULL OR next_retry_at <= NOW())
            ORDER BY created_at
            LIMIT $1
            "#,
        )
        .bind(self.batch_size)
        .fetch_all(&self.pg_pool)
        .await?;

        stats.scanned = rows.len();

        for (id, realm_id, payment_provider, event_type, payload) in &rows {
            match self
                .processor
                .reprocess_event(realm_id, payment_provider, event_type, payload)
                .await
            {
                Ok(()) => {
                    // Reuse mark_payment_event_processed semantics: flip
                    // processed=true and clear the processing lease so the row
                    // is no longer picked up by the webhook in-flight path.
                    if let Err(e) = sqlx::query(
                        "UPDATE payment_event \
                         SET processed = true, processing_started_at = NULL \
                         WHERE id = $1",
                    )
                    .bind(id)
                    .execute(&self.pg_pool)
                    .await
                    {
                        error!(
                            payment_event_id = %id,
                            error = %e,
                            "Failed to mark payment_event processed after successful retry"
                        );
                        stats.failed += 1;
                        continue;
                    }
                    stats.reprocessed += 1;
                }
                Err(e) => {
                    // Back off: do NOT mark processed. The row stays eligible
                    // for a future sweep once next_retry_at elapses.
                    warn!(
                        payment_event_id = %id,
                        realm_id = %realm_id,
                        payment_provider = %payment_provider,
                        event_type = %event_type,
                        error = %e,
                        backoff_secs = self.backoff_secs,
                        "Reprocessing payment_event failed; scheduling retry"
                    );
                    if let Err(update_err) = sqlx::query(
                        "UPDATE payment_event SET next_retry_at = NOW() + ($1 * INTERVAL '1 second') WHERE id = $2",
                    )
                    .bind(self.backoff_secs)
                    .bind(id)
                    .execute(&self.pg_pool)
                    .await
                    {
                        error!(
                            payment_event_id = %id,
                            error = %update_err,
                            "Failed to set next_retry_at backoff on failed payment_event"
                        );
                    }
                    stats.failed += 1;
                }
            }
        }

        info!(
            scanned = stats.scanned,
            reprocessed = stats.reprocessed,
            failed = stats.failed,
            "Payment event retry sweep completed"
        );

        Ok(stats)
    }
}
