// PostgreSQL implementation for Payment Attempt repository

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use sqlx::{PgPool, Row};
use std::sync::Arc;

use herald_domain::billing::BillingType;
use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::payment_attempt::{
    CreatePaymentAttemptInput, PaymentAttempt, PaymentAttemptErrorExt, PaymentAttemptRepository,
    PaymentAttemptStatus, PurchaseHistoryRow, RecordRenewalAttemptInput,
};
use herald_entity::payment_attempt as payment_attempt_entity;

/// PostgreSQL implementation of PaymentAttempt repository
pub struct PostgresPaymentAttemptRepository {
    db: Arc<DatabaseConnection>,
    pool: PgPool,
}

impl PostgresPaymentAttemptRepository {
    pub fn new(db: Arc<DatabaseConnection>, pool: PgPool) -> Self {
        Self { db, pool }
    }

    fn model_to_payment_attempt(
        model: payment_attempt_entity::Model,
    ) -> Result<PaymentAttempt, CoreError> {
        Ok(PaymentAttempt {
            id: model.id,
            realm_id: model.realm_id,
            user_id: model.user_id,
            payment_provider: model.payment_provider,
            target_type: model.target_type.parse()?,
            target_id: model.target_id,
            amount: model.amount,
            currency: model.currency,
            status: model.status.parse()?,
            is_one_time_role: model.is_one_time_role,
            provider_reference: model.provider_reference,
            provider_status: model.provider_status,
            metadata: model.metadata,
            expires_at: chrono::DateTime::from(model.expires_at),
            completed_at: model.completed_at.map(chrono::DateTime::from),
            created_at: chrono::DateTime::from(model.created_at),
            updated_at: chrono::DateTime::from(model.updated_at),
        })
    }

    /// The distribution trigger a purchase billing type resolves to at attempt
    /// creation: `OneTime` -> `topup`, `Recurring`/`NonRenewing` ->
    /// `subscription_initial`. This trigger selects which of the mapping's
    /// rules are snapshotted onto the attempt.
    fn trigger_for_billing_type(billing_type: BillingType) -> &'static str {
        match billing_type {
            BillingType::OneTime => "topup",
            BillingType::Recurring | BillingType::NonRenewing => "subscription_initial",
        }
    }

    /// Within an open transaction, insert the attempt row and return its
    /// domain representation. Mirrors the columns the legacy SeaORM ActiveModel
    /// wrote, minus the removed `bucket_id`.
    async fn insert_attempt_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        input: &CreatePaymentAttemptInput,
        status: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
        completed_at: Option<chrono::DateTime<chrono::Utc>>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<PaymentAttempt, CoreError> {
        let id = uuid::Uuid::now_v7();
        let row = sqlx::query(
            "INSERT INTO payment_attempts \
                (id, realm_id, user_id, payment_provider, target_type, target_id, amount, \
                 currency, status, is_one_time_role, provider_reference, provider_status, \
                 metadata, expires_at, completed_at, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) \
             RETURNING id, realm_id, user_id, payment_provider, target_type, target_id, amount, \
                       currency, status, is_one_time_role, provider_reference, provider_status, \
                       metadata, expires_at, completed_at, created_at, updated_at",
        )
        .bind(id)
        .bind(&input.realm_id)
        .bind(input.user_id)
        .bind(&input.payment_provider)
        .bind(&input.target_type)
        .bind(input.target_id)
        .bind(input.amount)
        .bind(&input.currency)
        .bind(status)
        .bind(input.is_one_time_role)
        .bind(input.provider_reference.as_deref())
        .bind(Option::<String>::None)
        .bind(input.metadata.as_ref())
        .bind(expires_at)
        .bind(completed_at)
        .bind(now)
        .bind(now)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to create payment attempt: {e}")))?;

        Self::row_to_payment_attempt(&row)
    }

    /// Build a `PaymentAttempt` from a raw sqlx row carrying all
    /// `payment_attempts` columns.
    fn row_to_payment_attempt(row: &sqlx::postgres::PgRow) -> Result<PaymentAttempt, CoreError> {
        use sqlx::Row;
        let target_type: String = row.get("target_type");
        let status: String = row.get("status");
        Ok(PaymentAttempt {
            id: row.get("id"),
            realm_id: row.get("realm_id"),
            user_id: row.get("user_id"),
            payment_provider: row.get("payment_provider"),
            target_type: target_type.parse()?,
            target_id: row.get("target_id"),
            amount: row.get("amount"),
            currency: row.get("currency"),
            status: status.parse()?,
            is_one_time_role: row.get("is_one_time_role"),
            provider_reference: row.get("provider_reference"),
            provider_status: row.get("provider_status"),
            metadata: row.get("metadata"),
            expires_at: row.get("expires_at"),
            completed_at: row.get("completed_at"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    /// Within an open transaction, snapshot the distribution rules matched at
    /// purchase creation onto `payment_attempt_point_rules`. Selects the
    /// target mapping's enabled rules whose `trigger_sources` contain the
    /// billing-type trigger, in stable `(display_order, rule_id)` order, and
    /// captures each rule's `bucket_id`. Zero matched rules is valid (an empty
    /// snapshot; first fulfillment then completes a zero-result event).
    async fn snapshot_matched_rules_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        payment_attempt_id: uuid::Uuid,
        realm_id: &str,
        mapping_id: uuid::Uuid,
        trigger: &str,
    ) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO payment_attempt_point_rules \
                (payment_attempt_id, \
                 rule_id, bucket_id, created_at) \
             SELECT $1, r.id, r.bucket_id, NOW() \
             FROM points_distribution_rules r \
             WHERE r.realm_id = $2 \
               AND r.entitlement_mapping_id = $3 \
               AND r.enabled = TRUE \
               AND $4 = ANY(r.trigger_sources) \
             ORDER BY r.display_order, r.id",
        )
        .bind(payment_attempt_id)
        .bind(realm_id)
        .bind(mapping_id)
        .bind(trigger)
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to snapshot payment attempt rules: {e}"))
        })?;
        Ok(())
    }
}

impl PaymentAttemptRepository for PostgresPaymentAttemptRepository {
    async fn create_payment_attempt(
        &self,
        input: CreatePaymentAttemptInput,
    ) -> Result<PaymentAttempt, CoreError> {
        let now = chrono::Utc::now();
        let expires_at = now + chrono::Duration::hours(2);
        let trigger = Self::trigger_for_billing_type(input.billing_type.clone());

        // Atomically write the attempt row AND its rule/bucket snapshot in one
        // transaction. The snapshot is the contract for first fulfillment: a
        // rule disabled after this point still fires for this already-initiated
        // purchase.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to begin tx: {e}")))?;

        let attempt =
            Self::insert_attempt_in_tx(&mut tx, &input, "Pending", expires_at, None, now).await?;

        Self::snapshot_matched_rules_in_tx(
            &mut tx,
            attempt.id,
            &input.realm_id,
            input.target_id,
            trigger,
        )
        .await?;

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit payment attempt: {e}"))
        })?;

        Ok(attempt)
    }

    async fn insert_succeeded_renewal_attempt(
        &self,
        input: RecordRenewalAttemptInput,
    ) -> Result<PaymentAttempt, CoreError> {
        // Renewal attempt: already-Succeeded charge, no expiry semantics.
        // expires_at = completed_at (NOT NULL column; already-succeeded has no real expiry).
        // Renewals do NOT snapshot rules: a renewal is a subscription lifecycle
        // event whose fulfillment resolves the mapping's CURRENT enabled rules
        // via the `CurrentOwnerRules` executor selection at renewal time
        // when the renewal event is fulfilled.
        let now = chrono::Utc::now();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to begin tx: {e}")))?;

        let attempt = Self::insert_attempt_in_tx(
            &mut tx,
            &CreatePaymentAttemptInput {
                realm_id: input.realm_id,
                user_id: input.user_id,
                payment_provider: input.payment_provider,
                target_type: "entitlement_mapping".to_string(),
                target_id: input.target_id,
                // Unused for renewal (no snapshot written); Recurring is the
                // only subscription shape that renews.
                billing_type: BillingType::Recurring,
                amount: input.amount,
                currency: input.currency,
                provider_reference: Some(input.provider_reference),
                metadata: None,
                is_one_time_role: false,
            },
            "Succeeded",
            input.completed_at,
            Some(input.completed_at),
            now,
        )
        .await?;

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit renewal attempt: {e}"))
        })?;

        Ok(attempt)
    }

    async fn find_payment_attempt_by_id(
        &self,
        realm_id: &str,
        attempt_id: uuid::Uuid,
    ) -> Result<Option<PaymentAttempt>, CoreError> {
        let result = payment_attempt_entity::Entity::find_by_id(attempt_id)
            .filter(payment_attempt_entity::Column::RealmId.eq(realm_id))
            .one(self.db.as_ref())
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to find payment attempt: {e}"))
            })?;

        match result {
            Some(model) => Self::model_to_payment_attempt(model).map(Some),
            None => Ok(None),
        }
    }

    async fn find_payment_attempt_by_id_only(
        &self,
        attempt_id: uuid::Uuid,
    ) -> Result<Option<PaymentAttempt>, CoreError> {
        let result = payment_attempt_entity::Entity::find_by_id(attempt_id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to find payment attempt: {e}"))
            })?;

        match result {
            Some(model) => Self::model_to_payment_attempt(model).map(Some),
            None => Ok(None),
        }
    }

    async fn find_payment_attempts_by_user(
        &self,
        realm_id: &str,
        user_id: uuid::Uuid,
        limit: u64,
    ) -> Result<Vec<PaymentAttempt>, CoreError> {
        let results = payment_attempt_entity::Entity::find()
            .filter(payment_attempt_entity::Column::RealmId.eq(realm_id))
            .filter(payment_attempt_entity::Column::UserId.eq(user_id))
            .order_by_desc(payment_attempt_entity::Column::CreatedAt)
            .limit(limit)
            .all(self.db.as_ref())
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to find payment attempts by user: {e}"))
            })?;

        results
            .into_iter()
            .map(Self::model_to_payment_attempt)
            .collect()
    }

    async fn find_payment_attempt_by_provider_reference(
        &self,
        provider: &str,
        reference: &str,
    ) -> Result<Option<PaymentAttempt>, CoreError> {
        let result = payment_attempt_entity::Entity::find()
            .filter(payment_attempt_entity::Column::PaymentProvider.eq(provider))
            .filter(payment_attempt_entity::Column::ProviderReference.eq(reference))
            .one(self.db.as_ref())
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!(
                    "Failed to find payment attempt by provider reference: {}",
                    e
                ))
            })?;

        match result {
            Some(model) => Self::model_to_payment_attempt(model).map(Some),
            None => Ok(None),
        }
    }

    async fn update_payment_attempt(
        &self,
        attempt: PaymentAttempt,
    ) -> Result<PaymentAttempt, CoreError> {
        let attempt_model = payment_attempt_entity::ActiveModel {
            id: Set(attempt.id),
            realm_id: Set(attempt.realm_id),
            user_id: Set(attempt.user_id),
            payment_provider: Set(attempt.payment_provider),
            target_type: Set(attempt.target_type.to_string()),
            target_id: Set(attempt.target_id),
            amount: Set(attempt.amount),
            currency: Set(attempt.currency),
            status: Set(attempt.status.to_string()),
            is_one_time_role: Set(attempt.is_one_time_role),
            provider_reference: Set(attempt.provider_reference),
            provider_status: Set(attempt.provider_status),
            metadata: Set(attempt.metadata),
            expires_at: Set(attempt.expires_at.into()),
            completed_at: Set(attempt.completed_at.map(|dt| dt.into())),
            created_at: Set(attempt.created_at.into()),
            updated_at: Set(chrono::Utc::now().into()),
        };

        let result = attempt_model.update(self.db.as_ref()).await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to update payment attempt: {e}"))
        })?;

        Self::model_to_payment_attempt(result)
    }

    async fn update_payment_attempt_with_status_guard(
        &self,
        attempt: PaymentAttempt,
        expected_status: PaymentAttemptStatus,
    ) -> Result<PaymentAttempt, CoreError> {
        let target_status = attempt.status.clone();
        let result = sqlx::query(
            "UPDATE payment_attempts
             SET status = $1,
                 provider_reference = $2,
                 provider_status = $3,
                 completed_at = $4,
                 updated_at = NOW()
             WHERE id = $5
               AND realm_id = $6
               AND status = $7",
        )
        .bind(target_status.to_string())
        .bind(attempt.provider_reference.as_deref())
        .bind(attempt.provider_status.as_deref())
        .bind(attempt.completed_at)
        .bind(attempt.id)
        .bind(&attempt.realm_id)
        .bind(expected_status.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to guarded-update payment attempt: {e}"))
        })?;

        if result.rows_affected() > 0 {
            return self
                .find_payment_attempt_by_id(&attempt.realm_id, attempt.id)
                .await?
                .ok_or_else(|| CoreError::attempt_not_found(&attempt.id.to_string()));
        }

        let current = self
            .find_payment_attempt_by_id(&attempt.realm_id, attempt.id)
            .await?
            .ok_or_else(|| CoreError::attempt_not_found(&attempt.id.to_string()))?;

        if current.status == target_status {
            return Ok(current);
        }

        Err(CoreError::invalid_status_transition(
            &expected_status.to_string(),
            &current.status.to_string(),
        ))
    }

    async fn list_expired_attempts(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<PaymentAttempt>, CoreError> {
        let results = payment_attempt_entity::Entity::find()
            .filter(payment_attempt_entity::Column::ExpiresAt.lt(before))
            .filter(payment_attempt_entity::Column::Status.eq("Pending"))
            .all(self.db.as_ref())
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to list expired attempts: {e}"))
            })?;

        results
            .into_iter()
            .map(Self::model_to_payment_attempt)
            .collect()
    }

    async fn list_purchase_history(
        &self,
        realm_id: &str,
        user_id: Option<uuid::Uuid>,
        payment_provider: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<PurchaseHistoryRow>, i64), CoreError> {
        let offset = (page - 1) * page_size;
        let provider_filter = payment_provider.unwrap_or("");
        let start_filter = start_date.unwrap_or("");
        let end_filter = end_date.unwrap_or("");

        // Count query
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM payment_attempts pa \
             WHERE pa.realm_id = $1 AND ($2::uuid IS NULL OR pa.user_id = $2) \
             AND pa.status = 'Succeeded' AND pa.target_type = 'entitlement_mapping' \
             AND ($3 = '' OR pa.payment_provider = $3) \
             AND ($4 = '' OR pa.created_at >= $4::timestamptz) \
             AND ($5 = '' OR pa.created_at <= $5::timestamptz)",
        )
        .bind(realm_id)
        .bind(user_id)
        .bind(provider_filter)
        .bind(start_filter)
        .bind(end_filter)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to count purchase history: {e}")))?;

        // Data query
        let rows = sqlx::query(
            "SELECT pa.id AS attempt_id, pa.user_id, pa.target_id AS target_mapping_id, \
             pem.provider_product_info, \
             (SELECT SUM(l.granted_amount)::bigint \
                FROM points_distribution_events e \
                JOIN points_credit_ledger l ON l.distribution_event_id = e.id \
               WHERE e.source_id = pa.id::text) AS granted_points, \
             pa.amount, pa.currency, pa.payment_provider, pa.status, \
             pa.completed_at, pa.created_at \
             FROM payment_attempts pa \
             LEFT JOIN provider_entitlement_mappings pem ON pa.target_id = pem.id \
             WHERE pa.realm_id = $1 AND ($2::uuid IS NULL OR pa.user_id = $2) \
             AND pa.status = 'Succeeded' AND pa.target_type = 'entitlement_mapping' \
             AND ($3 = '' OR pa.payment_provider = $3) \
             AND ($4 = '' OR pa.created_at >= $4::timestamptz) \
             AND ($5 = '' OR pa.created_at <= $5::timestamptz) \
             ORDER BY pa.created_at DESC \
             LIMIT $6 OFFSET $7",
        )
        .bind(realm_id)
        .bind(user_id)
        .bind(provider_filter)
        .bind(start_filter)
        .bind(end_filter)
        .bind(page_size as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to fetch purchase history: {e}")))?;

        let items: Vec<PurchaseHistoryRow> = rows
            .into_iter()
            .map(|row| {
                let product_info: Option<serde_json::Value> = row.get("provider_product_info");
                let product_name = product_info
                    .as_ref()
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let granted_points: Option<i64> = row.get("granted_points");

                PurchaseHistoryRow {
                    attempt_id: row.get("attempt_id"),
                    user_id: row.get("user_id"),
                    target_mapping_id: row.get("target_mapping_id"),
                    product_name,
                    points: granted_points,
                    amount: row.get("amount"),
                    currency: row.get("currency"),
                    payment_provider: row.get("payment_provider"),
                    status: row.get("status"),
                    completed_at: row.get("completed_at"),
                    created_at: row.get("created_at"),
                }
            })
            .collect();

        Ok((items, count))
    }

    async fn has_succeeded_attempt(
        &self,
        user_id: uuid::Uuid,
        target_id: uuid::Uuid,
    ) -> Result<bool, CoreError> {
        // `status` is stored as the PascalCase string ("Succeeded"), matching
        // `PaymentAttemptStatus::Succeeded`'s `to_string()` and the renewal
        // insert path. `target_id` is the entitlement mapping id.
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT 1 FROM payment_attempts \
             WHERE user_id = $1 AND target_id = $2 AND status = 'Succeeded' \
             LIMIT 1",
        )
        .bind(user_id)
        .bind(target_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to check succeeded attempt: {e}")))?;

        Ok(row.is_some())
    }

    async fn find_captured_rule_refs(
        &self,
        realm_id: &str,
        attempt_id: uuid::Uuid,
    ) -> Result<Vec<herald_domain::points::CapturedRuleRef>, CoreError> {
        // Frozen at capture: read the snapshot rows directly, ignoring the
        // rule's current `enabled` state (a disabled-after-capture rule still
        // fires for this attempt). The bucket is the captured snapshot bucket.
        let rows: Vec<(uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
            "SELECT s.rule_id, s.bucket_id \
             FROM payment_attempt_point_rules s \
             JOIN points_distribution_rules r ON r.id = s.rule_id \
             WHERE s.payment_attempt_id = $1 AND r.realm_id = $2 \
             ORDER BY r.display_order, r.id",
        )
        .bind(attempt_id)
        .bind(realm_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to load captured rule refs: {e}")))?;

        Ok(rows
            .into_iter()
            .map(
                |(rule_id, bucket_id)| herald_domain::points::CapturedRuleRef {
                    rule_id,
                    bucket_id,
                },
            )
            .collect())
    }
}
