//! Webhook Event Repository
//!
//! Provides specialized repository for webhook event handling with idempotency
//! and transaction management. This repository encapsulates the common pattern
//! of storing payment events, checking idempotency, and marking events as processed.

use chrono::{DateTime, Duration, Utc};
use sqlx::{PgPool, Row, Transaction};
use uuid::Uuid;

use herald_domain::billing::PaymentEvent;
use herald_domain::common::entities::app_errors::CoreError;

/// Result of processing a webhook event with idempotency check
pub enum IdempotencyResult {
    /// Event is claimed by this request and should be processed
    Claimed { event_id: Uuid },
    /// Event was already processed, should be skipped
    AlreadyProcessed { event_id: Uuid },
    /// Event is currently being processed by another request
    InProgress { event_id: Uuid },
}

/// Repository for webhook event handling
///
/// This repository provides high-level methods for webhook processing,
/// encapsulating idempotency checks and transaction management.
pub struct WebhookEventRepository {
    pool: PgPool,
}

impl WebhookEventRepository {
    const PROCESSING_LEASE_TIMEOUT: Duration = Duration::minutes(5);

    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Begin a new database transaction for webhook processing
    pub async fn begin_transaction(
        &self,
    ) -> Result<Transaction<'static, sqlx::Postgres>, CoreError> {
        self.pool
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))
    }

    /// Create a new payment event record with idempotency check
    ///
    /// This method inserts a payment event record and checks if it has already been processed.
    /// Uses a transaction to ensure atomicity and FOR UPDATE to lock the event row.
    ///
    /// # Arguments
    /// * `tx` - The database transaction to use
    /// * `realm_id` - The realm ID
    /// * `external_event_id` - The external event ID from payment provider
    /// * `payment_provider` - The payment provider (e.g., "stripe", "creem")
    /// * `event_type` - The event type (e.g., "subscription_contracts/create")
    /// * `payload` - The event payload as JSON
    ///
    /// # Returns
    /// * `IdempotencyResult::Claimed` with event_id if the event is new or lease expired
    /// * `IdempotencyResult::AlreadyProcessed` with event_id if already processed
    /// * `IdempotencyResult::InProgress` with event_id if another request holds the lease
    pub async fn create_event_with_idempotency_check(
        &self,
        tx: &mut Transaction<'static, sqlx::Postgres>,
        realm_id: &str,
        external_event_id: &str,
        payment_provider: &str,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<IdempotencyResult, CoreError> {
        // Insert the event with ON CONFLICT to handle duplicates
        let event_id = Uuid::now_v7();
        sqlx::query(
            r#"
            INSERT INTO payment_event
                (id, realm_id, external_event_id, payment_provider, event_type, payload, processed, created_at)
            VALUES
                ($1, $2, $3, $4, $5, $6, false, NOW())
            ON CONFLICT (realm_id, external_event_id, payment_provider) DO NOTHING
            "#,
        )
        .bind(event_id)
        .bind(realm_id)
        .bind(external_event_id)
        .bind(payment_provider)
        .bind(event_type)
        .bind(payload)
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        // Lock and check the event status
        let event_row = sqlx::query(
            r#"
            SELECT id, processed, processing_started_at
            FROM payment_event
            WHERE realm_id = $1
              AND external_event_id = $2
              AND payment_provider = $3
            FOR UPDATE
            "#,
        )
        .bind(realm_id)
        .bind(external_event_id)
        .bind(payment_provider)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        let event_id: Uuid = event_row
            .try_get("id")
            .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?;
        let already_processed: bool = event_row
            .try_get("processed")
            .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?;
        let processing_started_at: Option<DateTime<Utc>> = event_row
            .try_get("processing_started_at")
            .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?;

        if already_processed {
            return Ok(IdempotencyResult::AlreadyProcessed { event_id });
        }

        let lease_expired = processing_started_at
            .map(|started_at| started_at <= Utc::now() - Self::PROCESSING_LEASE_TIMEOUT)
            .unwrap_or(true);

        if !lease_expired {
            return Ok(IdempotencyResult::InProgress { event_id });
        }

        sqlx::query("UPDATE payment_event SET processing_started_at = NOW() WHERE id = $1")
            .bind(event_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(IdempotencyResult::Claimed { event_id })
    }

    /// Mark a payment event as processed
    ///
    /// # Arguments
    /// * `event_id` - The payment event ID to mark as processed
    pub async fn mark_event_processed(&self, event_id: Uuid) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE payment_event
             SET processed = true, processing_started_at = NULL
             WHERE id = $1",
        )
        .bind(event_id)
        .execute(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    /// Release a processing lease after handler failure.
    pub async fn mark_event_failed(&self, event_id: Uuid) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE payment_event
             SET processing_started_at = NULL
             WHERE id = $1",
        )
        .bind(event_id)
        .execute(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    /// Find a payment event by external event ID and provider
    ///
    /// # Arguments
    /// * `external_event_id` - The external event ID from payment provider
    /// * `payment_provider` - The payment provider (e.g., "stripe", "creem")
    ///
    /// # Returns
    /// * `Some(PaymentEvent)` if the event exists
    /// * `None` if the event does not exist
    pub async fn find_event_by_external_id(
        &self,
        external_event_id: &str,
        payment_provider: &str,
    ) -> Result<Option<PaymentEvent>, CoreError> {
        let result = sqlx::query(
            r#"
            SELECT id, realm_id, external_event_id, payment_provider, event_type,
                   subscription_id, payload, processed, processing_started_at, created_at
            FROM payment_event
            WHERE external_event_id = $1
              AND payment_provider = $2
            "#,
        )
        .bind(external_event_id)
        .bind(payment_provider)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        match result {
            Some(row) => {
                let payload: serde_json::Value = row
                    .try_get("payload")
                    .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?;
                let created_at: DateTime<Utc> = row
                    .try_get("created_at")
                    .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?;

                Ok(Some(PaymentEvent {
                    id: row
                        .try_get("id")
                        .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?,
                    realm_id: row
                        .try_get("realm_id")
                        .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?,
                    external_event_id: row
                        .try_get("external_event_id")
                        .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?,
                    payment_provider: row
                        .try_get("payment_provider")
                        .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?,
                    event_type: row
                        .try_get("event_type")
                        .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?,
                    subscription_id: row
                        .try_get("subscription_id")
                        .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?,
                    payload,
                    processed: row
                        .try_get("processed")
                        .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?,
                    processing_started_at: row
                        .try_get("processing_started_at")
                        .map_err(|e: sqlx::Error| CoreError::DatabaseError(e.to_string()))?,
                    created_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// Check if an event has been processed
    ///
    /// # Arguments
    /// * `external_event_id` - The external event ID from payment provider
    /// * `payment_provider` - The payment provider (e.g., "stripe", "creem")
    ///
    /// # Returns
    /// * `true` if the event has been processed
    /// * `false` if the event has not been processed or does not exist
    pub async fn is_event_processed(
        &self,
        external_event_id: &str,
        payment_provider: &str,
    ) -> Result<bool, CoreError> {
        let result = sqlx::query(
            r#"
            SELECT processed
            FROM payment_event
            WHERE external_event_id = $1
              AND payment_provider = $2
            "#,
        )
        .bind(external_event_id)
        .bind(payment_provider)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(result
            .map(|row| row.try_get::<bool, _>("processed").unwrap_or(false))
            .unwrap_or(false))
    }
}
