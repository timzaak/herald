// PostgreSQL implementation of Points Repository

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
use sqlx::{FromRow, PgPool, Postgres, Row, Transaction};
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::points::{
    DistributionEvent, DistributionGrantResult, DistributionPolicy, DistributionRuleOwner,
    DistributionRuleSelection, DistributionTrigger, PointsDistributionRule, ReplayResultRows,
    credit_pair_for_trigger, fold_replay_results, quota_source_type_for_trigger,
    select_and_sort_rules,
};
use herald_domain::points::{
    dtos::RevokePointsOutput,
    entities::{
        ConsumptionAllocationView, CreditLedgerStatus, CreditSourceType, CreditType, Paginated,
        PointsConsumptionAllocation, PointsCreditLedger, PointsQuotaEntitlement,
        PointsRevocationRecord, PointsTransaction, PointsWallet, QuotaWindow, RevocationType,
        TransactionType, WalletStatus,
    },
    errors::PointsErrorExt,
    expiration_service::ExpirationSummary,
    ports::{
        LedgerFilters, LedgerUpdate, PointsRepository, ReclaimLocator, TransactionFilters,
        WalletDelta, WalletFilters,
    },
    service::{MixedConsumePlan, plan_mixed_consume},
};
// Import mapping functions for ORM conversions
use crate::points::{
    points_consumption_allocation_from_model, points_credit_ledger_from_model,
    points_credit_ledger_to_active_model, points_revocation_record_from_model,
    points_revocation_record_to_active_model,
};
use herald_entity::{
    account, points_consumption_allocation, points_credit_ledger, points_grant_schedule,
    points_transaction, points_wallet,
};

/// Custom struct for SQLx query results from points_wallets table
/// Implements FromRow to work with sqlx::query_as
/// The 5 per-type balance fields and
/// `total_balance` are gone; only the 4 lifetime analytics columns remain.
#[derive(Debug, FromRow)]
struct PointsWalletRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub realm_id: String,
    pub bucket_id: Uuid,
    pub total_topup_granted: i64,
    pub total_subscription_granted: i64,
    pub total_recharged: i64,
    pub total_consumed: i64,
    pub status: String,
    pub created_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub updated_at: sea_orm::prelude::DateTimeWithTimeZone,
}

/// Custom struct for SQLx query results from points_transactions table
/// Implements FromRow to work with sqlx::query_as
#[allow(dead_code)]
#[derive(Debug, FromRow)]
struct PointsTransactionRow {
    pub id: Uuid,
    pub realm_id: String,
    pub wallet_id: Uuid,
    pub user_id: Uuid,
    pub bucket_id: Uuid,
    pub r#type: String,
    pub amount: i64,
    pub balance_after: i64,
    pub topup_balance_after: Option<i64>,
    pub subscription_balance_after: Option<i64>,
    pub credit_type: Option<String>,
    pub description: Option<String>,
    pub client_app_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub external_ref_id: Option<String>,
    pub correlation_id: Option<String>,
    /// Sourced via LEFT JOIN to `points_credit_ledger.effective_at` for
    /// grant-type transactions. `None`
    /// for consume/refund/revoke/expiration or when the ledger row's
    /// `effective_at` is NULL (immediately available). The domain
    /// `PointsTransaction.effective_at` is populated from this column.
    /// `#[sqlx(default)]` so `SELECT * FROM points_transactions` (which cannot
    /// see the JOIN column) still deserializes — the value defaults to `None`
    /// when absent, which is the correct semantics for the write-side replay
    /// paths (`replay_consume_by_primary`, refund idempotency check) that do
    /// not need effective_at. Only the admin/audit `find_transactions` read
    /// path supplies the column via an explicit LEFT JOIN.
    #[sqlx(default)]
    pub effective_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    #[sqlx(default)]
    pub distribution_event_id: Option<Uuid>,
    #[sqlx(default)]
    pub distribution_rule_id: Option<Uuid>,
    pub created_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub updated_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub expires_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
}

#[derive(Debug, FromRow)]
struct PointsCreditLedgerRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub realm_id: String,
    pub bucket_id: Uuid,
    pub credit_type: String,
    pub source_type: String,
    pub source_id: String,
    pub granted_amount: i64,
    pub used_amount: i64,
    pub revoked_amount: i64,
    pub remaining_amount: i64,
    pub expires_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    /// Expected effective time. NULL ⟺ immediately
    /// available; non-null ⟺ gated by the `(effective_at IS NULL OR
    /// effective_at <= NOW())` predicate in consumption selection and derived
    /// balance.
    pub effective_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    pub status: String,
    /// Distribution attribution. `#[sqlx(default)]` so legacy `SELECT`
    /// projections that predate the columns still deserialize to NULL rather
    /// than erroring (the write paths populate them).
    #[sqlx(default)]
    pub distribution_event_id: Option<Uuid>,
    #[sqlx(default)]
    pub distribution_rule_id: Option<Uuid>,
    pub created_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub updated_at: sea_orm::prelude::DateTimeWithTimeZone,
}

/// Row captured during pre-grant reclaim (BE-TR production fix). `yanked` is
/// the row's pre-update `remaining_amount` — the unused portion this reclaim
/// moves into `revoked_amount`. `points_credit_ledger.remaining_amount` is a
/// GENERATED column (`granted_amount - used_amount - revoked_amount`), so it
/// regenerates to 0 the instant `revoked_amount` increases; reading it back
/// from `RETURNING *` would yield 0 and the shortfall record would be written
/// with `revoked_amount = 0`, violating
/// `points_revocation_records.revoked_amount > 0`. The reclaim UPDATE uses a
/// CTE that locks + captures the pre-update value so the debt record carries
/// the real yanked portion.
#[derive(Debug, FromRow)]
struct ReclaimTargetRow {
    id: Uuid,
    user_id: Uuid,
    realm_id: String,
    source_id: String,
    used_amount: i64,
    yanked: i64,
}

/// `points_quota_entitlements` sqlx row. Mirrors `PointsCreditLedgerRow`'s
/// style: timestamps as `DateTimeWithTimeZone`, enums as raw strings parsed
/// in the domain converter, and `quota_windows` read as a raw JSON value
#[derive(Debug, FromRow)]
struct PointsQuotaEntitlementRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub realm_id: String,
    pub bucket_id: Uuid,
    pub credit_type: String,
    pub source_type: String,
    pub source_id: String,
    pub quota_windows: serde_json::Value,
    pub effective_from: sea_orm::prelude::DateTimeWithTimeZone,
    pub effective_until: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    pub status: String,
    pub idempotency_key: String,
    #[sqlx(default)]
    pub distribution_event_id: Option<Uuid>,
    #[sqlx(default)]
    pub distribution_rule_id: Option<Uuid>,
    pub created_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub updated_at: sea_orm::prelude::DateTimeWithTimeZone,
}

/// JSONB serde boundary for `quota_windows`. The DB column is
/// `[{windowSeconds, limit, key}]` (camelCase); the domain `QuotaWindow` is
/// snake_case Rust. This struct isolates the rename at the infra boundary so
/// the domain entity stays as defined (no DB-driven serde attrs on
/// the entity). Mirrors how `client/mod.rs` round-trips `redirect_uris`.
/// `pub(crate)` so the billing infra repository can reuse the same boundary for
/// `provider_entitlement_mappings.quota_windows` instead of forking a
/// second camelCase↔snake_case mapping.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct QuotaWindowDbJson {
    #[serde(rename = "windowSeconds")]
    pub(crate) window_seconds: i64,
    pub(crate) limit: i64,
    pub(crate) key: String,
}

/// Parse a raw JSONB value into the domain `Vec<QuotaWindow>`. `None` /
/// `Null` ⟹ empty vec (the column is nullable; empty means "no window-model
/// grant"). Shares the `QuotaWindowDbJson` serde boundary
/// with the `points_quota_entitlements.quota_windows` path.
/// `pub(crate)` so the billing infra repository reuses it for
/// `provider_entitlement_mappings.quota_windows`.
pub(crate) fn parse_quota_windows_value(
    raw: Option<serde_json::Value>,
) -> Result<Vec<QuotaWindow>, CoreError> {
    match raw {
        None | Some(serde_json::Value::Null) => Ok(Vec::new()),
        Some(value) => {
            let windows: Vec<QuotaWindowDbJson> = serde_json::from_value(value).map_err(|e| {
                CoreError::DatabaseError(format!("invalid free_periodic_quota_windows JSONB: {e}"))
            })?;
            Ok(windows
                .into_iter()
                .map(|w| QuotaWindow {
                    window_seconds: w.window_seconds,
                    limit: w.limit,
                    key: w.key,
                })
                .collect())
        }
    }
}

/// Serialize a `Vec<QuotaWindow>` into the JSONB column shape
/// Returns `None` for an empty slice so
/// the stored value stays SQL `NULL` (consistent with the read-path
/// `parse_quota_windows_value` treating `None`/`Null` as "no window grant").
/// `pub(crate)` so the billing infra repository reuses it for
/// `provider_entitlement_mappings.quota_windows`.
pub(crate) fn serialize_quota_windows_value(
    windows: &[QuotaWindow],
) -> Result<Option<serde_json::Value>, CoreError> {
    if windows.is_empty() {
        return Ok(None);
    }
    let mapped: Vec<QuotaWindowDbJson> = windows
        .iter()
        .map(|w| QuotaWindowDbJson {
            window_seconds: w.window_seconds,
            limit: w.limit,
            key: w.key.clone(),
        })
        .collect();
    serde_json::to_value(mapped).map(Some).map_err(|e| {
        CoreError::DatabaseError(format!("serialize free_periodic_quota_windows: {e}"))
    })
}

/// Pure output of [`PostgresPointsRepository::plan_consume_allocation`].
/// Describes how a consume request splits across the locked ledgers, independent
/// of wallet/transaction writes so it can be unit-tested without Postgres.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ConsumePlan {
    allocations: Vec<PlannedAllocation>,
    fully_covers: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedAllocation {
    /// Index into the ledgers slice passed to `plan_consume_allocation`.
    ledger_index: usize,
    amount: i64,
}

/// Pure split of a single bucket's `window_part` across the two window
/// credit_types, per the priority:
///
/// - `subscription_credit` is consumed first (`sub_part`),
/// - `free_periodic_credit` makes up the remainder (`free_part`).
///
/// Inputs are the per-credit_type window spendables (each = `min over active
/// windows of (limit − used)`). Output is the per-type consume amounts plus the
/// residual `window_remainder` that could not be covered by this bucket's
/// windows and must overflow to the next bucket (or to the pool side, which is
/// already accounted for via the wholesale `pool_avail`).
///
/// This is deliberately free of DB / wallet concerns so the priority ordering,
/// the `sub_part ≤ subscription_spendable` / `free_part ≤ free_spendable`
/// overspend invariants and the exact `sub_part + free_part ≤ window_part`
/// accounting can be unit-tested without Postgres (mirrors
/// `plan_consume_allocation`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowCreditSplit {
    sub_part: i64,
    free_part: i64,
    /// Part of `window_part` this bucket's windows could not absorb
    /// (`window_part - sub_part - free_part`); overflowed to the next bucket.
    window_remainder: i64,
}

/// PostgreSQL implementation of PointsRepository
pub struct PostgresPointsRepository {
    db: Arc<DatabaseConnection>,
    pool: PgPool,
}

/// Unique constraint names in the database
mod constraints {
    pub const UK_POINTS_WALLETS_USER_ID: &str = "uk_points_wallets_user_id";
}

impl PostgresPointsRepository {
    fn proportional_refund_for_grant(
        granted_amount: i64,
        remaining_amount: i64,
        refund_amount: i64,
        original_payment_amount: i64,
    ) -> i64 {
        let rounded = (i128::from(granted_amount) * i128::from(refund_amount)
            + i128::from(original_payment_amount) / 2)
            / i128::from(original_payment_amount);
        i64::try_from(rounded)
            .unwrap_or(i64::MAX)
            .min(remaining_amount)
    }

    pub fn new(db: Arc<DatabaseConnection>, pool: PgPool) -> Self {
        Self { db, pool }
    }

    /// Convert database model to domain PointsWallet
    fn model_to_points_wallet(model: points_wallet::Model) -> Result<PointsWallet, CoreError> {
        // Balance columns gone; analytics only.
        Ok(PointsWallet {
            id: model.id,
            user_id: model.user_id,
            realm_id: model.realm_id,
            bucket_id: Some(model.bucket_id),
            total_topup_granted: model.total_topup_granted,
            total_subscription_granted: model.total_subscription_granted,
            total_recharged: model.total_recharged,
            total_consumed: model.total_consumed,
            status: model.status.parse()?,
            created_at: chrono::DateTime::from(model.created_at),
            updated_at: chrono::DateTime::from(model.updated_at),
        })
    }

    /// Convert PointsWalletRow to domain PointsWallet
    fn row_to_points_wallet(row: PointsWalletRow) -> Result<PointsWallet, CoreError> {
        use herald_domain::points::entities::WalletStatus;

        let status = WalletStatus::from_str(&row.status)
            .map_err(|_| CoreError::BadRequest(format!("Invalid wallet status: {}", row.status)))?;

        Ok(PointsWallet {
            id: row.id,
            user_id: row.user_id,
            realm_id: row.realm_id,
            bucket_id: Some(row.bucket_id),
            total_topup_granted: row.total_topup_granted,
            total_subscription_granted: row.total_subscription_granted,
            total_recharged: row.total_recharged,
            total_consumed: row.total_consumed,
            status,
            created_at: chrono::DateTime::from(row.created_at),
            updated_at: chrono::DateTime::from(row.updated_at),
        })
    }

    /// Convert domain PointsWallet to database active model
    fn points_wallet_to_active_model(account: PointsWallet) -> points_wallet::ActiveModel {
        points_wallet::ActiveModel {
            id: Set(account.id),
            user_id: Set(account.user_id),
            realm_id: Set(account.realm_id.clone()),
            bucket_id: Set(account
                .bucket_id
                .expect("bucket_id is required for wallet persistence")),
            total_topup_granted: Set(account.total_topup_granted),
            total_subscription_granted: Set(account.total_subscription_granted),
            total_recharged: Set(account.total_recharged),
            total_consumed: Set(account.total_consumed),
            status: Set(account.status.as_str().to_string()),
            created_at: Set(sea_orm::prelude::DateTimeWithTimeZone::from(
                account.created_at,
            )),
            updated_at: Set(sea_orm::prelude::DateTimeWithTimeZone::from(
                account.updated_at,
            )),
        }
    }

    /// Convert database model to domain PointsTransaction
    fn model_to_points_transaction(
        model: points_transaction::Model,
    ) -> Result<PointsTransaction, CoreError> {
        use herald_domain::points::entities::CreditType;

        let credit_type = match model.credit_type {
            Some(ref ct) => Some(CreditType::from_str(ct)?),
            None => None,
        };

        Ok(PointsTransaction {
            id: model.id,
            wallet_id: model.wallet_id,
            user_id: model.user_id,
            realm_id: model.realm_id,
            bucket_id: model.bucket_id,
            transaction_type: model.r#type.parse()?,
            amount: model.amount,
            balance_after: model.balance_after,
            topup_balance_after: model.topup_balance_after,
            subscription_balance_after: model.subscription_balance_after,
            credit_type,
            description: model.description,
            client_app_id: model.client_app_id,
            subscription_id: model.subscription_id,
            external_ref_id: model.external_ref_id,
            correlation_id: model.correlation_id,
            // SeaORM `points_transaction::Model` has no `effective_at` column
            // (the value lives on `points_credit_ledger`); callers needing it
            // must use the raw-SQL path (`points_transaction_row_to_domain`)
            // which JOINs the ledger. None here is correct for SeaORM-sourced
            // reads (e.g. `find_transaction_by_id`); the admin/audit
            // `list_transactions` path uses the JOIN-aware raw SQL.
            effective_at: None,
            created_at: chrono::DateTime::from(model.created_at),
            distribution_event_id: model.distribution_event_id,
            distribution_rule_id: model.distribution_rule_id,
        })
    }

    /// Convert SQLx query result row to domain PointsTransaction
    fn points_transaction_row_to_domain(
        row: PointsTransactionRow,
    ) -> Result<PointsTransaction, CoreError> {
        use herald_domain::points::entities::CreditType;

        let credit_type = match row.credit_type {
            Some(ref ct) => Some(CreditType::from_str(ct)?),
            None => None,
        };

        Ok(PointsTransaction {
            id: row.id,
            wallet_id: row.wallet_id,
            user_id: row.user_id,
            realm_id: row.realm_id,
            bucket_id: row.bucket_id,
            transaction_type: row.r#type.parse()?,
            amount: row.amount,
            balance_after: row.balance_after,
            topup_balance_after: row.topup_balance_after,
            subscription_balance_after: row.subscription_balance_after,
            credit_type,
            description: row.description,
            client_app_id: row.client_app_id,
            subscription_id: row.subscription_id,
            external_ref_id: row.external_ref_id,
            correlation_id: row.correlation_id.clone(),
            // Part B: sourced via LEFT JOIN to
            // `points_credit_ledger.effective_at` in the raw-SQL queries that
            // populate this row (see `find_transactions`, refund replay).
            effective_at: row.effective_at.map(chrono::DateTime::from),
            created_at: chrono::DateTime::from(row.created_at),
            distribution_event_id: row.distribution_event_id,
            distribution_rule_id: row.distribution_rule_id,
        })
    }

    /// Convert domain PointsTransaction to database active model
    fn points_transaction_to_active_model(
        transaction: PointsTransaction,
    ) -> points_transaction::ActiveModel {
        let credit_type_str = transaction
            .credit_type
            .as_ref()
            .map(|ct| ct.as_str().to_string());

        points_transaction::ActiveModel {
            id: Set(transaction.id),
            wallet_id: Set(transaction.wallet_id),
            user_id: Set(transaction.user_id),
            realm_id: Set(transaction.realm_id),
            bucket_id: Set(transaction.bucket_id),
            r#type: Set(transaction.transaction_type.as_str().to_string()),
            amount: Set(transaction.amount),
            balance_after: Set(transaction.balance_after),
            topup_balance_after: Set(transaction.topup_balance_after),
            subscription_balance_after: Set(transaction.subscription_balance_after),
            credit_type: Set(credit_type_str),
            description: Set(transaction.description),
            client_app_id: Set(transaction.client_app_id),
            subscription_id: Set(transaction.subscription_id),
            external_ref_id: Set(transaction.external_ref_id),
            correlation_id: Set(transaction.correlation_id),
            created_at: Set(sea_orm::prelude::DateTimeWithTimeZone::from(
                transaction.created_at,
            )),
            distribution_event_id: Set(transaction.distribution_event_id),
            distribution_rule_id: Set(transaction.distribution_rule_id),
        }
    }

    fn model_to_grant_schedule(
        model: points_grant_schedule::Model,
    ) -> Result<herald_domain::points::grant_schedule::PointsGrantSchedule, CoreError> {
        let grant_period_type = herald_domain::points::grant_schedule::GrantPeriodType::from_str(
            &model.grant_period_type,
        )?;

        Ok(herald_domain::points::grant_schedule::PointsGrantSchedule {
            id: model.id,
            user_id: model.user_id,
            realm_id: model.realm_id,
            bucket_id: model.bucket_id,
            subscription_id: model.subscription_id,
            entitlement_key: Some(model.entitlement_key),
            grant_period_type,
            base_time: chrono::DateTime::from(model.base_time),
            next_grant_time: chrono::DateTime::from(model.next_grant_time),
            points_per_period: model.points_per_period,
            validity_days: model.validity_days,
            granted_periods: model.granted_periods,
            max_periods: model.max_periods,
            active: model.active,
            created_at: chrono::DateTime::from(model.created_at),
            updated_at: chrono::DateTime::from(model.updated_at),
            distribution_event_id: model.distribution_event_id,
            distribution_rule_id: model.distribution_rule_id,
        })
    }

    /// Apply transaction filters to a query (shared by find_transactions and count_transactions)
    fn apply_transaction_filters(
        mut query: sea_orm::Select<points_transaction::Entity>,
        filters: &TransactionFilters,
    ) -> sea_orm::Select<points_transaction::Entity> {
        if let Some(user_id) = filters.user_id {
            query = query.filter(points_transaction::Column::UserId.eq(user_id));
        }

        if let Some(bucket_id) = filters.bucket_id {
            query = query.filter(points_transaction::Column::BucketId.eq(bucket_id));
        }

        if let Some(transaction_type) = &filters.transaction_type {
            query = query
                .filter(points_transaction::Column::Type.eq(transaction_type.as_str().to_string()));
        }

        if let Some(client_app_id) = filters.client_app_id {
            query = query.filter(points_transaction::Column::ClientAppId.eq(client_app_id));
        }

        if let Some(subscription_id) = filters.subscription_id {
            query = query.filter(points_transaction::Column::SubscriptionId.eq(subscription_id));
        }

        if !filters.external_ref_id.is_empty() {
            query = query
                .filter(points_transaction::Column::ExternalRefId.eq(&filters.external_ref_id));
        }

        if let Some(start_time) = &filters.start_time {
            query = query.filter(
                points_transaction::Column::CreatedAt
                    .gte(sea_orm::prelude::DateTimeWithTimeZone::from(*start_time)),
            );
        }

        if let Some(end_time) = &filters.end_time {
            query = query.filter(
                points_transaction::Column::CreatedAt
                    .lte(sea_orm::prelude::DateTimeWithTimeZone::from(*end_time)),
            );
        }

        query
    }

    async fn apply_wallet_filters(
        &self,
        mut query: sea_orm::Select<points_wallet::Entity>,
        realm_id: &str,
        filters: &WalletFilters,
    ) -> Result<Option<sea_orm::Select<points_wallet::Entity>>, CoreError> {
        if let Some(user_id) = filters.user_id {
            query = query.filter(points_wallet::Column::UserId.eq(user_id));
        }

        if let Some(status) = &filters.status {
            query = query.filter(points_wallet::Column::Status.eq(status.clone()));
        }

        if let Some(bucket_id) = filters.bucket_id {
            query = query.filter(points_wallet::Column::BucketId.eq(bucket_id));
        }

        if let Some(search) = &filters.search {
            if let Ok(user_id) = Uuid::parse_str(search) {
                query = query.filter(points_wallet::Column::UserId.eq(user_id));
            } else {
                let user_id: Option<Uuid> = account::Entity::find()
                    .select_only()
                    .column(account::Column::Id)
                    .filter(account::Column::RealmId.eq(realm_id.to_string()))
                    .filter(account::Column::Email.eq(search.clone()))
                    .into_tuple::<Uuid>()
                    .one(&*self.db)
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

                if let Some(uid) = user_id {
                    query = query.filter(points_wallet::Column::UserId.eq(uid));
                } else {
                    return Ok(None);
                }
            }
        }

        Ok(Some(query))
    }
    fn row_to_points_credit_ledger(
        row: PointsCreditLedgerRow,
    ) -> Result<PointsCreditLedger, CoreError> {
        Ok(PointsCreditLedger {
            id: row.id,
            user_id: row.user_id,
            realm_id: row.realm_id,
            bucket_id: row.bucket_id,
            credit_type: row.credit_type.parse()?,
            source_type: row.source_type.parse()?,
            source_id: row.source_id,
            granted_amount: row.granted_amount,
            used_amount: row.used_amount,
            revoked_amount: row.revoked_amount,
            remaining_amount: row.remaining_amount,
            expires_at: row.expires_at.map(chrono::DateTime::from),
            effective_at: row.effective_at.map(chrono::DateTime::from),
            status: row.status.parse()?,
            created_at: chrono::DateTime::from(row.created_at),
            updated_at: chrono::DateTime::from(row.updated_at),
            distribution_event_id: row.distribution_event_id,
            distribution_rule_id: row.distribution_rule_id,
        })
    }

    /// Convert `points_quota_entitlements` sqlx row to the domain entity.
    /// The camelCase ↔ snake_case JSONB mapping for `quota_windows` is done
    /// here at the infra boundary.
    fn row_to_points_quota_entitlement(
        row: PointsQuotaEntitlementRow,
    ) -> Result<PointsQuotaEntitlement, CoreError> {
        let windows: Vec<QuotaWindowDbJson> = serde_json::from_value(row.quota_windows)
            .map_err(|e| CoreError::DatabaseError(format!("invalid quota_windows JSONB: {e}")))?;
        let quota_windows = windows
            .into_iter()
            .map(|w| QuotaWindow {
                window_seconds: w.window_seconds,
                limit: w.limit,
                key: w.key,
            })
            .collect();
        Ok(PointsQuotaEntitlement {
            id: row.id,
            user_id: row.user_id,
            realm_id: row.realm_id,
            bucket_id: row.bucket_id,
            credit_type: row.credit_type.parse()?,
            source_type: row.source_type.parse()?,
            source_id: row.source_id,
            quota_windows,
            effective_from: chrono::DateTime::from(row.effective_from),
            effective_until: row.effective_until.map(chrono::DateTime::from),
            status: row.status.parse()?,
            idempotency_key: row.idempotency_key,
            created_at: chrono::DateTime::from(row.created_at),
            updated_at: chrono::DateTime::from(row.updated_at),
            distribution_event_id: row.distribution_event_id,
            distribution_rule_id: row.distribution_rule_id,
        })
    }

    /// Find a wallet for a specific (user, bucket) pair, locking the row for update
    /// (single-wallet cleanup).
    async fn find_wallet_by_user_bucket_for_update(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
    ) -> Result<Option<PointsWallet>, CoreError> {
        let row = sqlx::query_as::<_, PointsWalletRow>(
            r#"
            SELECT * FROM points_wallets
            WHERE realm_id = $1 AND user_id = $2 AND bucket_id = $3
            FOR UPDATE
            "#,
        )
        .bind(realm_id)
        .bind(user_id)
        .bind(bucket_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        row.map(Self::row_to_points_wallet).transpose()
    }

    async fn create_wallet_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
    ) -> Result<PointsWallet, CoreError> {
        // Use ON CONFLICT to handle concurrent wallet creation:
        // two concurrent requests for the same (user, bucket) may both see no wallet,
        // then both try to INSERT. ON CONFLICT returns the existing row instead.
        let row = sqlx::query_as::<_, PointsWalletRow>(
            r#"
            INSERT INTO points_wallets (
                id, user_id, realm_id, bucket_id,
                total_recharged, total_consumed, total_topup_granted,
                total_subscription_granted, status, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, 0, 0, 0, 0, 'active', NOW(), NOW()
            )
            ON CONFLICT (realm_id, user_id, bucket_id) DO NOTHING
            RETURNING *
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(realm_id)
        .bind(bucket_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        // If ON CONFLICT DO NOTHING suppressed the INSERT, fetch the existing wallet
        let row = match row {
            Some(r) => r,
            None => sqlx::query_as::<_, PointsWalletRow>(
                "SELECT * FROM points_wallets WHERE realm_id = $1 AND user_id = $2 AND bucket_id = $3 FOR UPDATE",
            )
            .bind(realm_id)
            .bind(user_id)
            .bind(bucket_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?,
        };

        Self::row_to_points_wallet(row)
    }

    fn determine_transaction_type(
        credit_type: CreditType,
        source_type: CreditSourceType,
    ) -> TransactionType {
        match (credit_type, source_type) {
            (CreditType::RegistrationCredit, _) => TransactionType::RegistrationGrant,
            (CreditType::FreePeriodicCredit, _) => TransactionType::FreePeriodicGrant,
            (CreditType::SubscriptionCredit, CreditSourceType::SubscriptionInitial) => {
                TransactionType::SubscriptionGrant
            }
            (CreditType::SubscriptionCredit, CreditSourceType::SubscriptionRenewal) => {
                TransactionType::SubscriptionRenewal
            }
            (CreditType::SubscriptionCredit, CreditSourceType::SubscriptionUpgrade) => {
                TransactionType::SubscriptionUpgrade
            }
            (CreditType::SubscriptionCredit, CreditSourceType::SubscriptionDowngrade) => {
                TransactionType::SubscriptionDowngrade
            }
            (CreditType::GrantedCredit, CreditSourceType::AdminGrant)
            | (CreditType::GrantedCredit, CreditSourceType::SdkGrant) => TransactionType::Grant,
            _ => TransactionType::Recharge,
        }
    }

    /// Ensure a wallet exists for the (user, bucket) pair within a transaction.
    /// All write paths operate on an explicit bucket; there is no
    /// single-wallet fallback.
    async fn ensure_wallet_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
    ) -> Result<PointsWallet, CoreError> {
        match Self::find_wallet_by_user_bucket_for_update(tx, realm_id, user_id, bucket_id).await? {
            Some(wallet) => Ok(wallet),
            None => Self::create_wallet_in_tx(tx, realm_id, user_id, bucket_id).await,
        }
    }

    async fn find_active_ledgers_by_expiration_for_update(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        user_id: Uuid,
        bucket_ids: &[Uuid],
    ) -> Result<Vec<PointsCreditLedger>, CoreError> {
        // Empty covered set is a caller bug; resolved higher up as
        // NoCoveredPointsPool. Guard against accidental full-table lock.
        if bucket_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, PointsCreditLedgerRow>(
            r#"
            SELECT * FROM points_credit_ledger
            WHERE realm_id = $1
              AND user_id = $2
              AND bucket_id = ANY($3)
              AND status = 'active'
              AND remaining_amount > 0
              AND (expires_at IS NULL OR expires_at > NOW())
              AND (effective_at IS NULL OR effective_at <= NOW())
            ORDER BY expires_at ASC NULLS LAST, created_at ASC
            FOR UPDATE
            "#,
        )
        .bind(realm_id)
        .bind(user_id)
        .bind(bucket_ids)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        rows.into_iter()
            .map(Self::row_to_points_credit_ledger)
            .collect()
    }

    /// Pure allocation plan for a multi-pool consume.
    /// Inputs: the already-locked active ledgers in `expires_at ASC NULLS LAST,
    /// created_at ASC` order, and the requested `amount`. Output is a sequence of
    /// `(ledger_index, allocated_amount)` covering the request greedily in expiry
    /// order, plus a `fully_covers` flag.
    /// This is deliberately free of DB / wallet concerns so the cross-pool split,
    /// permanent-pool-last ordering, partial-coverage rejection and exact-amount
    /// boundary can be unit-tested without Postgres (testing requirement).
    fn plan_consume_allocation(ledgers: &[PointsCreditLedger], amount: i64) -> ConsumePlan {
        let mut allocations: Vec<PlannedAllocation> = Vec::new();
        let mut remaining = amount;
        for (index, ledger) in ledgers.iter().enumerate() {
            if remaining <= 0 {
                break;
            }
            if ledger.remaining_amount <= 0 {
                continue;
            }
            let take = ledger.remaining_amount.min(remaining);
            allocations.push(PlannedAllocation {
                ledger_index: index,
                amount: take,
            });
            remaining -= take;
        }
        let fully_covers = remaining <= 0;
        ConsumePlan {
            allocations,
            fully_covers,
        }
    }

    /// Resolve the covered Bucket set for a client app.
    /// Explicit `credit_bucket_client_apps` rows joined to enabled
    /// `credit_buckets` only — no default-bucket merging. Sorted ascending
    /// for deterministic lock ordering downstream.
    async fn find_covered_bucket_ids_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        client_app_id: Uuid,
    ) -> Result<Vec<Uuid>, CoreError> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            r#"
            SELECT bca.bucket_id
            FROM credit_bucket_client_apps bca
            JOIN credit_buckets b ON b.id = bca.bucket_id
            WHERE bca.realm_id = $1
              AND bca.client_app_id = $2
              AND b.enabled = true
            ORDER BY bca.bucket_id ASC
            "#,
        )
        .bind(realm_id)
        .bind(client_app_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Idempotency replay. Given the primary
    /// transaction_id stored on `idempotency_keys`, reassemble the original
    /// consume result set. Multi-pool rows share a `correlation_id` → fetch all
    /// sibling transactions ordered by bucket_id. Legacy single-pool rows have
    /// NULL `correlation_id` → return just the primary transaction.
    /// Allocations are NOT re-fetched here; this fn returns the original
    /// transaction rows so the caller can hand them back verbatim without
    /// re-deducting.
    async fn replay_consume_by_primary(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        primary_txn_id: Uuid,
    ) -> Result<Vec<PointsTransaction>, CoreError> {
        // Read the primary transaction to discover its correlation_id.
        let primary_row = sqlx::query_as::<_, PointsTransactionRow>(
            r#"
            SELECT * FROM points_transactions
            WHERE realm_id = $1 AND id = $2
            "#,
        )
        .bind(realm_id)
        .bind(primary_txn_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?
        .ok_or(CoreError::NotFound)?;

        let primary = Self::points_transaction_row_to_domain(primary_row)?;

        match &primary.correlation_id {
            // Legacy single-pool consume row (pre multi-pool) → return as-is.
            None => Ok(vec![primary]),
            Some(correlation_id) => {
                let rows = sqlx::query_as::<_, PointsTransactionRow>(
                    r#"
                    SELECT * FROM points_transactions
                    WHERE realm_id = $1 AND correlation_id = $2
                    ORDER BY bucket_id ASC, created_at ASC
                    "#,
                )
                .bind(realm_id)
                .bind(correlation_id)
                .fetch_all(&mut **tx)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                rows.into_iter()
                    .map(Self::points_transaction_row_to_domain)
                    .collect()
            }
        }
    }

    async fn find_active_ledgers_by_credit_type_and_bucket_for_update(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
    ) -> Result<Vec<PointsCreditLedger>, CoreError> {
        let rows = sqlx::query_as::<_, PointsCreditLedgerRow>(
            r#"
            SELECT * FROM points_credit_ledger
            WHERE realm_id = $1
              AND user_id = $2
              AND bucket_id = $3
              AND credit_type = $4
              AND status = 'active'
              AND remaining_amount > 0
            ORDER BY created_at ASC
            FOR UPDATE
            "#,
        )
        .bind(realm_id)
        .bind(user_id)
        .bind(bucket_id)
        .bind(credit_type.to_string())
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        rows.into_iter()
            .map(Self::row_to_points_credit_ledger)
            .collect()
    }

    async fn find_expired_ledgers_for_update(
        tx: &mut Transaction<'_, Postgres>,
        expiration_time: chrono::DateTime<chrono::Utc>,
        limit: usize,
    ) -> Result<Vec<PointsCreditLedger>, CoreError> {
        let rows = sqlx::query_as::<_, PointsCreditLedgerRow>(
            r#"
            SELECT * FROM points_credit_ledger
            WHERE expires_at <= $1
              AND status = 'active'
              AND remaining_amount > 0
            ORDER BY expires_at ASC
            LIMIT $2
            FOR UPDATE
            "#,
        )
        .bind(expiration_time)
        .bind(limit as i64)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        rows.into_iter()
            .map(Self::row_to_points_credit_ledger)
            .collect()
    }

    /// SINGLE writer of the `points_wallets` lifetime analytics columns.
    /// The 5 per-type balance columns and
    /// `total_balance` were physically dropped; available balance is now a
    /// derived SUM over `points_credit_ledger`. This fn only advances the 4
    /// monotonic analytics columns (`total_recharged` / `total_consumed` /
    /// `total_topup_granted` / `total_subscription_granted`) inside the same
    /// transaction as the ledger mutation, so analytics stay drift-free.
    /// MUST be the only writer of these columns — any future direct ledger
    /// write that bypasses this fn re-introduces analytics drift.
    /// Returns the post-delta row. Callers that need the post-mutation
    /// available balance for `balance_after` on a `points_transactions` row
    /// must call `compute_available_balance_in_tx` (same predicate as the
    /// derived balance) — this fn no longer returns any balance.
    async fn apply_wallet_delta_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        wallet_id: Uuid,
        delta: WalletDelta,
    ) -> Result<PointsWallet, CoreError> {
        let row = sqlx::query_as::<_, PointsWalletRow>(
            r#"
            UPDATE points_wallets
               SET total_recharged            = total_recharged            + $2,
                   total_consumed             = total_consumed             + $3,
                   total_topup_granted        = total_topup_granted        + $4,
                   total_subscription_granted = total_subscription_granted + $5,
                   updated_at                 = NOW()
             WHERE id = $1
             RETURNING *
            "#,
        )
        .bind(wallet_id)
        .bind(delta.total_recharged)
        .bind(delta.total_consumed)
        .bind(delta.total_topup_granted)
        .bind(delta.total_subscription_granted)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?
        .ok_or(CoreError::NotFound)?;
        Self::row_to_points_wallet(row)
    }

    /// In-transaction derived available balance.
    /// Same predicate as `compute_available_balance` (the public port uses
    /// `&self.pool`; this variant runs against the caller's open tx so the
    /// post-mutation snapshot reflects uncommitted ledger writes). Used to
    /// fill `points_transactions.balance_after` (+typed snapshots) with the
    /// REAL derived value at grant/consume/refund/revoke write time, replacing
    /// the old `updated_wallet.total_balance` source that no longer exists.
    /// `bucket_ids` empty ⟺ aggregate across ALL the user's buckets; non-empty
    /// ⟺ restrict to listed buckets (used by per-bucket consume loop).
    async fn compute_available_balance_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        user_id: Uuid,
        bucket_ids: &[Uuid],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<(CreditType, i64)>, CoreError> {
        let rows: Vec<(String, i64)> = if bucket_ids.is_empty() {
            sqlx::query_as(
                r#"
                SELECT credit_type, COALESCE(SUM(remaining_amount), 0)::BIGINT AS available
                FROM points_credit_ledger
                WHERE realm_id = $1
                  AND user_id = $2
                  AND status = 'active'
                  AND remaining_amount > 0
                  AND (effective_at IS NULL OR effective_at <= $3)
                  AND (expires_at  IS NULL OR expires_at  >  $3)
                GROUP BY credit_type
                "#,
            )
            .bind(realm_id)
            .bind(user_id)
            .bind(now)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?
        } else {
            sqlx::query_as(
                r#"
                SELECT credit_type, COALESCE(SUM(remaining_amount), 0)::BIGINT AS available
                FROM points_credit_ledger
                WHERE realm_id = $1
                  AND user_id = $2
                  AND bucket_id = ANY($3)
                  AND status = 'active'
                  AND remaining_amount > 0
                  AND (effective_at IS NULL OR effective_at <= $4)
                  AND (expires_at  IS NULL OR expires_at  >  $4)
                GROUP BY credit_type
                "#,
            )
            .bind(realm_id)
            .bind(user_id)
            .bind(bucket_ids)
            .bind(now)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?
        };

        rows.into_iter()
            .map(|(credit_type, amount)| {
                let credit_type: CreditType = credit_type.parse().map_err(|_| {
                    CoreError::DatabaseError(format!("invalid credit_type: {credit_type}"))
                })?;
                Ok((credit_type, amount))
            })
            .collect()
    }

    /// Pure window-credit split for a single bucket. See [`WindowCreditSplit`].
    /// Defensive: negative spendables (shrunk quota / aggregation glitch) are
    /// clamped to 0 so the overspend invariant holds even if a negative slips
    /// through — mirrors `plan_mixed_consume`'s clamp.
    fn split_window_part_by_credit_type(
        window_part: i64,
        subscription_spendable: i64,
        free_spendable: i64,
    ) -> WindowCreditSplit {
        let sub_spendable = subscription_spendable.max(0);
        let free_spend = free_spendable.max(0);
        // Priority: subscription_credit first, free_periodic_credit 补足.
        let sub_part = window_part.min(sub_spendable);
        let remaining_after_sub = window_part - sub_part;
        let free_part = remaining_after_sub.min(free_spend);
        let window_remainder = window_part - sub_part - free_part;
        WindowCreditSplit {
            sub_part,
            free_part,
            window_remainder,
        }
    }

    /// In-transaction window spendable for `(user, bucket, credit_type)`.
    /// Returns the min over
    /// active entitlement windows of `(limit − Σ consume in window)`, i.e. the
    /// tightest window's remaining headroom for that credit_type.
    /// Mirrors [`Self::compute_available_balance_in_tx`]: runs against the
    /// caller's open tx so the window aggregation reflects the just-locked
    /// `points_wallets FOR UPDATE` serialization (the anti-overspend
    /// invariant). Reuses the SQL text + `PointsQuotaEntitlementRow` /
    /// `QuotaWindowDbJson` serde boundary structs — does NOT duplicate the
    /// query strings.
    /// `now` is the consume timestamp; the window-start is derived per window as
    /// `now - window_seconds`.
    async fn compute_window_spendable_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, CoreError> {
        // Reuse the SELECT text for active entitlements — same predicate,
        // run in-tx instead of on `self.pool`.
        let entitlement_rows = sqlx::query_as::<_, PointsQuotaEntitlementRow>(
            r#"
            SELECT * FROM points_quota_entitlements
            WHERE realm_id = $1
              AND user_id = $2
              AND bucket_id = $3
              AND credit_type = $4
              AND status = 'active'
              AND effective_from <= $5
              AND (effective_until IS NULL OR effective_until > $5)
            "#,
        )
        .bind(realm_id)
        .bind(user_id)
        .bind(bucket_id)
        .bind(credit_type.as_str())
        .bind(now)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        if entitlement_rows.is_empty() {
            // No active entitlement of this credit_type ⟹ no window capacity.
            return Ok(0);
        }

        let mut tightest: Option<i64> = None;
        for row in entitlement_rows {
            let windows: Vec<QuotaWindowDbJson> = serde_json::from_value(row.quota_windows)
                .map_err(|e| {
                    CoreError::DatabaseError(format!("invalid quota_windows JSONB: {e}"))
                })?;
            for window in windows {
                if window.window_seconds <= 0 || window.limit < 0 {
                    // Defensive: a malformed window snapshot cannot add capacity.
                    // Treat it as a zero-remaining constraint so the min stays
                    // safe (overspend invariant over correctness).
                    return Ok(0);
                }
                let window_start = now - chrono::Duration::seconds(window.window_seconds);
                // Reuse the window-aggregation SQL text in-tx.
                let used: i64 = sqlx::query_scalar(
                    r#"
                    SELECT COALESCE(SUM(ABS(amount)), 0)::BIGINT
                    FROM points_transactions
                    WHERE realm_id = $1
                      AND user_id = $2
                      AND bucket_id = $3
                      AND credit_type = $4
                      AND type = 'consume'
                      AND created_at >= $5
                    "#,
                )
                .bind(realm_id)
                .bind(user_id)
                .bind(bucket_id)
                .bind(credit_type.as_str())
                .bind(window_start)
                .fetch_one(&mut **tx)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                let remaining = window.limit - used;
                tightest = Some(tightest.map_or(remaining, |t| t.min(remaining)));
            }
        }

        Ok(tightest.unwrap_or(0).max(0))
    }

    /// Build `(balance_after, topup_balance_after, subscription_balance_after)`
    /// from a derived SUM result, for writing into `points_transactions`.
    /// `balance_after` = total across all credit types; typed snapshots are
    /// populated for topup/subscription and left `None` otherwise (matching
    /// the original write convention where only those two were snapshotted).
    fn derived_to_balance_snapshots(
        derived: &[(CreditType, i64)],
    ) -> (i64, Option<i64>, Option<i64>) {
        let mut total = 0i64;
        let mut topup = Some(0i64);
        let mut subscription = Some(0i64);
        for (credit_type, amount) in derived {
            total += amount;
            match credit_type {
                CreditType::TopupCredit => topup = Some(topup.unwrap_or(0) + amount),
                CreditType::SubscriptionCredit => {
                    subscription = Some(subscription.unwrap_or(0) + amount)
                }
                _ => {}
            }
        }
        (total, topup, subscription)
    }

    async fn update_ledger_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        ledger_id: Uuid,
        updates: LedgerUpdate,
    ) -> Result<PointsCreditLedger, CoreError> {
        let ledger = sqlx::query_as::<_, PointsCreditLedgerRow>(
            "SELECT * FROM points_credit_ledger WHERE id = $1 FOR UPDATE",
        )
        .bind(ledger_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?
        .map(Self::row_to_points_credit_ledger)
        .transpose()?
        .ok_or_else(|| CoreError::BadRequest(format!("Ledger not found: {}", ledger_id)))?;

        let (used_amount, revoked_amount, expires_at, status) = match updates {
            LedgerUpdate::Consumption(amount) => {
                let used_amount = ledger.used_amount + amount;
                let remaining = ledger.granted_amount - used_amount - ledger.revoked_amount;
                if remaining < 0 {
                    return Err(CoreError::concurrent_modification());
                }
                (
                    used_amount,
                    ledger.revoked_amount,
                    ledger.expires_at,
                    if remaining == 0 {
                        CreditLedgerStatus::FullyUsed
                    } else {
                        ledger.status
                    },
                )
            }
            LedgerUpdate::Revocation(amount) => {
                let revoked_amount = ledger.revoked_amount + amount;
                let remaining = ledger.granted_amount - ledger.used_amount - revoked_amount;
                if remaining < 0 {
                    return Err(CoreError::concurrent_modification());
                }
                (
                    ledger.used_amount,
                    revoked_amount,
                    ledger.expires_at,
                    if remaining == 0 {
                        CreditLedgerStatus::Revoked
                    } else {
                        ledger.status
                    },
                )
            }
            LedgerUpdate::SetExpiration(expires_at) => (
                ledger.used_amount,
                ledger.revoked_amount,
                Some(expires_at),
                ledger.status,
            ),
            LedgerUpdate::SetStatus(status) => (
                ledger.used_amount,
                ledger.revoked_amount,
                ledger.expires_at,
                status,
            ),
        };

        let row = sqlx::query_as::<_, PointsCreditLedgerRow>(
            r#"
            UPDATE points_credit_ledger
            SET used_amount = $2,
                revoked_amount = $3,
                expires_at = $4,
                status = $5,
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(ledger_id)
        .bind(used_amount)
        .bind(revoked_amount)
        .bind(expires_at)
        .bind(status.to_string())
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Self::row_to_points_credit_ledger(row)
    }

    async fn create_transaction_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        transaction: PointsTransaction,
    ) -> Result<PointsTransaction, CoreError> {
        let row = sqlx::query_as::<_, PointsTransactionRow>(
            r#"
            INSERT INTO points_transactions (
                id, wallet_id, user_id, realm_id, bucket_id, type, amount, balance_after,
                topup_balance_after, subscription_balance_after, credit_type, description,
                client_app_id, subscription_id, external_ref_id, correlation_id,
                distribution_event_id, distribution_rule_id, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8,
                $9, $10, $11, $12,
                $13, $14, $15, $16,
                $17, $18, $19, NOW()
            )
            RETURNING id, realm_id, wallet_id, user_id, bucket_id, type, amount, balance_after,
                      topup_balance_after, subscription_balance_after, credit_type,
                      description, client_app_id, subscription_id, external_ref_id, correlation_id,
                      NULL::timestamptz AS effective_at,
                      distribution_event_id, distribution_rule_id,
                      created_at, updated_at, expires_at
            "#,
        )
        .bind(transaction.id)
        .bind(transaction.wallet_id)
        .bind(transaction.user_id)
        .bind(transaction.realm_id)
        .bind(transaction.bucket_id)
        .bind(transaction.transaction_type.as_str())
        .bind(transaction.amount)
        .bind(transaction.balance_after)
        .bind(transaction.topup_balance_after)
        .bind(transaction.subscription_balance_after)
        .bind(transaction.credit_type.map(|v| v.to_string()))
        .bind(transaction.description)
        .bind(transaction.client_app_id)
        .bind(transaction.subscription_id)
        .bind(transaction.external_ref_id)
        .bind(transaction.correlation_id.clone())
        .bind(transaction.distribution_event_id)
        .bind(transaction.distribution_rule_id)
        .bind(transaction.created_at)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Self::points_transaction_row_to_domain(row)
    }

    async fn create_ledger_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        ledger: &PointsCreditLedger,
    ) -> Result<PointsCreditLedger, CoreError> {
        let row = sqlx::query_as::<_, PointsCreditLedgerRow>(
            r#"
            INSERT INTO points_credit_ledger (
                id, user_id, realm_id, bucket_id, credit_type, source_type, source_id,
                granted_amount, used_amount, revoked_amount, expires_at, effective_at,
                status, distribution_event_id, distribution_rule_id, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6,
                $7, $8, $9, $10, $11, $12,
                $13, $14, $15, $16, $17
            )
            RETURNING *
            "#,
        )
        .bind(ledger.id)
        .bind(ledger.user_id)
        .bind(&ledger.realm_id)
        .bind(ledger.bucket_id)
        .bind(ledger.credit_type.to_string())
        .bind(ledger.source_type.to_string())
        .bind(&ledger.source_id)
        .bind(ledger.granted_amount)
        .bind(ledger.used_amount)
        .bind(ledger.revoked_amount)
        .bind(ledger.expires_at)
        .bind(ledger.effective_at)
        .bind(ledger.status.to_string())
        .bind(ledger.distribution_event_id)
        .bind(ledger.distribution_rule_id)
        .bind(ledger.created_at)
        .bind(ledger.updated_at)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Self::row_to_points_credit_ledger(row)
    }

    async fn create_consumption_allocation_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        allocation: &PointsConsumptionAllocation,
    ) -> Result<(), CoreError> {
        // wallet_id is NOT NULL on points_consumption_allocations. The consume
        // write path always resolves it; fail loud rather than silently writing NULL.
        let wallet_id = allocation.wallet_id.ok_or_else(|| {
            CoreError::InternalServerError(format!(
                "consumption allocation requires wallet_id (ledger={}, transaction={})",
                allocation.ledger_id, allocation.transaction_id
            ))
        })?;
        sqlx::query(
            r#"
            INSERT INTO points_consumption_allocations (
                id, transaction_id, ledger_id, wallet_id, user_id, realm_id, bucket_id,
                allocated_amount, ledger_remaining_after, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(allocation.id)
        .bind(allocation.transaction_id)
        .bind(allocation.ledger_id)
        .bind(wallet_id)
        .bind(allocation.user_id)
        .bind(&allocation.realm_id)
        .bind(allocation.bucket_id)
        .bind(allocation.allocated_amount)
        .bind(allocation.ledger_remaining_after)
        .bind(allocation.created_at)
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn create_revocation_record_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        record: &PointsRevocationRecord,
    ) -> Result<(), CoreError> {
        sqlx::query(
            r#"
            INSERT INTO points_revocation_records (
                id, ledger_id, user_id, realm_id, revocation_type,
                revoked_amount, reason, reference_id, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(record.id)
        .bind(record.ledger_id)
        .bind(record.user_id)
        .bind(&record.realm_id)
        .bind(record.revocation_type.to_string())
        .bind(record.revoked_amount)
        .bind(&record.reason)
        .bind(&record.reference_id)
        .bind(record.created_at)
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn check_completed_idempotency_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<Uuid>, CoreError> {
        let row = sqlx::query("SELECT transaction_id FROM idempotency_keys WHERE realm_id = $1 AND idempotency_key = $2 AND status = 'completed' AND expires_at > NOW() LIMIT 1")
            .bind(realm_id)
            .bind(idempotency_key)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(row.and_then(|row| row.try_get("transaction_id").ok()))
    }

    async fn record_completed_idempotency_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        idempotency_key: &str,
        transaction_id: Uuid,
    ) -> Result<(), CoreError> {
        sqlx::query(
            r#"
            INSERT INTO idempotency_keys (
                id, realm_id, idempotency_key, status, request_data,
                response_data, transaction_id, created_at, expires_at, updated_at
            ) VALUES (
                $1, $2, $3, 'completed', '{}', '{}', $4, NOW(), NOW() + INTERVAL '24 hours', NOW()
            )
            ON CONFLICT (realm_id, idempotency_key)
            DO UPDATE SET transaction_id = EXCLUDED.transaction_id,
                          status = 'completed',
                          expires_at = NOW() + INTERVAL '24 hours',
                          updated_at = NOW()
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(realm_id)
        .bind(idempotency_key)
        .bind(transaction_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    async fn revoke_ledger_list_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        user_id: Uuid,
        ledgers: Vec<(Uuid, i64)>,
        revocation_type: RevocationType,
        reason: &str,
        reference_id: Option<&str>,
    ) -> Result<(i64, Vec<Uuid>), CoreError> {
        let mut total_revoked = 0i64;
        let mut ledger_ids = Vec::new();
        for (ledger_id, amount_to_revoke) in ledgers {
            let updated_ledger = Self::update_ledger_in_tx(
                tx,
                ledger_id,
                LedgerUpdate::Revocation(amount_to_revoke),
            )
            .await?;
            let record = PointsRevocationRecord {
                id: Uuid::now_v7(),
                ledger_id: updated_ledger.id,
                user_id,
                realm_id: realm_id.to_string(),
                revocation_type,
                revoked_amount: amount_to_revoke,
                reason: reason.to_string(),
                reference_id: reference_id.map(|s| s.to_string()),
                created_at: chrono::Utc::now(),
            };
            Self::create_revocation_record_in_tx(tx, &record).await?;
            total_revoked += amount_to_revoke;
            ledger_ids.push(updated_ledger.id);
        }
        Ok((total_revoked, ledger_ids))
    }

    async fn revoke_distribution_source_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        user_id: Uuid,
        source_id: &str,
        revocation_type: RevocationType,
        reason: &str,
        reference_id: &str,
    ) -> Result<RevokePointsOutput, CoreError> {
        let ledgers = sqlx::query_as::<_, (Uuid, i64)>(
            "SELECT l.id, l.remaining_amount \
             FROM points_credit_ledger l \
             JOIN points_distribution_events e ON e.id = l.distribution_event_id \
             WHERE e.realm_id = $1 AND e.user_id = $2 \
               AND (e.source_id = $3 OR e.event_key LIKE $4) \
               AND l.distribution_rule_id IS NOT NULL AND l.remaining_amount > 0 \
             ORDER BY l.bucket_id, l.id FOR UPDATE OF l",
        )
        .bind(realm_id)
        .bind(user_id)
        .bind(source_id)
        .bind(format!("subscription:{source_id}:%"))
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        let (total_revoked, ledger_ids) = Self::revoke_ledger_list_in_tx(
            tx,
            realm_id,
            user_id,
            ledgers,
            revocation_type,
            reason,
            Some(reference_id),
        )
        .await?;

        sqlx::query(
            "UPDATE points_quota_entitlements q \
             SET status = 'revoked', effective_until = LEAST(COALESCE(effective_until, NOW()), NOW()), \
                 updated_at = NOW() \
             FROM points_distribution_events e \
             WHERE e.id = q.distribution_event_id \
               AND e.realm_id = $1 AND e.user_id = $2 \
               AND (e.source_id = $3 OR e.event_key LIKE $4) \
               AND q.distribution_rule_id IS NOT NULL AND q.status = 'active'",
        )
        .bind(realm_id)
        .bind(user_id)
        .bind(source_id)
        .bind(format!("subscription:{source_id}:%"))
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(RevokePointsOutput {
            revocation_id: Uuid::now_v7(),
            ledger_ids,
            total_revoked,
            revoked_at: chrono::Utc::now(),
        })
    }

    async fn deactivate_free_periodic_results_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        user_id: Uuid,
        deactivate_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE points_quota_entitlements q \
             SET status = 'revoked', \
                 effective_until = LEAST(COALESCE(q.effective_until, $3), $3), \
                 updated_at = NOW() \
             FROM points_distribution_rules r \
             WHERE r.id = q.distribution_rule_id \
               AND q.realm_id = $1 AND q.user_id = $2 AND q.status = 'active' \
               AND q.source_type = 'free_periodic_grant' \
               AND r.owner_type = 'realm_registration' \
               AND 'free_periodic_grant' = ANY(r.trigger_sources)",
        )
        .bind(realm_id)
        .bind(user_id)
        .bind(deactivate_at)
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        sqlx::query(
            "UPDATE points_grant_schedules s \
             SET active = FALSE, updated_at = NOW() \
             FROM points_distribution_rules r \
             WHERE r.id = s.distribution_rule_id \
               AND s.realm_id = $1 AND s.user_id = $2 AND s.active = TRUE \
               AND r.owner_type = 'realm_registration' \
               AND 'free_periodic_grant' = ANY(r.trigger_sources)",
        )
        .bind(realm_id)
        .bind(user_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    // ===================================================================
    // Multi-rule distribution executor (atomic + idempotent + replay)
    // ===================================================================

    /// Validate a target bucket exists, belongs to the realm and is enabled.
    /// Disabled or cross-realm buckets fail the whole event (all-or-nothing).
    async fn bucket_enabled_in_realm_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        bucket_id: Uuid,
    ) -> Result<bool, CoreError> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM credit_buckets WHERE id = $1 AND realm_id = $2 AND enabled = TRUE)",
        )
        .bind(bucket_id)
        .bind(realm_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(exists)
    }

    /// Load the owner's enabled rules that declare `trigger`, in stable
    /// `(display_order, rule_id)` order. Used by the `CurrentOwnerRules`
    /// first-run selection. For a registration event the caller selects both
    /// `Registration` and `FreePeriodicGrant` (two passes through this loader
    /// share one transaction so the new user's whole initial set is atomic).
    async fn load_current_owner_rules_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        owner: &DistributionRuleOwner,
        trigger: DistributionTrigger,
    ) -> Result<Vec<PointsDistributionRule>, CoreError> {
        let (owner_type, mapping_id) = match owner {
            DistributionRuleOwner::EntitlementMapping(id) => ("entitlement_mapping", Some(*id)),
            DistributionRuleOwner::RealmRegistration => ("realm_registration", None),
        };
        let rows = sqlx::query(
            "SELECT id, realm_id, owner_type, entitlement_mapping_id, bucket_id, trigger_sources, \
                    grant_mode, points_amount, validity_days, grant_period_type, quota_windows, \
                    enabled, display_order \
             FROM points_distribution_rules \
             WHERE realm_id = $1 AND owner_type = $2 AND enabled = TRUE \
               AND ($3::uuid IS NULL OR entitlement_mapping_id = $3) \
             ORDER BY display_order, id",
        )
        .bind(realm_id)
        .bind(owner_type)
        .bind(mapping_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        let all: Vec<PointsDistributionRule> = rows
            .into_iter()
            .map(|r| rule_row_to_domain(&r))
            .collect::<Result<_, _>>()?;
        Ok(select_and_sort_rules_owned(all, trigger))
    }

    /// Load a single rule by id (for `ScheduledRule`). Returns the rule
    /// regardless of `enabled` state: a scheduled free-periodic period replays
    /// the schedule-bound rule even if the rule was later disabled, because the
    /// schedule is the active contract for subsequent periods.
    async fn load_single_rule_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        rule_id: Uuid,
    ) -> Result<PointsDistributionRule, CoreError> {
        let row = sqlx::query(
            "SELECT id, realm_id, owner_type, entitlement_mapping_id, bucket_id, trigger_sources, \
                    grant_mode, points_amount, validity_days, grant_period_type, quota_windows, \
                    enabled, display_order \
             FROM points_distribution_rules \
             WHERE id = $1 AND realm_id = $2",
        )
        .bind(rule_id)
        .bind(realm_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?
        .ok_or(CoreError::NotFound)?;
        rule_row_to_domain(&row)
    }

    /// Load the payment-attempt rule snapshot (for `CapturedPaymentRules`).
    /// Returns `(rule, captured_bucket_id)` per snapshot row, in stable
    /// `(display_order, rule_id)` order. A rule disabled after snapshot capture
    /// is still fulfilled: the snapshot is the contract for an already-paid
    /// attempt, and the captured bucket is used (not the rule's current
    /// bucket).
    async fn load_captured_payment_rules(
        tx: &mut Transaction<'_, Postgres>,
        realm_id: &str,
        payment_attempt_id: Uuid,
        trigger: DistributionTrigger,
    ) -> Result<Vec<(PointsDistributionRule, Uuid)>, CoreError> {
        use sqlx::Row;
        let rows = sqlx::query(
            "SELECT r.id, r.realm_id, r.owner_type, r.entitlement_mapping_id, r.bucket_id, \
                    r.trigger_sources, r.grant_mode, r.points_amount, r.validity_days, \
                    r.grant_period_type, r.quota_windows, r.enabled, r.display_order, \
                    s.bucket_id AS captured_bucket_id \
             FROM payment_attempt_point_rules s \
             JOIN points_distribution_rules r ON r.id = s.rule_id \
             WHERE s.payment_attempt_id = $1 AND r.realm_id = $2 \
             ORDER BY r.display_order, r.id",
        )
        .bind(payment_attempt_id)
        .bind(realm_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        let mut out: Vec<(PointsDistributionRule, Uuid)> = Vec::with_capacity(rows.len());
        for row in rows {
            let rule = rule_row_to_domain(&row)?;
            let captured_bucket_id: Uuid = row.get("captured_bucket_id");
            out.push((rule, captured_bucket_id));
        }
        // Stable trigger containment + de-dup (pure). The captured snapshot may
        // carry rules declaring multiple triggers; only those declaring this
        // event's trigger fire.
        let picked = select_and_sort_captured(out, trigger);
        Ok(picked)
    }

    /// Insert a `processing` event row. On the unique-key conflict, another
    /// concurrent caller has already committed (or is committing) the same
    /// `(realm, user, trigger, event_key)`; return the existing row's id +
    /// status so the caller can take the replay branch. A `processing` row is
    /// never committed (the inserting transaction either upgrades it to
    /// `completed` in the same commit or rolls back), so an observed
    /// `processing` status here means another in-flight caller — serialize by
    /// retrying the lock.
    async fn insert_or_load_event_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        event: &DistributionEvent,
    ) -> Result<EventInsertOutcome, CoreError> {
        use sqlx::Row;
        let event_id = Uuid::now_v7();
        let owner_type = event.owner.as_str();
        let mapping_id = event.owner.mapping_id();
        let trigger_str = event.trigger.as_str();
        let inserted = sqlx::query(
            "INSERT INTO points_distribution_events \
                (id, realm_id, user_id, trigger, event_key, source_id, owner_type, \
                 entitlement_mapping_id, status) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'processing') \
             ON CONFLICT (realm_id, user_id, trigger, event_key) DO NOTHING",
        )
        .bind(event_id)
        .bind(&event.realm_id)
        .bind(event.user_id)
        .bind(trigger_str)
        .bind(&event.event_key)
        .bind(&event.source_id)
        .bind(owner_type)
        .bind(mapping_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        if inserted.rows_affected() == 1 {
            return Ok(EventInsertOutcome::InsertedProcessing(event_id));
        }
        // Conflict: load the existing row. Lock FOR UPDATE so concurrent
        // replayers serialize; a processing row blocks until that tx commits/
        // rolls back (then the caller re-enters the executor).
        let row = sqlx::query(
            "SELECT id, status FROM points_distribution_events \
             WHERE realm_id = $1 AND user_id = $2 AND trigger = $3 AND event_key = $4 \
             FOR UPDATE",
        )
        .bind(&event.realm_id)
        .bind(event.user_id)
        .bind(trigger_str)
        .bind(&event.event_key)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        let id: Uuid = row.get("id");
        let status: String = row.get("status");
        Ok(EventInsertOutcome::Existing { id, status })
    }

    /// Lock + read a completed event for replay: `id`, `result_count`.
    async fn lock_completed_event_for_replay_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        event_id: Uuid,
    ) -> Result<Option<i32>, CoreError> {
        use sqlx::Row;
        let row = sqlx::query(
            "SELECT result_count FROM points_distribution_events \
             WHERE id = $1 AND status = 'completed' FOR UPDATE",
        )
        .bind(event_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(row
            .map(|r| r.get::<Option<i32>, _>("result_count"))
            .and_then(|c| c))
    }

    /// Finalize an event: set `status='completed'`, `result_count`,
    /// `completed_at=NOW()`. Only valid for a row inserted in this same
    /// transaction (a processing row); replayed completed rows take the replay
    /// branch instead.
    async fn complete_event_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        event_id: Uuid,
        result_count: i32,
    ) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE points_distribution_events \
             SET status = 'completed', result_count = $2, completed_at = NOW() \
             WHERE id = $1 AND status = 'processing'",
        )
        .bind(event_id)
        .bind(result_count)
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    /// Replay: read fixed ledger rows attributed to `event_id`. Returns
    /// `(rule_id, bucket_id, ledger_id, amount)`. The first-period ledger of a
    /// free-periodic schedule is included here and folded out by
    /// `fold_replay_results` via the schedule's first-ledger id.
    async fn replay_ledger_rows_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        event_id: Uuid,
    ) -> Result<Vec<(Uuid, Uuid, Uuid, i64)>, CoreError> {
        use sqlx::Row;
        let rows = sqlx::query(
            "SELECT distribution_rule_id, bucket_id, id, granted_amount \
             FROM points_credit_ledger \
             WHERE distribution_event_id = $1 AND distribution_rule_id IS NOT NULL \
             ORDER BY distribution_rule_id",
        )
        .bind(event_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<Uuid, _>("distribution_rule_id"),
                    r.get::<Uuid, _>("bucket_id"),
                    r.get::<Uuid, _>("id"),
                    r.get::<i64, _>("granted_amount"),
                )
            })
            .collect())
    }

    /// Replay: read quota entitlement rows attributed to `event_id`. Returns
    /// `(rule_id, bucket_id, entitlement_id)`.
    async fn replay_quota_rows_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        event_id: Uuid,
    ) -> Result<Vec<(Uuid, Uuid, Uuid)>, CoreError> {
        use sqlx::Row;
        let rows = sqlx::query(
            "SELECT distribution_rule_id, bucket_id, id \
             FROM points_quota_entitlements \
             WHERE distribution_event_id = $1 AND distribution_rule_id IS NOT NULL \
             ORDER BY distribution_rule_id",
        )
        .bind(event_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<Uuid, _>("distribution_rule_id"),
                    r.get::<Uuid, _>("bucket_id"),
                    r.get::<Uuid, _>("id"),
                )
            })
            .collect())
    }

    /// Replay: read schedule rows attributed to `event_id` joined to their
    /// first-period ledger via `points_grant_records`. Returns
    /// `(rule_id, bucket_id, schedule_id, first_ledger_id)`. The first-ledger is
    /// resolved through the grant-record bridge so it folds into the Schedule
    /// result rather than being double-counted as a Fixed result.
    async fn replay_schedule_rows_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        event_id: Uuid,
    ) -> Result<Vec<(Uuid, Uuid, Uuid, Uuid)>, CoreError> {
        use sqlx::Row;
        let rows = sqlx::query(
            "SELECT s.distribution_rule_id, s.bucket_id, s.id AS schedule_id, \
                    gr.ledger_id AS first_ledger_id \
             FROM points_grant_schedules s \
             LEFT JOIN points_grant_records gr \
                    ON gr.schedule_id = s.id AND gr.period_number = 1 \
             WHERE s.distribution_event_id = $1 \
             ORDER BY s.distribution_rule_id",
        )
        .bind(event_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let first_ledger_id: Option<Uuid> = r.get("first_ledger_id");
            out.push((
                r.get::<Uuid, _>("distribution_rule_id"),
                r.get::<Uuid, _>("bucket_id"),
                r.get::<Uuid, _>("schedule_id"),
                first_ledger_id.unwrap_or(Uuid::nil()),
            ));
        }
        Ok(out)
    }

    /// First-run execution: resolve rules for `selection`, validate every
    /// target bucket, write all results + the first-period schedule of
    /// free-periodic fixed rules, then finalize the event as `completed` with
    /// `result_count`. Everything commits in the caller's transaction; any
    /// failure propagates as `Err` so the caller rolls back and the
    /// `processing` event row never persists.
    ///
    /// Zero matched rules is a valid completed event: writes nothing but
    /// `result_count = 0` and the completion record.
    #[allow(clippy::too_many_arguments)]
    async fn execute_first_run_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        event_id: Uuid,
        event: &DistributionEvent,
        selection: &DistributionRuleSelection,
    ) -> Result<Vec<DistributionGrantResult>, CoreError> {
        let realm_id = &event.realm_id;
        let trigger = event.trigger;

        // Resolve the rule set + target buckets for the selection. Each variant
        // produces `(rule, effective_bucket_id)` pairs in stable
        // `(display_order, rule_id)` order, de-duplicated by rule id.
        let mut resolved: Vec<(PointsDistributionRule, Uuid, DistributionTrigger)> = match selection
        {
            DistributionRuleSelection::CapturedPaymentRules(refs) => {
                if refs.is_empty() {
                    // No captured rules: a valid zero-result completed event.
                    Vec::new()
                } else {
                    // The payment attempt id is the event source for a topup /
                    // subscription_initial fulfillment. The snapshot is loaded by
                    // payment_attempt_id; the JOIN already returns the captured
                    // bucket, which is the contract for an already-paid attempt.
                    let payment_attempt_id = Uuid::parse_str(&event.source_id).map_err(|_| {
                        CoreError::DatabaseError(format!(
                            "captured-payment selection requires a UUID source_id, got '{}'",
                            event.source_id
                        ))
                    })?;
                    Self::load_captured_payment_rules(tx, realm_id, payment_attempt_id, trigger)
                        .await?
                        .into_iter()
                        .map(|(rule, bucket_id)| (rule, bucket_id, trigger))
                        .collect()
                }
            }
            DistributionRuleSelection::CurrentOwnerRules => {
                // A registration event selects both Registration and
                // FreePeriodicGrant rules in one transaction (a new user's whole
                // initial grant set is atomic). For all other triggers a single
                // pass suffices.
                let triggers: &[DistributionTrigger] = match trigger {
                    DistributionTrigger::Registration => &[
                        DistributionTrigger::Registration,
                        DistributionTrigger::FreePeriodicGrant,
                    ],
                    _ => std::slice::from_ref(&trigger),
                };
                let mut acc: Vec<(PointsDistributionRule, Uuid, DistributionTrigger)> = Vec::new();
                for t in triggers {
                    let rules =
                        Self::load_current_owner_rules_in_tx(tx, realm_id, &event.owner, *t)
                            .await?;
                    for r in rules {
                        if !acc.iter().any(|(x, _, _)| x.id == r.id) {
                            let bucket_id = r.bucket_id;
                            acc.push((r, bucket_id, *t));
                        }
                    }
                }
                acc
            }
            DistributionRuleSelection::ScheduledRule(rule_id) => {
                let rule = Self::load_single_rule_in_tx(tx, realm_id, *rule_id).await?;
                let bucket_id: Uuid = sqlx::query_scalar(
                    "SELECT bucket_id FROM points_grant_schedules \
                     WHERE realm_id = $1 AND user_id = $2 AND distribution_rule_id = $3",
                )
                .bind(realm_id)
                .bind(event.user_id)
                .bind(rule_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?
                .ok_or(CoreError::NotFound)?;
                vec![(rule, bucket_id, DistributionTrigger::FreePeriodicGrant)]
            }
        };
        resolved.sort_by(|(left, _, _), (right, _, _)| {
            left.display_order
                .cmp(&right.display_order)
                .then_with(|| left.id.cmp(&right.id))
        });

        // Validate every target bucket exists, is in-realm and enabled BEFORE
        // any write (all-or-nothing). Use the effective bucket (captured for
        // snapshots, rule.bucket_id otherwise).
        for (rule, bucket_id, _) in &resolved {
            if !Self::bucket_enabled_in_realm_in_tx(tx, realm_id, *bucket_id).await? {
                return Err(CoreError::BadRequest(format!(
                    "distribution target bucket {} for rule {} is disabled or outside realm",
                    bucket_id, rule.id
                )));
            }
        }

        let now = chrono::Utc::now();

        let mut results: Vec<DistributionGrantResult> = Vec::new();
        for (rule, bucket_id, execution_trigger) in &resolved {
            let (credit_type, source_type) = credit_pair_for_trigger(*execution_trigger);
            match &rule.policy {
                DistributionPolicy::Fixed {
                    amount,
                    validity_days,
                    grant_period_type,
                } => {
                    if matches!(selection, DistributionRuleSelection::ScheduledRule(_)) {
                        let ledger = Self::execute_scheduled_fixed_in_tx(
                            tx,
                            event_id,
                            event,
                            rule.id,
                            *bucket_id,
                            source_type,
                            now,
                        )
                        .await?;
                        results.push(DistributionGrantResult::Fixed {
                            rule_id: rule.id,
                            bucket_id: *bucket_id,
                            ledger_id: ledger.id,
                            amount: ledger.granted_amount,
                        });
                    } else if let Some(period_type) = grant_period_type {
                        // Free-periodic fixed: schedule + first-period ledger +
                        // grant record + transaction. The first ledger is folded
                        // into the Schedule result (not emitted as Fixed).
                        let schedule_id = Uuid::now_v7();
                        let expires_at = period_type.calculate_expiration(now, *validity_days);
                        let first_ledger = Self::write_rule_ledger_in_tx(
                            tx,
                            event_id,
                            rule.id,
                            *bucket_id,
                            event.user_id,
                            realm_id,
                            credit_type,
                            source_type,
                            *amount,
                            *validity_days,
                            Some(now),
                            expires_at,
                            now,
                        )
                        .await?;
                        // The wallet + transaction for the first grant.
                        Self::write_rule_transaction_in_tx(
                            tx,
                            event_id,
                            rule.id,
                            *bucket_id,
                            event.user_id,
                            realm_id,
                            credit_type,
                            source_type,
                            *amount,
                            now,
                        )
                        .await?;
                        // points_grant_records bridges (schedule, period=1) →
                        // ledger so replay can fold the first ledger out.
                        Self::write_grant_record_in_tx(
                            tx,
                            schedule_id,
                            first_ledger.id,
                            event.user_id,
                            realm_id,
                            1,
                            *amount,
                            now,
                        )
                        .await?;
                        // Schedule row (NOT NULL attribution). next_grant_time
                        // = base + 1 period; granted_periods = 1.
                        Self::write_schedule_in_tx(
                            tx,
                            schedule_id,
                            event_id,
                            rule.id,
                            *bucket_id,
                            event.user_id,
                            realm_id,
                            *period_type,
                            now,
                            *amount,
                            *validity_days,
                        )
                        .await?;
                        results.push(DistributionGrantResult::Schedule {
                            rule_id: rule.id,
                            bucket_id: *bucket_id,
                            schedule_id,
                            first_ledger_id: first_ledger.id,
                        });
                    } else {
                        // Plain fixed grant: ledger + transaction. Result is a
                        // Fixed carrying the ledger id.
                        let expires_at = Self::fixed_expires_at(*validity_days, credit_type);
                        let ledger = Self::write_rule_ledger_in_tx(
                            tx,
                            event_id,
                            rule.id,
                            *bucket_id,
                            event.user_id,
                            realm_id,
                            credit_type,
                            source_type,
                            *amount,
                            *validity_days,
                            None,
                            expires_at,
                            now,
                        )
                        .await?;
                        Self::write_rule_transaction_in_tx(
                            tx,
                            event_id,
                            rule.id,
                            *bucket_id,
                            event.user_id,
                            realm_id,
                            credit_type,
                            source_type,
                            *amount,
                            now,
                        )
                        .await?;
                        results.push(DistributionGrantResult::Fixed {
                            rule_id: rule.id,
                            bucket_id: *bucket_id,
                            ledger_id: ledger.id,
                            amount: *amount,
                        });
                    }
                }
                DistributionPolicy::Quota { windows } => {
                    let quota_source = quota_source_type_for_trigger(*execution_trigger)?;
                    let entitlement = Self::write_quota_entitlement_in_tx(
                        tx,
                        event_id,
                        rule.id,
                        *bucket_id,
                        event.user_id,
                        realm_id,
                        credit_type,
                        quota_source,
                        &event.source_id,
                        windows,
                        event.effective_from,
                        event.effective_until,
                        now,
                    )
                    .await?;
                    results.push(DistributionGrantResult::Quota {
                        rule_id: rule.id,
                        bucket_id: *bucket_id,
                        entitlement_id: entitlement.id,
                    });
                }
            }
        }

        // Finalize the event: result_count covers zero-rule events too.
        let result_count = i32::try_from(results.len()).map_err(|_| {
            CoreError::InternalServerError("distribution event result count overflow".to_string())
        })?;
        Self::complete_event_in_tx(tx, event_id, result_count).await?;
        Ok(results)
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_scheduled_fixed_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        event_id: Uuid,
        event: &DistributionEvent,
        rule_id: Uuid,
        bucket_id: Uuid,
        source_type: CreditSourceType,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<PointsCreditLedger, CoreError> {
        let row = sqlx::query(
            "SELECT id, grant_period_type, base_time, points_per_period, validity_days, \
                    granted_periods, max_periods, active \
             FROM points_grant_schedules \
             WHERE realm_id = $1 AND user_id = $2 AND distribution_rule_id = $3 \
             FOR UPDATE",
        )
        .bind(&event.realm_id)
        .bind(event.user_id)
        .bind(rule_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?
        .ok_or(CoreError::NotFound)?;

        let schedule_id: Uuid = row.get("id");
        let active: bool = row.get("active");
        if !active {
            return Err(CoreError::BadRequest(
                "free-periodic grant schedule is inactive".to_string(),
            ));
        }
        let period_type = herald_domain::points::grant_schedule::GrantPeriodType::from_str(
            row.get::<&str, _>("grant_period_type"),
        )
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        let base_time: chrono::DateTime<chrono::Utc> = row.get("base_time");
        let points_per_period: i64 = row.get("points_per_period");
        let validity_days: i64 = row.get("validity_days");
        let granted_periods: i64 = row.get("granted_periods");
        let max_periods: Option<i64> = row.get("max_periods");
        let period_number = granted_periods + 1;
        let expires_at = period_type.calculate_expiration(event.effective_from, validity_days);

        let ledger = Self::write_rule_ledger_in_tx(
            tx,
            event_id,
            rule_id,
            bucket_id,
            event.user_id,
            &event.realm_id,
            CreditType::FreePeriodicCredit,
            source_type,
            points_per_period,
            validity_days,
            Some(event.effective_from),
            expires_at,
            now,
        )
        .await?;
        Self::write_rule_transaction_in_tx(
            tx,
            event_id,
            rule_id,
            bucket_id,
            event.user_id,
            &event.realm_id,
            CreditType::FreePeriodicCredit,
            source_type,
            points_per_period,
            now,
        )
        .await?;
        Self::write_grant_record_in_tx(
            tx,
            schedule_id,
            ledger.id,
            event.user_id,
            &event.realm_id,
            period_number,
            points_per_period,
            event.effective_from,
        )
        .await?;

        let should_stop = period_type.should_stop(period_number, max_periods);
        let next_grant_time = period_type.next_grant_time(base_time, period_number);
        sqlx::query(
            "UPDATE points_grant_schedules \
             SET next_grant_time = $2, granted_periods = $3, active = $4, updated_at = NOW() \
             WHERE id = $1",
        )
        .bind(schedule_id)
        .bind(next_grant_time)
        .bind(period_number)
        .bind(!should_stop)
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(ledger)
    }

    /// Expiration for a plain (non-periodic) fixed grant. validity_days == 0
    /// ⟺ permanent (None). credit_type drives nothing special here today but
    /// is accepted so future per-type validity rules live in one place.
    fn fixed_expires_at(
        validity_days: i64,
        _credit_type: CreditType,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        if validity_days == 0 {
            None
        } else {
            Some(chrono::Utc::now() + chrono::Duration::days(validity_days))
        }
    }

    /// Write a rule-attributed ledger row + ensure its wallet exists + advance
    /// the wallet lifetime totals. Returns the persisted ledger.
    async fn write_rule_ledger_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        event_id: Uuid,
        rule_id: Uuid,
        bucket_id: Uuid,
        user_id: Uuid,
        realm_id: &str,
        credit_type: CreditType,
        source_type: CreditSourceType,
        amount: i64,
        _validity_days: i64,
        effective_at: Option<chrono::DateTime<chrono::Utc>>,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<PointsCreditLedger, CoreError> {
        let wallet = Self::ensure_wallet_in_tx(tx, realm_id, user_id, bucket_id).await?;
        if wallet.status != WalletStatus::Active {
            return Err(CoreError::BadRequest(format!(
                "Cannot grant points to {} wallet",
                wallet.status.as_str()
            )));
        }
        let ledger = PointsCreditLedger {
            id: Uuid::now_v7(),
            user_id,
            realm_id: realm_id.to_string(),
            bucket_id,
            credit_type,
            source_type,
            source_id: format!("distribution:{event_id}"),
            granted_amount: amount,
            used_amount: 0,
            revoked_amount: 0,
            remaining_amount: amount,
            expires_at,
            effective_at,
            status: CreditLedgerStatus::Active,
            created_at: now,
            updated_at: now,
            distribution_event_id: Some(event_id),
            distribution_rule_id: Some(rule_id),
        };
        let created = Self::create_ledger_in_tx(tx, &ledger).await?;
        let delta = WalletDelta::grant(credit_type, amount);
        let _ = Self::apply_wallet_delta_in_tx(tx, wallet.id, delta).await?;
        Ok(created)
    }

    /// Write the rule-attributed `points_transactions` row for a grant. Uses the
    /// derived bucket available balance as `balance_after` so it matches the
    /// user-visible semantics.
    async fn write_rule_transaction_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        event_id: Uuid,
        rule_id: Uuid,
        bucket_id: Uuid,
        user_id: Uuid,
        realm_id: &str,
        credit_type: CreditType,
        source_type: CreditSourceType,
        amount: i64,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), CoreError> {
        let wallet = Self::ensure_wallet_in_tx(tx, realm_id, user_id, bucket_id).await?;
        let derived = Self::compute_available_balance_in_tx(
            tx,
            realm_id,
            user_id,
            std::slice::from_ref(&bucket_id),
            now,
        )
        .await?;
        let (balance_after, topup_after, subscription_after) =
            Self::derived_to_balance_snapshots(&derived);
        let transaction_type = Self::determine_transaction_type(credit_type, source_type);
        let external_ref_id = format!("distribution:{event_id}:{rule_id}");
        let txn = PointsTransaction {
            id: Uuid::now_v7(),
            wallet_id: wallet.id,
            user_id,
            realm_id: realm_id.to_string(),
            bucket_id,
            transaction_type,
            amount,
            balance_after,
            topup_balance_after: topup_after,
            subscription_balance_after: subscription_after,
            credit_type: Some(credit_type),
            description: Some(format!(
                "{}: {} points granted",
                source_type.as_str(),
                amount
            )),
            client_app_id: None,
            subscription_id: None,
            external_ref_id: Some(external_ref_id),
            correlation_id: None,
            effective_at: None,
            created_at: now,
            distribution_event_id: Some(event_id),
            distribution_rule_id: Some(rule_id),
        };
        let _ = Self::create_transaction_in_tx(tx, txn).await?;
        Ok(())
    }

    /// Write a `points_grant_records` bridge row linking
    /// `(schedule_id, period_number)` to its ledger, so replay can fold the
    /// schedule's first-period ledger out of the Fixed result set.
    async fn write_grant_record_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        schedule_id: Uuid,
        ledger_id: Uuid,
        user_id: Uuid,
        realm_id: &str,
        period_number: i64,
        granted_amount: i64,
        grant_time: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO points_grant_records \
                (id, schedule_id, user_id, realm_id, ledger_id, period_number, \
                 granted_amount, grant_time, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())",
        )
        .bind(Uuid::now_v7())
        .bind(schedule_id)
        .bind(user_id)
        .bind(realm_id)
        .bind(ledger_id)
        .bind(period_number)
        .bind(granted_amount)
        .bind(grant_time)
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    /// Write a `points_grant_schedules` row with NOT NULL distribution
    /// attribution. `granted_periods = 1` and `next_grant_time` = base + 1
    /// period because the first period was just granted inline.
    async fn write_schedule_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        schedule_id: Uuid,
        event_id: Uuid,
        rule_id: Uuid,
        bucket_id: Uuid,
        user_id: Uuid,
        realm_id: &str,
        period_type: herald_domain::points::grant_schedule::GrantPeriodType,
        base_time: chrono::DateTime<chrono::Utc>,
        points_per_period: i64,
        validity_days: i64,
    ) -> Result<(), CoreError> {
        let next_grant_time = period_type.next_grant_time(base_time, 1);
        sqlx::query(
            "INSERT INTO points_grant_schedules \
                (id, user_id, realm_id, bucket_id, subscription_id, entitlement_key, \
                 grant_period_type, base_time, next_grant_time, points_per_period, \
                 validity_days, granted_periods, max_periods, active, \
                 distribution_event_id, distribution_rule_id, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, NULL, '', $5, $6, $7, $8, $9, 1, NULL, $10, $11, $12, NOW(), NOW())",
        )
        .bind(schedule_id)
        .bind(user_id)
        .bind(realm_id)
        .bind(bucket_id)
        .bind(period_type.to_string())
        .bind(base_time)
        .bind(next_grant_time)
        .bind(points_per_period)
        .bind(validity_days)
        .bind(!period_type.should_stop(1, None))
        .bind(event_id)
        .bind(rule_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    /// Write a rule-attributed `points_quota_entitlements` row. The unique
    /// `(distribution_event_id, distribution_rule_id)` partial index makes the
    /// rule-attributed grant idempotent at the (event, rule) level.
    async fn write_quota_entitlement_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        event_id: Uuid,
        rule_id: Uuid,
        bucket_id: Uuid,
        user_id: Uuid,
        realm_id: &str,
        credit_type: CreditType,
        source_type: herald_domain::points::entities::QuotaSourceType,
        source_id: &str,
        windows: &[QuotaWindow],
        effective_from: chrono::DateTime<chrono::Utc>,
        effective_until: Option<chrono::DateTime<chrono::Utc>>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<PointsQuotaEntitlement, CoreError> {
        let windows_json = serialize_quota_windows_value(windows)?;
        let id = Uuid::now_v7();
        let idempotency_key = format!("distribution:{event_id}:{rule_id}");
        let row = sqlx::query_as::<_, PointsQuotaEntitlementRow>(
            "INSERT INTO points_quota_entitlements \
                (id, user_id, realm_id, bucket_id, credit_type, source_type, source_id, \
                 quota_windows, effective_from, effective_until, status, idempotency_key, \
                 distribution_event_id, distribution_rule_id, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'active', $11, $12, $13, $14, $14) \
             RETURNING *",
        )
        .bind(id)
        .bind(user_id)
        .bind(realm_id)
        .bind(bucket_id)
        .bind(credit_type.as_str())
        .bind(source_type.as_str())
        .bind(source_id)
        .bind(&windows_json)
        .bind(effective_from)
        .bind(effective_until)
        .bind(&idempotency_key)
        .bind(event_id)
        .bind(rule_id)
        .bind(now)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
        Self::row_to_points_quota_entitlement(row)
    }
}

/// Outcome of attempting to insert a `processing` event row.
enum EventInsertOutcome {
    /// This transaction inserted the processing row; the caller owns first-run
    /// execution.
    InsertedProcessing(Uuid),
    /// A row for this key already exists (committed by another caller). When
    /// `status == "completed"` the caller replays; when `status ==
    /// "processing"` another in-flight caller holds it and the caller retries.
    Existing { id: Uuid, status: String },
}

/// Hydrate a `PointsDistributionRule` from a raw sqlx row carrying the rule
/// columns. Used by the executor's rule-loading helpers so they stay on raw
/// sqlx (sharing the caller's transaction) rather than SeaORM, which cannot run
/// against a raw `sqlx::Transaction`.
fn rule_row_to_domain(row: &sqlx::postgres::PgRow) -> Result<PointsDistributionRule, CoreError> {
    use sqlx::Row;
    let owner = match row.try_get::<String, _>("owner_type") {
        Ok(s) if s == "entitlement_mapping" => {
            DistributionRuleOwner::EntitlementMapping(row.get("entitlement_mapping_id"))
        }
        _ => DistributionRuleOwner::RealmRegistration,
    };
    let trigger_sources: Vec<String> = row.get("trigger_sources");
    let trigger_sources = trigger_sources
        .iter()
        .filter_map(|s| match s.parse::<DistributionTrigger>() {
            Ok(t) => Some(t),
            Err(_) => {
                tracing::warn!(
                    trigger = %s,
                    "Unknown distribution trigger on rule; dropping from parsed set"
                );
                None
            }
        })
        .collect();
    let grant_mode: String = row.get("grant_mode");
    let policy = if grant_mode == "quota" {
        let raw: Option<serde_json::Value> = row.get("quota_windows");
        DistributionPolicy::Quota {
            windows: parse_quota_windows_value(raw).unwrap_or_default(),
        }
    } else {
        let grant_period_type: Option<String> = row.get("grant_period_type");
        DistributionPolicy::Fixed {
            amount: row.get("points_amount"),
            validity_days: row.get("validity_days"),
            grant_period_type: grant_period_type.as_deref().and_then(|s| s.parse().ok()),
        }
    };
    Ok(PointsDistributionRule {
        id: row.get("id"),
        realm_id: row.get("realm_id"),
        owner,
        bucket_id: row.get("bucket_id"),
        trigger_sources,
        policy,
        enabled: row.get("enabled"),
        display_order: row.get("display_order"),
    })
}

/// Owned-rule stable selection (the trait helper takes `&[PointsDistributionRule]`
/// by reference; the executor owns the loaded set so adapt to owned values).
fn select_and_sort_rules_owned(
    rules: Vec<PointsDistributionRule>,
    trigger: DistributionTrigger,
) -> Vec<PointsDistributionRule> {
    // De-dup + filter + stable (display_order, rule_id) via the pure helper;
    // `PointsDistributionRule: Clone`, so collect owned directly.
    select_and_sort_rules(&rules, trigger)
        .into_iter()
        .cloned()
        .collect()
}

/// Captured-snapshot stable selection: keep rules declaring `trigger`, using
/// the captured bucket id (not the rule's current bucket). Stable
/// `(display_order, rule_id)` order, de-dup by rule id.
fn select_and_sort_captured(
    captured: Vec<(PointsDistributionRule, Uuid)>,
    trigger: DistributionTrigger,
) -> Vec<(PointsDistributionRule, Uuid)> {
    let mut picked: Vec<(PointsDistributionRule, Uuid)> = captured
        .into_iter()
        .filter(|(r, _)| r.trigger_sources.contains(&trigger))
        .collect();
    picked.sort_by(|a, b| {
        a.0.display_order
            .cmp(&b.0.display_order)
            .then_with(|| a.0.id.cmp(&b.0.id))
    });
    let mut out: Vec<(PointsDistributionRule, Uuid)> = Vec::with_capacity(picked.len());
    for item in picked {
        if !out.iter().any(|(r, _)| r.id == item.0.id) {
            out.push(item);
        }
    }
    out
}

impl PointsRepository for PostgresPointsRepository {
    /// User-total wallet view (read path for `get_balance` / `get_wallet`).
    /// A user may hold one wallet row per Bucket; this returns a single
    /// aggregated `PointsWallet` with `bucket_id = None` whose balance fields
    /// are the SUM across the user's per-bucket wallet rows. For a single-bucket
    /// user the result equals that bucket's row.
    /// O(1) per row: reads the maintained projection columns, never aggregates
    /// the ledger. Returns `None` if the user has no wallet row.
    async fn find_by_user_id(
        &self,
        realm_id: &str,
        user_id: Uuid,
    ) -> Result<Option<PointsWallet>, CoreError> {
        let rows: Vec<PointsWalletRow> = sqlx::query_as::<_, PointsWalletRow>(
            "SELECT * FROM points_wallets WHERE realm_id = $1 AND user_id = $2 ORDER BY created_at ASC",
        )
        .bind(realm_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        if rows.is_empty() {
            return Ok(None);
        }

        // Aggregate into a single user-total view. Bucket_id = None signals
        // "not tied to a specific pool" (matches the synthesized zero-balance
        // wallet used when no row exists). Status reflects the most restrictive
        // pool (any non-active row dominates); created_at/updated_at from the
        // oldest / newest row respectively so the view is monotonic.
        // Only analytics columns are aggregated;
        // available balance is derived separately via `compute_available_balance`.
        let mut agg = PointsWallet {
            id: rows[0].id,
            user_id,
            realm_id: realm_id.to_string(),
            bucket_id: None,
            total_topup_granted: 0,
            total_subscription_granted: 0,
            total_recharged: 0,
            total_consumed: 0,
            status: WalletStatus::Active,
            created_at: chrono::DateTime::from(rows[0].created_at),
            updated_at: chrono::DateTime::from(rows[0].updated_at),
        };
        for row in rows {
            agg.total_recharged += row.total_recharged;
            agg.total_consumed += row.total_consumed;
            agg.total_topup_granted += row.total_topup_granted;
            agg.total_subscription_granted += row.total_subscription_granted;
            let created = chrono::DateTime::from(row.created_at);
            let updated = chrono::DateTime::from(row.updated_at);
            if created < agg.created_at {
                agg.created_at = created;
            }
            if updated > agg.updated_at {
                agg.updated_at = updated;
            }
            let status = WalletStatus::from_str(&row.status).map_err(|_| {
                CoreError::BadRequest(format!("Invalid wallet status: {}", row.status))
            })?;
            if !matches!(status, WalletStatus::Active) {
                agg.status = status;
            }
        }
        Ok(Some(agg))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<PointsWallet>, CoreError> {
        let result = points_wallet::Entity::find_by_id(id)
            .one(&*self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        result.map(Self::model_to_points_wallet).transpose()
    }

    async fn create_wallet(&self, account: PointsWallet) -> Result<PointsWallet, CoreError> {
        let active_model = Self::points_wallet_to_active_model(account);

        let result = active_model.insert(&*self.db).await.map_err(|e| {
            // Check for unique constraint violation
            if e.to_string()
                .contains(constraints::UK_POINTS_WALLETS_USER_ID)
            {
                CoreError::BadRequest("Points wallet already exists for this user".to_string())
            } else {
                CoreError::DatabaseError(e.to_string())
            }
        })?;

        Self::model_to_points_wallet(result)
    }

    async fn create_transaction(
        &self,
        transaction: PointsTransaction,
    ) -> Result<PointsTransaction, CoreError> {
        let active_model = Self::points_transaction_to_active_model(transaction);

        let result = active_model
            .insert(&*self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Self::model_to_points_transaction(result)
    }

    async fn find_transaction_by_id(
        &self,
        realm_id: &str,
        transaction_id: Uuid,
    ) -> Result<Option<PointsTransaction>, CoreError> {
        let result = points_transaction::Entity::find()
            .filter(points_transaction::Column::RealmId.eq(realm_id))
            .filter(points_transaction::Column::Id.eq(transaction_id))
            .one(&*self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        result.map(Self::model_to_points_transaction).transpose()
    }

    async fn find_expired_recharge_transactions(
        &self,
        realm_id: &str,
        user_id: Uuid,
    ) -> Result<Vec<PointsTransaction>, CoreError> {
        // Use raw SQL to find expired recharge transactions
        // We need to filter by expires_at < NOW() which is not easily expressed in SeaORM
        let query = r#"
            SELECT id, realm_id, wallet_id, user_id, type, amount, balance_after,
                   topup_balance_after, subscription_balance_after, credit_type,
                   description, client_app_id, subscription_id, external_ref_id, correlation_id,
                   created_at, updated_at, expires_at
            FROM points_transactions
            WHERE realm_id = $1
              AND user_id = $2
              AND type = 'recharge'
              AND expires_at < NOW()
              AND expires_at IS NOT NULL
            ORDER BY expires_at ASC
        "#;

        let rows = sqlx::query_as::<_, PointsTransactionRow>(query)
            .bind(realm_id)
            .bind(user_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to find expired transactions: {}", e))
            })?;

        rows.into_iter()
            .map(Self::points_transaction_row_to_domain)
            .collect::<Result<Vec<_>, _>>()
    }

    async fn find_transactions(
        &self,
        realm_id: &str,
        filters: TransactionFilters,
    ) -> Result<Paginated<PointsTransaction>, CoreError> {
        let page = filters.page.unwrap_or(1);
        let page_size = filters.page_size.unwrap_or(20).min(100);

        // Count via the existing SeaORM filter builder (no JOIN needed).
        let count_query = Self::apply_transaction_filters(
            points_transaction::Entity::find()
                .filter(points_transaction::Column::RealmId.eq(realm_id)),
            &filters,
        );
        let total = count_query
            .count(&*self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        // Part B: raw-SQL paged fetch sourcing `effective_at`
        // for grant-type transactions. The
        // transaction→ledger relation has no FK, so we resolve the matching
        // ledger row via a LATERAL subquery that picks EXACTLY ONE row per
        // transaction. This fixes two issues with the prior plain LEFT JOIN:
        // R1 (fan-out): a plain LEFT JOIN on the OR-based LIKE/equality
        // conditions could match >1 ledger row per transaction (e.g. when
        // the same `source_id` is reused across multiple grants), duplicating
        // `points_transactions` rows and breaking pagination (`total` vs
        // `len(data)` and LIMIT/OFFSET). LATERAL + LIMIT 1 guarantees at
        // most one ledger row per transaction.
        // R2 (subscription-grant miss): subscription grants write
        // `ledger.source_id = "<entitlement_key>:<idempotency_key>"` and
        // `txn.external_ref_id = "<idempotency_key>"`. The prior third
        // branch `l.source_id LIKE (t.external_ref_id || ':%')` only matched
        // when the entitlement_key prefix was absent. The corrected
        // suffix-match branch `l.source_id LIKE '%:' || t.external_ref_id`
        // matches the `<entitlement_key>:<idempotency_key>` form regardless
        // of the entitlement-key prefix, so `points.manage` now returns the
        // real `effective_at` for subscription grants.
        // Match precedence (deterministic single pick via ORDER BY):
        // 1. Exact equality `t.external_ref_id = l.source_id` (strongest),
        // 2. Prefix form `t.external_ref_id LIKE l.source_id || ':%'`
        // (grant_points_atomic / pregrant: txn ref = `<source_id>:<tx_id>`),
        // 3. Suffix form `l.source_id LIKE '%:' || t.external_ref_id`
        // (subscription grant: ledger source_id = `<ek>:<idem_key>`),
        // then tie-break on `created_at DESC` for determinism.
        // Non-grant rows (consume/refund/expiration) have NULL or non-matching
        // external_ref_id and yield NULL effective_at (correct semantics).
        // The filter clauses mirror `apply_transaction_filters` exactly so
        // count and page stay consistent.
        let mut sql = String::from(
            r#"
            SELECT t.id, t.realm_id, t.wallet_id, t.user_id, t.bucket_id, t.type, t.amount,
                   t.balance_after, t.topup_balance_after, t.subscription_balance_after,
                   t.credit_type, t.description, t.client_app_id, t.subscription_id,
                   t.external_ref_id, t.correlation_id, l.effective_at,
                   t.created_at, t.updated_at, t.expires_at
            FROM points_transactions t
            LEFT JOIN LATERAL (
                SELECT l.effective_at, l.created_at, l.source_id
                FROM points_credit_ledger l
                WHERE l.realm_id = t.realm_id
                  AND l.user_id = t.user_id
                  AND l.bucket_id = t.bucket_id
                  AND t.external_ref_id IS NOT NULL
                  AND (
                      t.external_ref_id = l.source_id
                      OR t.external_ref_id LIKE (l.source_id || ':%')
                      OR l.source_id LIKE ('%:' || t.external_ref_id)
                  )
                ORDER BY
                    (t.external_ref_id = l.source_id) DESC,
                    (t.external_ref_id LIKE (l.source_id || ':%')) DESC,
                    l.created_at DESC
                LIMIT 1
            ) l ON true
            WHERE t.realm_id = $1
            "#,
        );
        let mut param_idx = 2u32;

        if filters.user_id.is_some() {
            sql.push_str(&format!(" AND t.user_id = ${}", param_idx));
            param_idx += 1;
        }
        if filters.bucket_id.is_some() {
            sql.push_str(&format!(" AND t.bucket_id = ${}", param_idx));
            param_idx += 1;
        }
        if filters.transaction_type.is_some() {
            sql.push_str(&format!(" AND t.type = ${}", param_idx));
            param_idx += 1;
        }
        if filters.client_app_id.is_some() {
            sql.push_str(&format!(" AND t.client_app_id = ${}", param_idx));
            param_idx += 1;
        }
        if filters.subscription_id.is_some() {
            sql.push_str(&format!(" AND t.subscription_id = ${}", param_idx));
            param_idx += 1;
        }
        if !filters.external_ref_id.is_empty() {
            sql.push_str(&format!(" AND t.external_ref_id = ${}", param_idx));
            param_idx += 1;
        }
        if filters.start_time.is_some() {
            sql.push_str(&format!(" AND t.created_at >= ${}", param_idx));
            param_idx += 1;
        }
        if filters.end_time.is_some() {
            sql.push_str(&format!(" AND t.created_at <= ${}", param_idx));
            param_idx += 1;
        }

        sql.push_str(" ORDER BY t.created_at DESC");
        sql.push_str(&format!(" LIMIT ${} OFFSET ${}", param_idx, param_idx + 1));
        let offset = (page.saturating_sub(1)) * page_size;

        // Bind in the same order as the WHERE clauses were appended, then the
        // trailing LIMIT/OFFSET. Re-evaluating each condition keeps the bind
        // type concrete (sqlx::Query::bind is monomorphic per call).
        let mut query = sqlx::query_as::<_, PointsTransactionRow>(&sql).bind(realm_id);
        if let Some(user_id) = filters.user_id {
            query = query.bind(user_id);
        }
        if let Some(bucket_id) = filters.bucket_id {
            query = query.bind(bucket_id);
        }
        if let Some(transaction_type) = &filters.transaction_type {
            query = query.bind(transaction_type.as_str());
        }
        if let Some(client_app_id) = filters.client_app_id {
            query = query.bind(client_app_id);
        }
        if let Some(subscription_id) = filters.subscription_id {
            query = query.bind(subscription_id);
        }
        if !filters.external_ref_id.is_empty() {
            query = query.bind(&filters.external_ref_id);
        }
        if let Some(start_time) = &filters.start_time {
            query = query.bind(start_time);
        }
        if let Some(end_time) = &filters.end_time {
            query = query.bind(end_time);
        }
        let rows = query
            .bind(page_size as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        let transactions = rows
            .into_iter()
            .map(Self::points_transaction_row_to_domain)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Paginated {
            total,
            page,
            page_size,
            data: transactions,
        })
    }

    async fn count_transactions(
        &self,
        realm_id: &str,
        filters: &TransactionFilters,
    ) -> Result<u64, CoreError> {
        let query = Self::apply_transaction_filters(
            points_transaction::Entity::find()
                .filter(points_transaction::Column::RealmId.eq(realm_id)),
            filters,
        );

        let count = query
            .count(&*self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(count)
    }

    async fn check_idempotency_key(
        &self,
        realm_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<Uuid>, CoreError> {
        // Query the idempotency_keys table
        let query = r#"
            SELECT transaction_id
            FROM idempotency_keys
            WHERE realm_id = $1 AND idempotency_key = $2
              AND status = 'completed'
            LIMIT 1
        "#;

        let result = sqlx::query_as::<_, (Uuid,)>(query)
            .bind(realm_id)
            .bind(idempotency_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to check idempotency key: {}", e))
            })?;

        Ok(result.map(|(transaction_id,)| transaction_id))
    }

    async fn record_idempotency_key(
        &self,
        realm_id: &str,
        idempotency_key: &str,
        transaction_id: Uuid,
    ) -> Result<(), CoreError> {
        // Insert into idempotency_keys table with ON CONFLICT DO NOTHING
        // This ensures idempotency at the database level
        let query = r#"
            INSERT INTO idempotency_keys (
                realm_id,
                idempotency_key,
                status,
                request_data,
                response_data,
                transaction_id,
                created_at,
                updated_at,
                expires_at
            ) VALUES (
                $1, $2, 'completed', '{}', '{}', $3, NOW(), NOW(), NOW() + INTERVAL '24 hours'
            )
            ON CONFLICT (realm_id, idempotency_key) DO NOTHING
        "#;

        sqlx::query(query)
            .bind(realm_id)
            .bind(idempotency_key)
            .bind(transaction_id)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                CoreError::DatabaseError(format!("Failed to record idempotency key: {}", e))
            })?;

        tracing::debug!(
            realm_id = %realm_id,
            idempotency_key = %idempotency_key,
            transaction_id = %transaction_id,
            "Recorded idempotency key"
        );

        Ok(())
    }

    async fn find_transaction_by_ref(
        &self,
        realm_id: &str,
        user_id: Uuid,
        external_ref_id: &str,
    ) -> Result<Option<PointsTransaction>, CoreError> {
        let result = points_transaction::Entity::find()
            .filter(points_transaction::Column::RealmId.eq(realm_id))
            .filter(points_transaction::Column::UserId.eq(user_id))
            .filter(points_transaction::Column::ExternalRefId.eq(external_ref_id))
            .one(&*self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        result.map(Self::model_to_points_transaction).transpose()
    }

    async fn list_wallets(
        &self,
        realm_id: &str,
        filters: WalletFilters,
    ) -> Result<Paginated<PointsWallet>, CoreError> {
        let page = filters.page.unwrap_or(1);
        let page_size = filters.page_size.unwrap_or(20).min(100);

        let query =
            points_wallet::Entity::find().filter(points_wallet::Column::RealmId.eq(realm_id));

        let query = self.apply_wallet_filters(query, realm_id, &filters).await?;

        let mut query = match query {
            Some(q) => q,
            None => {
                return Ok(Paginated {
                    total: 0,
                    page,
                    page_size,
                    data: vec![],
                });
            }
        };

        // Get total count
        let total = query
            .clone()
            .count(&*self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        // Order by created_at DESC
        query = query.order_by_desc(points_wallet::Column::CreatedAt);

        // Apply pagination
        let results = query
            .paginate(&*self.db, page_size)
            .fetch_page(page - 1)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        let accounts = results
            .into_iter()
            .map(Self::model_to_points_wallet)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Paginated {
            total,
            page,
            page_size,
            data: accounts,
        })
    }

    async fn count_wallets(
        &self,
        realm_id: &str,
        filters: &WalletFilters,
    ) -> Result<u64, CoreError> {
        let query =
            points_wallet::Entity::find().filter(points_wallet::Column::RealmId.eq(realm_id));

        let query = self.apply_wallet_filters(query, realm_id, filters).await?;

        let query = match query {
            Some(q) => q,
            None => return Ok(0),
        };

        let count = query
            .count(&*self.db)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

        Ok(count)
    }

    fn create_ledger(
        &self,
        ledger: PointsCreditLedger,
    ) -> impl std::future::Future<Output = Result<PointsCreditLedger, CoreError>> + Send {
        let db = self.db.clone();
        async move {
            let active_model = points_credit_ledger_to_active_model(&ledger);
            let result = active_model
                .insert(&*db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            Ok(points_credit_ledger_from_model(result))
        }
    }

    fn find_ledgers_by_user_id(
        &self,
        realm_id: &str,
        user_id: Uuid,
        filters: LedgerFilters,
    ) -> impl std::future::Future<Output = Result<Paginated<PointsCreditLedger>, CoreError>> + Send
    {
        let db = self.db.clone();
        let realm_id = realm_id.to_string();
        async move {
            let mut query = points_credit_ledger::Entity::find()
                .filter(points_credit_ledger::Column::RealmId.eq(&realm_id))
                .filter(points_credit_ledger::Column::UserId.eq(user_id));

            if let Some(credit_type) = filters.credit_type {
                query = query
                    .filter(points_credit_ledger::Column::CreditType.eq(credit_type.to_string()));
            }

            if let Some(status) = filters.status {
                query = query.filter(points_credit_ledger::Column::Status.eq(status.to_string()));
            }

            if let Some(start_time) = filters.start_time {
                query = query.filter(points_credit_ledger::Column::CreatedAt.gte(start_time));
            }

            if let Some(end_time) = filters.end_time {
                query = query.filter(points_credit_ledger::Column::CreatedAt.lte(end_time));
            }

            // Always order by created_at DESC for ledger queries (newest first)
            query = query.order_by_desc(points_credit_ledger::Column::CreatedAt);

            let pagination = filters.pagination.unwrap_or_default();

            let total = query
                .clone()
                .count(&*db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            let items = query
                .paginate(&*db, pagination.page_size)
                .fetch_page((pagination.page - 1) * pagination.page_size)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            let items: Result<Vec<PointsCreditLedger>, CoreError> = items
                .into_iter()
                .map(|m| Ok(points_credit_ledger_from_model(m)))
                .collect();
            let items = items?;

            Ok(Paginated {
                data: items,
                total,
                page: pagination.page,
                page_size: pagination.page_size,
            })
        }
    }

    fn find_ledger_by_id(
        &self,
        ledger_id: Uuid,
    ) -> impl std::future::Future<Output = Result<Option<PointsCreditLedger>, CoreError>> + Send
    {
        let db = self.db.clone();
        async move {
            let result = points_credit_ledger::Entity::find_by_id(ledger_id)
                .one(&*db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            Ok(result.map(points_credit_ledger_from_model))
        }
    }

    fn find_ledger_by_source_id(
        &self,
        realm_id: &str,
        source_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<PointsCreditLedger>, CoreError>> + Send
    {
        let db = self.db.clone();
        let realm_id = realm_id.to_string();
        let source_id = source_id.to_string();
        async move {
            let result = points_credit_ledger::Entity::find()
                .filter(points_credit_ledger::Column::RealmId.eq(&realm_id))
                .filter(points_credit_ledger::Column::SourceId.eq(&source_id))
                .one(&*db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            Ok(result.map(points_credit_ledger_from_model))
        }
    }

    fn find_consumption_allocations_by_correlation_id(
        &self,
        realm_id: &str,
        correlation_id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<ConsumptionAllocationView>, CoreError>> + Send
    {
        let db = self.db.clone();
        let realm_id = realm_id.to_string();
        let correlation_id = correlation_id.to_string();
        async move {
            // Two-step lookup: transactions sharing the correlation_id → their
            // allocations, each joined with its ledger to surface credit_type
            // (needed for the SDK consume response AllocationDetail).
            let txn_ids: Vec<Uuid> = points_transaction::Entity::find()
                .filter(points_transaction::Column::RealmId.eq(&realm_id))
                .filter(points_transaction::Column::CorrelationId.eq(&correlation_id))
                .all(&*db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?
                .into_iter()
                .map(|m| m.id)
                .collect();

            if txn_ids.is_empty() {
                return Ok(Vec::new());
            }

            let rows = points_consumption_allocation::Entity::find()
                .filter(points_consumption_allocation::Column::TransactionId.is_in(txn_ids))
                .find_also_related(points_credit_ledger::Entity)
                .all(&*db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            rows.into_iter()
                .map(|(alloc_model, ledger_model)| {
                    let ledger = ledger_model.ok_or_else(|| {
                        CoreError::DatabaseError(format!(
                            "consumption allocation {} missing parent ledger {}",
                            alloc_model.id, alloc_model.ledger_id
                        ))
                    })?;
                    let credit_type = CreditType::from_str(&ledger.credit_type)?;
                    Ok(ConsumptionAllocationView {
                        allocation: points_consumption_allocation_from_model(alloc_model),
                        credit_type,
                    })
                })
                .collect()
        }
    }

    fn create_revocation_record(
        &self,
        record: PointsRevocationRecord,
    ) -> impl std::future::Future<Output = Result<PointsRevocationRecord, CoreError>> + Send {
        let db = self.db.clone();
        async move {
            let active_model = points_revocation_record_to_active_model(&record);
            let result = active_model
                .insert(&*db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            Ok(points_revocation_record_from_model(result))
        }
    }

    fn find_expired_ledgers(
        &self,
        expiration_time: chrono::DateTime<chrono::Utc>,
        limit: usize,
    ) -> impl std::future::Future<Output = Result<Vec<PointsCreditLedger>, CoreError>> + Send {
        let db = self.db.clone();
        async move {
            let ledgers = points_credit_ledger::Entity::find()
                .filter(points_credit_ledger::Column::ExpiresAt.lte(
                    sea_orm::prelude::DateTimeWithTimeZone::from(expiration_time),
                ))
                .filter(
                    points_credit_ledger::Column::Status.eq(CreditLedgerStatus::Active.to_string()),
                )
                .filter(points_credit_ledger::Column::RemainingAmount.gt(0))
                .order_by_asc(points_credit_ledger::Column::ExpiresAt)
                .limit(limit as u64)
                .all(&*db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            ledgers
                .into_iter()
                .map(|m| Ok(points_credit_ledger_from_model(m)))
                .collect()
        }
    }

    fn find_due_grant_schedules(
        &self,
        before: chrono::DateTime<chrono::Utc>,
        limit: u64,
    ) -> impl std::future::Future<
        Output = Result<Vec<herald_domain::points::grant_schedule::PointsGrantSchedule>, CoreError>,
    > + Send {
        let db = self.db.clone();
        async move {
            let models = points_grant_schedule::Entity::find()
                .filter(
                    points_grant_schedule::Column::NextGrantTime
                        .lte(sea_orm::prelude::DateTimeWithTimeZone::from(before)),
                )
                .filter(points_grant_schedule::Column::Active.eq(true))
                .order_by_asc(points_grant_schedule::Column::NextGrantTime)
                .limit(limit)
                .all(&*db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            models
                .into_iter()
                .map(Self::model_to_grant_schedule)
                .collect()
        }
    }

    fn consume_points_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        client_app_id: Uuid,
        amount: i64,
        description: Option<String>,
        idempotency_key: Option<String>,
    ) -> impl std::future::Future<Output = Result<Vec<PointsTransaction>, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        async move {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            // consume, reassemble the original result set WITHOUT re-deducting.
            // Primary transaction → correlation_id → all N sibling transactions
            // (ordered by bucket_id) + their allocations. Legacy single-pool rows
            // with NULL correlation_id replay by the single primary transaction_id.
            if let Some(ref key) = idempotency_key
                && let Some(primary_txn_id) =
                    Self::check_completed_idempotency_in_tx(&mut tx, &realm_id, key).await?
            {
                let replayed =
                    Self::replay_consume_by_primary(&mut tx, &realm_id, primary_txn_id).await?;
                tx.commit()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                return Ok(replayed);
            }

            let correlation_id = Uuid::now_v7().to_string();

            // only, enabled=true; no default-bucket merging).
            let covered_bucket_ids =
                Self::find_covered_bucket_ids_in_tx(&mut tx, &realm_id, client_app_id).await?;
            if covered_bucket_ids.is_empty() {
                return Err(CoreError::NoCoveredPointsPool { client_app_id });
            }

            // `points_wallets FOR UPDATE` per bucket, bucket_id ASC) BEFORE the
            // window+pool availability computation. This serializes the
            // mixed-consume coordination: two concurrent
            // consumes for the same (user, bucket) compute window+pool spendable
            // under the same row lock, so neither can overdraw a window or the
            // pool. The window-quota model is per-(user, bucket, credit_type), so
            // the lock must cover every bucket whose entitlements may be drawn.
            // A closed/frozen wallet is rejected wholesale (account-status rule).
            let now = chrono::Utc::now();
            use std::collections::BTreeMap;
            let mut bucket_wallets: BTreeMap<Uuid, Uuid> = BTreeMap::new();
            for bucket_id in &covered_bucket_ids {
                let wallet =
                    Self::ensure_wallet_in_tx(&mut tx, &realm_id, user_id, *bucket_id).await?;
                if wallet.status != WalletStatus::Active {
                    return Err(CoreError::BadRequest(format!(
                        "Cannot consume points from {} wallet",
                        wallet.status.as_str()
                    )));
                }
                bucket_wallets.insert(*bucket_id, wallet.id);
            }

            // Lock pool ledgers after the bucket wallet rows. Refund/revoke paths
            // also lock wallet before ledger, so this ordering avoids wallet<->ledger
            // deadlocks under concurrent consume/refund races.
            let ledgers = Self::find_active_ledgers_by_expiration_for_update(
                &mut tx,
                &realm_id,
                user_id,
                &covered_bucket_ids,
            )
            .await?;

            // window_avail_total = Σ over covered buckets of (min over active
            // windows of (limit − used)) for BOTH window credit_types
            // (subscription_credit + free_periodic_credit). Pool_avail is the
            // existing locked-ledger remaining sum (unchanged source).
            // `plan_mixed_consume` rejects wholesale when
            // `amount > window_avail + pool_avail` (no partial deduction).
            let pool_available: i64 = ledgers.iter().map(|l| l.remaining_amount).sum();
            let mut window_avail_total: i64 = 0;
            for bucket_id in &covered_bucket_ids {
                let sub_spendable = Self::compute_window_spendable_in_tx(
                    &mut tx,
                    &realm_id,
                    user_id,
                    *bucket_id,
                    CreditType::SubscriptionCredit,
                    now,
                )
                .await?;
                let free_spendable = Self::compute_window_spendable_in_tx(
                    &mut tx,
                    &realm_id,
                    user_id,
                    *bucket_id,
                    CreditType::FreePeriodicCredit,
                    now,
                )
                .await?;
                // Per-bucket window headroom = subscription + free (each already
                // clamped ≥ 0 by the helper).
                window_avail_total += sub_spendable + free_spendable;
            }

            let mixed = plan_mixed_consume(window_avail_total, pool_available, amount);
            let (window_part, pool_part) = match mixed {
                MixedConsumePlan::Ok {
                    window_part,
                    pool_part,
                } => (window_part, pool_part),
                // Wholesale reject — rollback the whole transaction; NO partial
                // deduction.
                MixedConsumePlan::Insufficient => {
                    return Err(CoreError::insufficient_points(
                        amount,
                        window_avail_total + pool_available,
                    ));
                }
            };

            // Pool-side allocation plan, now scoped to `pool_part` only (the
            // window side does NOT touch the ledger). The plan / ledger-decrement
            // / per-bucket transaction bodies below are UNCHANGED — they simply
            // receive `pool_part` instead of `amount`.
            let plan = Self::plan_consume_allocation(&ledgers, pool_part);
            if pool_part > 0 && !plan.fully_covers {
                // Defensive: the gate already guaranteed pool_part ≤ pool_available,
                // so a non-covering plan is drift (concurrent ledger mutation
                // slipped past the wallet lock). Fail loud rather than under-deduct.
                return Err(CoreError::InternalServerError(format!(
                    "pool consume allocation drift: planned {} but ledgers cover < pool_part",
                    pool_part
                )));
            }

            // `per_bucket` keyed by bucket_id; processed in bucket_id ASC below for
            // deterministic wallet-lock ordering (deadlock avoidance).
            #[derive(Default)]
            struct BucketAccumulator {
                wallet_id: Option<Uuid>,
                total: i64,
            }
            let mut per_bucket: BTreeMap<Uuid, BucketAccumulator> = BTreeMap::new();
            let mut allocations: Vec<PointsConsumptionAllocation> = Vec::new();
            // transaction_id is assigned per-bucket during the write loop below;
            // allocations reference the transaction of their owning bucket.
            let mut bucket_txn_id: BTreeMap<Uuid, Uuid> = BTreeMap::new();

            for planned in &plan.allocations {
                let ledger = &ledgers[planned.ledger_index];
                let updated_ledger = Self::update_ledger_in_tx(
                    &mut tx,
                    ledger.id,
                    LedgerUpdate::Consumption(planned.amount),
                )
                .await?;

                let bucket_id = ledger.bucket_id;
                // The wallet for this (user, bucket) was already locked in
                // Step 4b; `bucket_wallets` holds its id. Re-resolve via
                // `ensure_wallet_in_tx` would re-lock the same row (a no-op in
                // the same tx), so reuse the locked id directly.
                let wallet_id = *bucket_wallets
                    .get(&bucket_id)
                    .expect("bucket_wallets covers every covered bucket (Step 4b)");
                per_bucket.entry(bucket_id).or_default().wallet_id = Some(wallet_id);

                let acc = per_bucket.entry(bucket_id).or_default();
                acc.total += planned.amount;

                // Defer the transaction_id binding: we allocate it lazily the first
                // time a bucket is touched so allocations can reference it.
                let txn_id = *bucket_txn_id.entry(bucket_id).or_insert_with(Uuid::now_v7);

                allocations.push(PointsConsumptionAllocation {
                    id: Uuid::now_v7(),
                    transaction_id: txn_id,
                    ledger_id: updated_ledger.id,
                    wallet_id: Some(wallet_id),
                    user_id,
                    realm_id: realm_id.clone(),
                    bucket_id,
                    allocated_amount: planned.amount,
                    ledger_remaining_after: updated_ledger.remaining_amount,
                    created_at: now,
                });
            }

            // Sanity: the pool plan guaranteed full coverage of `pool_part`;
            // guard against drift.
            let pool_consumed_total: i64 = per_bucket.values().map(|a| a.total).sum();
            if pool_consumed_total != pool_part {
                return Err(CoreError::InternalServerError(format!(
                    "consume pool allocation drift: planned {} but accumulated {}",
                    pool_part, pool_consumed_total
                )));
            }

            // Distribute `window_part` across covered buckets in bucket_id ASC
            // (window-first per bucket, mirroring the pool's greedy-in-order
            // pattern), and within each bucket split by credit_type priority
            // (subscription_credit first, free_periodic_credit 补足) via the
            // pure `split_window_part_by_credit_type`. Each non-zero part writes
            // ONE `points_transactions(type='consume', credit_type=<window type>)`
            // row — NO ledger decrement (the window model tracks usage via the
            // consume row itself, counted by `sum_consume_in_window`).
            // Overspend invariant (P0): per-bucket `sub_part ≤ subscription_spendable`
            // and `free_part ≤ free_spendable` (enforced by the pure split's
            // `min`); Σ `sub_part + free_part` ≤ `window_part` (any residual
            // overflows to the next bucket; the already guaranteed
            // `window_part ≤ window_avail_total`).
            let mut transactions: Vec<PointsTransaction> = Vec::new();
            let mut window_remaining = window_part;
            // Per-bucket window consumption totals, folded into the same
            // `WalletDelta.total_consumed` accounting as the pool side so wallet
            // analytics stay correct.
            let mut window_per_bucket: BTreeMap<Uuid, i64> = BTreeMap::new();
            for bucket_id in covered_bucket_ids.iter().copied() {
                if window_remaining <= 0 {
                    break;
                }
                let sub_spendable = Self::compute_window_spendable_in_tx(
                    &mut tx,
                    &realm_id,
                    user_id,
                    bucket_id,
                    CreditType::SubscriptionCredit,
                    now,
                )
                .await?;
                let free_spendable = Self::compute_window_spendable_in_tx(
                    &mut tx,
                    &realm_id,
                    user_id,
                    bucket_id,
                    CreditType::FreePeriodicCredit,
                    now,
                )
                .await?;
                let split = Self::split_window_part_by_credit_type(
                    window_remaining,
                    sub_spendable,
                    free_spendable,
                );
                let wallet_id = *bucket_wallets
                    .get(&bucket_id)
                    .expect("bucket_wallets covers every covered bucket (Step 4b)");

                for (credit_type, part) in [
                    (CreditType::SubscriptionCredit, split.sub_part),
                    (CreditType::FreePeriodicCredit, split.free_part),
                ] {
                    if part <= 0 {
                        continue;
                    }
                    // Wallet delta for this window consume (total_consumed only).
                    let delta = WalletDelta {
                        total_recharged: 0,
                        total_consumed: part,
                        total_topup_granted: 0,
                        total_subscription_granted: 0,
                    };
                    let _updated_wallet =
                        Self::apply_wallet_delta_in_tx(&mut tx, wallet_id, delta).await?;
                    // Pool-derived balance snapshot for this bucket (window rows
                    // do not touch ledger, so this is unaffected by the window
                    // consume — consistent with the pool-side transaction write).
                    let derived = Self::compute_available_balance_in_tx(
                        &mut tx,
                        &realm_id,
                        user_id,
                        std::slice::from_ref(&bucket_id),
                        now,
                    )
                    .await?;
                    let (balance_after, topup_after, subscription_after) =
                        Self::derived_to_balance_snapshots(&derived);
                    let transaction = Self::create_transaction_in_tx(
                        &mut tx,
                        PointsTransaction {
                            id: Uuid::now_v7(),
                            wallet_id,
                            user_id,
                            realm_id: realm_id.clone(),
                            bucket_id,
                            transaction_type: TransactionType::Consume,
                            amount: -part,
                            balance_after,
                            topup_balance_after: topup_after,
                            subscription_balance_after: subscription_after,
                            credit_type: Some(credit_type),
                            description: description.clone(),
                            client_app_id: Some(client_app_id),
                            subscription_id: None,
                            external_ref_id: None,
                            correlation_id: Some(correlation_id.clone()),
                            effective_at: None,
                            created_at: now,
                            distribution_event_id: None,
                            distribution_rule_id: None,
                        },
                    )
                    .await?;
                    transactions.push(transaction);
                    *window_per_bucket.entry(bucket_id).or_insert(0) += part;
                }
                window_remaining = split.window_remainder;
            }
            // The guaranteed window_part ≤ window_avail_total, so the
            // per-bucket distribution MUST have fully absorbed it. Fail loud on
            // drift (concurrent entitlement revoke slipped past the wallet lock).
            let window_consumed_total: i64 = window_per_bucket.values().sum();
            if window_consumed_total != window_part {
                return Err(CoreError::InternalServerError(format!(
                    "consume window allocation drift: planned {} but accumulated {}",
                    window_part, window_consumed_total
                )));
            }

            // (BTreeMap iterates ascending). One transaction per affected bucket,
            // all sharing correlation_id; external_ref_id NULL.
            // `balance_after` is the REAL post-consume derived SUM for
            // this bucket (same predicate as `compute_available_balance`),
            // sourced in-tx so it reflects the just-applied ledger mutations.
            // The per-type consume split is preserved in the ledger `used_amount`
            // increments (Step 5) and in the consumption allocations (Step 8),
            // not in any Stored balance column (dropped).
            for (bucket_id, acc) in per_bucket.iter() {
                let wallet_id = acc.wallet_id.ok_or_else(|| {
                    CoreError::InternalServerError(format!(
                        "consume: bucket {} accumulator missing wallet_id",
                        bucket_id
                    ))
                })?;
                let delta = WalletDelta {
                    total_recharged: 0,
                    total_consumed: acc.total,
                    total_topup_granted: 0,
                    total_subscription_granted: 0,
                };
                let _updated_wallet =
                    Self::apply_wallet_delta_in_tx(&mut tx, wallet_id, delta).await?;

                // Real post-consume derived snapshot for THIS bucket only.
                let derived = Self::compute_available_balance_in_tx(
                    &mut tx,
                    &realm_id,
                    user_id,
                    std::slice::from_ref(bucket_id),
                    now,
                )
                .await?;
                let (balance_after, topup_after, subscription_after) =
                    Self::derived_to_balance_snapshots(&derived);

                let txn_id = *bucket_txn_id
                    .get(bucket_id)
                    .expect("bucket_txn_id populated alongside per_bucket");

                let transaction = Self::create_transaction_in_tx(
                    &mut tx,
                    PointsTransaction {
                        id: txn_id,
                        wallet_id,
                        user_id,
                        realm_id: realm_id.clone(),
                        bucket_id: *bucket_id,
                        transaction_type: TransactionType::Consume,
                        amount: -acc.total,
                        balance_after,
                        topup_balance_after: topup_after,
                        subscription_balance_after: subscription_after,
                        credit_type: None,
                        description: description.clone(),
                        client_app_id: Some(client_app_id),
                        subscription_id: None,
                        external_ref_id: None,
                        correlation_id: Some(correlation_id.clone()),
                        effective_at: None,
                        created_at: now,
                        distribution_event_id: None,
                        distribution_rule_id: None,
                    },
                )
                .await?;
                transactions.push(transaction);
            }

            for allocation in &allocations {
                Self::create_consumption_allocation_in_tx(&mut tx, allocation).await?;
            }

            // ASC (transactions Vec is already in that order from the loop above).
            if let Some(ref key) = idempotency_key {
                let primary_txn_id = transactions.first().map(|t| t.id).ok_or_else(|| {
                    CoreError::InternalServerError("consume produced no transactions".to_string())
                })?;
                Self::record_completed_idempotency_in_tx(&mut tx, &realm_id, key, primary_txn_id)
                    .await?;
            }

            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(transactions)
        }
    }

    fn replay_consume_by_primary(
        &self,
        realm_id: &str,
        primary_txn_id: Uuid,
    ) -> impl std::future::Future<Output = Result<Vec<PointsTransaction>, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        async move {
            // Read-only replay. Opens its own short transaction so the HTTP-layer
            // Redis-cache replay path can reassemble the original per-bucket
            // result set without re-deducting. Legacy single-pool
            // rows (NULL correlation_id) replay as a 1-element vec.
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            let replayed =
                Self::replay_consume_by_primary(&mut tx, &realm_id, primary_txn_id).await?;
            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(replayed)
        }
    }

    fn revoke_points_by_credit_type_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        revocation_type: RevocationType,
        reason: String,
        reference_id: Option<String>,
        idempotency_key: Option<String>,
    ) -> impl std::future::Future<Output = Result<RevokePointsOutput, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        async move {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            if let Some(ref key) = idempotency_key
                && Self::check_completed_idempotency_in_tx(&mut tx, &realm_id, key)
                    .await?
                    .is_some()
            {
                tx.commit()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                return Ok(RevokePointsOutput::empty());
            }

            // 如果该 (user, bucket) 钱包不存在，返回"撤销 0 点"的结果（webhook 幂等处理）
            let _wallet = match Self::find_wallet_by_user_bucket_for_update(
                &mut tx, &realm_id, user_id, bucket_id,
            )
            .await?
            {
                Some(acc) => acc,
                None => {
                    tx.commit()
                        .await
                        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                    return Ok(RevokePointsOutput::empty());
                }
            };

            let ledgers = Self::find_active_ledgers_by_credit_type_and_bucket_for_update(
                &mut tx,
                &realm_id,
                user_id,
                bucket_id,
                credit_type,
            )
            .await?;

            let ledger_tuples: Vec<(Uuid, i64)> = ledgers
                .into_iter()
                .filter(|l| l.remaining_amount > 0)
                .map(|l| (l.id, l.remaining_amount))
                .collect();

            let (total_revoked, ledger_ids) = Self::revoke_ledger_list_in_tx(
                &mut tx,
                &realm_id,
                user_id,
                ledger_tuples,
                revocation_type,
                &reason,
                reference_id.as_deref(),
            )
            .await?;

            if let Some(ref key) = idempotency_key {
                Self::record_completed_idempotency_in_tx(&mut tx, &realm_id, key, Uuid::now_v7())
                    .await?;
            }

            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(RevokePointsOutput {
                revocation_id: Uuid::now_v7(),
                ledger_ids,
                total_revoked,
                revoked_at: chrono::Utc::now(),
            })
        }
    }

    fn revoke_points_by_source_id_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        source_id: &str,
        revocation_type: RevocationType,
        reason: String,
        reference_id: Option<String>,
        idempotency_key: Option<String>,
    ) -> impl std::future::Future<Output = Result<RevokePointsOutput, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        let source_id = source_id.to_string();
        async move {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            if let Some(ref key) = idempotency_key
                && Self::check_completed_idempotency_in_tx(&mut tx, &realm_id, key)
                    .await?
                    .is_some()
            {
                tx.commit()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                return Ok(RevokePointsOutput::empty());
            }

            let _wallet = match Self::find_wallet_by_user_bucket_for_update(
                &mut tx, &realm_id, user_id, bucket_id,
            )
            .await?
            {
                Some(acc) => acc,
                None => {
                    tx.commit()
                        .await
                        .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                    return Ok(RevokePointsOutput::empty());
                }
            };

            // Find the specific ledger by source_id (scoped to the target bucket
            // so we never touch credits belonging to a different pool)
            let ledger = sqlx::query_as::<_, (Uuid, i64, String)>(
                "SELECT id, remaining_amount, credit_type FROM points_credit_ledger WHERE realm_id = $1 AND user_id = $2 AND bucket_id = $3 AND source_id = $4 AND remaining_amount > 0 FOR UPDATE"
            )
            .bind(&realm_id)
            .bind(user_id)
            .bind(bucket_id)
            .bind(&source_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            let Some((ledger_id, remaining_amount, credit_type_str)) = ledger else {
                // No active ledger for this source_id — nothing to revoke
                tx.commit()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                return Ok(RevokePointsOutput::empty());
            };

            let _credit_type: CreditType = credit_type_str.as_str().parse().map_err(|_| {
                CoreError::DatabaseError(format!(
                    "Invalid credit_type in ledger: {}",
                    credit_type_str
                ))
            })?;

            // Update ledger remaining_amount to 0
            let updated_ledger = Self::update_ledger_in_tx(
                &mut tx,
                ledger_id,
                LedgerUpdate::Revocation(remaining_amount),
            )
            .await?;

            // Create revocation record
            let record = PointsRevocationRecord {
                id: Uuid::now_v7(),
                ledger_id: updated_ledger.id,
                user_id,
                realm_id: realm_id.clone(),
                revocation_type,
                revoked_amount: remaining_amount,
                reason,
                reference_id,
                created_at: chrono::Utc::now(),
            };
            Self::create_revocation_record_in_tx(&mut tx, &record).await?;

            if let Some(ref key) = idempotency_key {
                Self::record_completed_idempotency_in_tx(&mut tx, &realm_id, key, Uuid::now_v7())
                    .await?;
            }

            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            Ok(RevokePointsOutput {
                revocation_id: Uuid::now_v7(),
                ledger_ids: vec![updated_ledger.id],
                total_revoked: remaining_amount,
                revoked_at: chrono::Utc::now(),
            })
        }
    }

    fn revoke_topup_proportional_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        refund_amount: i64,
        original_payment_amount: i64,
        refund_id: &str,
    ) -> impl std::future::Future<Output = Result<RevokePointsOutput, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        let refund_id = refund_id.to_string();
        async move {
            let idempotency_key = format!("refund:topup:{}", refund_id);
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            if Self::check_completed_idempotency_in_tx(&mut tx, &realm_id, &idempotency_key)
                .await?
                .is_some()
            {
                tx.commit()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                return Ok(RevokePointsOutput::empty());
            }

            let ledgers = Self::find_active_ledgers_by_credit_type_and_bucket_for_update(
                &mut tx,
                &realm_id,
                user_id,
                bucket_id,
                CreditType::TopupCredit,
            )
            .await?;

            let mut total_revoked = 0i64;
            let mut ledger_ids = Vec::new();
            for ledger in ledgers {
                let amount_to_revoke = Self::proportional_refund_for_grant(
                    ledger.remaining_amount,
                    ledger.remaining_amount,
                    refund_amount,
                    original_payment_amount,
                );
                if amount_to_revoke <= 0 {
                    continue;
                }

                let updated_ledger = Self::update_ledger_in_tx(
                    &mut tx,
                    ledger.id,
                    LedgerUpdate::Revocation(amount_to_revoke),
                )
                .await?;
                Self::create_revocation_record_in_tx(
                    &mut tx,
                    &PointsRevocationRecord {
                        id: Uuid::now_v7(),
                        ledger_id: updated_ledger.id,
                        user_id,
                        realm_id: realm_id.clone(),
                        revocation_type: RevocationType::RefundRevoke,
                        revoked_amount: amount_to_revoke,
                        reason: format!(
                            "Proportional refund ({}/{})",
                            refund_amount, original_payment_amount
                        ),
                        reference_id: Some(refund_id.clone()),
                        created_at: chrono::Utc::now(),
                    },
                )
                .await?;
                total_revoked += amount_to_revoke;
                ledger_ids.push(updated_ledger.id);
            }

            Self::record_completed_idempotency_in_tx(
                &mut tx,
                &realm_id,
                &idempotency_key,
                Uuid::now_v7(),
            )
            .await?;
            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(RevokePointsOutput {
                revocation_id: Uuid::now_v7(),
                ledger_ids,
                total_revoked,
                revoked_at: chrono::Utc::now(),
            })
        }
    }

    fn revoke_topup_source_proportional_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        source_id: &str,
        refund_amount: i64,
        original_payment_amount: i64,
        refund_id: &str,
    ) -> impl std::future::Future<Output = Result<RevokePointsOutput, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        let source_id = source_id.to_string();
        let refund_id = refund_id.to_string();
        async move {
            let idempotency_key = format!("refund:topup:{}", refund_id);
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            if Self::check_completed_idempotency_in_tx(&mut tx, &realm_id, &idempotency_key)
                .await?
                .is_some()
            {
                tx.commit()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                return Ok(RevokePointsOutput::empty());
            }

            let ledgers = sqlx::query_as::<_, PointsCreditLedgerRow>(
                "SELECT l.* \
                 FROM points_credit_ledger l \
                 JOIN points_distribution_events e ON e.id = l.distribution_event_id \
                 WHERE e.realm_id = $1 AND e.user_id = $2 AND e.source_id = $3 \
                   AND l.credit_type = 'topup_credit' \
                   AND l.distribution_rule_id IS NOT NULL \
                   AND l.status = 'active' AND l.remaining_amount > 0 \
                 ORDER BY l.bucket_id, l.id FOR UPDATE OF l",
            )
            .bind(&realm_id)
            .bind(user_id)
            .bind(&source_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?
            .into_iter()
            .map(Self::row_to_points_credit_ledger)
            .collect::<Result<Vec<_>, _>>()?;

            let mut total_revoked = 0i64;
            let mut ledger_ids = Vec::new();
            for ledger in ledgers {
                // Calculate independently from each original rule grant, then
                // cap at its unused balance so unrelated credits are untouched.
                let amount_to_revoke = Self::proportional_refund_for_grant(
                    ledger.granted_amount,
                    ledger.remaining_amount,
                    refund_amount,
                    original_payment_amount,
                );
                if amount_to_revoke <= 0 {
                    continue;
                }

                let updated_ledger = Self::update_ledger_in_tx(
                    &mut tx,
                    ledger.id,
                    LedgerUpdate::Revocation(amount_to_revoke),
                )
                .await?;

                let record = PointsRevocationRecord {
                    id: Uuid::now_v7(),
                    ledger_id: updated_ledger.id,
                    user_id,
                    realm_id: realm_id.clone(),
                    revocation_type: RevocationType::RefundRevoke,
                    revoked_amount: amount_to_revoke,
                    reason: format!(
                        "Proportional refund ({}/{})",
                        refund_amount, original_payment_amount
                    ),
                    reference_id: Some(refund_id.clone()),
                    created_at: chrono::Utc::now(),
                };
                Self::create_revocation_record_in_tx(&mut tx, &record).await?;

                total_revoked += amount_to_revoke;
                ledger_ids.push(updated_ledger.id);
            }

            Self::record_completed_idempotency_in_tx(
                &mut tx,
                &realm_id,
                &idempotency_key,
                Uuid::now_v7(),
            )
            .await?;

            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            Ok(RevokePointsOutput {
                revocation_id: Uuid::now_v7(),
                ledger_ids,
                total_revoked,
                revoked_at: chrono::Utc::now(),
            })
        }
    }

    fn grant_points_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        source_type: CreditSourceType,
        amount: i64,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
        effective_at: Option<chrono::DateTime<chrono::Utc>>,
        source_id: Option<String>,
        description: Option<String>,
        idempotency_key: Option<String>,
    ) -> impl std::future::Future<Output = Result<PointsCreditLedger, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        async move {
            let user_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM account WHERE id = $1 AND realm_id = $2)",
            )
            .bind(user_id)
            .bind(&realm_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            if !user_exists {
                return Err(CoreError::NotFound);
            }

            // The bucket must exist in the grant's realm: the wallet/ledger
            // FK alone would accept a bucket UUID from another realm, silently
            // writing cross-tenant references into wallet and ledger rows
            // (bucket ids surface in transactions, so they are not secret).
            let bucket_in_realm: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM credit_buckets WHERE id = $1 AND realm_id = $2)",
            )
            .bind(bucket_id)
            .bind(&realm_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            if !bucket_in_realm {
                return Err(CoreError::NotFound);
            }

            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            // Idempotency guard: if a completed record exists, return a zero-amount placeholder
            if let Some(ref key) = idempotency_key
                && Self::check_completed_idempotency_in_tx(&mut tx, &realm_id, key)
                    .await?
                    .is_some()
            {
                tx.commit()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                let now = chrono::Utc::now();
                return Ok(PointsCreditLedger {
                    id: Uuid::now_v7(),
                    user_id,
                    realm_id,
                    bucket_id,
                    credit_type,
                    source_type,
                    source_id: "idempotency".to_string(),
                    granted_amount: 0,
                    used_amount: 0,
                    revoked_amount: 0,
                    remaining_amount: 0,
                    expires_at: None,
                    effective_at,
                    status: CreditLedgerStatus::Active,
                    created_at: now,
                    updated_at: now,
                    distribution_event_id: None,
                    distribution_rule_id: None,
                });
            }

            let wallet = Self::ensure_wallet_in_tx(&mut tx, &realm_id, user_id, bucket_id).await?;
            if wallet.status != WalletStatus::Active {
                return Err(CoreError::BadRequest(format!(
                    "Cannot grant points to {} wallet",
                    wallet.status.as_str()
                )));
            }
            let source_id = source_id.unwrap_or_else(|| "system".to_string());
            let now = chrono::Utc::now();
            let ledger = PointsCreditLedger {
                id: Uuid::now_v7(),
                user_id,
                realm_id: realm_id.clone(),
                bucket_id,
                credit_type,
                source_type,
                source_id: source_id.clone(),
                granted_amount: amount,
                used_amount: 0,
                revoked_amount: 0,
                remaining_amount: amount,
                expires_at,
                effective_at,
                status: CreditLedgerStatus::Active,
                created_at: now,
                updated_at: now,
                distribution_event_id: None,
                distribution_rule_id: None,
            };
            let created_ledger = Self::create_ledger_in_tx(&mut tx, &ledger).await?;
            let delta = WalletDelta::grant(credit_type, amount);
            let _ = Self::apply_wallet_delta_in_tx(&mut tx, wallet.id, delta).await?;
            // balance_after = real post-grant derived SUM for this bucket
            // (in-tx, reflects the just-inserted ledger row). For grant
            // rows with a future `effective_at`, the derived SUM predicate
            // excludes the row until effective_at <= NOW(), so balance_after
            // reflects the immediately-available balance (matching the user's
            // visible balance semantics).
            let derived = Self::compute_available_balance_in_tx(
                &mut tx,
                &realm_id,
                user_id,
                std::slice::from_ref(&bucket_id),
                now,
            )
            .await?;
            let (balance_after, topup_after, subscription_after) =
                Self::derived_to_balance_snapshots(&derived);
            let transaction_type = Self::determine_transaction_type(credit_type, source_type);
            let tx_description = description
                .unwrap_or_else(|| format!("{}: {} points granted", source_type.as_str(), amount));
            let transaction_id = Uuid::now_v7();
            // Use a unique external_ref_id by combining source_id with transaction_id
            // to avoid unique constraint violations when the same admin grants points
            // to the same user multiple times
            let external_ref_id = format!("{}:{}", source_id, transaction_id);
            let _ = Self::create_transaction_in_tx(
                &mut tx,
                PointsTransaction {
                    id: transaction_id,
                    wallet_id: wallet.id,
                    user_id,
                    realm_id: realm_id.clone(),
                    bucket_id,
                    transaction_type,
                    amount,
                    balance_after,
                    topup_balance_after: topup_after,
                    subscription_balance_after: subscription_after,
                    credit_type: Some(credit_type),
                    description: Some(tx_description),
                    client_app_id: None,
                    subscription_id: None,
                    external_ref_id: Some(external_ref_id),
                    correlation_id: None,
                    effective_at,
                    created_at: now,
                    distribution_event_id: None,
                    distribution_rule_id: None,
                },
            )
            .await?;

            // Record idempotency key after successful grant
            if let Some(ref key) = idempotency_key {
                Self::record_completed_idempotency_in_tx(&mut tx, &realm_id, key, transaction_id)
                    .await?;
            }

            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(created_ledger)
        }
    }

    fn recharge_points_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        source_type: CreditSourceType,
        amount: i64,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
        source_id: Option<String>,
        external_ref_id: Option<String>,
    ) -> impl std::future::Future<Output = Result<PointsTransaction, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        async move {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            let wallet = Self::ensure_wallet_in_tx(&mut tx, &realm_id, user_id, bucket_id).await?;

            if wallet.status != WalletStatus::Active {
                return Err(CoreError::BadRequest(format!(
                    "Cannot recharge points to {} wallet",
                    wallet.status.as_str()
                )));
            }

            let resolved_source_id = source_id.unwrap_or_else(|| {
                external_ref_id
                    .clone()
                    .unwrap_or_else(|| "system".to_string())
            });
            let now = chrono::Utc::now();

            let ledger = PointsCreditLedger {
                id: Uuid::now_v7(),
                user_id,
                realm_id: realm_id.clone(),
                bucket_id,
                credit_type,
                source_type,
                source_id: resolved_source_id.clone(),
                granted_amount: amount,
                used_amount: 0,
                revoked_amount: 0,
                remaining_amount: amount,
                expires_at,
                effective_at: None,
                status: CreditLedgerStatus::Active,
                created_at: now,
                updated_at: now,
                distribution_event_id: None,
                distribution_rule_id: None,
            };

            Self::create_ledger_in_tx(&mut tx, &ledger).await?;

            let delta = WalletDelta::grant(credit_type, amount);
            let _ = Self::apply_wallet_delta_in_tx(&mut tx, wallet.id, delta).await?;

            // balance_after = real post-recharge derived SUM for this bucket
            // (recharge is immediately available: effective_at = NULL).
            let derived = Self::compute_available_balance_in_tx(
                &mut tx,
                &realm_id,
                user_id,
                std::slice::from_ref(&bucket_id),
                now,
            )
            .await?;
            let (balance_after, topup_after, subscription_after) =
                Self::derived_to_balance_snapshots(&derived);

            let transaction_type = Self::determine_transaction_type(credit_type, source_type);

            let transaction = Self::create_transaction_in_tx(
                &mut tx,
                PointsTransaction {
                    id: Uuid::now_v7(),
                    wallet_id: wallet.id,
                    user_id,
                    realm_id: realm_id.clone(),
                    bucket_id,
                    transaction_type,
                    amount,
                    balance_after,
                    topup_balance_after: topup_after,
                    subscription_balance_after: subscription_after,
                    credit_type: Some(credit_type),
                    description: Some(format!("Points recharge ({})", source_type.as_str())),
                    client_app_id: None,
                    subscription_id: None,
                    external_ref_id,
                    correlation_id: None,
                    effective_at: None,
                    created_at: now,
                    distribution_event_id: None,
                    distribution_rule_id: None,
                },
            )
            .await?;

            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            Ok(transaction)
        }
    }

    fn scan_and_expire_points_atomic(
        &self,
        batch_size: usize,
    ) -> impl std::future::Future<Output = Result<ExpirationSummary, CoreError>> + Send {
        let pool = self.pool.clone();
        async move {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            let now = chrono::Utc::now();
            let ledgers = Self::find_expired_ledgers_for_update(&mut tx, now, batch_size).await?;
            if ledgers.is_empty() {
                tx.commit()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                return Ok(ExpirationSummary {
                    expired_count: 0,
                    total_expired: 0,
                    expired_at: now,
                });
            }

            let mut total_expired = 0i64;
            for ledger in &ledgers {
                let amount = ledger.remaining_amount;
                total_expired += amount;
                let _ = Self::update_ledger_in_tx(
                    &mut tx,
                    ledger.id,
                    LedgerUpdate::SetStatus(CreditLedgerStatus::Expired),
                )
                .await?;
                let record = PointsRevocationRecord {
                    id: Uuid::now_v7(),
                    ledger_id: ledger.id,
                    user_id: ledger.user_id,
                    realm_id: ledger.realm_id.clone(),
                    revocation_type: RevocationType::ExpireRevoke,
                    revoked_amount: amount,
                    reason: "Points expired".to_string(),
                    reference_id: None,
                    created_at: now,
                };
                Self::create_revocation_record_in_tx(&mut tx, &record).await?;
                // Lock this ledger's bound wallet (per-ledger by bucket_id)
                // and assert it exists; the revoke no longer mutates the
                // wallet projection (derived balance).
                let bucket_id = ledger.bucket_id;
                let _wallet = Self::find_wallet_by_user_bucket_for_update(
                    &mut tx,
                    &ledger.realm_id,
                    ledger.user_id,
                    bucket_id,
                )
                .await?
                .ok_or(CoreError::NotFound)?;
            }

            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(ExpirationSummary {
                expired_count: ledgers.len(),
                total_expired,
                expired_at: now,
            })
        }
    }

    /// Derived available balance SUM(remaining_amount) grouped by credit_type.
    /// Same predicate as consumption selection — "seen
    /// balance == spendable balance" — so future-effective rows are excluded
    /// from the user-visible balance and bucket totals. Replaces reading
    /// `points_wallets` Stored balance columns for available-balance semantics.
    /// `bucket_ids` empty ⟺ aggregate across ALL the user's buckets (used by
    /// `get_balance`'s user-total view); non-empty ⟺ restrict to listed
    /// buckets (per-bucket grant responses). Empty-slice maps to no
    /// `bucket_id` filter (the predicate still scopes by realm+user).
    fn compute_available_balance(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_ids: &[Uuid],
        now: chrono::DateTime<chrono::Utc>,
    ) -> impl std::future::Future<Output = Result<Vec<(CreditType, i64)>, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        let bucket_ids = bucket_ids.to_vec();
        async move {
            // Coalesce to a sentinel-safe form: empty slice ⇒ no bucket_id
            // filter (aggregate across all the user's buckets). ANY() on an
            // empty array evaluates to FALSE, so we branch the SQL.
            let rows: Vec<(String, i64)> = if bucket_ids.is_empty() {
                sqlx::query_as(
                    r#"
                    SELECT credit_type, COALESCE(SUM(remaining_amount), 0)::BIGINT AS available
                    FROM points_credit_ledger
                    WHERE realm_id = $1
                      AND user_id = $2
                      AND status = 'active'
                      AND remaining_amount > 0
                      AND (effective_at IS NULL OR effective_at <= $3)
                      AND (expires_at  IS NULL OR expires_at  >  $3)
                    GROUP BY credit_type
                    "#,
                )
                .bind(&realm_id)
                .bind(user_id)
                .bind(now)
                .fetch_all(&pool)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?
            } else {
                sqlx::query_as(
                    r#"
                    SELECT credit_type, COALESCE(SUM(remaining_amount), 0)::BIGINT AS available
                    FROM points_credit_ledger
                    WHERE realm_id = $1
                      AND user_id = $2
                      AND bucket_id = ANY($3)
                      AND status = 'active'
                      AND remaining_amount > 0
                      AND (effective_at IS NULL OR effective_at <= $4)
                      AND (expires_at  IS NULL OR expires_at  >  $4)
                    GROUP BY credit_type
                    "#,
                )
                .bind(&realm_id)
                .bind(user_id)
                .bind(&bucket_ids)
                .bind(now)
                .fetch_all(&pool)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?
            };

            rows.into_iter()
                .map(|(credit_type, amount)| {
                    let credit_type: CreditType = credit_type.parse().map_err(|_| {
                        CoreError::DatabaseError(format!("invalid credit_type: {credit_type}"))
                    })?;
                    Ok((credit_type, amount))
                })
                .collect()
        }
    }

    /// Explicitly covered, enabled bucket ids for a client app in a realm.
    /// Same coverage set as the in-tx consume path (`find_covered_bucket_ids_in_tx`):
    /// explicit `credit_bucket_client_apps` rows joined to enabled buckets only.
    fn find_covered_bucket_ids(
        &self,
        realm_id: &str,
        client_app_id: Uuid,
    ) -> impl std::future::Future<Output = Result<Vec<Uuid>, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        async move {
            let rows: Vec<(Uuid,)> = sqlx::query_as(
                r#"
                SELECT bca.bucket_id
                FROM credit_bucket_client_apps bca
                JOIN credit_buckets b ON b.id = bca.bucket_id
                WHERE bca.realm_id = $1
                  AND bca.client_app_id = $2
                  AND b.enabled = true
                ORDER BY bca.bucket_id ASC
                "#,
            )
            .bind(&realm_id)
            .bind(client_app_id)
            .fetch_all(&pool)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(rows.into_iter().map(|(id,)| id).collect())
        }
    }

    /// Derived available balance broken down by `(bucket_id, credit_type)`.
    /// Same predicate as `compute_available_balance`, used by
    /// bucket overview / bucket delete guard / `list_wallets` bulk-derived
    /// assembly so they no longer read `points_wallets.total_balance` (avoids
    /// future-effective leakage and bucket mis-judgement).
    /// NOTE: `bucket_ids` empty ⟺ no bucket filter (aggregate every bucket in
    /// the realm). Callers that need "current page only" should pass the
    /// concrete bucket_id list.
    fn compute_bucket_available_balances(
        &self,
        realm_id: &str,
        bucket_ids: &[Uuid],
        now: chrono::DateTime<chrono::Utc>,
    ) -> impl std::future::Future<Output = Result<Vec<(Uuid, CreditType, i64)>, CoreError>> + Send
    {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        let bucket_ids = bucket_ids.to_vec();
        async move {
            let rows: Vec<(Uuid, String, i64)> = if bucket_ids.is_empty() {
                sqlx::query_as(
                    r#"
                    SELECT bucket_id, credit_type, COALESCE(SUM(remaining_amount), 0)::BIGINT AS available
                    FROM points_credit_ledger
                    WHERE realm_id = $1
                      AND status = 'active'
                      AND remaining_amount > 0
                      AND (effective_at IS NULL OR effective_at <= $2)
                      AND (expires_at  IS NULL OR expires_at  >  $2)
                    GROUP BY bucket_id, credit_type
                    "#,
                )
                .bind(&realm_id)
                .bind(now)
                .fetch_all(&pool)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?
            } else {
                sqlx::query_as(
                    r#"
                    SELECT bucket_id, credit_type, COALESCE(SUM(remaining_amount), 0)::BIGINT AS available
                    FROM points_credit_ledger
                    WHERE realm_id = $1
                      AND bucket_id = ANY($2)
                      AND status = 'active'
                      AND remaining_amount > 0
                      AND (effective_at IS NULL OR effective_at <= $3)
                      AND (expires_at  IS NULL OR expires_at  >  $3)
                    GROUP BY bucket_id, credit_type
                    "#,
                )
                .bind(&realm_id)
                .bind(&bucket_ids)
                .bind(now)
                .fetch_all(&pool)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?
            };

            rows.into_iter()
                .map(|(bucket_id, credit_type, amount)| {
                    let credit_type: CreditType = credit_type.parse().map_err(|_| {
                        CoreError::DatabaseError(format!("invalid credit_type: {credit_type}"))
                    })?;
                    Ok((bucket_id, credit_type, amount))
                })
                .collect()
        }
    }

    /// Pre-grant the next period for a schedule. Writes a
    /// ledger row carrying `effective_at`/`expires_at` PLUS a
    /// `points_grant_records(schedule_id, period_number)` row (UNIQUE
    /// idempotency) linked to the new ledger via `ledger_id` FK. Idempotent:
    /// a re-call for an already-written `(schedule_id, period_number)` returns
    /// the existing ledger without re-writing.
    /// Real transactional pre-grant: `FOR UPDATE schedule`
    /// (serializes concurrent callers), check
    /// `points_grant_records(schedule_id, period_number)` existence (HIT →
    /// return existing ledger via `ledger_id` FK bridge), else create ledger +
    /// wallet delta + transaction record (mirroring `grant_points_atomic`
    /// in-tx), INSERT `points_grant_records(... ledger_id = ledger.id ...)`
    /// (FK bridge), and advance the schedule's `next_grant_time` /
    /// `granted_periods` to the just-granted period.
    fn pregrant_next_period_atomic(
        &self,
        realm_id: &str,
        schedule: &herald_domain::points::grant_schedule::PointsGrantSchedule,
        period_number: u32,
        effective_at: Option<chrono::DateTime<chrono::Utc>>,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> impl std::future::Future<Output = Result<PointsCreditLedger, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        let schedule = schedule.clone();
        async move {
            let period_number_i64 = i64::from(period_number);
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            // 1) Lock the schedule row for the duration of this pre-grant so
            // concurrent callers serialize on the (schedule_id, period_number)
            // idempotency check. Re-read current state inside the lock.
            let row =
                sqlx::query("SELECT active FROM points_grant_schedules WHERE id = $1 FOR UPDATE")
                    .bind(schedule.id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            let active: bool = row
                .as_ref()
                .and_then(|r| r.try_get("active").ok())
                .ok_or_else(|| {
                    CoreError::InternalServerError(format!(
                        "pregrant: schedule {} not found (cannot lock)",
                        schedule.id
                    ))
                })?;
            if !active {
                return Err(CoreError::BadRequest(format!(
                    "pregrant: schedule {} is not active",
                    schedule.id
                )));
            }

            // 2) Idempotency: if a grant_record already exists for this
            // (schedule_id, period_number), return its ledger row via the
            // ledger_id FK bridge.
            let existing_ledger_id: Option<Uuid> = sqlx::query_scalar(
                "SELECT ledger_id FROM points_grant_records WHERE schedule_id = $1 AND period_number = $2 LIMIT 1",
            )
            .bind(schedule.id)
            .bind(period_number_i64)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            if let Some(ledger_id) = existing_ledger_id {
                let row = sqlx::query_as::<_, PointsCreditLedgerRow>(
                    "SELECT * FROM points_credit_ledger WHERE id = $1",
                )
                .bind(ledger_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?
                .ok_or_else(|| {
                    CoreError::InternalServerError(format!(
                        "grant_record references missing ledger {} (schedule {} period {})",
                        ledger_id, schedule.id, period_number
                    ))
                })?;
                tx.commit()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                return Self::row_to_points_credit_ledger(row);
            }

            // 3) No prior grant_record — create the ledger row, apply wallet
            // delta, and write a points_transactions row (mirroring
            // `grant_points_atomic` in-tx). `source_type` /
            // `credit_type` come from the schedule's grant semantics.
            let credit_type = match schedule.subscription_id {
                Some(_) => CreditType::SubscriptionCredit,
                None => CreditType::FreePeriodicCredit,
            };
            let source_type = match schedule.subscription_id {
                Some(_) => CreditSourceType::SubscriptionRenewal,
                None => CreditSourceType::FreePeriodicGrant,
            };
            let user_id = schedule.user_id;
            let bucket_id = schedule.bucket_id;
            let amount = schedule.points_per_period;
            let source_id = format!("schedule:{}:period:{}", schedule.id, period_number);

            let wallet = Self::ensure_wallet_in_tx(&mut tx, &realm_id, user_id, bucket_id).await?;
            if wallet.status != WalletStatus::Active {
                return Err(CoreError::BadRequest(format!(
                    "Cannot pre-grant points to {} wallet",
                    wallet.status.as_str()
                )));
            }
            let now = chrono::Utc::now();
            let ledger = PointsCreditLedger {
                id: Uuid::now_v7(),
                user_id,
                realm_id: realm_id.clone(),
                bucket_id,
                credit_type,
                source_type,
                source_id: source_id.clone(),
                granted_amount: amount,
                used_amount: 0,
                revoked_amount: 0,
                remaining_amount: amount,
                expires_at,
                effective_at,
                status: CreditLedgerStatus::Active,
                created_at: now,
                updated_at: now,
                distribution_event_id: None,
                distribution_rule_id: None,
            };
            let created_ledger = Self::create_ledger_in_tx(&mut tx, &ledger).await?;
            let delta = WalletDelta::grant(credit_type, amount);
            let _ = Self::apply_wallet_delta_in_tx(&mut tx, wallet.id, delta).await?;
            // balance_after = real post-pregrant derived SUM for this bucket.
            // Pre-grant rows carry a future `effective_at`, so the
            // derived SUM predicate excludes them until the period starts —
            // `balance_after` reflects the currently-available balance, not the
            // pre-granted total.
            let derived = Self::compute_available_balance_in_tx(
                &mut tx,
                &realm_id,
                user_id,
                std::slice::from_ref(&bucket_id),
                now,
            )
            .await?;
            let (balance_after, topup_after, subscription_after) =
                Self::derived_to_balance_snapshots(&derived);
            let transaction_type = Self::determine_transaction_type(credit_type, source_type);
            let transaction_id = Uuid::now_v7();
            let external_ref_id = format!("{}:{}", source_id, transaction_id);
            Self::create_transaction_in_tx(
                &mut tx,
                PointsTransaction {
                    id: transaction_id,
                    wallet_id: wallet.id,
                    user_id,
                    realm_id: realm_id.clone(),
                    bucket_id,
                    transaction_type,
                    amount,
                    balance_after,
                    topup_balance_after: topup_after,
                    subscription_balance_after: subscription_after,
                    credit_type: Some(credit_type),
                    description: Some(format!(
                        "{}: {} points pre-granted (period {})",
                        source_type.as_str(),
                        amount,
                        period_number
                    )),
                    client_app_id: None,
                    subscription_id: schedule.subscription_id,
                    external_ref_id: Some(external_ref_id),
                    correlation_id: None,
                    effective_at,
                    created_at: now,
                    distribution_event_id: None,
                    distribution_rule_id: None,
                },
            )
            .await?;

            // 4) Insert the grant_record linking to the ledger row (FK bridge).
            // The UNIQUE(schedule_id, period_number) constraint is
            // the period-level business idempotency guarantee.
            // 4b) Insert the grant_record via raw SQL (the `&mut` sqlx tx
            // cannot be shared with sea-orm). Column order matches
            // `create_revocation_record_in_tx` for consistency.
            let grant_record_id = Uuid::now_v7();
            sqlx::query(
                r#"
                INSERT INTO points_grant_records (
                    id, schedule_id, user_id, realm_id, period_number,
                    granted_amount, grant_time, ledger_id, created_at
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#,
            )
            .bind(grant_record_id)
            .bind(schedule.id)
            .bind(user_id)
            .bind(&realm_id)
            .bind(period_number_i64)
            .bind(amount)
            .bind(now)
            .bind(created_ledger.id)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                // Surface UNIQUE violations loudly: a duplicate means a
                // concurrent pre-grant won the race between our existence
                // check and this INSERT; the caller's retry will hit the
                // idempotency branch.
                CoreError::DatabaseError(format!(
                    "pregrant grant_record insert failed (schedule {} period {}): {}",
                    schedule.id, period_number, e
                ))
            })?;

            // 5) Advance the schedule. `granted_periods` becomes the latest
            // period granted; `next_grant_time` becomes the start of the
            // next nominal period. Using the domain's own
            // `next_grant_time(base, n)` arithmetic keeps a single source
            // of truth for period cadence.
            let advanced_granted_periods = period_number_i64.max(schedule.granted_periods);
            let next_period_index = advanced_granted_periods + 1;
            let next_grant_time = schedule
                .grant_period_type
                .next_grant_time(schedule.base_time, next_period_index);
            sqlx::query(
                r#"
                UPDATE points_grant_schedules
                   SET next_grant_time = $2,
                       granted_periods = $3,
                       updated_at = NOW()
                 WHERE id = $1
                "#,
            )
            .bind(schedule.id)
            .bind(next_grant_time)
            .bind(advanced_granted_periods)
            .execute(&mut *tx)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(created_ledger)
        }
    }

    /// Scan for schedules whose next pre-grant is due. Returns
    /// **candidates** whose `active=TRUE AND next_grant_time <= $before`; this
    /// method intentionally does NOT do a SQL-side `NOT EXISTS` against
    /// `points_grant_records` (P2-2): `period_number` derivation depends on
    /// `first_period_start`/`nominal_period_duration` (domain
    /// `derive_period_number`) and duplicating it inside SQL would drift from
    /// the domain's single source of truth. The caller (worker
    /// `PointsPreGrantJob` / domain) re-derives `period_number` per-row and
    /// checks `points_grant_records` absence before calling
    /// `pregrant_next_period_atomic`. This is a best-effort warming scan;
    /// returning extra candidates is harmless.
    fn find_schedules_due_for_pregrant(
        &self,
        before: chrono::DateTime<chrono::Utc>,
        limit: u64,
    ) -> impl std::future::Future<
        Output = Result<Vec<herald_domain::points::grant_schedule::PointsGrantSchedule>, CoreError>,
    > + Send {
        let db = self.db.clone();
        async move {
            let models = points_grant_schedule::Entity::find()
                .filter(points_grant_schedule::Column::Active.eq(true))
                .filter(
                    points_grant_schedule::Column::NextGrantTime
                        .lte(sea_orm::prelude::DateTimeWithTimeZone::from(before)),
                )
                .order_by_asc(points_grant_schedule::Column::NextGrantTime)
                .limit(limit)
                .all(&*db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            models
                .into_iter()
                .map(Self::model_to_grant_schedule)
                .collect()
        }
    }

    /// Single-user free-periodic due schedule scan for read-path realization.
    /// `WHERE realm_id AND user_id AND active AND
    /// subscription_id IS NULL AND next_grant_time <= before` (lead_time=0,
    /// only already-due periods).
    fn find_due_free_grant_schedules_for_user(
        &self,
        realm_id: &str,
        user_id: Uuid,
        before: chrono::DateTime<chrono::Utc>,
        limit: u64,
    ) -> impl std::future::Future<
        Output = Result<Vec<herald_domain::points::grant_schedule::PointsGrantSchedule>, CoreError>,
    > + Send {
        let db = self.db.clone();
        let realm_id = realm_id.to_string();
        async move {
            let models = points_grant_schedule::Entity::find()
                .filter(points_grant_schedule::Column::RealmId.eq(realm_id))
                .filter(points_grant_schedule::Column::UserId.eq(user_id))
                .filter(points_grant_schedule::Column::Active.eq(true))
                .filter(points_grant_schedule::Column::SubscriptionId.is_null())
                .filter(
                    points_grant_schedule::Column::NextGrantTime
                        .lte(sea_orm::prelude::DateTimeWithTimeZone::from(before)),
                )
                .order_by_asc(points_grant_schedule::Column::NextGrantTime)
                .limit(limit)
                .all(&*db)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            models
                .into_iter()
                .map(Self::model_to_grant_schedule)
                .collect()
        }
    }

    /// Row-level reclaim of a pre-granted ledger row. Sets the
    /// resolved ledger row to `status='revoked'` and
    /// `revoked_amount += remaining_amount`; derived balance auto-excludes it,
    /// so no wallet back-adjustment is performed. Returns the number of rows
    /// affected (0 ⟺ locator did not resolve an active row, caller may treat
    /// as idempotent no-op). For partially-consumed rows (`used_amount > 0`),
    /// a `PointsRevocationRecord(reason='subscription_pre_grant_reclaim',
    /// revocation_type=CancelRevoke)` is written so the shortfall is auditable.
    /// `ReclaimLocator::BySourceId(src)` filters
    /// `points_credit_ledger.source_id` directly;
    /// `ReclaimLocator::BySchedulePeriod{..}` is resolved to the unique ledger
    /// row via the `points_grant_records.ledger_id` FK subquery
    /// (FK bridge — `points_credit_ledger` has no `schedule_id` /
    /// `period_number` columns).
    fn revoke_pregrant_ledger_row_atomic(
        &self,
        realm_id: &str,
        locator: ReclaimLocator,
        reason: &str,
    ) -> impl std::future::Future<Output = Result<usize, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        let reason = reason.to_string();
        async move {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            // resolves via the `points_grant_records.ledger_id` FK subquery
            // (UNIQUE(schedule_id, period_number) guarantees at most one row).
            // `points_credit_ledger.remaining_amount` is a GENERATED column
            // (`granted_amount - used_amount - revoked_amount`), so it
            // regenerates to 0 the moment `revoked_amount` increases. A plain
            // `UPDATE ... RETURNING *` would therefore hand back
            // `remaining_amount = 0`, and the shortfall record below would be
            // written with `revoked_amount = 0` (violating
            // `points_revocation_records.revoked_amount > 0`). The CTE locks +
            // captures the pre-update `remaining_amount` as `yanked` so the
            // debt record carries the real unused portion this reclaim removed.
            // `BySchedulePeriod` resolves via the `points_grant_records.ledger_id`
            // FK subquery (UNIQUE(schedule_id, period_number) ⟹ ≤1 row).
            let rows: Vec<ReclaimTargetRow> = match locator {
                ReclaimLocator::BySourceId(src) => sqlx::query_as::<_, ReclaimTargetRow>(
                    r#"
                        WITH target AS (
                            SELECT id, remaining_amount AS yanked
                              FROM points_credit_ledger
                             WHERE realm_id = $1
                               AND source_id = $2
                               AND status = 'active'
                               AND remaining_amount > 0
                             FOR UPDATE
                        )
                        UPDATE points_credit_ledger AS l
                           SET status        = 'revoked',
                               revoked_amount = l.revoked_amount + t.yanked,
                               updated_at     = NOW()
                          FROM target t
                         WHERE l.id = t.id
                        RETURNING l.id, l.user_id, l.realm_id, l.source_id,
                                  l.used_amount, t.yanked
                        "#,
                )
                .bind(&realm_id)
                .bind(&src)
                .fetch_all(&mut *tx)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?,
                ReclaimLocator::BySchedulePeriod {
                    schedule_id,
                    period_number,
                } => sqlx::query_as::<_, ReclaimTargetRow>(
                    r#"
                        WITH target AS (
                            SELECT id, remaining_amount AS yanked
                              FROM points_credit_ledger
                             WHERE realm_id = $1
                               AND status = 'active'
                               AND remaining_amount > 0
                               AND id IN (
                                    SELECT g.ledger_id
                                      FROM points_grant_records g
                                     WHERE g.schedule_id = $2
                                       AND g.period_number = $3
                               )
                             FOR UPDATE
                        )
                        UPDATE points_credit_ledger AS l
                           SET status        = 'revoked',
                               revoked_amount = l.revoked_amount + t.yanked,
                               updated_at     = NOW()
                          FROM target t
                         WHERE l.id = t.id
                        RETURNING l.id, l.user_id, l.realm_id, l.source_id,
                                  l.used_amount, t.yanked
                        "#,
                )
                .bind(&realm_id)
                .bind(schedule_id)
                .bind(i64::from(period_number))
                .fetch_all(&mut *tx)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?,
            };

            // Record shortfall for any partially-consumed row.
            // `used_amount > 0` means the pre-granted period was already spent
            // before reclaim; the revocation record makes that debt visible to
            // audit / billing. Fully-unused rows (used_amount == 0) need no
            // debt record — the ledger row itself is sufficient.
            let now = chrono::Utc::now();
            for row in &rows {
                if row.used_amount > 0 {
                    let record = PointsRevocationRecord {
                        id: Uuid::now_v7(),
                        ledger_id: row.id,
                        user_id: row.user_id,
                        realm_id: row.realm_id.clone(),
                        revocation_type: RevocationType::CancelRevoke,
                        // `yanked` is the pre-update remaining_amount (the
                        // unused portion this reclaim just moved into
                        // revoked_amount); `used_amount` is preserved on the
                        // ledger row itself for audit.
                        revoked_amount: row.yanked,
                        reason: reason.clone(),
                        reference_id: Some(format!(
                            "subscription_pre_grant_reclaim:{}",
                            row.source_id
                        )),
                        created_at: now,
                    };
                    Self::create_revocation_record_in_tx(&mut tx, &record).await?;
                }
            }

            let affected = rows.len();
            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(affected)
        }
    }

    /// Locate active quota entitlements for the consume / balance read path
    /// Window availability is computed by the
    /// caller from the returned snapshots + `sum_consume_in_window`.
    /// `bucket_id = None` omits the bucket filter, returning active
    /// entitlements across all the user's buckets.
    fn find_active_quota_entitlements(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Option<Uuid>,
        credit_type: CreditType,
        now: chrono::DateTime<chrono::Utc>,
    ) -> impl std::future::Future<Output = Result<Vec<PointsQuotaEntitlement>, CoreError>> + Send
    {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        async move {
            let rows = if let Some(bucket_id) = bucket_id {
                sqlx::query_as::<_, PointsQuotaEntitlementRow>(
                    r#"
                    SELECT * FROM points_quota_entitlements
                    WHERE realm_id = $1
                      AND user_id = $2
                      AND bucket_id = $3
                      AND credit_type = $4
                      AND status = 'active'
                      AND effective_from <= $5
                      AND (effective_until IS NULL OR effective_until > $5)
                    "#,
                )
                .bind(&realm_id)
                .bind(user_id)
                .bind(bucket_id)
                .bind(credit_type.as_str())
                .bind(now)
                .fetch_all(&pool)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?
            } else {
                sqlx::query_as::<_, PointsQuotaEntitlementRow>(
                    r#"
                    SELECT * FROM points_quota_entitlements
                    WHERE realm_id = $1
                      AND user_id = $2
                      AND credit_type = $3
                      AND status = 'active'
                      AND effective_from <= $4
                      AND (effective_until IS NULL OR effective_until > $4)
                    "#,
                )
                .bind(&realm_id)
                .bind(user_id)
                .bind(credit_type.as_str())
                .bind(now)
                .fetch_all(&pool)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?
            };

            rows.into_iter()
                .map(Self::row_to_points_quota_entitlement)
                .collect()
        }
    }

    /// Sliding-window consume aggregation. Backed by
    /// `idx_points_transactions_window_agg`. `window_start` is
    /// `now - window_seconds`; the caller computes it per window.
    fn sum_consume_in_window(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        window_start: chrono::DateTime<chrono::Utc>,
    ) -> impl std::future::Future<Output = Result<i64, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        async move {
            let total: i64 = sqlx::query_scalar(
                r#"
                SELECT COALESCE(SUM(ABS(amount)), 0)::BIGINT
                FROM points_transactions
                WHERE realm_id = $1
                  AND user_id = $2
                  AND bucket_id = $3
                  AND credit_type = $4
                  AND type = 'consume'
                  AND created_at >= $5
                "#,
            )
            .bind(&realm_id)
            .bind(user_id)
            .bind(bucket_id)
            .bind(credit_type.as_str())
            .bind(window_start)
            .fetch_one(&pool)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(total)
        }
    }

    /// Grant a quota entitlement atomically. Idempotent via
    /// `UNIQUE(realm_id, user_id, bucket_id, credit_type, idempotency_key)`.
    /// `ON CONFLICT DO NOTHING RETURNING *` returns the freshly inserted row;
    /// if it returns nothing (replay), a follow-up SELECT returns the existing
    /// row so the caller observes the persisted snapshot either way.
    fn grant_quota_entitlement_atomic(
        &self,
        entitlement: PointsQuotaEntitlement,
    ) -> impl std::future::Future<Output = Result<PointsQuotaEntitlement, CoreError>> + Send {
        let pool = self.pool.clone();
        async move {
            let windows_json = serde_json::to_value(
                entitlement
                    .quota_windows
                    .iter()
                    .map(|w| QuotaWindowDbJson {
                        window_seconds: w.window_seconds,
                        limit: w.limit,
                        key: w.key.clone(),
                    })
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| CoreError::DatabaseError(format!("serialize quota_windows: {e}")))?;

            let inserted = sqlx::query_as::<_, PointsQuotaEntitlementRow>(
                r#"
                INSERT INTO points_quota_entitlements (
                    id, user_id, realm_id, bucket_id, credit_type, source_type,
                    source_id, quota_windows, effective_from, effective_until,
                    status, idempotency_key,
                    distribution_event_id, distribution_rule_id,
                    created_at, updated_at
                )
                VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                    $13, $14,
                    $15, $15
                )
                ON CONFLICT (realm_id, user_id, bucket_id, credit_type, idempotency_key) WHERE distribution_rule_id IS NULL
                DO NOTHING
                RETURNING *
                "#,
            )
            .bind(entitlement.id)
            .bind(entitlement.user_id)
            .bind(&entitlement.realm_id)
            .bind(entitlement.bucket_id)
            .bind(entitlement.credit_type.as_str())
            .bind(entitlement.source_type.as_str())
            .bind(&entitlement.source_id)
            .bind(&windows_json)
            .bind(entitlement.effective_from)
            .bind(entitlement.effective_until)
            .bind(entitlement.status.as_str())
            .bind(&entitlement.idempotency_key)
            .bind(entitlement.distribution_event_id)
            .bind(entitlement.distribution_rule_id)
            .bind(entitlement.created_at)
            .fetch_optional(&pool)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

            // Idempotent replay path: conflict suppressed the INSERT — read the
            // pre-existing row and return it so the caller sees the persisted
            // snapshot.
            let row = match inserted {
                Some(row) => row,
                None => sqlx::query_as::<_, PointsQuotaEntitlementRow>(
                    r#"
                        SELECT * FROM points_quota_entitlements
                        WHERE realm_id = $1
                          AND user_id = $2
                          AND bucket_id = $3
                          AND credit_type = $4
                          AND idempotency_key = $5
                        "#,
                )
                .bind(&entitlement.realm_id)
                .bind(entitlement.user_id)
                .bind(entitlement.bucket_id)
                .bind(entitlement.credit_type.as_str())
                .bind(&entitlement.idempotency_key)
                .fetch_optional(&pool)
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?
                .ok_or(CoreError::DatabaseError(
                    "quota entitlement upsert returned no row".to_string(),
                ))?,
            };

            Self::row_to_points_quota_entitlement(row)
        }
    }

    /// Revoke the active quota entitlement for
    /// `(realm_id, user_id, bucket_id, credit_type, source_id)`.
    /// Sets `status='revoked'` + `effective_until=revoke_at`; already-consumed
    /// usage is NOT reverse-adjusted (ages out via window slide). No-op
    /// (`Ok()`) if no active entitlement matches — replay-safe.
    fn revoke_quota_entitlement_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        source_id: &str,
        revoke_at: chrono::DateTime<chrono::Utc>,
    ) -> impl std::future::Future<Output = Result<(), CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        let source_id = source_id.to_string();
        async move {
            sqlx::query(
                r#"
                UPDATE points_quota_entitlements
                SET status = 'revoked',
                    effective_until = $6,
                    updated_at = NOW()
                WHERE realm_id = $1
                  AND user_id = $2
                  AND bucket_id = $3
                  AND credit_type = $4
                  AND source_id = $5
                  AND status = 'active'
                "#,
            )
            .bind(&realm_id)
            .bind(user_id)
            .bind(bucket_id)
            .bind(credit_type.as_str())
            .bind(&source_id)
            .bind(revoke_at)
            .execute(&pool)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(())
        }
    }

    /// Sweep-expire quota entitlements whose `effective_until` has passed
    /// Sets matched rows to `status='expired'` in
    /// batches of `batch_size`. Postgres has no `UPDATE ... LIMIT`, so a
    /// CTE+ctid sub-select bounds the update (the standard Postgres idiom).
    /// NOT a correctness backstop — window availability is a pure function of
    /// the consume stream + effective interval.
    fn expire_quota_entitlements_batch(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        batch_size: usize,
    ) -> impl std::future::Future<Output = Result<usize, CoreError>> + Send {
        let pool = self.pool.clone();
        async move {
            let affected = sqlx::query(
                r#"
                WITH victims AS (
                    SELECT ctid
                    FROM points_quota_entitlements
                    WHERE status = 'active'
                      AND effective_until IS NOT NULL
                      AND effective_until <= $1
                    LIMIT $2
                )
                UPDATE points_quota_entitlements
                SET status = 'expired',
                    updated_at = NOW()
                FROM victims
                WHERE points_quota_entitlements.ctid = victims.ctid
                "#,
            )
            .bind(now)
            .bind(i64::try_from(batch_size).unwrap_or(i64::MAX))
            .execute(&pool)
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))?
            .rows_affected();
            Ok(usize::try_from(affected).unwrap_or(usize::MAX))
        }
    }

    fn execute_distribution_event_atomic(
        &self,
        event: DistributionEvent,
        selection: DistributionRuleSelection,
    ) -> impl std::future::Future<Output = Result<Vec<DistributionGrantResult>, CoreError>> + Send
    {
        let pool = self.pool.clone();
        async move {
            loop {
                let mut tx = pool
                    .begin()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;

                match Self::insert_or_load_event_in_tx(&mut tx, &event).await? {
                    EventInsertOutcome::Existing { id, status } if status == "completed" => {
                        // Replay branch: lock + read the completed event and
                        // reconstruct the FIRST-run result set WITHOUT reading
                        // current rule / bucket config.
                        let result_count =
                            match Self::lock_completed_event_for_replay_in_tx(&mut tx, id).await? {
                                Some(count) => count,
                                // Raced: the row is no longer completed between
                                // the load and the lock. Restart the loop so the
                                // insert path re-evaluates.
                                None => {
                                    drop(tx);
                                    continue;
                                }
                            };
                        let rows = ReplayResultRows {
                            ledger_rows: Self::replay_ledger_rows_in_tx(&mut tx, id).await?,
                            entitlement_rows: Self::replay_quota_rows_in_tx(&mut tx, id).await?,
                            schedule_rows: Self::replay_schedule_rows_in_tx(&mut tx, id).await?,
                        };
                        let results = fold_replay_results(rows, result_count, id)?;
                        // Commit releases the row locks; nothing was written.
                        tx.commit()
                            .await
                            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                        return Ok(results);
                    }
                    EventInsertOutcome::Existing { status: ref s, .. } if s == "processing" => {
                        // Another in-flight caller holds the processing row.
                        // Drop this transaction and retry; the holder will
                        // either commit (→ replay) or roll back (→ first run).
                        drop(tx);
                        continue;
                    }
                    EventInsertOutcome::Existing { id, status } => {
                        return Err(CoreError::DatabaseError(format!(
                            "distribution event {} in unexpected status '{}'",
                            id, status
                        )));
                    }
                    EventInsertOutcome::InsertedProcessing(event_id) => {
                        // First-run branch: resolve rules, validate buckets,
                        // write all results + complete the event in one commit.
                        let results =
                            Self::execute_first_run_in_tx(&mut tx, event_id, &event, &selection)
                                .await?;
                        tx.commit()
                            .await
                            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                        return Ok(results);
                    }
                }
            }
        }
    }

    fn revoke_distribution_source_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        source_id: &str,
        revocation_type: RevocationType,
        reason: String,
        idempotency_key: String,
    ) -> impl std::future::Future<Output = Result<RevokePointsOutput, CoreError>> + Send {
        let pool = self.pool.clone();
        let realm_id = realm_id.to_string();
        let source_id = source_id.to_string();
        async move {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            if Self::check_completed_idempotency_in_tx(&mut tx, &realm_id, &idempotency_key)
                .await?
                .is_some()
            {
                tx.commit()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                return Ok(RevokePointsOutput::empty());
            }
            let output = Self::revoke_distribution_source_in_tx(
                &mut tx,
                &realm_id,
                user_id,
                &source_id,
                revocation_type,
                &reason,
                &idempotency_key,
            )
            .await?;
            Self::record_completed_idempotency_in_tx(
                &mut tx,
                &realm_id,
                &idempotency_key,
                output.revocation_id,
            )
            .await?;
            tx.commit()
                .await
                .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
            Ok(output)
        }
    }

    fn replace_distribution_source_atomic(
        &self,
        source_id: &str,
        revocation_type: RevocationType,
        reason: String,
        event: DistributionEvent,
        selection: DistributionRuleSelection,
    ) -> impl std::future::Future<Output = Result<Vec<DistributionGrantResult>, CoreError>> + Send
    {
        let pool = self.pool.clone();
        let source_id = source_id.to_string();
        async move {
            loop {
                let mut tx = pool
                    .begin()
                    .await
                    .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                match Self::insert_or_load_event_in_tx(&mut tx, &event).await? {
                    EventInsertOutcome::Existing { id, status } if status == "completed" => {
                        let Some(result_count) =
                            Self::lock_completed_event_for_replay_in_tx(&mut tx, id).await?
                        else {
                            drop(tx);
                            continue;
                        };
                        let rows = ReplayResultRows {
                            ledger_rows: Self::replay_ledger_rows_in_tx(&mut tx, id).await?,
                            entitlement_rows: Self::replay_quota_rows_in_tx(&mut tx, id).await?,
                            schedule_rows: Self::replay_schedule_rows_in_tx(&mut tx, id).await?,
                        };
                        let results = fold_replay_results(rows, result_count, id)?;
                        tx.commit()
                            .await
                            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                        return Ok(results);
                    }
                    EventInsertOutcome::Existing { status: ref s, .. } if s == "processing" => {
                        drop(tx);
                        continue;
                    }
                    EventInsertOutcome::Existing { id, status } => {
                        return Err(CoreError::DatabaseError(format!(
                            "distribution event {} in unexpected status '{}'",
                            id, status
                        )));
                    }
                    EventInsertOutcome::InsertedProcessing(event_id) => {
                        Self::deactivate_free_periodic_results_in_tx(
                            &mut tx,
                            &event.realm_id,
                            event.user_id,
                            event.effective_from,
                        )
                        .await?;
                        Self::revoke_distribution_source_in_tx(
                            &mut tx,
                            &event.realm_id,
                            event.user_id,
                            &source_id,
                            revocation_type,
                            &reason,
                            &event.event_key,
                        )
                        .await?;
                        let results =
                            Self::execute_first_run_in_tx(&mut tx, event_id, &event, &selection)
                                .await?;
                        tx.commit()
                            .await
                            .map_err(|e| CoreError::DatabaseError(e.to_string()))?;
                        return Ok(results);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_conversion() {
        let model = points_wallet::Model {
            id: Uuid::now_v7(),
            user_id: Uuid::now_v7(),
            realm_id: "test-realm".to_string(),
            bucket_id: Uuid::now_v7(),
            total_topup_granted: 100,
            total_subscription_granted: 0,
            total_recharged: 1000,
            total_consumed: 900,
            status: "active".to_string(),
            created_at: sea_orm::prelude::DateTimeWithTimeZone::from(chrono::Utc::now()),
            updated_at: sea_orm::prelude::DateTimeWithTimeZone::from(chrono::Utc::now()),
        };

        let result = PostgresPointsRepository::model_to_points_wallet(model);
        assert!(result.is_ok());
        let account = result.unwrap();
        assert_eq!(account.total_topup_granted, 100);
    }

    #[test]
    fn proportional_refund_is_calculated_per_original_rule_grant() {
        assert_eq!(
            PostgresPointsRepository::proportional_refund_for_grant(100, 100, 1, 4),
            25
        );
        assert_eq!(
            PostgresPointsRepository::proportional_refund_for_grant(250, 250, 1, 4),
            63
        );
        assert_eq!(
            PostgresPointsRepository::proportional_refund_for_grant(250, 20, 1, 4),
            20,
            "refund revocation must never exceed the rule grant's unused balance"
        );
    }

    // The allocate-by-expiry loop is the heart of the consume allocation plan. It is extracted as
    // `plan_consume_allocation` so the cross-bucket split, permanent-pool-last
    // ordering, partial-coverage rejection and exact-amount boundary can be
    // verified without a database. These tests encode WHY the split matters:
    // wrong totals → wrong per-bucket transaction amounts / wallet balances;
    // wrong ordering → permanent credits drained before expiring ones (value loss
    // for the user); insufficient rejection → over-spending / negative balances.

    use herald_domain::points::entities::{
        CreditLedgerStatus, CreditSourceType, CreditType, PointsCreditLedger,
    };

    /// Helper: build a ledger with the given remaining amount and expiry.
    /// `bucket_id` distinguishes pools; ordering in the input slice is what the
    /// planner consumes (caller already sorted via the SQL ORDER BY).
    fn ledger(
        index_bucket: usize,
        credit_type: CreditType,
        remaining: i64,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> PointsCreditLedger {
        PointsCreditLedger {
            id: Uuid::now_v7(),
            user_id: Uuid::now_v7(),
            realm_id: "realm".to_string(),
            // Two distinct buckets: even index → bucket A, odd → bucket B.
            bucket_id: if index_bucket.is_multiple_of(2) {
                Uuid::from_u128(0xA)
            } else {
                Uuid::from_u128(0xB)
            },
            credit_type,
            source_type: CreditSourceType::Topup,
            source_id: "src".to_string(),
            granted_amount: remaining,
            used_amount: 0,
            revoked_amount: 0,
            remaining_amount: remaining,
            expires_at,
            effective_at: None,
            status: CreditLedgerStatus::Active,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            distribution_event_id: None,
            distribution_rule_id: None,
        }
    }

    #[test]
    fn plan_consume_splits_across_two_buckets_in_expiry_order() {
        // Bucket A: 30 expiring soon; Bucket B: 50 expiring later.
        // Request 50 → take all 30 from A, then 20 from B.
        let soon = chrono::Utc::now() + chrono::Duration::hours(1);
        let later = chrono::Utc::now() + chrono::Duration::days(7);
        let ledgers = vec![
            ledger(0, CreditType::TopupCredit, 30, Some(soon)),
            ledger(1, CreditType::TopupCredit, 50, Some(later)),
        ];

        let plan = PostgresPointsRepository::plan_consume_allocation(&ledgers, 50);

        assert!(plan.fully_covers);
        assert_eq!(
            plan.allocations,
            vec![
                PlannedAllocation {
                    ledger_index: 0,
                    amount: 30
                },
                PlannedAllocation {
                    ledger_index: 1,
                    amount: 20
                },
            ]
        );
        // Caller groups per bucket: A=30, B=20 — distinct transactions.
        let mut per_bucket = std::collections::BTreeMap::new();
        for p in &plan.allocations {
            let bid = ledgers[p.ledger_index].bucket_id;
            *per_bucket.entry(bid).or_insert(0i64) += p.amount;
        }
        let totals: Vec<_> = per_bucket.values().copied().collect();
        assert_eq!(totals, vec![30, 20]);
    }

    #[test]
    fn plan_consume_allocates_permanent_pool_null_expires_last() {
        // The SQL ORDER BY is `expires_at ASC NULLS LAST`, so the caller hands the
        // planner ledgers already in that order. Verify the planner respects the
        // given order: expiring ledger is consumed before the permanent one even
        // when the permanent one has more remaining.
        let soon = chrono::Utc::now() + chrono::Duration::minutes(5);
        let ledgers = vec![
            // expiring bucket, 10 left
            ledger(0, CreditType::SubscriptionCredit, 10, Some(soon)),
            // permanent (NULL expires_at) bucket, 1000 left
            ledger(1, CreditType::GrantedCredit, 1000, None),
        ];

        let plan = PostgresPointsRepository::plan_consume_allocation(&ledgers, 15);

        assert!(plan.fully_covers);
        // First 10 from the expiring ledger, remaining 5 from permanent.
        assert_eq!(plan.allocations[0].ledger_index, 0);
        assert_eq!(plan.allocations[0].amount, 10);
        assert_eq!(plan.allocations[1].ledger_index, 1);
        assert_eq!(plan.allocations[1].amount, 5);
        // Without correct ordering the planner would drain 15 from permanent and
        // let the 10 expiring credits lapse — this assertion guards that.
    }

    #[test]
    fn plan_consume_rejects_when_remaining_is_insufficient() {
        // Sum of remaining (30 + 10) < requested 50 → not fully covered.
        // The repository precheck rejects with `insufficient_points`; the plan
        // surfaces `fully_covers=false` so the caller can fail loud rather than
        // write a partial / negative-balance consume.
        let soon = chrono::Utc::now() + chrono::Duration::hours(1);
        let ledgers = vec![
            ledger(0, CreditType::TopupCredit, 30, Some(soon)),
            ledger(1, CreditType::TopupCredit, 10, Some(soon)),
        ];

        let plan = PostgresPointsRepository::plan_consume_allocation(&ledgers, 50);

        assert!(!plan.fully_covers);
        // Still records the partial take so the caller's precheck can report
        // have/need accurately if desired.
        let taken: i64 = plan.allocations.iter().map(|p| p.amount).sum();
        assert_eq!(taken, 40);
    }

    #[test]
    fn plan_consume_exact_amount_boundary_consumes_no_more_than_needed() {
        let soon = chrono::Utc::now() + chrono::Duration::hours(1);
        let ledgers = vec![
            ledger(0, CreditType::TopupCredit, 20, Some(soon)),
            ledger(1, CreditType::TopupCredit, 20, Some(soon)),
        ];

        // Request exactly 20 → only the first ledger is touched; the second is
        // left intact. Guards against over-consuming (negative balance bug).
        let plan = PostgresPointsRepository::plan_consume_allocation(&ledgers, 20);

        assert!(plan.fully_covers);
        assert_eq!(
            plan.allocations,
            vec![PlannedAllocation {
                ledger_index: 0,
                amount: 20
            }]
        );
    }

    #[test]
    fn plan_consume_single_pool_request_yields_single_allocation() {
        // Single-pool hit must produce a length-1 transaction downstream
        // (completion criterion: single-pool → Vec len 1, structurally uniform).
        let soon = chrono::Utc::now() + chrono::Duration::days(1);
        let ledgers = vec![ledger(0, CreditType::TopupCredit, 100, Some(soon))];

        let plan = PostgresPointsRepository::plan_consume_allocation(&ledgers, 40);

        assert!(plan.fully_covers);
        assert_eq!(plan.allocations.len(), 1);
        assert_eq!(plan.allocations[0].amount, 40);
    }

    #[test]
    fn plan_consume_empty_ledger_set_cannot_cover_nonzero_request() {
        // Mirrors the NoCoveredPointsPool / no-active-ledger path: an empty
        // covered set (or a user with no active ledgers in any covered bucket)
        // must NOT silently produce a zero-amount consume.
        let ledgers: Vec<PointsCreditLedger> = vec![];
        let plan = PostgresPointsRepository::plan_consume_allocation(&ledgers, 10);
        assert!(!plan.fully_covers);
        assert!(plan.allocations.is_empty());
    }

    // The priority rule (subscription_credit first, free_periodic_credit 补足)
    // and the per-credit_type overspend invariants are the testable contract of
    // the window side of the mixed consume. These pin WHY the ordering matters:
    // subscription credits are the paid entitlement and must be drawn before the
    // free periodic quota; the `min` clamps prevent any window from being
    // overdrawn within a single transaction.

    #[test]
    fn window_split_subscription_first_until_exhausted() {
        // window_part (40) fully covered by subscription (50) ⟹ free untouched.
        let split = PostgresPointsRepository::split_window_part_by_credit_type(40, 50, 30);
        assert_eq!(
            split,
            WindowCreditSplit {
                sub_part: 40,
                free_part: 0,
                window_remainder: 0,
            }
        );
    }

    #[test]
    fn window_split_free_makes_up_remainder_when_subscription_insufficient() {
        // Subscription covers 20 of 50; free covers the remaining 30.
        let split = PostgresPointsRepository::split_window_part_by_credit_type(50, 20, 30);
        assert_eq!(
            split,
            WindowCreditSplit {
                sub_part: 20,
                free_part: 30,
                window_remainder: 0,
            }
        );
    }

    #[test]
    fn window_split_overspill_to_next_bucket_when_both_window_types_insufficient() {
        // Both window types together (10+15=25) < window_part 40 ⟹ 15 must
        // overflow to the next bucket (window_remainder). This is the per-bucket
        // distribution contract: the caller hands the remainder to the next
        // covered bucket's window; the already guaranteed the TOTAL
        // window_part ≤ Σ all buckets' window spendable.
        let split = PostgresPointsRepository::split_window_part_by_credit_type(40, 10, 15);
        assert_eq!(split.sub_part, 10);
        assert_eq!(split.free_part, 15);
        assert_eq!(split.window_remainder, 15);
        // Overspend invariant (P0): neither part exceeds its spendable.
        assert!(split.sub_part <= 10);
        assert!(split.free_part <= 15);
    }

    #[test]
    fn window_split_no_window_capacity_overspills_entire_part() {
        // No active entitlements of either window type ⟹ entire window_part
        // overflows (this bucket contributes nothing; next bucket / pool handles
        // it per the gate). Guards against silently inventing window capacity.
        let split = PostgresPointsRepository::split_window_part_by_credit_type(30, 0, 0);
        assert_eq!(split.sub_part, 0);
        assert_eq!(split.free_part, 0);
        assert_eq!(split.window_remainder, 30);
    }

    #[test]
    fn window_split_priority_subscription_drawn_before_free_even_when_free_larger() {
        // Free has MORE capacity (100) than subscription (5), but priority says
        // subscription is drawn FIRST. With window_part=8: sub=5, free=3 — NOT
        // free=8. This pins the priority order against a naive max-first split.
        let split = PostgresPointsRepository::split_window_part_by_credit_type(8, 5, 100);
        assert_eq!(split.sub_part, 5);
        assert_eq!(split.free_part, 3);
        assert_eq!(split.window_remainder, 0);
    }

    #[test]
    fn window_split_negative_spendables_clamped_to_zero() {
        // Defensive: a negative remaining (shrunk quota / aggregation glitch) is
        // clamped to 0 so the overspend invariant holds. Behaves like no capacity
        // — entire part overflows.
        let split = PostgresPointsRepository::split_window_part_by_credit_type(20, -5, -3);
        assert_eq!(split.sub_part, 0);
        assert_eq!(split.free_part, 0);
        assert_eq!(split.window_remainder, 20);
    }

    #[test]
    fn window_split_exact_coverage_boundary() {
        // window_part exactly == sub_spendable + free_spendable ⟹ zero
        // remainder, both parts at capacity (boundary of the overflow rule).
        let split = PostgresPointsRepository::split_window_part_by_credit_type(30, 10, 20);
        assert_eq!(split.sub_part, 10);
        assert_eq!(split.free_part, 20);
        assert_eq!(split.window_remainder, 0);
    }
}
