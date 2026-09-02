use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Points account response.
///
/// The GET /wallets/{user} endpoint is a read-only user-total balance view:
/// for users with multiple Credit Buckets, `find_by_user_id` synthesizes a
/// single aggregated `PointsWallet` with `bucket_id = None`. In that case
/// `id` is `None` too — there is no single concrete wallet to point at, and a
/// fabricated id would be a misleading handle a client could mistake for "the
/// wallet". `id` is `Some` only for a single-bucket user, where it is the
/// wallet row id of that one bucket (still not a writable handle here).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PointsWalletResponse {
    pub id: Option<Uuid>,
    pub user_id: Uuid,
    pub realm_id: String,
    /// Credit Bucket this wallet belongs to. `None` for the
    /// aggregate user-total view (multi-bucket user).
    pub bucket_id: Option<Uuid>,
    pub balance: i64,
    /// Total points granted through paid topups and subscription entitlements.
    pub total_paid_granted: i64,
    /// Deprecated compatibility alias for total_paid_granted.
    pub total_recharged: i64,
    pub total_consumed: i64,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub unit: String,
    pub currency: String,
}

/// Per-credit-type balances (`balancesByType` / `byCreditType`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BalancesByType {
    pub topup: i64,
    pub subscription: i64,
    pub registration: i64,
    pub free_periodic: i64,
    pub granted: i64,
}

impl BalancesByType {
    /// Sum of all credit-type balances (the bucket total).
    pub fn total(&self) -> i64 {
        self.topup
            .saturating_add(self.subscription)
            .saturating_add(self.registration)
            .saturating_add(self.free_periodic)
            .saturating_add(self.granted)
    }
}

/// Quota window read view (`QuotaWindowView`), mirrors the domain entity
/// `herald_core::domain::points::QuotaWindowView` (design §4.2.2).
///
/// One row per distinct window `key` for a (user, bucket). `key` is the stable
/// display identity derived from the window length (e.g. `5h`/`week`/`month`),
/// NOT a row ordinal — re-renders / config edits keep the same key.
/// `isTightest` flags the minimum-remaining window (the spendable-from-quota
/// constraint); `exhausted` flags `remaining == 0`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindowViewResponse {
    /// Stable display key (config-derived, not row ordinal).
    pub key: String,
    pub limit: i64,
    pub used: i64,
    pub remaining: i64,
    /// Sliding window length in seconds (month ≈ 30d).
    pub window_seconds: i64,
    /// Approximate next reset point of the window (design D1 — precise
    /// oldest-consume reset is deferred). `None` when no consume has occurred
    /// in the window yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<chrono::DateTime<chrono::Utc>>,
    /// True if this window is the minimum-remaining (tightest) constraint.
    pub is_tightest: bool,
    /// True if `remaining == 0`.
    pub exhausted: bool,
}

impl QuotaWindowViewResponse {
    /// Map a domain `QuotaWindowView` into the HTTP response (1:1; the only
    /// job is crossing the domain/API boundary so the API contract is not
    /// bound to the domain entity).
    pub fn from_domain(view: herald_core::domain::points::QuotaWindowView) -> Self {
        Self {
            key: view.key,
            limit: view.limit,
            used: view.used,
            remaining: view.remaining,
            window_seconds: view.window_seconds,
            resets_at: view.resets_at,
            is_tightest: view.is_tightest,
            exhausted: view.exhausted,
        }
    }
}

/// Free-periodic quota window **request** shape (design §4.2.2 / §4.4.3).
///
/// Carries only the editable fields; `key` is derived by the backend from
/// `windowSeconds` (via `derive_window_key`) before persistence, so callers
/// cannot drift the stable window identity.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, validator::Validate)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindowInput {
    /// Sliding window length in seconds. Must be > 0.
    #[validate(range(min = 1))]
    pub window_seconds: i64,
    /// Quota limit. Must be >= 0 (0 = window grants nothing but is a valid
    /// config edge case).
    #[validate(range(min = 0))]
    pub limit: i64,
}

/// Wallet balances grouped by Credit Bucket (`WalletByBucket`).
///
/// For the admin (`billing/points/wallets`) view, `user_id` is populated and
/// the response groups per `(user, bucket)`. For the `users/me/points/wallets`
/// view, `user_id` is the calling user and rows group their own wallets by
/// bucket.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WalletByBucketResponse {
    pub bucket_id: Option<Uuid>,
    /// Display name (currently unset; filled when bucket directory is wired).
    pub name: Option<String>,
    /// Whether the bucket is enabled (currently unset; filled when bucket
    /// directory is wired).
    pub enabled: Option<bool>,
    /// User who owns these wallet rows (always present; the admin view spans
    /// users, the user view repeats the calling user).
    pub user_id: Uuid,
    pub balances_by_type: BalancesByType,
    /// Currently spendable total for this bucket = window-available
    /// (`spendable_from_quota`) + pool balance (`spendable_from_pool`).
    /// Semantically extended (design §4.2.2): for a pool-only bucket this
    /// equals the pool sum (zero-regression — `quota_windows` /
    /// `spendable_from_quota` are `None`); for a window bucket it folds in the
    /// tightest window's remaining.
    pub bucket_total: i64,
    /// Per-window quota view for this (user, bucket) (design §4.2.2).
    /// `None` for a pool-only bucket (no active subscription / free-periodic
    /// quota entitlement). `Some([])` is avoided — pool-only stays `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_windows: Option<Vec<QuotaWindowViewResponse>>,
    /// Window-quota available amount = minimum `remaining` across
    /// `quota_windows` (the tightest constraint). `None` for pool-only buckets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spendable_from_quota: Option<i64>,
    /// Pool-side balance sum (topup + registration + granted credit types)
    /// for this bucket. `None` for window-only buckets with no pool balance
    /// component; otherwise the pool contribution to `bucket_total`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spendable_from_pool: Option<i64>,
}

/// Aggregated wallets-by-bucket list response.
///
/// `cross_bucket_total` is the sum of every bucket's `bucketTotal`. The field
/// is always present; for a single-bucket realm it equals `bucketTotal` of the
/// sole row.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListWalletsByBucketResponse {
    pub items: Vec<WalletByBucketResponse>,
    pub cross_bucket_total: i64,
}

/// Points balance response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PointsBalanceResponse {
    pub user_id: Uuid,
    pub balance: i64,
    /// Total points granted through paid topups and subscription entitlements.
    pub total_paid_granted: i64,
    /// Deprecated compatibility alias for total_paid_granted.
    pub total_recharged: i64,
    pub total_consumed: i64,
    pub unit: String,
    pub currency: String,
    pub updated_at: String,
}

/// Points transaction response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PointsTransactionResponse {
    pub id: Uuid,
    pub wallet_id: Uuid,
    pub user_id: Uuid,
    pub realm_id: String,
    /// Credit Bucket the transaction landed in.
    pub bucket_id: Option<Uuid>,
    pub transaction_type: String,
    pub amount: i64,
    pub balance_after: i64,
    pub description: Option<String>,
    pub client_app_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub external_ref_id: Option<String>,
    pub created_at: String,
    /// Expected effective time. Read-only, for
    /// admin/audit reconciliation of pre-generated vs already-effective rows.
    /// `None` on the `points.view` (regular user) path — the handler forces it
    /// to `None` and `skip_serializing_if` omits the key from JSON entirely, so
    /// the field never appears in regular-user responses. Populated (when
    /// available) only on the `points.manage` path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Consume points request
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsumePointsRequest {
    pub user_id: String,
    pub client_app_id: String,
    pub amount: i64,
    pub description: Option<String>,
    pub idempotency_key: Option<String>,
}

/// Consume points response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsumePointsResponse {
    pub transaction_id: Uuid,
    pub wallet_id: Uuid,
    pub user_id: Uuid,
    pub amount: i64,
    pub balance_after: i64,
}

/// Query parameters for listing transactions
#[derive(Debug, Clone, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTransactionsQuery {
    pub user_id: Option<String>,
    pub transaction_type: Option<String>,
    pub client_app_id: Option<String>,
    pub subscription_id: Option<String>,
    pub external_ref_id: Option<String>,
    /// Filter by Credit Bucket. Applied at the handler because
    /// `TransactionFilters` does not yet carry `bucket_id`.
    pub bucket_id: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

/// Filters available to the current-user transaction endpoint.
#[derive(Debug, Clone, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserTransactionsQuery {
    pub transaction_type: Option<String>,
    pub client_app_id: Option<String>,
    pub subscription_id: Option<String>,
    pub external_ref_id: Option<String>,
    pub bucket_id: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

/// Query parameters for listing accounts
#[derive(Debug, Clone, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListWalletsQuery {
    pub status: Option<String>,
    pub search: Option<String>,
    /// Filter by Credit Bucket. Applied at the handler because
    /// `WalletFilters` does not yet carry `bucket_id`.
    pub bucket_id: Option<String>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

/// Filters available to the current-user wallet endpoint.
#[derive(Debug, Clone, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserWalletsQuery {
    pub status: Option<String>,
    pub bucket_id: Option<String>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

/// Read-side view of one registration distribution rule.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationRuleResponse {
    pub id: Uuid,
    /// Target credit bucket.
    pub bucket_id: Uuid,
    /// Non-empty; only `registration` / `free_periodic_grant`.
    pub trigger_sources: Vec<String>,
    /// `fixed` or `quota`.
    pub grant_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points_amount: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validity_days: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_period_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_windows: Option<Vec<QuotaWindowInput>>,
    pub enabled: bool,
    pub display_order: i32,
}

/// Write-side view of one registration distribution rule.
/// `id` is `None` to create, `Some` to update an existing rule owned by the
/// Realm's registration config.
#[derive(Debug, Clone, Deserialize, ToSchema, validator::Validate)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationRuleWrite {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    pub bucket_id: Uuid,
    /// Non-empty; the backend validates the registration-owner subset.
    pub trigger_sources: Vec<String>,
    /// `fixed` or `quota`.
    pub grant_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points_amount: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validity_days: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_period_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_windows: Option<Vec<QuotaWindowInput>>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub display_order: i32,
}

fn default_true() -> bool {
    true
}

/// GET `/api/points/{realmId}/registration-rules` response.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationRulesResponse {
    pub realm_id: String,
    pub rules: Vec<RegistrationRuleResponse>,
}

/// PUT `/api/points/{realmId}/registration-rules` request body.
#[derive(Debug, Clone, Deserialize, ToSchema, validator::Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpsertRegistrationRulesRequest {
    pub rules: Vec<RegistrationRuleWrite>,
}

/// Grant points request (admin)
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrantPointsRequest {
    pub user_id: String,
    /// Target Credit Bucket. REQUIRED — every grant must
    /// name an explicit bucket; missing → 400 `grant_bucket_required`.
    pub bucket_id: Option<String>,
    pub amount: i64,
    pub reason: String,
    pub validity_days: Option<i64>,
}

/// Grant points response (admin)
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrantPointsResponse {
    pub transaction_id: Uuid,
    pub user_id: Uuid,
    /// Credit Bucket the grant landed in. Mirrors the
    /// api-ext `ExtGrantPointsResponse.bucketId` and SDK `GrantPointsResponse`
    /// contract so consumers see one shape.
    pub bucket_id: Uuid,
    pub amount: i64,
    pub granted_balance: i64,
    pub total_balance: i64,
    pub expires_at: Option<String>,
}
