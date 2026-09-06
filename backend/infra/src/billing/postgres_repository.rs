use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction,
    EntityTrait, IntoActiveModel, JoinType, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    RelationTrait, Set, TransactionTrait,
};
use std::str::FromStr;
use uuid::Uuid;

use herald_domain::billing::credit_bucket::{
    CreateCreditBucketInput, CreditBucket, CreditBucketDetail, CreditBucketError,
    CreditBucketListItem, CreditBucketOverview, CreditBucketOverviewRow, UpdateCreditBucketInput,
};
use herald_domain::billing::entities::EntitlementMapping;
use herald_domain::billing::{
    BatchMappingError, BatchUpdateMappingsInput, BatchUpdateResult, BillingRepository,
    FeatureFacts, HistoryEventType, PaymentEvent, SortOrder, Subscription,
    SubscriptionHistoryEvent, SubscriptionHistoryQuery,
};
use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::points::{
    DistributionPolicy, DistributionRuleOwner, DistributionRuleReference, DistributionTrigger,
    PointsDistributionRule, RuleUpsert,
};
use herald_entity::{
    payment_event, points_distribution_rule, provider_entitlement_mapping, subscription,
    subscription_history,
};

use crate::points::postgres_repository::{
    parse_quota_windows_value, serialize_quota_windows_value,
};

/// Subscription statuses that keep an entitlement mapping protected from being
/// disabled. Single-PATCH (`update_mapping_in_tx`) and batch-PATCH must use
/// the same set or the two disable paths diverge in protection strength.
const ACCESS_GRANTING_SUBSCRIPTION_STATUSES_SQL: &str =
    "'active','trialing','past_due','scheduled_cancel','dispute'";

/// PostgreSQL implementation of billing repository
pub struct PostgresBillingRepository {
    db: DatabaseConnection,
}

impl PostgresBillingRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn begin_transaction(&self) -> Result<DatabaseTransaction, CoreError> {
        self.db
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))
    }

    /// Converts database model to domain Subscription
    fn model_to_subscription(model: subscription::Model) -> Result<Subscription, CoreError> {
        Ok(Subscription {
            id: model.id,
            realm_id: model.realm_id,
            user_id: model.user_id,
            external_subscription_id: model.external_subscription_id,
            external_product_id: model.external_product_id,
            payment_provider: model.payment_provider,
            status: model.status.parse()?,
            entitlement_key: model.entitlement_key,
            billing_type: model.billing_type.parse()?,
            external_price_id: model.external_price_id,
            provider_metadata: model.provider_metadata,
            synced_at: model.synced_at.map(chrono::DateTime::from),
            current_period_start: model.current_period_start.map(chrono::DateTime::from),
            current_period_end: model.current_period_end.map(chrono::DateTime::from),
            cancel_at_period_end: model.cancel_at_period_end,
            client_app_id: model.client_app_id,
            cancel_at: model.cancel_at.map(chrono::DateTime::from),
            created_at: chrono::DateTime::from(model.created_at),
            updated_at: chrono::DateTime::from(model.updated_at),
        })
    }

    /// Converts database model to domain PaymentEvent
    fn model_to_payment_event(mut model: payment_event::Model) -> PaymentEvent {
        PaymentEvent {
            id: model.id,
            realm_id: model.realm_id,
            external_event_id: model.external_event_id,
            payment_provider: model.payment_provider,
            event_type: model.event_type,
            subscription_id: model.subscription_id,
            payload: model.payload.take(),
            processed: model.processed,
            processing_started_at: model.processing_started_at.map(chrono::DateTime::from),
            created_at: chrono::DateTime::from(model.created_at),
        }
    }

    /// Converts database model to domain EntitlementMapping
    fn model_to_entitlement_mapping(
        model: provider_entitlement_mapping::Model,
    ) -> EntitlementMapping {
        EntitlementMapping {
            id: model.id,
            realm_id: model.realm_id,
            payment_provider: model.payment_provider,
            external_product_id: model.external_product_id,
            external_price_id: model.external_price_id,
            entitlement_key: model.entitlement_key,
            billing_type: model.billing_type.and_then(|s| s.parse().ok()),
            billing_period: model.billing_period,
            service_duration_days: model.service_duration_days.map(|v| v as i64),
            enabled: model.enabled,
            provider_product_info: model.provider_product_info,
            granted_role_ids: model.granted_role_ids,
            synced_at: model.synced_at.map(chrono::DateTime::from),
            created_at: chrono::DateTime::from(model.created_at),
            updated_at: chrono::DateTime::from(model.updated_at),
        }
    }

    /// Converts domain EntitlementMapping to database active model
    fn entitlement_mapping_to_active_model(
        mapping: EntitlementMapping,
    ) -> provider_entitlement_mapping::ActiveModel {
        provider_entitlement_mapping::ActiveModel {
            id: Set(mapping.id),
            realm_id: Set(mapping.realm_id),
            payment_provider: Set(mapping.payment_provider),
            external_product_id: Set(mapping.external_product_id),
            external_price_id: Set(mapping.external_price_id),
            entitlement_key: Set(mapping.entitlement_key),
            billing_type: Set(mapping.billing_type.map(|t| t.as_str().to_string())),
            billing_period: Set(mapping.billing_period),
            service_duration_days: Set(mapping.service_duration_days.map(|v| v as i32)),
            enabled: Set(mapping.enabled),
            provider_product_info: Set(mapping.provider_product_info),
            granted_role_ids: Set(mapping.granted_role_ids),
            synced_at: Set(mapping
                .synced_at
                .map(sea_orm::prelude::DateTimeWithTimeZone::from)),
            created_at: Set(sea_orm::prelude::DateTimeWithTimeZone::from(
                mapping.created_at,
            )),
            updated_at: Set(sea_orm::prelude::DateTimeWithTimeZone::from(
                mapping.updated_at,
            )),
        }
    }

    /// Decode a raw sqlx `RETURNING *` row from `provider_entitlement_mappings`
    /// into the domain [`EntitlementMapping`]. Mirrors
    /// [`Self::model_to_entitlement_mapping`] column-for-column; used by the
    /// transactional create/upsert paths that write the base row via raw sqlx on
    /// `&mut tx` (SeaORM `.insert()`/`.update()` cannot bind to a raw sqlx
    /// transaction).
    fn row_to_entitlement_mapping(row: &sqlx::postgres::PgRow) -> EntitlementMapping {
        use sqlx::Row;
        EntitlementMapping {
            id: row.get("id"),
            realm_id: row.get("realm_id"),
            payment_provider: row.get("payment_provider"),
            external_product_id: row.get("external_product_id"),
            external_price_id: row.get("external_price_id"),
            entitlement_key: row.get("entitlement_key"),
            billing_type: row
                .get::<Option<String>, _>("billing_type")
                .and_then(|s| s.parse().ok()),
            billing_period: row.get("billing_period"),
            service_duration_days: row
                .get::<Option<i32>, _>("service_duration_days")
                .map(|v| v as i64),
            enabled: row.get("enabled"),
            provider_product_info: row.get("provider_product_info"),
            granted_role_ids: row.get("granted_role_ids"),
            synced_at: row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("synced_at"),
            created_at: row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            updated_at: row.get::<chrono::DateTime<chrono::Utc>, _>("updated_at"),
        }
    }

    /// Converts database model to domain SubscriptionHistoryEvent
    fn model_to_subscription_history_event(
        model: subscription_history::Model,
    ) -> Result<SubscriptionHistoryEvent, CoreError> {
        let event_type_str = model.event_type.to_lowercase();
        let event_type = HistoryEventType::from_str(&event_type_str)?;

        Ok(SubscriptionHistoryEvent {
            id: model.id,
            subscription_id: model.subscription_id,
            event_type,
            timestamp: chrono::DateTime::from(model.timestamp),
            actor: model.actor,
            changes: model
                .changes
                .map(|json| serde_json::to_value(json).unwrap_or(serde_json::Value::Null)),
            previous_state: model
                .previous_state
                .map(|json| serde_json::to_value(json).unwrap_or(serde_json::Value::Null)),
            new_state: model
                .new_state
                .map(|json| serde_json::to_value(json).unwrap_or(serde_json::Value::Null)),
            realm_id: model.realm_id,
            created_at: chrono::DateTime::from(model.created_at),
        })
    }

    /// Converts domain SubscriptionHistoryEvent to database active model
    fn history_event_to_active_model(
        event: SubscriptionHistoryEvent,
    ) -> subscription_history::ActiveModel {
        subscription_history::ActiveModel {
            id: Set(event.id),
            subscription_id: Set(event.subscription_id),
            event_type: Set(event.event_type.as_str().to_string()),
            timestamp: Set(sea_orm::prelude::DateTimeWithTimeZone::from(
                event.timestamp,
            )),
            actor: Set(event.actor),
            changes: Set(event.changes),
            previous_state: Set(event.previous_state),
            new_state: Set(event.new_state),
            realm_id: Set(event.realm_id),
            created_at: Set(sea_orm::prelude::DateTimeWithTimeZone::from(
                event.created_at,
            )),
        }
    }

    fn subscription_to_active_model(sub: Subscription) -> subscription::ActiveModel {
        subscription::ActiveModel {
            id: Set(sub.id),
            realm_id: Set(sub.realm_id.clone()),
            user_id: Set(sub.user_id),
            external_subscription_id: Set(sub.external_subscription_id),
            external_product_id: Set(sub.external_product_id.clone()),
            payment_provider: Set(sub.payment_provider.clone()),
            status: Set(sub.status.as_str().to_string()),
            entitlement_key: Set(sub.entitlement_key.clone()),
            billing_type: Set(sub.billing_type.as_str().to_string()),
            external_price_id: Set(sub.external_price_id.clone()),
            provider_metadata: Set(sub.provider_metadata.clone()),
            synced_at: Set(sub
                .synced_at
                .map(sea_orm::prelude::DateTimeWithTimeZone::from)),
            current_period_start: Set(sub
                .current_period_start
                .map(sea_orm::prelude::DateTimeWithTimeZone::from)),
            current_period_end: Set(sub
                .current_period_end
                .map(sea_orm::prelude::DateTimeWithTimeZone::from)),
            cancel_at_period_end: Set(sub.cancel_at_period_end),
            client_app_id: Set(sub.client_app_id),
            cancel_at: Set(sub
                .cancel_at
                .map(sea_orm::prelude::DateTimeWithTimeZone::from)),
            created_at: Set(sea_orm::prelude::DateTimeWithTimeZone::from(sub.created_at)),
            updated_at: Set(sea_orm::prelude::DateTimeWithTimeZone::from(sub.updated_at)),
        }
    }

    fn apply_subscription_update(active_model: &mut subscription::ActiveModel, sub: Subscription) {
        active_model.realm_id = Set(sub.realm_id.clone());
        active_model.user_id = Set(sub.user_id);
        active_model.external_subscription_id = Set(sub.external_subscription_id.clone());
        active_model.external_product_id = Set(sub.external_product_id.clone());
        active_model.payment_provider = Set(sub.payment_provider.clone());
        active_model.status = Set(sub.status.as_str().to_string());
        active_model.entitlement_key = Set(sub.entitlement_key.clone());
        active_model.billing_type = Set(sub.billing_type.as_str().to_string());
        active_model.external_price_id = Set(sub.external_price_id.clone());
        active_model.provider_metadata = Set(sub.provider_metadata.clone());
        active_model.synced_at = Set(sub
            .synced_at
            .map(sea_orm::prelude::DateTimeWithTimeZone::from));
        active_model.current_period_start = Set(sub
            .current_period_start
            .map(sea_orm::prelude::DateTimeWithTimeZone::from));
        active_model.current_period_end = Set(sub
            .current_period_end
            .map(sea_orm::prelude::DateTimeWithTimeZone::from));
        active_model.cancel_at_period_end = Set(sub.cancel_at_period_end);
        active_model.client_app_id = Set(sub.client_app_id);
        active_model.cancel_at = Set(sub
            .cancel_at
            .map(sea_orm::prelude::DateTimeWithTimeZone::from));
        active_model.updated_at = Set(sea_orm::prelude::DateTimeWithTimeZone::from(sub.updated_at));
    }

    pub async fn create_subscription_conn<C: ConnectionTrait>(
        db: &C,
        sub: Subscription,
    ) -> Result<Subscription, CoreError> {
        let result = Self::subscription_to_active_model(sub).insert(db).await?;
        Self::model_to_subscription(result)
    }

    pub async fn find_by_external_subscription_id_conn<C: ConnectionTrait>(
        db: &C,
        external_sub_id: &str,
        provider: &str,
    ) -> Result<Option<Subscription>, CoreError> {
        let result = subscription::Entity::find()
            .filter(subscription::Column::ExternalSubscriptionId.eq(external_sub_id))
            .filter(subscription::Column::PaymentProvider.eq(provider))
            .one(db)
            .await?;

        result.map(Self::model_to_subscription).transpose()
    }

    pub async fn find_subscription_by_client_app_id_conn<C: ConnectionTrait>(
        db: &C,
        client_app_id: Uuid,
    ) -> Result<Option<Subscription>, CoreError> {
        let result = subscription::Entity::find()
            .filter(subscription::Column::ClientAppId.eq(client_app_id))
            .one(db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        result.map(Self::model_to_subscription).transpose()
    }

    pub async fn update_subscription_conn<C: ConnectionTrait>(
        db: &C,
        sub: Subscription,
    ) -> Result<Subscription, CoreError> {
        let existing = subscription::Entity::find_by_id(sub.id)
            .one(db)
            .await?
            .ok_or_else(|| CoreError::SubscriptionNotFound(sub.id.to_string()))?;

        let mut active_model: subscription::ActiveModel = existing.into_active_model();
        Self::apply_subscription_update(&mut active_model, sub);

        let result = active_model.update(db).await?;
        Self::model_to_subscription(result)
    }

    pub async fn save_history_event_conn<C: ConnectionTrait>(
        db: &C,
        event: SubscriptionHistoryEvent,
    ) -> Result<SubscriptionHistoryEvent, CoreError> {
        let active_model = Self::history_event_to_active_model(event);

        let result = subscription_history::Entity::insert(active_model)
            .exec(db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        let saved_event = subscription_history::Entity::find_by_id(result.last_insert_id)
            .one(db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?
            .ok_or(CoreError::NotFound)?;

        Self::model_to_subscription_history_event(saved_event)
    }

    // All multi-table writes run in a single sqlx transaction (matches the
    // invoice_postgres_repository pattern: `self.db.get_postgres_connection_pool().begin()`).
    // Coverage-set changes only affect future routing.

    /// Create a Credit Bucket with its coverage set.
    ///
    /// Transaction:
    /// 1. INSERT `credit_buckets` (raises `bucket_key` unique violation →
    ///    `BucketKeyDuplicate`).
    /// 2. INSERT `credit_bucket_client_apps` rows for the coverage set.
    ///
    /// A freshly-created bucket has no referencing rules, so its
    /// `rule_references` are empty.
    pub async fn create_credit_bucket(
        &self,
        input: CreateCreditBucketInput,
    ) -> Result<CreditBucketDetail, CreditBucketError> {
        let id = Uuid::now_v7();
        let mut tx = self
            .db
            .get_postgres_connection_pool()
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        // 1. Insert bucket. The UNIQUE(realm_id, bucket_key) constraint fires on
        //    a bucket_key collision → BucketKeyDuplicate.
        let row = sqlx::query(
            "INSERT INTO credit_buckets \
             (id, realm_id, bucket_key, name, description, display_order, \
              enabled, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW()) \
             RETURNING id, realm_id, bucket_key, name, description, display_order, enabled",
        )
        .bind(id)
        .bind(&input.realm_id)
        .bind(&input.bucket_key)
        .bind(&input.name)
        .bind(input.description.as_deref())
        .bind(input.display_order)
        .bind(input.enabled)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| Self::classify_bucket_insert_error(&e, &input.realm_id))?;

        let bucket = Self::row_to_credit_bucket(&row);

        // 2. Insert coverage set.
        for client_app_id in &input.client_app_ids {
            sqlx::query(
                "INSERT INTO credit_bucket_client_apps \
                 (bucket_id, client_app_id, realm_id, created_at) \
                 VALUES ($1, $2, $3, NOW()) \
                 ON CONFLICT (bucket_id, client_app_id) DO NOTHING",
            )
            .bind(id)
            .bind(client_app_id)
            .bind(&input.realm_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to insert coverage row: {}", e))
            })?;
        }

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit create_credit_bucket: {}", e))
        })?;

        Ok(CreditBucketDetail {
            bucket,
            client_app_ids: input.client_app_ids,
            // A brand-new bucket is not yet referenced by any rule.
            rule_references: Vec::new(),
        })
    }

    /// Get a single Credit Bucket with its coverage set and referencing rules.
    pub async fn get_credit_bucket(
        &self,
        realm_id: &str,
        bucket_id: Uuid,
    ) -> Result<Option<CreditBucketDetail>, CoreError> {
        let row = sqlx::query(
            "SELECT id, realm_id, bucket_key, name, description, display_order, enabled \
             FROM credit_buckets WHERE realm_id = $1 AND id = $2",
        )
        .bind(realm_id)
        .bind(bucket_id)
        .fetch_optional(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to fetch credit bucket: {}", e)))?;

        let Some(row) = row else {
            return Ok(None);
        };
        let bucket = Self::row_to_credit_bucket(&row);

        let client_app_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT client_app_id FROM credit_bucket_client_apps \
             WHERE realm_id = $1 AND bucket_id = $2 ORDER BY client_app_id",
        )
        .bind(realm_id)
        .bind(bucket_id)
        .fetch_all(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to fetch coverage set: {}", e)))?;

        let rule_references = Self::load_rule_references(
            self.db.get_postgres_connection_pool(),
            realm_id,
            &[bucket_id],
        )
        .await?
        .remove(&bucket_id)
        .unwrap_or_default();

        Ok(Some(CreditBucketDetail {
            bucket,
            client_app_ids,
            rule_references,
        }))
    }

    /// List Credit Buckets for a realm with aggregate counts.
    pub async fn list_credit_buckets(
        &self,
        realm_id: &str,
    ) -> Result<Vec<CreditBucketListItem>, CoreError> {
        use sqlx::Row;

        let rows = sqlx::query(
            "SELECT b.id, b.realm_id, b.bucket_key, b.name, b.description, b.display_order, \
                    b.enabled, \
                    COALESCE(ca.covered_count, 0) AS covered_client_app_count \
             FROM credit_buckets b \
             LEFT JOIN ( \
                 SELECT bucket_id, COUNT(*) AS covered_count \
                 FROM credit_bucket_client_apps GROUP BY bucket_id \
             ) ca ON ca.bucket_id = b.id \
             WHERE b.realm_id = $1 \
             ORDER BY b.display_order ASC, b.created_at ASC",
        )
        .bind(realm_id)
        .fetch_all(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to list credit buckets: {}", e)))?;

        let bucket_ids: Vec<Uuid> = rows.iter().map(|r| r.get("id")).collect();
        let ref_counts = Self::count_rule_references(
            self.db.get_postgres_connection_pool(),
            realm_id,
            &bucket_ids,
        )
        .await?;

        let items = rows
            .iter()
            .map(|row| {
                let id: Uuid = row.get("id");
                let bucket = Self::row_to_credit_bucket(row);
                CreditBucketListItem {
                    bucket,
                    covered_client_app_count: row.get("covered_client_app_count"),
                    rule_reference_count: ref_counts.get(&id).copied().unwrap_or(0),
                }
            })
            .collect();

        Ok(items)
    }

    /// Update a Credit Bucket: base fields + coverage-set replace.
    ///
    /// Bucket references are derived from distribution rules.
    pub async fn update_credit_bucket(
        &self,
        input: UpdateCreditBucketInput,
    ) -> Result<CreditBucketDetail, CreditBucketError> {
        let mut tx = self
            .db
            .get_postgres_connection_pool()
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        // Lock the bucket row and verify ownership.
        let exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM credit_buckets WHERE realm_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(&input.realm_id)
        .bind(input.bucket_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to lock credit bucket: {}", e)))?;
        if exists.is_none() {
            return Err(CoreError::NotFound.into());
        }

        // Update base fields. The UNIQUE(realm_id, bucket_key) is unchanged by a
        // PUT (bucket_key is immutable here), so no BucketKeyDuplicate is
        // expected, but classification is still applied for safety.
        let row = sqlx::query(
            "UPDATE credit_buckets \
             SET name = $3, description = $4, display_order = $5, \
                 enabled = $6, updated_at = NOW() \
             WHERE realm_id = $1 AND id = $2 \
             RETURNING id, realm_id, bucket_key, name, description, display_order, enabled",
        )
        .bind(&input.realm_id)
        .bind(input.bucket_id)
        .bind(&input.name)
        .bind(input.description.as_deref())
        .bind(input.display_order)
        .bind(input.enabled)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| Self::classify_bucket_insert_error(&e, &input.realm_id))?;

        let bucket = Self::row_to_credit_bucket(&row);

        // Replace coverage set (delete + insert). CASCADE-safe since we hold the
        // bucket lock; existing wallets/ledgers are untouched.
        sqlx::query("DELETE FROM credit_bucket_client_apps WHERE bucket_id = $1")
            .bind(input.bucket_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to clear coverage set: {}", e))
            })?;

        for client_app_id in &input.client_app_ids {
            sqlx::query(
                "INSERT INTO credit_bucket_client_apps \
                 (bucket_id, client_app_id, realm_id, created_at) \
                 VALUES ($1, $2, $3, NOW()) \
                 ON CONFLICT (bucket_id, client_app_id) DO NOTHING",
            )
            .bind(input.bucket_id)
            .bind(client_app_id)
            .bind(&input.realm_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to insert coverage row: {}", e))
            })?;
        }

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit update_credit_bucket: {}", e))
        })?;

        let rule_references = Self::load_rule_references(
            self.db.get_postgres_connection_pool(),
            &input.realm_id,
            &[input.bucket_id],
        )
        .await?
        .remove(&input.bucket_id)
        .unwrap_or_default();

        Ok(CreditBucketDetail {
            bucket,
            client_app_ids: input.client_app_ids,
            rule_references,
        })
    }

    /// Delete a Credit Bucket. Refused with `BucketInUse` when in-flight
    /// subscriptions, spendable balances, distribution rules, or quota entitlements reference it.
    pub async fn delete_credit_bucket(
        &self,
        realm_id: &str,
        bucket_id: Uuid,
    ) -> Result<(), CreditBucketError> {
        let mut tx = self
            .db
            .get_postgres_connection_pool()
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        let exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM credit_buckets WHERE realm_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(realm_id)
        .bind(bucket_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to lock credit bucket: {}", e)))?;
        if exists.is_none() {
            return Err(CoreError::NotFound.into());
        }

        // In-flight subscriptions reference buckets via their distribution-rule
        // grant results: a subscription's `subscription_credit` quota
        // entitlement / ledger rows carry `source_id = subscription_id` and a
        // `bucket_id`. An active subscription that has granted into this bucket
        // blocks the delete (independent of the residual-balance guard below, so
        // a future-effective row that drives derived balance to 0 still leaves
        // the subscription arm as the blocker).
        let active_subscriptions: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT s.id) FROM subscription s \
             WHERE s.realm_id = $1 \
               AND s.status IN ('active', 'trialing') \
               AND ( \
                    EXISTS (SELECT 1 FROM points_quota_entitlements q \
                            WHERE q.source_id = s.id::text \
                              AND q.bucket_id = $2 \
                              AND q.credit_type = 'subscription_credit' \
                              AND q.status = 'active') \
                    OR EXISTS (SELECT 1 FROM points_credit_ledger l \
                               WHERE l.source_id = s.id::text \
                                 AND l.bucket_id = $2 \
                                 AND l.credit_type = 'subscription_credit' \
                                 AND l.status = 'active' \
                                 AND l.remaining_amount > 0) \
                   )",
        )
        .bind(realm_id)
        .bind(bucket_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to count active subscriptions: {}", e))
        })?;

        // Holders with remaining *derived available* balance.
        // The delete guard uses the SAME derived
        // availability predicate as `compute_bucket_available_balances`
        // (`status='active' AND remaining_amount>0 AND (effective_at IS NULL
        // OR effective_at<=NOW()) AND (expires_at IS NULL OR
        // expires_at>NOW())`) instead of the Stored `points_wallets.total_balance`
        // column. Future-effective pre-grant rows do NOT block delete here (they
        // are not yet spendable); they are swept by
        // `clear_deletable_bucket_references_tx` below. If the predicate text
        // drifts from the one in `points/postgres_repository.rs`, step 2 will
        // catch it.
        let holders_with_balance: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM points_credit_ledger \
             WHERE bucket_id = $1 \
               AND status = 'active' \
               AND remaining_amount > 0 \
               AND (effective_at IS NULL OR effective_at <= NOW()) \
               AND (expires_at IS NULL OR expires_at > NOW())",
        )
        .bind(bucket_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to count holders: {}", e)))?;

        let has_rule_or_quota_references: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM points_distribution_rules WHERE bucket_id = $1) \
             OR EXISTS (SELECT 1 FROM points_quota_entitlements WHERE bucket_id = $1)",
        )
        .bind(bucket_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to check bucket references: {}", e))
        })?;

        if active_subscriptions > 0 || holders_with_balance > 0 || has_rule_or_quota_references {
            // Roll back before surfacing the structured error.
            let _ = tx.rollback().await;
            return Err(CreditBucketError::BucketInUse {
                bucket_id,
                active_subscriptions,
                holders_with_balance,
            });
        }

        Self::clear_deletable_bucket_references_tx(&mut tx, bucket_id).await?;

        // Safe to delete after clearing zero-balance points residue and the
        // non-active subscription / payment_attempt / mapping rows still bound
        // to the bucket. Coverage rows cascade from the bucket.
        sqlx::query("DELETE FROM credit_buckets WHERE realm_id = $1 AND id = $2")
            .bind(realm_id)
            .bind(bucket_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to delete credit bucket: {}", e))
            })?;

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit delete_credit_bucket: {}", e))
        })?;

        Ok(())
    }

    async fn update_mapping_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        mapping: EntitlementMapping,
    ) -> Result<EntitlementMapping, CoreError> {
        let was_enabled: bool = sqlx::query_scalar(
            "SELECT enabled FROM provider_entitlement_mappings WHERE id = $1 AND realm_id = $2 FOR UPDATE",
        )
        .bind(mapping.id).bind(&mapping.realm_id)
        .fetch_optional(&mut **tx).await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?
        .ok_or(CoreError::NotFound)?;
        if was_enabled && !mapping.enabled {
            let active: bool = sqlx::query_scalar(&format!(
                "SELECT EXISTS (SELECT 1 FROM subscription WHERE realm_id = $1 \
                 AND payment_provider = $2 AND external_product_id = $3 \
                 AND external_price_id IS NOT DISTINCT FROM $4 \
                 AND status IN ({ACCESS_GRANTING_SUBSCRIPTION_STATUSES_SQL}))"
            ))
            .bind(&mapping.realm_id)
            .bind(&mapping.payment_provider)
            .bind(&mapping.external_product_id)
            .bind(&mapping.external_price_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            if active {
                return Err(CoreError::Conflict(
                    "Cannot disable mapping with active subscriptions".to_string(),
                ));
            }
        }
        // Write ONLY the mutable base fields, ON the transaction so a subsequent
        // rule-write failure rolls them back (DEC-005). Raw sqlx on `&mut **tx`
        // mirrors upsert_rules_in_tx / batch_update_mappings. Identity columns (realm_id,
        // payment_provider, external_product_id, external_price_id, id,
        // created_at) are intentionally not written, matching the prior
        // SeaORM ActiveModel field set exactly.
        let row = sqlx::query(
            "UPDATE provider_entitlement_mappings SET \
                entitlement_key = $1, billing_type = $2, billing_period = $3, \
                service_duration_days = $4, enabled = $5, provider_product_info = $6, \
                granted_role_ids = $7, synced_at = $8, updated_at = $9 \
             WHERE id = $10 AND realm_id = $11 \
             RETURNING *",
        )
        .bind(&mapping.entitlement_key)
        .bind(
            mapping
                .billing_type
                .as_ref()
                .map(|t| t.as_str().to_string()),
        )
        .bind(&mapping.billing_period)
        .bind(mapping.service_duration_days.map(|v| v as i32))
        .bind(mapping.enabled)
        .bind(&mapping.provider_product_info)
        .bind(&mapping.granted_role_ids)
        .bind(mapping.synced_at)
        .bind(mapping.updated_at)
        .bind(mapping.id)
        .bind(&mapping.realm_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => CoreError::NotFound,
            other => CoreError::DatabaseError(other.to_string()),
        })?;
        let mapping = Self::row_to_entitlement_mapping(&row);
        Ok(mapping)
    }

    async fn clear_deletable_bucket_references_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        bucket_id: Uuid,
    ) -> Result<(), CoreError> {
        sqlx::query("DELETE FROM points_consumption_allocations WHERE bucket_id = $1")
            .bind(bucket_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!(
                    "Failed to delete bucket consumption allocations: {}",
                    e
                ))
            })?;

        sqlx::query("DELETE FROM points_transactions WHERE bucket_id = $1")
            .bind(bucket_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to delete bucket transactions: {}", e))
            })?;

        sqlx::query("DELETE FROM points_credit_ledger WHERE bucket_id = $1")
            .bind(bucket_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to delete bucket credit ledger: {}", e))
            })?;

        sqlx::query("DELETE FROM points_grant_schedules WHERE bucket_id = $1")
            .bind(bucket_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to delete bucket grant schedules: {}", e))
            })?;

        // `points_wallets.total_balance` was physically dropped; this is
        // a cleanup of orphan wallet rows for a deleted bucket,
        // NOT a balance-authority read. By this point all ledger rows for the
        // bucket are deleted above, so any remaining wallet rows are orphans
        // (their analytics are retained only for historical totals; with the
        // bucket gone they have no remaining referent). Delete unconditionally.
        sqlx::query("DELETE FROM points_wallets WHERE bucket_id = $1")
            .bind(bucket_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to delete orphan bucket wallets: {}", e))
            })?;

        // Singular routing columns have been removed (subscriptions / attempts / mappings
        // reference buckets indirectly via distribution rule results, which are
        // already swept above via the ledger/transaction/schedule deletes). The
        // per-table routing-bound DELETEs are therefore no longer needed
        // here; the rule-result linkage cleanup is owned by the lifecycle items.

        Ok(())
    }

    /// Overview matrix: per-bucket × credit-type aggregates (residual rows kept for
    /// disabled buckets) plus a SEPARATE grand total across all buckets.
    ///
    /// The per-bucket available balance is the derived
    /// SUM over `points_credit_ledger` using the SAME availability predicate as
    /// `compute_bucket_available_balances` in `points/postgres_repository.rs`
    /// (`status='active' AND remaining_amount>0 AND (effective_at IS NULL OR
    /// effective_at<=NOW()) AND (expires_at IS NULL OR expires_at>NOW())`),
    /// grouped by `(bucket_id, credit_type)`. This replaces the previous
    /// `LEFT JOIN points_wallets ... SUM(w.<x>_balance)` aggregation, which read
    /// Stored/GENERATED columns and would (a) leak future-effective pre-grant
    /// rows into the overview and (b) misjudge a bucket as in-use. If the
    /// predicate text drifts from the points repository, step 2 will catch
    /// it.
    pub async fn list_bucket_overview(
        &self,
        realm_id: &str,
    ) -> Result<CreditBucketOverview, CoreError> {
        use sqlx::Row;

        // Bucket metadata ordered for display.
        let bucket_rows = sqlx::query(
            "SELECT id AS bucket_id, name, enabled \
             FROM credit_buckets \
             WHERE realm_id = $1 \
             ORDER BY display_order ASC, created_at ASC",
        )
        .bind(realm_id)
        .fetch_all(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to load bucket overview: {}", e)))?;

        // Derived per-(bucket_id, credit_type) available-balance aggregates using
        // the shared availability predicate (same text as
        // `compute_bucket_available_balances`).
        let agg_rows: Vec<(Uuid, String, i64)> = sqlx::query_as(
            "SELECT bucket_id, credit_type, COALESCE(SUM(remaining_amount), 0)::bigint AS available \
             FROM points_credit_ledger \
             WHERE realm_id = $1 \
               AND status = 'active' \
               AND remaining_amount > 0 \
               AND (effective_at IS NULL OR effective_at <= NOW()) \
               AND (expires_at IS NULL OR expires_at > NOW()) \
             GROUP BY bucket_id, credit_type",
        )
        .bind(realm_id)
        .fetch_all(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to load bucket overview: {}", e)))?;

        // Index aggregates by bucket_id for O(1) lookup while walking buckets.
        let mut agg_by_bucket: std::collections::HashMap<
            Uuid,
            herald_domain::billing::credit_bucket::BucketByCreditType,
        > = std::collections::HashMap::new();
        for (bucket_id, credit_type, available) in agg_rows {
            let entry = agg_by_bucket.entry(bucket_id).or_default();
            match credit_type.as_str() {
                "topup_credit" => entry.topup = entry.topup.saturating_add(available),
                "subscription_credit" => {
                    entry.subscription = entry.subscription.saturating_add(available)
                }
                "granted_credit" => entry.granted = entry.granted.saturating_add(available),
                "registration_credit" => {
                    entry.registration = entry.registration.saturating_add(available)
                }
                "free_periodic_credit" => {
                    entry.free_periodic = entry.free_periodic.saturating_add(available)
                }
                other => {
                    return Err(CoreError::DatabaseError(format!(
                        "invalid credit_type in points_credit_ledger: {other}"
                    )));
                }
            }
        }

        let mut out_rows = Vec::with_capacity(bucket_rows.len());
        let mut grand_total_topup = 0i64;
        let mut grand_total_subscription = 0i64;
        let mut grand_total_registration = 0i64;
        let mut grand_total_free_periodic = 0i64;
        let mut grand_total_granted = 0i64;

        for row in &bucket_rows {
            let bucket_id: Uuid = row.get("bucket_id");
            let by_credit_type = agg_by_bucket.remove(&bucket_id).unwrap_or_default();

            grand_total_topup = grand_total_topup.saturating_add(by_credit_type.topup);
            grand_total_subscription =
                grand_total_subscription.saturating_add(by_credit_type.subscription);
            grand_total_registration =
                grand_total_registration.saturating_add(by_credit_type.registration);
            grand_total_free_periodic =
                grand_total_free_periodic.saturating_add(by_credit_type.free_periodic);
            grand_total_granted = grand_total_granted.saturating_add(by_credit_type.granted);

            out_rows.push(CreditBucketOverviewRow {
                bucket_id,
                name: row.get("name"),
                enabled: row.get("enabled"),
                bucket_total: by_credit_type.total(),
                by_credit_type,
            });
        }

        let grand_total = herald_domain::billing::credit_bucket::BucketByCreditType {
            topup: grand_total_topup,
            subscription: grand_total_subscription,
            registration: grand_total_registration,
            free_periodic: grand_total_free_periodic,
            granted: grand_total_granted,
        };

        Ok(CreditBucketOverview {
            rows: out_rows,
            grand_total,
        })
    }

    fn row_to_credit_bucket(row: &sqlx::postgres::PgRow) -> CreditBucket {
        use sqlx::Row;
        CreditBucket {
            id: row.get("id"),
            realm_id: row.get("realm_id"),
            bucket_key: row.get("bucket_key"),
            name: row.get("name"),
            description: row.get("description"),
            display_order: row.get("display_order"),
            enabled: row.get("enabled"),
        }
    }

    /// Convert a `points_distribution_rules` SeaORM model into the domain rule.
    ///
    /// `trigger_sources` is parsed best-effort: an unknown trigger is logged and
    /// dropped rather than failing the whole read (a malformed row must not
    /// poison a list). `quota_windows` is hydrated via the shared infra serde
    /// boundary. The owner is materialized from `owner_type` +
    /// `entitlement_mapping_id`.
    fn rule_from_model(model: points_distribution_rule::Model) -> PointsDistributionRule {
        let owner = match model.owner_type.as_str() {
            "entitlement_mapping" => DistributionRuleOwner::EntitlementMapping(
                model.entitlement_mapping_id.unwrap_or(Uuid::nil()),
            ),
            _ => DistributionRuleOwner::RealmRegistration,
        };
        let trigger_sources = model
            .trigger_sources
            .iter()
            .filter_map(|s| match s.parse::<DistributionTrigger>() {
                Ok(t) => Some(t),
                Err(_) => {
                    tracing::warn!(
                        rule_id = %model.id,
                        trigger = %s,
                        "Unknown distribution trigger on rule; dropping from parsed set"
                    );
                    None
                }
            })
            .collect();
        let policy = match model.grant_mode.as_str() {
            "quota" => DistributionPolicy::Quota {
                windows: parse_quota_windows_value(model.quota_windows)
                    .map_err(|e| {
                        tracing::warn!(error = %e, rule_id = %model.id, "Malformed quota_windows JSONB on rule");
                        e
                    })
                    .ok()
                    .unwrap_or_default(),
            },
            // Default to fixed when grant_mode is missing/unexpected (DB CHECK
            // guarantees 'fixed' | 'quota'; fixed requires points_amount > 0).
            _ => DistributionPolicy::Fixed {
                amount: model.points_amount.unwrap_or(0),
                validity_days: model.validity_days.unwrap_or(0),
                grant_period_type: model
                    .grant_period_type
                    .as_deref()
                    .and_then(|s| s.parse().ok()),
            },
        };
        PointsDistributionRule {
            id: model.id,
            realm_id: model.realm_id,
            owner,
            bucket_id: model.bucket_id,
            trigger_sources,
            policy,
            enabled: model.enabled,
            display_order: model.display_order,
        }
    }

    /// Load referencing rules for the given buckets, grouped by `bucket_id`.
    /// Returns one [`DistributionRuleReference`] per rule; the caller groups by
    /// bucket. Buckets with no referencing rules contribute no rows. An empty
    /// `bucket_ids` returns an empty map.
    async fn load_rule_references(
        executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
        realm_id: &str,
        bucket_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, Vec<DistributionRuleReference>>, CoreError> {
        use sqlx::Row;
        if bucket_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows = sqlx::query(
            "SELECT bucket_id, id, owner_type, entitlement_mapping_id, trigger_sources, enabled \
             FROM points_distribution_rules \
             WHERE realm_id = $1 AND bucket_id = ANY($2) \
             ORDER BY bucket_id, display_order, id",
        )
        .bind(realm_id)
        .bind(bucket_ids)
        .fetch_all(executor)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to load rule references: {}", e)))?;
        let mut out: std::collections::HashMap<Uuid, Vec<DistributionRuleReference>> =
            std::collections::HashMap::new();
        for row in rows {
            let bucket_id: Uuid = row.get("bucket_id");
            let owner_type: String = row.get("owner_type");
            let entitlement_mapping_id: Option<Uuid> = row.get("entitlement_mapping_id");
            let trigger_sources: Vec<String> = row.get("trigger_sources");
            let enabled: bool = row.get("enabled");
            out.entry(bucket_id)
                .or_default()
                .push(DistributionRuleReference {
                    rule_id: row.get("id"),
                    owner_type,
                    entitlement_mapping_id,
                    trigger_sources,
                    enabled,
                });
        }
        Ok(out)
    }

    /// Count referencing rules per bucket (list view aggregate). Returns a
    /// `bucket_id → count` map; buckets with no references are absent.
    async fn count_rule_references(
        executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
        realm_id: &str,
        bucket_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, i64>, CoreError> {
        use sqlx::Row;
        if bucket_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows = sqlx::query(
            "SELECT bucket_id, COUNT(*)::bigint AS ref_count \
             FROM points_distribution_rules \
             WHERE realm_id = $1 AND bucket_id = ANY($2) \
             GROUP BY bucket_id",
        )
        .bind(realm_id)
        .bind(bucket_ids)
        .fetch_all(executor)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to count rule references: {}", e)))?;
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let bucket_id: Uuid = row.get("bucket_id");
            let ref_count: i64 = row.get("ref_count");
            out.insert(bucket_id, ref_count);
        }
        Ok(out)
    }

    /// Upsert a rule set under the given owner within a single sqlx
    /// transaction (DEC-005 atomic upsert).
    ///
    /// Semantics:
    /// - rules with `id = None` are created (fresh id) under the owner;
    /// - rules with `id = Some(existing)` are updated; the existing rule MUST
    ///   belong to the same owner (same `owner_type`, and — for mapping owners —
    ///   the same `entitlement_mapping_id`), otherwise the write is rejected
    ///   with `distribution_rule_conflict`;
    /// - rules NOT present in `rules` are left untouched (DEC-007: disabling
    ///   requires explicit `enabled = false`; referenced rules are never
    ///   hard-deleted).
    ///
    /// Writes go through raw SQL against the caller's sqlx transaction so they
    /// share the caller's commit/rollback boundary.
    async fn upsert_rules_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        realm_id: &str,
        owner: DistributionRuleOwner,
        rules: Vec<RuleUpsert>,
    ) -> Result<(), CoreError> {
        let owner_type = owner.as_str();
        let mapping_id = owner.mapping_id();
        for upsert in rules {
            let existing_id = upsert.id;
            let resolved = upsert.into_rule_for_owner(realm_id, owner.clone());
            let bucket_state: Option<(String, bool)> =
                sqlx::query_as("SELECT realm_id, enabled FROM credit_buckets WHERE id = $1")
                    .bind(resolved.bucket_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| {
                        CoreError::DatabaseError(format!(
                            "Failed to load target bucket for realm check: {}",
                            e
                        ))
                    })?;
            let (bucket_realm_id, bucket_enabled) = bucket_state.ok_or(CoreError::NotFound)?;
            if bucket_realm_id != realm_id {
                return Err(CoreError::Conflict(format!(
                    "distribution_rule_conflict: bucket {} does not belong to realm {}",
                    resolved.bucket_id, realm_id
                )));
            }
            // A disabled bucket must not receive new/updated rules
            // (multi-wallet PRD §4.6): grants route to it only while enabled,
            // so a disabled target would fail loud at event time instead of
            // at save time.
            if !bucket_enabled {
                return Err(CoreError::Conflict(format!(
                    "distribution_rule_conflict: bucket {} is disabled",
                    resolved.bucket_id
                )));
            }
            match existing_id {
                // Some(id) → update existing rule (must belong to this owner).
                Some(rule_id) => {
                    use sqlx::Row;
                    let row = sqlx::query(
                        "SELECT owner_type, entitlement_mapping_id, realm_id \
                         FROM points_distribution_rules WHERE id = $1",
                    )
                    .bind(rule_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| {
                        CoreError::DatabaseError(format!(
                            "Failed to load rule for owner check: {}",
                            e
                        ))
                    })?;
                    let row = row.ok_or(CoreError::NotFound)?;
                    let existing_owner_type: String = row.get("owner_type");
                    let existing_mapping_id: Option<Uuid> = row.get("entitlement_mapping_id");
                    let existing_realm_id: String = row.get("realm_id");
                    if existing_realm_id != realm_id
                        || existing_owner_type != owner_type
                        || existing_mapping_id != mapping_id
                    {
                        return Err(CoreError::Conflict(format!(
                            "distribution_rule_conflict: rule {} does not belong to this owner",
                            rule_id
                        )));
                    }
                    // Update the existing row in place via raw SQL (SeaORM
                    // ActiveModel update needs the tx connection adapter; raw
                    // SQL keeps it simple and shares the tx).
                    let trigger_sources: Vec<String> = resolved
                        .trigger_sources
                        .iter()
                        .map(|t| t.to_string())
                        .collect();
                    let (
                        grant_mode,
                        points_amount,
                        validity_days,
                        grant_period_type,
                        quota_windows,
                    ) = Self::policy_to_columns(resolved.policy)?;
                    sqlx::query(
                        "UPDATE points_distribution_rules \
                         SET bucket_id = $2, trigger_sources = $3, grant_mode = $4, \
                             points_amount = $5, validity_days = $6, grant_period_type = $7, \
                             quota_windows = $8, enabled = $9, display_order = $10, updated_at = NOW() \
                         WHERE id = $1",
                    )
                    .bind(rule_id)
                    .bind(resolved.bucket_id)
                    .bind(&trigger_sources)
                    .bind(&grant_mode)
                    .bind(points_amount)
                    .bind(validity_days)
                    .bind(&grant_period_type)
                    .bind(&quota_windows)
                    .bind(resolved.enabled)
                    .bind(resolved.display_order)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| CoreError::DatabaseError(format!("Failed to update rule: {}", e)))?;
                }
                // None (or nil) → create a new rule under the owner.
                _ => {
                    let new_id = Uuid::now_v7();
                    let trigger_sources: Vec<String> = resolved
                        .trigger_sources
                        .iter()
                        .map(|t| t.to_string())
                        .collect();
                    let (
                        grant_mode,
                        points_amount,
                        validity_days,
                        grant_period_type,
                        quota_windows,
                    ) = Self::policy_to_columns(resolved.policy)?;
                    sqlx::query(
                        "INSERT INTO points_distribution_rules \
                         (id, realm_id, owner_type, entitlement_mapping_id, bucket_id, \
                          trigger_sources, grant_mode, points_amount, validity_days, \
                          grant_period_type, quota_windows, enabled, display_order, \
                          created_at, updated_at) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, NOW(), NOW())",
                    )
                    .bind(new_id)
                    .bind(&resolved.realm_id)
                    .bind(owner_type)
                    .bind(mapping_id)
                    .bind(resolved.bucket_id)
                    .bind(&trigger_sources)
                    .bind(&grant_mode)
                    .bind(points_amount)
                    .bind(validity_days)
                    .bind(&grant_period_type)
                    .bind(&quota_windows)
                    .bind(resolved.enabled)
                    .bind(resolved.display_order)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| CoreError::DatabaseError(format!("Failed to insert rule: {}", e)))?;
                }
            }
        }
        Ok(())
    }

    /// Project a [`DistributionPolicy`] onto the `points_distribution_rules`
    /// write columns shared by the INSERT and UPDATE branches below.
    fn policy_to_columns(policy: DistributionPolicy) -> Result<PolicyColumns, CoreError> {
        Ok(match policy {
            DistributionPolicy::Fixed {
                amount,
                validity_days,
                grant_period_type,
            } => {
                let gpt = grant_period_type.map(|t| t.to_string());
                (
                    "fixed".to_string(),
                    Some(amount),
                    Some(validity_days),
                    gpt,
                    None,
                )
            }
            DistributionPolicy::Quota { windows } => {
                let qw = serialize_quota_windows_value(&windows).map_err(|e| {
                    CoreError::DatabaseError(format!("Failed to serialize quota windows: {}", e))
                })?;
                ("quota".to_string(), None, None, None, qw)
            }
        })
    }

    /// Map an INSERT/UPDATE error to a structured bucket error.
    ///
    /// Distinguishes the `credit_buckets` uniqueness violation by its
    /// constraint name:
    /// - `UNIQUE(realm_id, bucket_key)` → `BucketKeyDuplicate`
    ///   (400 `bucket_key_duplicate`).
    ///
    /// Matching is intentionally robust to constraint-name drift: in production
    /// the migration assigns the explicit name `uq_credit_buckets_realm_key`
    /// while cloned/restored schemas may derive PostgreSQL's auto-generated
    /// `credit_buckets_realm_id_bucket_key_key`. Both names are accepted, and the
    /// rendered message is inspected as a final fallback so the classification
    /// cannot silently degrade to a 500 if the driver omits `constraint()` or a
    /// future rename occurs.
    fn classify_bucket_insert_error(e: &sqlx::Error, realm_id: &str) -> CreditBucketError {
        let constraint = e.as_database_error().and_then(|db| db.constraint());
        let msg = e.to_string();

        match constraint
            .and_then(classify_bucket_constraint)
            .or_else(|| classify_from_message(&msg))
        {
            Some(BucketConstraintKind::RealmKey) => CreditBucketError::BucketKeyDuplicate {
                realm_id: realm_id.to_string(),
            },
            None => CoreError::DatabaseError(msg).into(),
        }
    }
}

impl BillingRepository for PostgresBillingRepository {
    async fn create_subscription(&self, sub: Subscription) -> Result<Subscription, CoreError> {
        Self::create_subscription_conn(&self.db, sub).await
    }

    async fn find_by_realm_id(&self, realm_id: &str) -> Result<Option<Subscription>, CoreError> {
        let result = subscription::Entity::find()
            .filter(subscription::Column::RealmId.eq(realm_id))
            .one(&self.db)
            .await?;

        result.map(Self::model_to_subscription).transpose()
    }

    async fn find_by_external_subscription_id(
        &self,
        external_sub_id: &str,
        provider: &str,
    ) -> Result<Option<Subscription>, CoreError> {
        Self::find_by_external_subscription_id_conn(&self.db, external_sub_id, provider).await
    }

    async fn find_subscription_by_id(
        &self,
        subscription_id: Uuid,
    ) -> Result<Option<Subscription>, CoreError> {
        let result = subscription::Entity::find_by_id(subscription_id)
            .one(&self.db)
            .await?;

        result.map(Self::model_to_subscription).transpose()
    }

    async fn update_subscription(&self, sub: Subscription) -> Result<Subscription, CoreError> {
        Self::update_subscription_conn(&self.db, sub).await
    }

    async fn create_payment_event(&self, event: PaymentEvent) -> Result<PaymentEvent, CoreError> {
        let model = payment_event::ActiveModel {
            id: Set(event.id),
            realm_id: Set(event.realm_id.clone()),
            external_event_id: Set(event.external_event_id.clone()),
            payment_provider: Set(event.payment_provider.clone()),
            event_type: Set(event.event_type.clone()),
            subscription_id: Set(event.subscription_id),
            payload: Set(event.payload.clone()),
            processed: Set(event.processed),
            processing_started_at: Set(event
                .processing_started_at
                .map(sea_orm::prelude::DateTimeWithTimeZone::from)),
            next_retry_at: Set(None),
            created_at: Set(sea_orm::prelude::DateTimeWithTimeZone::from(
                event.created_at,
            )),
        };

        let result = model.insert(&self.db).await?;
        Ok(Self::model_to_payment_event(result))
    }

    async fn find_payment_event_by_external_id(
        &self,
        realm_id: &str,
        external_event_id: &str,
        payment_provider: &str,
    ) -> Result<Option<PaymentEvent>, CoreError> {
        Ok(payment_event::Entity::find()
            .filter(payment_event::Column::RealmId.eq(realm_id))
            .filter(payment_event::Column::ExternalEventId.eq(external_event_id))
            .filter(payment_event::Column::PaymentProvider.eq(payment_provider))
            .one(&self.db)
            .await?
            .map(Self::model_to_payment_event))
    }

    async fn mark_payment_event_processed(&self, id: Uuid) -> Result<(), CoreError> {
        let existing = payment_event::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        let mut active_model: payment_event::ActiveModel = existing.into_active_model();
        active_model.processed = Set(true);

        active_model.update(&self.db).await?;
        Ok(())
    }

    async fn find_subscription_by_client_app_id(
        &self,
        client_app_id: Uuid,
    ) -> Result<Option<Subscription>, CoreError> {
        Self::find_subscription_by_client_app_id_conn(&self.db, client_app_id).await
    }

    async fn cancel_subscription(
        &self,
        subscription_id: Uuid,
        cancel_at_period_end: bool,
    ) -> Result<Subscription, CoreError> {
        let subscription = subscription::Entity::find_by_id(subscription_id)
            .one(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?
            .ok_or(CoreError::NotFound)?;

        // Extract current_period_end before moving subscription
        let current_period_end = subscription.current_period_end;

        let mut active_model: subscription::ActiveModel = subscription.into_active_model();

        if cancel_at_period_end {
            // Set cancel_at to period_end if it exists
            if current_period_end.is_some() {
                active_model.cancel_at = Set(current_period_end);
            }
            // Set cancel_at_period_end flag
            active_model.cancel_at_period_end = Set(true);
        } else {
            // Immediate cancellation
            active_model.cancel_at = Set(Some(sea_orm::prelude::DateTimeWithTimeZone::from(
                chrono::Utc::now(),
            )));
            active_model.status = Set("canceled".to_string());
        }

        let updated: subscription::Model = active_model
            .update(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Self::model_to_subscription(updated)
    }

    async fn cancel_subscriptions_by_external_id(
        &self,
        realm_id: &str,
        user_id: Uuid,
        external_subscription_id: &str,
    ) -> Result<u64, CoreError> {
        sqlx::query(
            "UPDATE subscription
             SET status = 'canceled', cancel_at = NOW(), updated_at = NOW()
             WHERE realm_id = $1 AND user_id = $2 AND external_subscription_id = $3
               AND status IN ('active', 'trialing', 'past_due', 'scheduled_cancel', 'pending')",
        )
        .bind(realm_id)
        .bind(user_id)
        .bind(external_subscription_id)
        .execute(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))
        .map(|r| r.rows_affected())
    }

    async fn cancel_subscriptions_by_entitlement_key(
        &self,
        realm_id: &str,
        user_id: Uuid,
        entitlement_key: &str,
    ) -> Result<u64, CoreError> {
        sqlx::query(
            "UPDATE subscription
             SET status = 'canceled', cancel_at = NOW(), updated_at = NOW()
             WHERE realm_id = $1 AND user_id = $2 AND entitlement_key = $3
               AND status IN ('active', 'trialing', 'past_due', 'scheduled_cancel', 'pending')",
        )
        .bind(realm_id)
        .bind(user_id)
        .bind(entitlement_key)
        .execute(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))
        .map(|r| r.rows_affected())
    }

    async fn list_active_subscriptions_by_user(
        &self,
        realm_id: &str,
        user_id: Uuid,
    ) -> Result<Vec<Subscription>, CoreError> {
        // In-effect statuses mirror SubscriptionStatus::has_access
        // (active / trialing / scheduled_cancel / dispute). The `status` column
        // stores the lowercase as_str text (see SubscriptionStatus::as_str), so
        // we filter on those exact text values.
        let results = subscription::Entity::find()
            .filter(subscription::Column::RealmId.eq(realm_id))
            .filter(subscription::Column::UserId.eq(user_id))
            .filter(subscription::Column::Status.is_in([
                "active",
                "trialing",
                "scheduled_cancel",
                "dispute",
            ]))
            .all(&self.db)
            .await?;

        results
            .into_iter()
            .map(Self::model_to_subscription)
            .collect()
    }

    async fn save_history_event(
        &self,
        event: SubscriptionHistoryEvent,
    ) -> Result<SubscriptionHistoryEvent, CoreError> {
        Self::save_history_event_conn(&self.db, event).await
    }

    async fn get_subscription_history(
        &self,
        realm_id: &str,
        subscription_id: &Uuid,
    ) -> Result<Vec<SubscriptionHistoryEvent>, CoreError> {
        let results = subscription_history::Entity::find()
            .filter(subscription_history::Column::RealmId.eq(realm_id))
            .filter(subscription_history::Column::SubscriptionId.eq(*subscription_id))
            .order_by_desc(subscription_history::Column::Timestamp)
            .all(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        results
            .into_iter()
            .map(Self::model_to_subscription_history_event)
            .collect()
    }

    async fn list_subscription_history(
        &self,
        realm_id: &str,
        query: SubscriptionHistoryQuery,
    ) -> Result<(Vec<SubscriptionHistoryEvent>, u64), CoreError> {
        use sea_orm::EntityTrait;

        let page = query.page.unwrap_or(1);
        let page_size = query.page_size.unwrap_or(20).min(100); // Max 100 per page

        let mut select = subscription_history::Entity::find()
            .filter(subscription_history::Column::RealmId.eq(realm_id));

        let requires_subscription_join = query.user_id.is_some()
            || query.entitlement_key.is_some()
            || query.subscription_status.is_some();

        if requires_subscription_join {
            select = select.join(
                JoinType::InnerJoin,
                subscription_history::Relation::Subscription.def(),
            );
        }

        // Apply filters

        if let Some(event_type) = query.event_type {
            select = select.filter(subscription_history::Column::EventType.eq(event_type.as_str()));
        }

        if let Some(user_id) = query.user_id {
            select = select.filter(subscription::Column::UserId.eq(user_id));
        }

        if let Some(entitlement_key) = query.entitlement_key {
            select = select.filter(subscription::Column::EntitlementKey.eq(entitlement_key));
        }

        if let Some(subscription_status) = query.subscription_status {
            select = select.filter(subscription::Column::Status.eq(subscription_status));
        }

        if let Some(from_date) = query.from_date {
            select = select.filter(
                subscription_history::Column::Timestamp
                    .gte(sea_orm::prelude::DateTimeWithTimeZone::from(from_date)),
            );
        }

        if let Some(to_date) = query.to_date {
            select = select.filter(
                subscription_history::Column::Timestamp
                    .lte(sea_orm::prelude::DateTimeWithTimeZone::from(to_date)),
            );
        }

        // Get total count
        let total = select
            .clone()
            .count(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        // Apply sorting
        let sort_by = query
            .sort_by
            .unwrap_or_else(|| "timestamp".to_string())
            .to_lowercase();
        let sort_order = query.sort_order.unwrap_or(SortOrder::Desc);

        select = match sort_by.as_str() {
            "timestamp" => {
                if matches!(sort_order, SortOrder::Desc) {
                    select.order_by_desc(subscription_history::Column::Timestamp)
                } else {
                    select.order_by_asc(subscription_history::Column::Timestamp)
                }
            }
            "created_at" => {
                if matches!(sort_order, SortOrder::Desc) {
                    select.order_by_desc(subscription_history::Column::CreatedAt)
                } else {
                    select.order_by_asc(subscription_history::Column::CreatedAt)
                }
            }
            "event_type" => {
                if matches!(sort_order, SortOrder::Desc) {
                    select.order_by_desc(subscription_history::Column::EventType)
                } else {
                    select.order_by_asc(subscription_history::Column::EventType)
                }
            }
            _ => {
                // Default to timestamp DESC
                select.order_by_desc(subscription_history::Column::Timestamp)
            }
        };

        // Apply pagination
        let results = select
            .paginate(&self.db, page_size)
            .fetch_page(page - 1)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        let events = results
            .into_iter()
            .map(Self::model_to_subscription_history_event)
            .collect::<Result<Vec<_>, _>>()?;

        Ok((events, total))
    }

    async fn create_entitlement_mapping(
        &self,
        mapping: EntitlementMapping,
    ) -> Result<EntitlementMapping, CoreError> {
        let active_model = Self::entitlement_mapping_to_active_model(mapping);
        let result = active_model.insert(&self.db).await.map_err(|e| {
            if e.to_string().contains("duplicate key")
                || e.to_string()
                    .contains("provider_entitlement_mappings_realm_id_payment_provider_external_product_id_key")
            {
                CoreError::Conflict("Entitlement mapping already exists for this provider and product".to_string())
            } else {
                CoreError::DatabaseError(e.to_string())
            }
        })?;
        Ok(Self::model_to_entitlement_mapping(result))
    }

    async fn find_entitlement_mapping_by_id(
        &self,
        mapping_id: Uuid,
    ) -> Result<Option<EntitlementMapping>, CoreError> {
        let result = provider_entitlement_mapping::Entity::find_by_id(mapping_id)
            .one(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(result.map(Self::model_to_entitlement_mapping))
    }

    async fn list_entitlement_mappings(
        &self,
        realm_id: &str,
        payment_provider: Option<&str>,
        enabled: Option<bool>,
        page: Option<u64>,
        page_size: Option<u64>,
    ) -> Result<(Vec<EntitlementMapping>, u64), CoreError> {
        let page = page.unwrap_or(1);
        let page_size = page_size.unwrap_or(20).min(100);

        let mut query = provider_entitlement_mapping::Entity::find()
            .filter(provider_entitlement_mapping::Column::RealmId.eq(realm_id));

        if let Some(provider) = payment_provider {
            query =
                query.filter(provider_entitlement_mapping::Column::PaymentProvider.eq(provider));
        }
        if let Some(enabled) = enabled {
            query = query.filter(provider_entitlement_mapping::Column::Enabled.eq(enabled));
        }

        let total = query
            .clone()
            .count(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        let results = query
            .order_by_asc(provider_entitlement_mapping::Column::CreatedAt)
            .paginate(&self.db, page_size)
            .fetch_page(page - 1)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        let mappings = results
            .into_iter()
            .map(Self::model_to_entitlement_mapping)
            .collect();

        Ok((mappings, total))
    }

    async fn update_entitlement_mapping(
        &self,
        mapping: EntitlementMapping,
    ) -> Result<EntitlementMapping, CoreError> {
        let mut tx = self
            .db
            .get_postgres_connection_pool()
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        let mapping = Self::update_mapping_in_tx(&mut tx, mapping).await?;
        tx.commit()
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(mapping)
    }

    async fn upsert_entitlement_mapping(
        &self,
        mapping: EntitlementMapping,
    ) -> Result<EntitlementMapping, CoreError> {
        // Try to find existing by the price-level unique constraint
        // (realm_id, payment_provider, external_product_id, external_price_id).
        // external_price_id IS NULL (Creem) dedups via NULLS NOT DISTINCT
        // (migration `20260607_product_reduce.sql`).
        let existing = self
            .find_entitlement_mapping_by_provider_product_price(
                &mapping.realm_id,
                &mapping.payment_provider,
                &mapping.external_product_id,
                mapping.external_price_id.as_deref(),
            )
            .await?;

        match existing {
            Some(mut existing_mapping) => {
                // Update existing
                existing_mapping.entitlement_key = mapping.entitlement_key;
                existing_mapping.external_price_id = mapping.external_price_id;
                existing_mapping.billing_type = mapping.billing_type;
                existing_mapping.billing_period = mapping.billing_period;
                existing_mapping.service_duration_days = mapping.service_duration_days;
                existing_mapping.enabled = mapping.enabled;
                existing_mapping.provider_product_info = mapping.provider_product_info;
                existing_mapping.granted_role_ids = mapping.granted_role_ids;
                existing_mapping.synced_at = mapping.synced_at;
                existing_mapping.updated_at = chrono::Utc::now();
                self.update_entitlement_mapping(existing_mapping).await
            }
            None => self.create_entitlement_mapping(mapping).await,
        }
    }

    async fn find_entitlement_mapping_by_provider_product_price(
        &self,
        realm_id: &str,
        payment_provider: &str,
        external_product_id: &str,
        external_price_id: Option<&str>,
    ) -> Result<Option<EntitlementMapping>, CoreError> {
        // NULL handling is encapsulated here, not at the call site.
        // Some(price_id) -> external_price_id = $x (Stripe)
        // None          -> external_price_id IS NULL (Creem; dedup via
        //                  NULLS NOT DISTINCT)
        let mut query = provider_entitlement_mapping::Entity::find()
            .filter(provider_entitlement_mapping::Column::RealmId.eq(realm_id))
            .filter(provider_entitlement_mapping::Column::PaymentProvider.eq(payment_provider))
            .filter(
                provider_entitlement_mapping::Column::ExternalProductId.eq(external_product_id),
            );
        query = match external_price_id {
            Some(pid) => {
                query.filter(provider_entitlement_mapping::Column::ExternalPriceId.eq(pid))
            }
            None => query.filter(provider_entitlement_mapping::Column::ExternalPriceId.is_null()),
        };

        let result = query
            .one(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(result.map(Self::model_to_entitlement_mapping))
    }

    async fn find_entitlement_mapping_by_key(
        &self,
        realm_id: &str,
        entitlement_key: &str,
    ) -> Result<Option<EntitlementMapping>, CoreError> {
        let result = provider_entitlement_mapping::Entity::find()
            .filter(provider_entitlement_mapping::Column::RealmId.eq(realm_id))
            .filter(provider_entitlement_mapping::Column::EntitlementKey.eq(entitlement_key))
            .one(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(result.map(Self::model_to_entitlement_mapping))
    }

    async fn find_entitlement_mapping_by_key_price(
        &self,
        realm_id: &str,
        entitlement_key: &str,
        external_price_id: Option<&str>,
    ) -> Result<Option<EntitlementMapping>, CoreError> {
        // NULL handling encapsulated here (symmetric to
        // find_entitlement_mapping_by_provider_product_price).
        let mut query = provider_entitlement_mapping::Entity::find()
            .filter(provider_entitlement_mapping::Column::RealmId.eq(realm_id))
            .filter(provider_entitlement_mapping::Column::EntitlementKey.eq(entitlement_key));
        query = match external_price_id {
            Some(pid) => {
                query.filter(provider_entitlement_mapping::Column::ExternalPriceId.eq(pid))
            }
            None => query.filter(provider_entitlement_mapping::Column::ExternalPriceId.is_null()),
        };

        let result = query
            .one(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(result.map(Self::model_to_entitlement_mapping))
    }

    async fn list_entitlement_mappings_by_provider_product(
        &self,
        realm_id: &str,
        payment_provider: &str,
        external_product_id: &str,
    ) -> Result<Vec<EntitlementMapping>, CoreError> {
        let results = provider_entitlement_mapping::Entity::find()
            .filter(provider_entitlement_mapping::Column::RealmId.eq(realm_id))
            .filter(provider_entitlement_mapping::Column::PaymentProvider.eq(payment_provider))
            .filter(provider_entitlement_mapping::Column::ExternalProductId.eq(external_product_id))
            .all(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(Self::model_to_entitlement_mapping)
            .collect())
    }

    async fn list_one_time_mappings(
        &self,
        realm_id: &str,
    ) -> Result<Vec<EntitlementMapping>, CoreError> {
        use sea_orm::QueryFilter;

        let results = provider_entitlement_mapping::Entity::find()
            .filter(provider_entitlement_mapping::Column::RealmId.eq(realm_id))
            .filter(provider_entitlement_mapping::Column::Enabled.eq(true))
            .filter(provider_entitlement_mapping::Column::BillingType.eq("one_time"))
            .filter(provider_entitlement_mapping::Column::ProviderProductInfo.is_not_null())
            .order_by_asc(provider_entitlement_mapping::Column::CreatedAt)
            .all(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(Self::model_to_entitlement_mapping)
            .collect())
    }

    async fn find_enabled_stripe_mappings_by_entitlement(
        &self,
        realm_id: &str,
        entitlement_key: &str,
    ) -> Result<Vec<EntitlementMapping>, CoreError> {
        use sea_orm::QueryFilter;

        let results = provider_entitlement_mapping::Entity::find()
            .filter(provider_entitlement_mapping::Column::RealmId.eq(realm_id))
            .filter(provider_entitlement_mapping::Column::EntitlementKey.eq(entitlement_key))
            .filter(provider_entitlement_mapping::Column::PaymentProvider.eq("stripe"))
            .filter(provider_entitlement_mapping::Column::Enabled.eq(true))
            .order_by_asc(provider_entitlement_mapping::Column::CreatedAt)
            .all(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(Self::model_to_entitlement_mapping)
            .collect())
    }

    async fn find_external_subscription_id_by_payment_intent(
        &self,
        payment_intent: &str,
        provider: &str,
        realm_id: &str,
    ) -> Result<Option<String>, CoreError> {
        let stripe_subscription_id: Option<String> = sqlx::query_scalar(
            "SELECT payload->'data'->'object'->>'subscription' \
             FROM payment_event \
             WHERE payment_provider = $1 \
               AND realm_id = $3 \
               AND event_type IN ('checkout.session.completed', 'invoice.payment_succeeded') \
               AND payload->'data'->'object'->>'payment_intent' = $2 \
             LIMIT 1",
        )
        .bind(provider)
        .bind(payment_intent)
        .bind(realm_id)
        .fetch_optional(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| {
            CoreError::InternalServerError(format!(
                "Failed to lookup subscription by payment_intent: {}",
                e
            ))
        })?
        .flatten();

        Ok(stripe_subscription_id)
    }

    async fn list_subscriptions(
        &self,
        realm_id: &str,
        entitlement_key: Option<&str>,
        status: Option<&str>,
        payment_provider: Option<&str>,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<Subscription>, u64), CoreError> {
        let page = page.max(1);
        let offset = (page - 1) * page_size;

        // Build dynamic WHERE clause
        let mut conditions = vec!["realm_id = $1".to_string()];
        let mut param_idx = 2u32;

        let entitlement_key_param;
        let status_param;
        let payment_provider_param;

        if let Some(ek) = entitlement_key {
            conditions.push(format!("entitlement_key = ${}", param_idx));
            entitlement_key_param = Some(ek.to_string());
            param_idx += 1;
        } else {
            entitlement_key_param = None;
        }

        if let Some(s) = status {
            conditions.push(format!("status = ${}", param_idx));
            status_param = Some(s.to_string());
            param_idx += 1;
        } else {
            status_param = None;
        }

        if let Some(pp) = payment_provider {
            conditions.push(format!("payment_provider = ${}", param_idx));
            payment_provider_param = Some(pp.to_string());
            param_idx += 1;
        } else {
            payment_provider_param = None;
        }

        let where_clause = conditions.join(" AND ");
        let pool = self.db.get_postgres_connection_pool();

        // Count query
        let count_sql = format!("SELECT COUNT(*) FROM subscription WHERE {}", where_clause);
        let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql).bind(realm_id);
        if let Some(ref ek) = entitlement_key_param {
            count_query = count_query.bind(ek);
        }
        if let Some(ref s) = status_param {
            count_query = count_query.bind(s);
        }
        if let Some(ref pp) = payment_provider_param {
            count_query = count_query.bind(pp);
        }
        let total = count_query.fetch_one(pool).await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to count subscriptions: {}", e))
        })?;

        // Data query
        let data_sql = format!(
            "SELECT id, realm_id, user_id, external_subscription_id, external_product_id, \
             payment_provider, status, entitlement_key, billing_type, external_price_id, provider_metadata, \
             synced_at, current_period_start, current_period_end, cancel_at_period_end, \
             client_app_id, cancel_at, created_at, updated_at \
             FROM subscription WHERE {} ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
            where_clause,
            param_idx,
            param_idx + 1
        );
        let mut data_query = sqlx::query(&data_sql).bind(realm_id);
        if let Some(ref ek) = entitlement_key_param {
            data_query = data_query.bind(ek);
        }
        if let Some(ref s) = status_param {
            data_query = data_query.bind(s);
        }
        if let Some(ref pp) = payment_provider_param {
            data_query = data_query.bind(pp);
        }
        data_query = data_query.bind(page_size as i64).bind(offset as i64);

        let rows = data_query.fetch_all(pool).await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to list subscriptions: {}", e))
        })?;

        let subs: Vec<Subscription> = rows
            .iter()
            .map(|row| {
                use sqlx::Row;
                let status_str: String = row.get("status");
                Ok(Subscription {
                    id: row.get("id"),
                    realm_id: row.get("realm_id"),
                    user_id: row.get("user_id"),
                    external_subscription_id: row.get("external_subscription_id"),
                    external_product_id: row.get("external_product_id"),
                    payment_provider: row.get("payment_provider"),
                    status: status_str.parse()?,
                    entitlement_key: row.get("entitlement_key"),
                    billing_type: {
                        let bt: String = row.get("billing_type");
                        bt.parse()?
                    },
                    external_price_id: row.get("external_price_id"),
                    provider_metadata: row.get("provider_metadata"),
                    synced_at: row.get("synced_at"),
                    current_period_start: row.get("current_period_start"),
                    current_period_end: row.get("current_period_end"),
                    cancel_at_period_end: row.get("cancel_at_period_end"),
                    client_app_id: row.get("client_app_id"),
                    cancel_at: row.get("cancel_at"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                })
            })
            .collect::<Result<Vec<_>, CoreError>>()?;

        Ok((subs, total as u64))
    }

    async fn check_feature_facts(
        &self,
        realm_id: &str,
        pool: &sqlx::PgPool,
    ) -> Result<FeatureFacts, CoreError> {
        use sqlx::Row;

        let row = sqlx::query(
            r#"
            WITH configured_providers AS (
                SELECT 'stripe'
                WHERE EXISTS (
                    SELECT 1 FROM realm_config
                    WHERE realm_id = $1 AND config_type = 'stripe'
                      AND config_key = 'api_key' AND enabled = true
                )
                UNION ALL
                SELECT 'creem'
                WHERE EXISTS (
                    SELECT 1 FROM realm_config
                    WHERE realm_id = $1 AND config_type = 'creem'
                      AND config_key = 'api_key' AND enabled = true
                )
            )
            SELECT
                EXISTS (SELECT 1 FROM configured_providers) AS has_payment_providers,
                EXISTS (SELECT 1 FROM provider_entitlement_mappings WHERE realm_id = $1) AS has_entitlement_mappings,
                EXISTS (SELECT 1 FROM provider_entitlement_mappings WHERE realm_id = $1 AND enabled = true) AS has_enabled_mappings,
                EXISTS (SELECT 1 FROM provider_entitlement_mappings WHERE realm_id = $1 AND billing_type = 'one_time' AND enabled = true) AS has_one_time_mappings,
                EXISTS (SELECT 1 FROM provider_entitlement_mappings WHERE realm_id = $1 AND billing_type = 'recurring' AND enabled = true) AS has_recurring_mappings,
                EXISTS (SELECT 1 FROM invoice_seller_config WHERE realm_id = $1) AS has_invoice_seller_config,
                EXISTS (SELECT 1 FROM invoice WHERE realm_id = $1) AS has_invoices,
                EXISTS (SELECT 1 FROM subscription_history WHERE realm_id = $1) AS has_subscription_history
            "#,
        )
        .bind(realm_id)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to load feature availability facts: {}", e))
        })?;

        Ok(FeatureFacts {
            has_payment_providers: row.get("has_payment_providers"),
            has_entitlement_mappings: row.get("has_entitlement_mappings"),
            has_enabled_mappings: row.get("has_enabled_mappings"),
            has_one_time_mappings: row.get("has_one_time_mappings"),
            has_recurring_mappings: row.get("has_recurring_mappings"),
            has_invoice_seller_config: row.get("has_invoice_seller_config"),
            has_invoices: row.get("has_invoices"),
            has_subscription_history: row.get("has_subscription_history"),
        })
    }

    /// Atomically batch-upsert all price rows for one product.
    ///
    /// Pipeline within a single sqlx transaction:
    /// 1. `SELECT ... FOR UPDATE` the mappings named in `updates` (lock the
    ///    group) and verify every `mapping_id` belongs to the
    ///    `(realm, provider, product)` group — else `MappingNotInGroup` (400).
    /// 2. Active-subscription lock: for every row transitioning
    ///    `enabled` true→false, count access-granting subscriptions anchored to
    ///    that mapping's `(realm, provider, product, external_price_id)`. Any >0
    ///    → roll back the whole tx and return `ActiveSubscriptionLock` (409).
    /// 3. Upsert each row (UPDATE existing in place; the batch is scoped to
    ///    already-synced price rows, so no INSERT path is exercised here).
    /// 4. Re-read the product's full latest price-row set and return it.
    async fn batch_update_mappings(
        &self,
        input: BatchUpdateMappingsInput,
    ) -> Result<BatchUpdateResult, BatchMappingError> {
        use sqlx::Row;

        if input.updates.is_empty() {
            // Nothing to write — return the current full set for the product.
            let prices = self
                .list_entitlement_mappings_by_provider_product(
                    &input.realm_id,
                    &input.payment_provider,
                    &input.external_product_id,
                )
                .await?;
            return Ok(BatchUpdateResult { saved: 0, prices });
        }

        let mut tx = self
            .db
            .get_postgres_connection_pool()
            .begin()
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to begin batch transaction: {}", e))
            })?;

        // 1. Lock + ownership check for every requested mapping_id.
        let requested_ids: Vec<Uuid> = input.updates.iter().map(|u| u.mapping_id).collect();
        let rows = sqlx::query(
            "SELECT id, external_price_id, enabled \
             FROM provider_entitlement_mappings \
             WHERE realm_id = $1 AND payment_provider = $2 AND external_product_id = $3 \
               AND id = ANY($4) FOR UPDATE",
        )
        .bind(&input.realm_id)
        .bind(&input.payment_provider)
        .bind(&input.external_product_id)
        .bind(&requested_ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to lock batch mappings: {}", e)))?;

        if rows.len() != input.updates.len() {
            // Identify the first offender for a precise error.
            let found: std::collections::HashSet<Uuid> =
                rows.iter().map(|r| r.get::<Uuid, _>("id")).collect();
            let offender = input
                .updates
                .iter()
                .map(|u| u.mapping_id)
                .find(|id| !found.contains(id))
                .unwrap_or_else(|| input.updates[0].mapping_id);
            let _ = tx.rollback().await;
            return Err(BatchMappingError::MappingNotInGroup {
                mapping_id: offender,
                provider: input.payment_provider.clone(),
                product: input.external_product_id.clone(),
            });
        }

        // Index current state by id for diffing.
        let mut current_by_id: std::collections::HashMap<Uuid, (Option<String>, bool)> =
            std::collections::HashMap::with_capacity(input.updates.len());
        for r in rows {
            current_by_id.insert(
                r.get::<Uuid, _>("id"),
                (
                    r.get::<Option<String>, _>("external_price_id"),
                    r.get::<bool, _>("enabled"),
                ),
            );
        }

        // 2. Active-subscription lock: sum access-granting subscriptions across
        // every row that transitions enabled true→false. Any >0 rolls back the
        // WHOLE batch.
        // Collect both the non-null price ids (Stripe) and whether any
        // disabling row is price-less (Creem, NULL external_price_id). The
        // count query ORs the two matching branches so NULL-price subscriptions
        // are covered without a sentinel.
        let mut disabling_price_ids: Vec<String> = Vec::new();
        let mut disabling_includes_null_price = false;
        for u in &input.updates {
            let Some(false) = u.enabled else {
                continue;
            };
            let Some((price_id, was_enabled)) = current_by_id.get(&u.mapping_id) else {
                continue;
            };
            if *was_enabled {
                match price_id {
                    Some(pid) => disabling_price_ids.push(pid.clone()),
                    None => disabling_includes_null_price = true,
                }
            }
        }
        if !disabling_price_ids.is_empty() || disabling_includes_null_price {
            // Access-granting statuses mirror SubscriptionStatus::has_access().
            // Branch 1: subscriptions whose external_price_id is one of the
            // disabling non-null price ids. Branch 2 (only when a disabling row
            // is price-less): subscriptions with NULL external_price_id.
            let active_count: i64 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM subscription \
                 WHERE realm_id = $1 \
                   AND payment_provider = $2 \
                   AND external_product_id = $3 \
                   AND status IN ({ACCESS_GRANTING_SUBSCRIPTION_STATUSES_SQL}) \
                   AND ( \
                        external_price_id = ANY($4) \
                     OR ($5 AND external_price_id IS NULL) \
                   )"
            ))
            .bind(&input.realm_id)
            .bind(&input.payment_provider)
            .bind(&input.external_product_id)
            .bind(&disabling_price_ids)
            .bind(disabling_includes_null_price)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!(
                    "Failed to count active subscriptions for batch: {}",
                    e
                ))
            })?;
            if active_count > 0 {
                let _ = tx.rollback().await;
                return Err(BatchMappingError::ActiveSubscriptionLock {
                    provider: input.payment_provider.clone(),
                    product: input.external_product_id.clone(),
                    active_subscriptions: active_count,
                });
            }
        }

        // 3. Upsert (UPDATE) each row in tx order. Fields the client omits
        // (`None`) are preserved via COALESCE — matches the single-PATCH contract
        // (entitlement_mapping_handlers.rs `update_entitlement_mapping`).
        // Points distribution rules are owned by each mapping; when an update
        // carries `point_rules`, the rule set is upserted (in this same
        // transaction) after the mapping base row is written, via the shared
        // rule-upsert helper. The old scalar credit columns are no longer written here.
        let now = chrono::Utc::now();
        let mut saved: u32 = 0;
        // Collect (mapping_id, rules) pairs for the in-tx rule upsert after the
        // base rows are written.
        let mut rule_upserts: Vec<(Uuid, Vec<RuleUpsert>)> = Vec::new();
        for u in &input.updates {
            let billing_type_str = u.billing_type.as_deref();
            // `granted_role_ids` is a `UUID[]` column (NOT COALESCE'd: the caller
            // must be able to CLEAR to `{}` by passing `Some([])`). `None` ⟺ leave
            // unchanged (column omitted from SET). sqlx encodes `Vec<Uuid>` →
            // `uuid[]` (matches the account `provider_ids` path).
            let granted_role_ids_value: Option<Vec<Uuid>> =
                u.granted_role_ids.as_ref().map(|ids| ids.to_vec());

            // Build the UPDATE per-row with sqlx::QueryBuilder so placeholder
            // numbering is automatic. COALESCE preserves the DB value when the
            // caller leaves a field unchanged (`None`); `granted_role_ids` is SET
            // explicitly only when provided, so the caller can CLEAR it.
            let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                "UPDATE provider_entitlement_mappings SET billing_type = COALESCE(",
            );
            qb.push_bind(billing_type_str.map(|s| s.to_string()));
            qb.push(", billing_type), enabled = COALESCE(");
            qb.push_bind(u.enabled);
            qb.push(", enabled)");
            if let Some(role_ids) = granted_role_ids_value {
                qb.push(", granted_role_ids = ");
                qb.push_bind(role_ids);
            }
            qb.push(", updated_at = ");
            qb.push_bind(now);
            qb.push(" WHERE realm_id = ");
            qb.push_bind(input.realm_id.clone());
            qb.push(" AND payment_provider = ");
            qb.push_bind(input.payment_provider.clone());
            qb.push(" AND external_product_id = ");
            qb.push_bind(input.external_product_id.clone());
            qb.push(" AND id = ");
            qb.push_bind(u.mapping_id);

            let result = qb.build().execute(&mut *tx).await.map_err(|e| {
                CoreError::DatabaseError(format!("Failed to update mapping in batch: {}", e))
            })?;
            saved += result.rows_affected() as u32;

            if let Some(rules) = u.point_rules.clone() {
                rule_upserts.push((u.mapping_id, rules));
            }
        }

        // 4. Upsert each row's rule set within the same transaction.
        for (mapping_id, rules) in rule_upserts {
            Self::upsert_rules_in_tx(
                &mut tx,
                &input.realm_id,
                DistributionRuleOwner::EntitlementMapping(mapping_id),
                rules,
            )
            .await
            .map_err(BatchMappingError::Other)?;
        }

        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit batch_update_mappings: {}", e))
        })?;

        // 5. Re-read the product's full latest price-row set.
        let prices = self
            .list_entitlement_mappings_by_provider_product(
                &input.realm_id,
                &input.payment_provider,
                &input.external_product_id,
            )
            .await?;
        Ok(BatchUpdateResult { saved, prices })
    }

    // ===== Distribution Rules =====

    async fn create_entitlement_mapping_with_rules(
        &self,
        mapping: EntitlementMapping,
        rules: Vec<RuleUpsert>,
    ) -> Result<EntitlementMapping, CoreError> {
        let mut tx = self
            .db
            .get_postgres_connection_pool()
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        // Insert the mapping base row ON the transaction so a subsequent
        // rule-write failure rolls it back (DEC-005 atomic upsert). Raw sqlx on
        // `&mut *tx` mirrors upsert_rules_in_tx / batch_update_mappings; SeaORM
        // `.insert()` cannot bind to a raw sqlx transaction (the prior code
        // escaped the tx via `&self.db`, breaking atomicity).
        let row = sqlx::query(
            "INSERT INTO provider_entitlement_mappings \
             (id, realm_id, payment_provider, external_product_id, external_price_id, \
              entitlement_key, billing_type, billing_period, service_duration_days, \
              enabled, provider_product_info, granted_role_ids, synced_at, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) \
             RETURNING *",
        )
        .bind(mapping.id)
        .bind(&mapping.realm_id)
        .bind(&mapping.payment_provider)
        .bind(&mapping.external_product_id)
        .bind(&mapping.external_price_id)
        .bind(&mapping.entitlement_key)
        .bind(
            mapping
                .billing_type
                .as_ref()
                .map(|t| t.as_str().to_string()),
        )
        .bind(&mapping.billing_period)
        .bind(mapping.service_duration_days.map(|v| v as i32))
        .bind(mapping.enabled)
        .bind(&mapping.provider_product_info)
        .bind(&mapping.granted_role_ids)
        .bind(mapping.synced_at)
        .bind(mapping.created_at)
        .bind(mapping.updated_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            if e.to_string().contains("duplicate key") {
                CoreError::Conflict(
                    "Entitlement mapping already exists for this provider and product".to_string(),
                )
            } else {
                CoreError::DatabaseError(e.to_string())
            }
        })?;
        let mapping = Self::row_to_entitlement_mapping(&row);
        // Upsert the rule set under the new mapping id.
        Self::upsert_rules_in_tx(
            &mut tx,
            &mapping.realm_id,
            DistributionRuleOwner::EntitlementMapping(mapping.id),
            rules,
        )
        .await?;
        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!(
                "Failed to commit create_entitlement_mapping_with_rules: {}",
                e
            ))
        })?;
        Ok(mapping)
    }

    async fn upsert_mapping_with_rules(
        &self,
        realm_id: &str,
        mapping: EntitlementMapping,
        rules: Vec<RuleUpsert>,
    ) -> Result<EntitlementMapping, CoreError> {
        let mut tx = self
            .db
            .get_postgres_connection_pool()
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        let mapping = Self::update_mapping_in_tx(&mut tx, mapping).await?;
        // Upsert the rule set under the mapping id.
        Self::upsert_rules_in_tx(
            &mut tx,
            realm_id,
            DistributionRuleOwner::EntitlementMapping(mapping.id),
            rules,
        )
        .await?;
        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit upsert_mapping_with_rules: {}", e))
        })?;
        Ok(mapping)
    }

    async fn find_mapping_rules(
        &self,
        realm_id: &str,
        mapping_id: Uuid,
    ) -> Result<Vec<PointsDistributionRule>, CoreError> {
        let rules = points_distribution_rule::Entity::find()
            .filter(points_distribution_rule::Column::RealmId.eq(realm_id))
            .filter(points_distribution_rule::Column::OwnerType.eq("entitlement_mapping"))
            .filter(points_distribution_rule::Column::EntitlementMappingId.eq(mapping_id))
            .order_by_asc(points_distribution_rule::Column::DisplayOrder)
            .order_by_asc(points_distribution_rule::Column::Id)
            .all(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(rules.into_iter().map(Self::rule_from_model).collect())
    }

    async fn upsert_registration_rules(
        &self,
        realm_id: &str,
        rules: Vec<RuleUpsert>,
    ) -> Result<Vec<PointsDistributionRule>, CoreError> {
        let mut tx = self
            .db
            .get_postgres_connection_pool()
            .begin()
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Self::upsert_rules_in_tx(
            &mut tx,
            realm_id,
            DistributionRuleOwner::RealmRegistration,
            rules,
        )
        .await?;
        tx.commit().await.map_err(|e| {
            CoreError::DatabaseError(format!("Failed to commit upsert_registration_rules: {}", e))
        })?;
        // Return the full current registration rule set, ordered stably.
        self.find_registration_rules(realm_id).await
    }

    async fn find_registration_rules(
        &self,
        realm_id: &str,
    ) -> Result<Vec<PointsDistributionRule>, CoreError> {
        let rules = points_distribution_rule::Entity::find()
            .filter(points_distribution_rule::Column::RealmId.eq(realm_id))
            .filter(points_distribution_rule::Column::OwnerType.eq("realm_registration"))
            .order_by_asc(points_distribution_rule::Column::DisplayOrder)
            .order_by_asc(points_distribution_rule::Column::Id)
            .all(&self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(rules.into_iter().map(Self::rule_from_model).collect())
    }
}

/// Projected `points_distribution_rules` write columns for a
/// [`DistributionPolicy`]: `(grant_mode, points_amount, validity_days,
/// grant_period_type, quota_windows)`.
type PolicyColumns = (
    String,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<serde_json::Value>,
);

/// Which `credit_buckets` uniqueness violation a Postgres error refers to.
///
/// See `classify_bucket_insert_error` for the full rationale; this enum is the
/// pure (constraint-name → kind) projection so it can be unit-tested without a
/// live database.
enum BucketConstraintKind {
    /// `UNIQUE(realm_id, bucket_key)` collision → 400 `bucket_key_duplicate`.
    RealmKey,
}

/// Map a Postgres constraint/index name to its bucket-error kind.
///
/// Accepts both the migration-assigned explicit names (`uq_credit_buckets_*`)
/// and PostgreSQL's auto-generated names (`credit_buckets_*`) produced when the
/// schema is cloned via `CREATE TABLE ... (LIKE ... INCLUDING ALL)` or restored
/// by pg_dump without preserving constraint names. This keeps
/// `classify_bucket_insert_error` stable across schema-name drift.
///
fn classify_bucket_constraint(constraint: &str) -> Option<BucketConstraintKind> {
    // Migration-assigned explicit names.
    if constraint == "uq_credit_buckets_realm_key" {
        return Some(BucketConstraintKind::RealmKey);
    }
    // PostgreSQL auto-generated names. `<table>_<cols>_key` is the default for
    // an unnamed UNIQUE constraint.
    match constraint {
        "credit_buckets_realm_id_bucket_key_key" => Some(BucketConstraintKind::RealmKey),
        _ => None,
    }
}

/// Last-resort classification from the rendered Postgres error message.
///
/// Used when the driver omits `constraint()` (older sqlx/PG combos) or when an
/// unforeseen constraint name appears. Matches the name family quoted inside
/// the standard `duplicate key value violates unique constraint "<name>"` text.
fn classify_from_message(msg: &str) -> Option<BucketConstraintKind> {
    let is_dup = msg.contains("duplicate key value violates unique constraint")
        || msg.contains("duplicate key value");
    if !is_dup {
        return None;
    }
    if msg.contains("uq_credit_buckets_realm_key")
        || msg.contains("credit_buckets_realm_id_bucket_key")
    {
        return Some(BucketConstraintKind::RealmKey);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard: `credit_buckets` uniqueness violations must classify
    /// into `BucketKeyDuplicate` (→ 400) regardless of whether the runtime
    /// schema carries the migration-assigned explicit constraint name or
    /// PostgreSQL's auto-generated name (the latter appear when the schema is
    /// cloned via `CREATE TABLE ... (LIKE ... INCLUDING ALL)`, e.g. the test
    /// harness, and historically caused a silent 500 regression).
    #[test]
    fn classifies_bucket_constraint_names_for_both_name_families() {
        // Migration-assigned explicit name (production schema).
        let explicit = classify_bucket_constraint("uq_credit_buckets_realm_key");
        assert!(
            matches!(explicit, Some(BucketConstraintKind::RealmKey)),
            "explicit realm_key name must classify as RealmKey"
        );

        // PostgreSQL auto-generated name (cloned/restored schema — the actual
        // runtime name observed in the failing scenarios).
        let auto = classify_bucket_constraint("credit_buckets_realm_id_bucket_key_key");
        assert!(
            matches!(auto, Some(BucketConstraintKind::RealmKey)),
            "auto-named realm+bucket_key unique must classify as RealmKey"
        );

        // Unrelated names must NOT be force-classified (they fall through to a
        // generic DB error → 500), so this guard also catches over-eager
        // matchers that would mask unrelated failures.
        assert!(
            classify_bucket_constraint("credit_buckets_pkey").is_none(),
            "primary-key violations must not be misclassified"
        );
    }

    /// The message-based fallback must catch the collision when the driver
    /// omits `constraint()` (older sqlx/PG combos) — using the real runtime
    /// error text observed in the regression.
    #[test]
    fn classifies_from_runtime_duplicate_key_messages() {
        let realm_key_msg = "error returned from database: duplicate key value \
             violates unique constraint \"credit_buckets_realm_id_bucket_key_key\"";
        assert!(matches!(
            classify_from_message(realm_key_msg),
            Some(BucketConstraintKind::RealmKey)
        ));

        // A non-duplicate error must not be classified.
        assert!(
            classify_from_message("relation does not exist").is_none(),
            "non-duplicate errors must not be force-classified"
        );
    }
}
