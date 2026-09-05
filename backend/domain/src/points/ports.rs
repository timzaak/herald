use std::future::Future;

use uuid::Uuid;

use crate::common::entities::app_errors::CoreError;
use crate::points::dtos::RevokePointsOutput;
use crate::points::entities::CreditSourceType;
use crate::points::entities::{
    ConsumptionAllocationView, CreditLedgerStatus, CreditType, Paginated, PointsCreditLedger,
    PointsQuotaEntitlement, PointsRevocationRecord, PointsTransaction, PointsWallet,
    RevocationType, TransactionType,
};
use crate::points::{
    DistributionEvent, DistributionGrantResult, DistributionRuleSelection, PointsGrantSchedule,
};

/// Row-level locator for pre-grant reclaim.
///
/// `points_credit_ledger` itself carries no `schedule_id`/`period_number`
/// columns — the business idempotency key `(schedule_id, period_number)` lives
/// only in `points_grant_records`. So reclaim must resolve to the unique
/// ledger row through one of:
/// - `BySourceId` — direct lookup on `points_credit_ledger.source_id`.
/// - `BySchedulePeriod` — resolved through the `points_grant_records.ledger_id`
///   FK subquery: the unique ledger row linked to
///   `(schedule_id, period_number)`.
///
/// The trait layer declares only the locator shape; the infra impl
/// owns the resolution SQL.
#[derive(Debug, Clone)]
pub enum ReclaimLocator {
    /// Reclaim the single ledger row whose `source_id` matches.
    BySourceId(String),
    /// Reclaim the unique ledger row linked to `(schedule_id, period_number)`
    /// via `points_grant_records.ledger_id` FK.
    BySchedulePeriod {
        schedule_id: Uuid,
        period_number: u32,
    },
}

/// Transaction filters
#[derive(Debug, Clone, Default)]
pub struct TransactionFilters {
    pub user_id: Option<Uuid>,
    pub bucket_id: Option<Uuid>,
    pub transaction_type: Option<TransactionType>,
    pub client_app_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub external_ref_id: String,
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

/// Account filters
#[derive(Debug, Clone, Default)]
pub struct WalletFilters {
    pub user_id: Option<Uuid>,
    pub bucket_id: Option<Uuid>,
    pub status: Option<String>,
    pub search: Option<String>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

/// Ledger filters
#[derive(Debug, Clone, Default)]
pub struct LedgerFilters {
    pub credit_type: Option<CreditType>,
    pub status: Option<CreditLedgerStatus>,
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    pub pagination: Option<Pagination>,
}

/// Pagination params
#[derive(Debug, Clone, Copy)]
pub struct Pagination {
    pub page: u64,
    pub page_size: u64,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            page: 1,
            page_size: 20,
        }
    }
}

/// Ledger update type
#[derive(Debug, Clone)]
pub enum LedgerUpdate {
    Consumption(i64),
    Revocation(i64),
    SetExpiration(chrono::DateTime<chrono::Utc>),
    SetStatus(CreditLedgerStatus),
}

/// Delta applied to a wallet row's lifetime analytics columns by the single
/// writer `apply_wallet_delta_in_tx`.
///
/// The 5 per-type balance delta fields are gone
/// (the underlying `points_wallets` balance columns were physically dropped;
/// available balance is now a derived SUM over `points_credit_ledger`).
/// Only the 4 monotonic lifetime analytics columns remain Stored, so this
/// delta only describes how to advance them.
///
/// `total_*_granted` and `total_consumed` are monotonic lifetime totals —
/// grant/consume writers add positive deltas; revocation/refund writers leave
/// them unchanged (revocation does not "un-consume" or "un-grant", it only
/// moves `remaining_amount` on the ledger).
#[derive(Debug, Clone, Copy, Default)]
pub struct WalletDelta {
    pub total_recharged: i64,
    pub total_consumed: i64,
    pub total_topup_granted: i64,
    pub total_subscription_granted: i64,
}

impl WalletDelta {
    pub fn zero() -> Self {
        Self::default()
    }

    /// Delta for granting `amount` of `credit_type`. Advances the matching
    /// lifetime total; paid grants (topup / subscription) also accrue
    /// `total_recharged`. No longer touches any balance column.
    pub fn grant(credit_type: CreditType, amount: i64) -> Self {
        let (total_topup_granted, total_subscription_granted) = match credit_type {
            CreditType::TopupCredit => (amount, 0),
            CreditType::SubscriptionCredit => (0, amount),
            _ => (0, 0),
        };
        Self {
            total_recharged: total_topup_granted + total_subscription_granted,
            total_consumed: 0,
            total_topup_granted,
            total_subscription_granted,
        }
    }
}

/// Repository for points operations
#[cfg_attr(test, mockall::automock)]
pub trait PointsRepository: Send + Sync {
    /// Find points wallet by user ID
    fn find_by_user_id(
        &self,
        realm_id: &str,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Option<PointsWallet>, CoreError>> + Send;

    /// Find points wallet by ID
    fn find_by_id(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<PointsWallet>, CoreError>> + Send;

    /// Create a new points wallet
    fn create_wallet(
        &self,
        account: PointsWallet,
    ) -> impl Future<Output = Result<PointsWallet, CoreError>> + Send;

    /// Change the operational state of one concrete (user, bucket) wallet.
    fn update_wallet_status(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        status: crate::points::entities::WalletStatus,
    ) -> impl Future<Output = Result<Option<PointsWallet>, CoreError>> + Send;

    /// Create a points transaction
    fn create_transaction(
        &self,
        transaction: PointsTransaction,
    ) -> impl Future<Output = Result<PointsTransaction, CoreError>> + Send;

    /// Find transactions with filters
    fn find_transactions(
        &self,
        realm_id: &str,
        filters: TransactionFilters,
    ) -> impl Future<Output = Result<Paginated<PointsTransaction>, CoreError>> + Send;

    /// Find expired recharge transactions for a user
    ///
    /// Returns all recharge transactions that have expired (expires_at < NOW())
    /// and have not yet been processed for expiration.
    fn find_expired_recharge_transactions(
        &self,
        realm_id: &str,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Vec<PointsTransaction>, CoreError>> + Send;

    /// Find a single transaction by ID
    fn find_transaction_by_id(
        &self,
        realm_id: &str,
        transaction_id: Uuid,
    ) -> impl Future<Output = Result<Option<PointsTransaction>, CoreError>> + Send;

    /// Count transactions for pagination
    fn count_transactions(
        &self,
        realm_id: &str,
        filters: &TransactionFilters,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;

    fn check_idempotency_key(
        &self,
        realm_id: &str,
        idempotency_key: &str,
    ) -> impl Future<Output = Result<Option<Uuid>, CoreError>> + Send;

    fn record_idempotency_key(
        &self,
        realm_id: &str,
        idempotency_key: &str,
        transaction_id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// Find transaction by external reference ID (for idempotency check)
    /// DEPRECATED: Use check_idempotency_key instead
    fn find_transaction_by_ref(
        &self,
        realm_id: &str,
        user_id: Uuid,
        external_ref_id: &str,
    ) -> impl Future<Output = Result<Option<PointsTransaction>, CoreError>> + Send;

    /// List accounts with filters
    fn list_wallets(
        &self,
        realm_id: &str,
        filters: WalletFilters,
    ) -> impl Future<Output = Result<Paginated<PointsWallet>, CoreError>> + Send;

    /// Count accounts for pagination
    fn count_wallets(
        &self,
        realm_id: &str,
        filters: &WalletFilters,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;

    /// Create a new credit ledger entry
    fn create_ledger(
        &self,
        ledger: PointsCreditLedger,
    ) -> impl Future<Output = Result<PointsCreditLedger, CoreError>> + Send;

    /// Find ledgers by user ID with filters
    fn find_ledgers_by_user_id(
        &self,
        realm_id: &str,
        user_id: Uuid,
        filters: LedgerFilters,
    ) -> impl Future<Output = Result<Paginated<PointsCreditLedger>, CoreError>> + Send;

    /// Find a single ledger by ID
    fn find_ledger_by_id(
        &self,
        ledger_id: Uuid,
    ) -> impl Future<Output = Result<Option<PointsCreditLedger>, CoreError>> + Send;

    /// Find a single ledger by source_id (e.g., payment_attempt_id for idempotency checks)
    fn find_ledger_by_source_id(
        &self,
        realm_id: &str,
        source_id: &str,
    ) -> impl Future<Output = Result<Option<PointsCreditLedger>, CoreError>> + Send;

    /// Find consumption allocations for ALL transactions sharing a consume
    /// `correlation_id`. Used by the SDK consume response
    /// to surface the ledger-level truth source of a multi-bucket consume without
    /// re-deducting. Legacy single-pool rows (NULL correlation_id) are excluded.
    /// Returns each allocation joined with its ledger's `credit_type` so the
    /// response can populate `AllocationDetail.credit_type`.
    fn find_consumption_allocations_by_correlation_id(
        &self,
        realm_id: &str,
        correlation_id: &str,
    ) -> impl Future<Output = Result<Vec<ConsumptionAllocationView>, CoreError>> + Send;

    /// Create a revocation record
    fn create_revocation_record(
        &self,
        record: PointsRevocationRecord,
    ) -> impl Future<Output = Result<PointsRevocationRecord, CoreError>> + Send;

    /// Find expired ledgers for cleanup
    fn find_expired_ledgers(
        &self,
        expiration_time: chrono::DateTime<chrono::Utc>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<PointsCreditLedger>, CoreError>> + Send;

    /// Find active grant schedules due for granting
    fn find_due_grant_schedules(
        &self,
        before: chrono::DateTime<chrono::Utc>,
        limit: u64,
    ) -> impl Future<Output = Result<Vec<PointsGrantSchedule>, CoreError>> + Send;

    fn consume_points_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        client_app_id: Uuid,
        amount: i64,
        description: Option<String>,
        idempotency_key: Option<String>,
    ) -> impl Future<Output = Result<Vec<PointsTransaction>, CoreError>> + Send;

    /// Reassemble a consume result set from its primary transaction id, WITHOUT
    /// re-deducting (idempotency replay). Multi-pool rows share a
    /// `correlation_id` → return all N sibling transactions ordered by
    /// bucket_id. Legacy single-pool rows (NULL `correlation_id`) return just
    /// the primary transaction.
    fn replay_consume_by_primary(
        &self,
        realm_id: &str,
        primary_txn_id: Uuid,
    ) -> impl Future<Output = Result<Vec<PointsTransaction>, CoreError>> + Send;

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
    ) -> impl Future<Output = Result<RevokePointsOutput, CoreError>> + Send;

    /// Revoke all remaining points from a specific ledger identified by source_id.
    /// Unlike `revoke_points_by_credit_type_atomic`, this only targets the single
    /// ledger whose `source_id` matches, avoiding over-broad revocation.
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
    ) -> impl Future<Output = Result<RevokePointsOutput, CoreError>> + Send;

    fn revoke_topup_proportional_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        refund_amount: i64,
        original_payment_amount: i64,
        refund_id: &str,
    ) -> impl Future<Output = Result<RevokePointsOutput, CoreError>> + Send;

    fn revoke_topup_source_proportional_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        source_id: &str,
        refund_amount: i64,
        original_payment_amount: i64,
        refund_id: &str,
    ) -> impl Future<Output = Result<RevokePointsOutput, CoreError>> + Send;

    fn grant_points_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        source_type: CreditSourceType,
        amount: i64,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
        // Expected effective time. `None` ⟺ immediately
        // available; `Some(t)` ⟺ enters the available set only when
        // `effective_at <= NOW()`. INSERT writes the column; derived balance
        // and consumption predicates gate on it.
        effective_at: Option<chrono::DateTime<chrono::Utc>>,
        source_id: Option<String>,
        description: Option<String>,
        idempotency_key: Option<String>,
    ) -> impl Future<Output = Result<PointsCreditLedger, CoreError>> + Send;

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
    ) -> impl Future<Output = Result<PointsTransaction, CoreError>> + Send;

    fn scan_and_expire_points_atomic(
        &self,
        batch_size: usize,
    ) -> impl Future<
        Output = Result<crate::points::expiration_service::ExpirationSummary, CoreError>,
    > + Send;

    /// Derived available balance. SUM(remaining_amount)
    /// over the shared predicate `status='active' AND remaining_amount>0 AND
    /// (effective_at IS NULL OR effective_at<=now) AND (expires_at IS NULL OR
    /// expires_at>now)` grouped by `credit_type`. Same source as consumption
    /// selection — "seen balance == spendable balance". Replaces reading
    /// `points_wallets` Stored balance columns for available-balance semantics.
    ///
    /// `bucket_ids` semantics: empty slice ⟺ aggregate across ALL the user's
    /// buckets (for `get_balance`'s user-total view); non-empty ⟺ restrict to
    /// the listed buckets (for per-bucket grant responses). The infra impl
    /// maps empty to "no `bucket_id` filter".
    fn compute_available_balance(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_ids: &[Uuid],
        now: chrono::DateTime<chrono::Utc>,
    ) -> impl Future<Output = Result<Vec<(CreditType, i64)>, CoreError>> + Send;

    /// Explicitly covered, enabled bucket ids for a client app in a realm
    /// (`credit_bucket_client_apps` joined to enabled `credit_buckets`).
    /// Used to scope client-app-bound API keys to the buckets their app
    /// actually covers — the same coverage set consumption spends from.
    fn find_covered_bucket_ids(
        &self,
        realm_id: &str,
        client_app_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Uuid>, CoreError>> + Send;

    /// Derived available balance broken down by `(bucket_id, credit_type)`.
    /// Same predicate as `compute_available_balance`, used by
    /// bucket overview / bucket delete guard so they no longer read
    /// `points_wallets.total_balance` (avoids future-effective leakage and
    /// bucket mis-judgement).
    fn compute_bucket_available_balances(
        &self,
        realm_id: &str,
        bucket_ids: &[Uuid],
        now: chrono::DateTime<chrono::Utc>,
    ) -> impl Future<Output = Result<Vec<(Uuid, CreditType, i64)>, CoreError>> + Send;

    /// Pre-grant the next period for a schedule. Writes a
    /// ledger row carrying `effective_at`/`expires_at` PLUS a
    /// `points_grant_records(schedule_id, period_number)` row (UNIQUE
    /// idempotency) linked to the new ledger via `ledger_id` FK. Idempotent:
    /// re-call for the same `(schedule_id, period_number)` returns the
    /// existing ledger row without re-writing. Subscription and free-periodic
    /// pre-grant share this port; `effective_at` anchors to period start
    /// (subscription `period_start` / free-period `next_grant_time`).
    fn pregrant_next_period_atomic(
        &self,
        realm_id: &str,
        schedule: &crate::points::PointsGrantSchedule,
        period_number: u32,
        effective_at: Option<chrono::DateTime<chrono::Utc>>,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> impl Future<Output = Result<PointsCreditLedger, CoreError>> + Send;

    /// Scan for schedules whose next pre-grant is due. Returns
    /// candidates whose `next_grant_time` is within the caller's per-row
    /// `lead_time` window; caller (worker `PointsPreGrantJob`) re-checks
    /// per-row against its `lead_time_map`. Used as a belt-and-braces warming
    /// scan — correctness is not gated on this port.
    fn find_schedules_due_for_pregrant(
        &self,
        before: chrono::DateTime<chrono::Utc>,
        limit: u64,
    ) -> impl Future<Output = Result<Vec<crate::points::PointsGrantSchedule>, CoreError>> + Send;

    /// Single-user free-periodic due schedule scan for read-path realization.
    /// `WHERE realm_id AND user_id AND active AND
    /// subscription_id IS NULL AND next_grant_time <= before` (lead_time=0,
    /// only already-due periods). Single-user scope avoids cross-user lock
    /// contention; subscription schedules are excluded so the request path
    /// never guesses paid-grant fulfillment.
    fn find_due_free_grant_schedules_for_user(
        &self,
        realm_id: &str,
        user_id: Uuid,
        before: chrono::DateTime<chrono::Utc>,
        limit: u64,
    ) -> impl Future<Output = Result<Vec<crate::points::PointsGrantSchedule>, CoreError>> + Send;

    /// Row-level reclaim of a pre-granted ledger row. Sets
    /// the resolved ledger row to `status='revoked'` and
    /// `revoked_amount += remaining_amount`; derived balance auto-excludes it,
    /// so no wallet back-adjustment is needed. Returns the number of rows
    /// affected (0 if the locator did not resolve — caller treats as
    /// idempotent no-op or surfaces per reclaim policy). Used by webhook reclaim
    /// via the trait (cross-crate, cannot call infra private
    /// helpers directly).
    fn revoke_pregrant_ledger_row_atomic(
        &self,
        realm_id: &str,
        locator: ReclaimLocator,
        reason: &str,
    ) -> impl Future<Output = Result<usize, CoreError>> + Send;

    /// Locate active quota entitlements for the consume / balance read path.
    ///
    /// Maps to `status = 'active' AND effective_from <= now
    /// AND (effective_until IS NULL OR effective_until > now)` scoped by
    /// `(realm_id, user_id, credit_type)`. When `bucket_id` is `Some`, the
    /// result is further restricted to that bucket; `None` returns active
    /// entitlements across all the user's buckets (used by the coarse
    /// `reconcile_due_for_user` read-path check). Window availability is
    /// computed from the returned entitlement snapshots.
    fn find_active_quota_entitlements(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Option<Uuid>,
        credit_type: CreditType,
        now: chrono::DateTime<chrono::Utc>,
    ) -> impl Future<Output = Result<Vec<PointsQuotaEntitlement>, CoreError>> + Send;

    /// Sliding-window consume aggregation. Returns
    /// `COALESCE(SUM(ABS(amount)), 0)` over `points_transactions` filtered by
    /// `(realm_id, user_id, bucket_id, credit_type, type='consume')` and
    /// `created_at >= window_start`. Backed by the
    /// `idx_points_transactions_window_agg` covering index.
    fn sum_consume_in_window(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        window_start: chrono::DateTime<chrono::Utc>,
    ) -> impl Future<Output = Result<i64, CoreError>> + Send;

    /// Grant a quota entitlement atomically. Idempotent: the
    /// `UNIQUE(realm_id, user_id, bucket_id, credit_type, idempotency_key)`
    /// constraint guarantees a replayed grant returns the existing row without
    /// re-writing. Returns the persisted entitlement
    /// (created or pre-existing).
    fn grant_quota_entitlement_atomic(
        &self,
        entitlement: PointsQuotaEntitlement,
    ) -> impl Future<Output = Result<PointsQuotaEntitlement, CoreError>> + Send;

    /// Revoke the active quota entitlement identified by
    /// `(realm_id, user_id, bucket_id, credit_type, source_id)`. Sets
    /// `status = 'revoked'` and `effective_until = revoke_at`; already-consumed
    /// usage is NOT reverse-adjusted (it ages out via window slide).
    /// No-op (returns `Ok(())`) if no active entitlement matches, so revoke is
    /// idempotent across replayed webhook events.
    fn revoke_quota_entitlement_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        bucket_id: Uuid,
        credit_type: CreditType,
        source_id: &str,
        revoke_at: chrono::DateTime<chrono::Utc>,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// Sweep-expire quota entitlements whose `effective_until` has passed
    /// (`status = 'active' AND effective_until <= now`), in batches of
    /// `batch_size`. Sets matched rows to `status = 'expired'`. Returns the
    /// number of rows expired. Invoked by the expiry worker; NOT a
    /// correctness backstop — window availability is a pure function of the
    /// consume stream + effective interval.
    fn expire_quota_entitlements_batch(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        batch_size: usize,
    ) -> impl Future<Output = Result<usize, CoreError>> + Send;

    /// Execute a multi-rule distribution event atomically and idempotently.
    ///
    /// First execution is all-or-nothing: fixed points, quota entitlements and
    /// the first-period schedule of every matched rule, plus the
    /// `points_distribution_events` completion record, all commit in one
    /// transaction; any failure rolls the whole thing back. A completed event —
    /// including a zero-rule event — is finalized with `result_count` and
    /// `completed_at`.
    ///
    /// Replay: when the `(realm, user, trigger, event_key)` row already exists
    /// as `completed`, the executor locks it, reconstructs the FIRST-run result
    /// set by querying ledger / quota entitlement / schedule rows by
    /// `distribution_event_id`, folds the schedule first-ledger out, validates
    /// the logical count equals `result_count` (fail-loud on corruption) and
    /// returns the reconstructed results WITHOUT reading the current
    /// rule / bucket config. Concurrent callers are serialized by the unique
    /// constraint.
    ///
    /// Attribution: every result row this executor creates carries BOTH
    /// `distribution_event_id` and `distribution_rule_id` (non-null). Direct
    /// writes (admin/sdk grant, demo/test-only internal quota) bypass this
    /// executor and keep both NULL.
    fn execute_distribution_event_atomic(
        &self,
        event: DistributionEvent,
        selection: DistributionRuleSelection,
    ) -> impl Future<Output = Result<Vec<DistributionGrantResult>, CoreError>> + Send;

    /// Revoke every rule-attributed fixed/quota result produced for a business
    /// source, across all target buckets. The original event/rule attribution
    /// is the only locator; current rule configuration is never consulted.
    fn revoke_distribution_source_atomic(
        &self,
        realm_id: &str,
        user_id: Uuid,
        source_id: &str,
        revocation_type: RevocationType,
        reason: String,
        idempotency_key: String,
    ) -> impl Future<Output = Result<RevokePointsOutput, CoreError>> + Send;

    /// Subscription upgrade transaction: revoke all prior results for the
    /// subscription source, then execute the new Mapping's upgrade rules.
    /// Either both halves commit or neither does.
    fn replace_distribution_source_atomic(
        &self,
        source_id: &str,
        revocation_type: RevocationType,
        reason: String,
        event: DistributionEvent,
        selection: DistributionRuleSelection,
    ) -> impl Future<Output = Result<Vec<DistributionGrantResult>, CoreError>> + Send;
}
