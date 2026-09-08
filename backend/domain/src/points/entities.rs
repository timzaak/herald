use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::common::entities::app_errors::CoreError;
// Import Points-specific error extensions
use crate::points::errors::PointsErrorExt;

/// Safe arithmetic operations with overflow protection
pub trait SafeArithmetics: Copy + std::fmt::Display {
    /// Safe addition that returns an error on overflow
    fn safe_add(self, other: Self) -> Result<Self, CoreError>;

    /// Safe subtraction that returns an error on underflow
    fn safe_sub(self, other: Self) -> Result<Self, CoreError>;

    /// Safe multiplication that returns an error on overflow
    fn safe_mul(self, other: Self) -> Result<Self, CoreError>;
}

impl SafeArithmetics for i64 {
    fn safe_add(self, other: Self) -> Result<Self, CoreError> {
        self.checked_add(other).ok_or_else(|| {
            CoreError::overflow_error(&format!("Overflow in addition: {} + {}", self, other))
        })
    }

    fn safe_sub(self, other: Self) -> Result<Self, CoreError> {
        if self < other {
            return Err(CoreError::overflow_error(&format!(
                "Underflow in subtraction: {} - {} = {} (negative result not allowed)",
                self,
                other,
                self - other
            )));
        }
        Ok(self - other)
    }

    fn safe_mul(self, other: Self) -> Result<Self, CoreError> {
        self.checked_mul(other).ok_or_else(|| {
            CoreError::overflow_error(&format!("Overflow in multiplication: {} * {}", self, other))
        })
    }
}

/// Helper for parsing string enums with a default error message
fn parse_enum<T, F>(s: &str, error_prefix: &str, variants: F) -> Result<T, CoreError>
where
    F: Fn(&str) -> Option<T>,
{
    variants(s).ok_or_else(|| CoreError::BadRequest(format!("{}: {}", error_prefix, s)))
}

/// Account status enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WalletStatus {
    Active,
    Frozen,
    Closed,
}

impl WalletStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WalletStatus::Active => "active",
            WalletStatus::Frozen => "frozen",
            WalletStatus::Closed => "closed",
        }
    }
}

impl std::str::FromStr for WalletStatus {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_enum(
            s.to_lowercase().as_str(),
            "Invalid wallet status",
            |s| match s {
                "active" => Some(WalletStatus::Active),
                "frozen" => Some(WalletStatus::Frozen),
                "closed" => Some(WalletStatus::Closed),
                _ => None,
            },
        )
    }
}

impl std::fmt::Display for WalletStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Transaction type enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    Recharge,
    Consume,
    SubscriptionGrant,
    SubscriptionRenewal,
    SubscriptionUpgrade,
    SubscriptionDowngrade,
    RegistrationGrant,
    FreePeriodicGrant,
    RefundRevoke,
    ExpireRevoke,
    CancelRevoke,
    Expiration,
    Refund,
    Grant,
}

impl TransactionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransactionType::Recharge => "recharge",
            TransactionType::Consume => "consume",
            TransactionType::SubscriptionGrant => "subscription_grant",
            TransactionType::SubscriptionRenewal => "subscription_renewal",
            TransactionType::SubscriptionUpgrade => "subscription_upgrade",
            TransactionType::SubscriptionDowngrade => "subscription_downgrade",
            TransactionType::RegistrationGrant => "registration_grant",
            TransactionType::FreePeriodicGrant => "free_periodic_grant",
            TransactionType::RefundRevoke => "refund_revoke",
            TransactionType::ExpireRevoke => "expire_revoke",
            TransactionType::CancelRevoke => "cancel_revoke",
            TransactionType::Expiration => "expiration",
            TransactionType::Refund => "refund",
            TransactionType::Grant => "grant",
        }
    }
}

impl std::str::FromStr for TransactionType {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_enum(
            s.to_lowercase().as_str(),
            "Invalid transaction type",
            |s| match s {
                "recharge" => Some(TransactionType::Recharge),
                "consume" => Some(TransactionType::Consume),
                "subscription_grant" => Some(TransactionType::SubscriptionGrant),
                "subscription_renewal" => Some(TransactionType::SubscriptionRenewal),
                "subscription_upgrade" => Some(TransactionType::SubscriptionUpgrade),
                "subscription_downgrade" => Some(TransactionType::SubscriptionDowngrade),
                "registration_grant" => Some(TransactionType::RegistrationGrant),
                "free_periodic_grant" => Some(TransactionType::FreePeriodicGrant),
                "refund_revoke" => Some(TransactionType::RefundRevoke),
                "expire_revoke" => Some(TransactionType::ExpireRevoke),
                "cancel_revoke" => Some(TransactionType::CancelRevoke),
                "expiration" => Some(TransactionType::Expiration),
                "refund" => Some(TransactionType::Refund),
                "grant" => Some(TransactionType::Grant),
                _ => None,
            },
        )
    }
}

impl std::fmt::Display for TransactionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Recharge type enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RechargeType {
    Subscribe,
    Renewal,
}

impl RechargeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RechargeType::Subscribe => "subscribe",
            RechargeType::Renewal => "renewal",
        }
    }
}

impl std::str::FromStr for RechargeType {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_enum(
            s.to_lowercase().as_str(),
            "Invalid recharge type",
            |s| match s {
                "subscribe" => Some(RechargeType::Subscribe),
                "renewal" => Some(RechargeType::Renewal),
                _ => None,
            },
        )
    }
}

impl std::fmt::Display for RechargeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Credit type enum (积分类型) - Extended
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CreditType {
    /// 充值积分（用户主动购买）
    TopupCredit,
    /// 会员积分（订阅套餐赠送）
    SubscriptionCredit,
    /// 注册初始积分（注册时一次性赠送）
    RegistrationCredit,
    /// 免费定期积分（按周期自动发放）
    FreePeriodicCredit,
    /// 主动发放积分（管理员或 SDK 发放）
    GrantedCredit,
}

impl CreditType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CreditType::TopupCredit => "topup_credit",
            CreditType::SubscriptionCredit => "subscription_credit",
            CreditType::RegistrationCredit => "registration_credit",
            CreditType::FreePeriodicCredit => "free_periodic_credit",
            CreditType::GrantedCredit => "granted_credit",
        }
    }

    pub fn is_topup(&self) -> bool {
        matches!(self, CreditType::TopupCredit)
    }

    pub fn is_subscription(&self) -> bool {
        matches!(self, CreditType::SubscriptionCredit)
    }

    pub fn is_free_credit(&self) -> bool {
        matches!(
            self,
            CreditType::RegistrationCredit | CreditType::FreePeriodicCredit
        )
    }

    pub fn is_granted(&self) -> bool {
        matches!(self, CreditType::GrantedCredit)
    }
}

impl std::str::FromStr for CreditType {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_enum(
            s.to_lowercase().as_str(),
            "Invalid credit_type",
            |s| match s {
                "topup_credit" => Some(CreditType::TopupCredit),
                "subscription_credit" => Some(CreditType::SubscriptionCredit),
                "registration_credit" => Some(CreditType::RegistrationCredit),
                "free_periodic_credit" => Some(CreditType::FreePeriodicCredit),
                "granted_credit" => Some(CreditType::GrantedCredit),
                _ => None,
            },
        )
    }
}

impl std::fmt::Display for CreditType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Credit source type enum (积分来源类型) - Extended
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CreditSourceType {
    /// 订阅首次赠送
    SubscriptionInitial,
    /// 订阅续费赠送
    SubscriptionRenewal,
    /// 订阅升级（回收老积分，发放新积分）
    SubscriptionUpgrade,
    /// 订阅降级（仅记录，不回收积分）
    SubscriptionDowngrade,
    /// 充值购买
    Topup,
    /// 注册赠送
    Registration,
    /// 免费定期发放
    FreePeriodicGrant,
    /// 系统发放（补偿、奖励等）
    SystemGrant,
    /// 退款回收
    RefundRevoke,
    /// 过期回收
    ExpireRevoke,
    /// 取消订阅回收
    CancelRevoke,
    /// 升级时回收（免费用户升级到付费）
    UpgradeRevoke,
    /// 管理员主动发放
    AdminGrant,
    /// SDK/第三方应用发放
    SdkGrant,
}

impl CreditSourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CreditSourceType::SubscriptionInitial => "subscription_initial",
            CreditSourceType::SubscriptionRenewal => "subscription_renewal",
            CreditSourceType::SubscriptionUpgrade => "subscription_upgrade",
            CreditSourceType::SubscriptionDowngrade => "subscription_downgrade",
            CreditSourceType::Topup => "topup",
            CreditSourceType::Registration => "registration",
            CreditSourceType::FreePeriodicGrant => "free_periodic_grant",
            CreditSourceType::SystemGrant => "system_grant",
            CreditSourceType::RefundRevoke => "refund_revoke",
            CreditSourceType::ExpireRevoke => "expire_revoke",
            CreditSourceType::CancelRevoke => "cancel_revoke",
            CreditSourceType::UpgradeRevoke => "upgrade_revoke",
            CreditSourceType::AdminGrant => "admin_grant",
            CreditSourceType::SdkGrant => "sdk_grant",
        }
    }
}

impl std::str::FromStr for CreditSourceType {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_enum(
            s.to_lowercase().as_str(),
            "Invalid source_type",
            |s| match s {
                "subscription_initial" => Some(CreditSourceType::SubscriptionInitial),
                "subscription_renewal" => Some(CreditSourceType::SubscriptionRenewal),
                "subscription_upgrade" => Some(CreditSourceType::SubscriptionUpgrade),
                "subscription_downgrade" => Some(CreditSourceType::SubscriptionDowngrade),
                "topup" => Some(CreditSourceType::Topup),
                "registration" => Some(CreditSourceType::Registration),
                "free_periodic_grant" => Some(CreditSourceType::FreePeriodicGrant),
                "system_grant" => Some(CreditSourceType::SystemGrant),
                "refund_revoke" => Some(CreditSourceType::RefundRevoke),
                "expire_revoke" => Some(CreditSourceType::ExpireRevoke),
                "cancel_revoke" => Some(CreditSourceType::CancelRevoke),
                "upgrade_revoke" => Some(CreditSourceType::UpgradeRevoke),
                "admin_grant" => Some(CreditSourceType::AdminGrant),
                "sdk_grant" => Some(CreditSourceType::SdkGrant),
                _ => None,
            },
        )
    }
}

impl std::fmt::Display for CreditSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Credit ledger status enum (账本状态)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum CreditLedgerStatus {
    Active,
    Revoked,
    Expired,
    FullyUsed,
}

impl CreditLedgerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CreditLedgerStatus::Active => "active",
            CreditLedgerStatus::Revoked => "revoked",
            CreditLedgerStatus::Expired => "expired",
            CreditLedgerStatus::FullyUsed => "fully_used",
        }
    }
}

impl std::str::FromStr for CreditLedgerStatus {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_enum(
            s.to_lowercase().as_str(),
            "Invalid ledger status",
            |s| match s {
                "active" => Some(CreditLedgerStatus::Active),
                "revoked" => Some(CreditLedgerStatus::Revoked),
                "expired" => Some(CreditLedgerStatus::Expired),
                "fully_used" => Some(CreditLedgerStatus::FullyUsed),
                _ => None,
            },
        )
    }
}

impl std::fmt::Display for CreditLedgerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Revocation type enum (回收类型)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RevocationType {
    RefundRevoke,
    ExpireRevoke,
    CancelRevoke,
    /// Upgrade revocation (free user upgrading to paid subscription)
    UpgradeRevoke,
}

impl RevocationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RevocationType::RefundRevoke => "refund_revoke",
            RevocationType::ExpireRevoke => "expire_revoke",
            RevocationType::CancelRevoke => "cancel_revoke",
            RevocationType::UpgradeRevoke => "upgrade_revoke",
        }
    }
}

impl std::str::FromStr for RevocationType {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_enum(
            s.to_lowercase().as_str(),
            "Invalid revocation type",
            |s| match s {
                "refund_revoke" => Some(RevocationType::RefundRevoke),
                "expire_revoke" => Some(RevocationType::ExpireRevoke),
                "cancel_revoke" => Some(RevocationType::CancelRevoke),
                "upgrade_revoke" => Some(RevocationType::UpgradeRevoke),
                _ => None,
            },
        )
    }
}

impl std::fmt::Display for RevocationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Points Credit Ledger entity (积分账本实体)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsCreditLedger {
    pub id: Uuid,
    pub user_id: Uuid,
    pub realm_id: String,
    pub bucket_id: Uuid,
    pub credit_type: CreditType,
    pub source_type: CreditSourceType,
    pub source_id: String,
    pub granted_amount: i64,
    pub used_amount: i64,
    pub revoked_amount: i64,
    pub remaining_amount: i64,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Expected effective time. NULL ⟺ immediately
    /// available (current behavior; existing rows zero-regression); non-null ⟺
    /// enters the available set only when `effective_at <= NOW()`. Consumption
    /// selection AND derived balance share the same predicate so future-effective
    /// active rows are excluded from both until the effective moment — zero
    /// state migration, zero delay.
    pub effective_at: Option<chrono::DateTime<chrono::Utc>>,
    pub status: CreditLedgerStatus,
    /// Distribution attribution. Both `None` = direct write (admin/sdk grant,
    /// demo/test-only internal quota); both `Some` = rule-executed grant.
    pub distribution_event_id: Option<Uuid>,
    pub distribution_rule_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Points Consumption Allocation entity (消费分配实体)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsConsumptionAllocation {
    pub id: Uuid,
    pub transaction_id: Uuid,
    pub ledger_id: Uuid,
    pub wallet_id: Option<Uuid>,
    pub user_id: Uuid,
    pub realm_id: String,
    pub bucket_id: Uuid,
    pub allocated_amount: i64,
    pub ledger_remaining_after: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Consumption allocation joined with its ledger's credit type — the shape
/// needed to surface the SDK consume response `allocations` slice (`AllocationDetail`).
/// `PointsConsumptionAllocation` alone does not
/// carry `credit_type` (it lives on the ledger), so the consume-surface query
/// returns this view instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumptionAllocationView {
    pub allocation: PointsConsumptionAllocation,
    pub credit_type: CreditType,
}

/// Points Revocation Record entity (积分回收记录实体)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsRevocationRecord {
    pub id: Uuid,
    pub ledger_id: Uuid,
    pub user_id: Uuid,
    pub realm_id: String,
    pub revocation_type: RevocationType,
    pub revoked_amount: i64,
    pub reason: String,
    pub reference_id: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Points Wallet entity
///
/// The 5 per-type balance columns and
/// `total_balance` are gone from `points_wallets`; available balance is a
/// derived SUM over `points_credit_ledger`. This entity now carries only the
/// 4 lifetime analytics columns plus identity/status. Callers that need the
/// available balance must use `compute_available_balance` /
/// `compute_bucket_available_balances` (PointsRepository).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsWallet {
    pub id: Uuid,
    pub user_id: Uuid,
    pub realm_id: String,
    pub bucket_id: Option<Uuid>,
    pub total_topup_granted: i64,
    pub total_subscription_granted: i64,
    // Historical column name. Semantically this is total_topup_granted + total_subscription_granted,
    // not all granted credit types.
    pub total_recharged: i64,
    pub total_consumed: i64,
    pub status: WalletStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Points Transaction entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsTransaction {
    pub id: Uuid,
    pub wallet_id: Uuid,
    pub user_id: Uuid,
    pub realm_id: String,
    pub bucket_id: Uuid,
    pub transaction_type: TransactionType,
    pub amount: i64,
    pub balance_after: i64,
    pub topup_balance_after: Option<i64>,
    pub subscription_balance_after: Option<i64>,
    pub credit_type: Option<CreditType>,
    pub description: Option<String>,
    pub client_app_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub external_ref_id: Option<String>,
    /// Cross-bucket consumption grouping key. Non-unique; shared
    /// by the N transactions of a single multi-bucket consume so idempotency
    /// replay can reassemble the full result set. NULL for non-consume / legacy
    /// single-pool consume rows.
    pub correlation_id: Option<String>,
    /// Expected effective time of the granted points.
    /// Sourced from the linked `points_credit_ledger` row for
    /// grant-type transactions; `None` for consume/refund/revoke/expiration
    /// (immediate operations) and for grant rows whose ledger had a NULL
    /// `effective_at` (immediately available). Surfaced to admin/audit only
    /// (`points.manage`); the HTTP layer forces `None` on the `points.view`
    /// path and `skip_serializing_if` omits the key entirely.
    pub effective_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Distribution attribution (see `PointsCreditLedger`).
    pub distribution_event_id: Option<Uuid>,
    pub distribution_rule_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Points Balance DTO
///
/// The 5 typed balance fields and `total_balance`
/// are no longer read from `points_wallets` Stored columns. They are retained
/// on this DTO (external contract) and populated exclusively by the derived
/// SUM (`compute_available_balance`) in `PointsService::build_balance_from_derived`.
/// There is intentionally no `From<PointsWallet>` impl anymore — a bare
/// wallet row carries no balance information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsBalance {
    pub user_id: Uuid,
    pub balance: i64,
    pub topup_balance: i64,
    pub subscription_balance: i64,
    pub granted_balance: i64,
    pub registration_balance: i64,
    pub free_periodic_balance: i64,
    // Compatibility name for paid topup + subscription grants.
    pub total_recharged: i64,
    pub total_consumed: i64,
    pub unit: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Idempotency status enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IdempotencyStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

impl IdempotencyStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            IdempotencyStatus::Pending => "pending",
            IdempotencyStatus::Processing => "processing",
            IdempotencyStatus::Completed => "completed",
            IdempotencyStatus::Failed => "failed",
        }
    }
}

impl std::str::FromStr for IdempotencyStatus {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_enum(
            s.to_lowercase().as_str(),
            "Invalid idempotency status",
            |s| match s {
                "pending" => Some(IdempotencyStatus::Pending),
                "processing" => Some(IdempotencyStatus::Processing),
                "completed" => Some(IdempotencyStatus::Completed),
                "failed" => Some(IdempotencyStatus::Failed),
                _ => None,
            },
        )
    }
}

/// Idempotency result enum
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdempotencyResult<T> {
    /// New request, should proceed with processing
    New,
    /// Cached response from previous successful request
    Cached { transaction: T },
}

/// Idempotency key entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyKey {
    pub id: Uuid,
    pub realm_id: String,
    pub idempotency_key: String,
    pub status: IdempotencyStatus,
    pub request_data: Option<String>,
    pub response_data: Option<String>,
    pub transaction_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Paginated response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paginated<T> {
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
    pub data: Vec<T>,
}

/// Quota window snapshot (配额窗口快照) — captured at grant time (A2).
///
/// `key` is a stable display key derived from config (e.g. "5h"/"week"/"month"),
/// NOT a row ordinal. Downstream DTO / frontend identifies a window row by `key`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuotaWindow {
    /// Sliding window length in seconds (month ≈ 30d)
    pub window_seconds: i64,
    /// Quota limit (>= 0)
    pub limit: i64,
    /// Stable display key (config-derived, not row ordinal)
    pub key: String,
}

/// Quota source type enum (配额权益来源类型) — corresponds to the
/// points_quota_entitlements.source_type CHECK constraint.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum QuotaSourceType {
    /// 订阅首次赠送
    SubscriptionInitial,
    /// 订阅续费赠送
    SubscriptionRenewal,
    /// 订阅升级
    SubscriptionUpgrade,
    /// 免费定期发放
    FreePeriodicGrant,
}

impl QuotaSourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            QuotaSourceType::SubscriptionInitial => "subscription_initial",
            QuotaSourceType::SubscriptionRenewal => "subscription_renewal",
            QuotaSourceType::SubscriptionUpgrade => "subscription_upgrade",
            QuotaSourceType::FreePeriodicGrant => "free_periodic_grant",
        }
    }
}

impl std::str::FromStr for QuotaSourceType {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_enum(
            s.to_lowercase().as_str(),
            "Invalid quota source_type",
            |s| match s {
                "subscription_initial" => Some(QuotaSourceType::SubscriptionInitial),
                "subscription_renewal" => Some(QuotaSourceType::SubscriptionRenewal),
                "subscription_upgrade" => Some(QuotaSourceType::SubscriptionUpgrade),
                "free_periodic_grant" => Some(QuotaSourceType::FreePeriodicGrant),
                _ => None,
            },
        )
    }
}

impl std::fmt::Display for QuotaSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Quota entitlement status enum (配额权益状态) — corresponds to the
/// points_quota_entitlements.status CHECK constraint.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum QuotaEntitlementStatus {
    Active,
    Revoked,
    Expired,
}

impl QuotaEntitlementStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            QuotaEntitlementStatus::Active => "active",
            QuotaEntitlementStatus::Revoked => "revoked",
            QuotaEntitlementStatus::Expired => "expired",
        }
    }
}

impl std::str::FromStr for QuotaEntitlementStatus {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_enum(
            s.to_lowercase().as_str(),
            "Invalid quota entitlement status",
            |s| match s {
                "active" => Some(QuotaEntitlementStatus::Active),
                "revoked" => Some(QuotaEntitlementStatus::Revoked),
                "expired" => Some(QuotaEntitlementStatus::Expired),
                _ => None,
            },
        )
    }
}

impl std::fmt::Display for QuotaEntitlementStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Points Quota Entitlement entity (配额权益实体) — window-based grant replacing
/// per-period ledger issuance for subscription_credit / free_periodic_credit.
/// `credit_type` reuses the existing CreditType (only Subscription | FreePeriodic
/// are valid for the window model; enforced at DB CHECK).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsQuotaEntitlement {
    pub id: Uuid,
    pub user_id: Uuid,
    pub realm_id: String,
    pub bucket_id: Uuid,
    pub credit_type: CreditType,
    pub source_type: QuotaSourceType,
    pub source_id: String,
    /// Snapshot of windows captured at grant time (A2)
    pub quota_windows: Vec<QuotaWindow>,
    pub effective_from: chrono::DateTime<chrono::Utc>,
    /// NULL ⟺ ongoing; set on revoke/expire
    pub effective_until: Option<chrono::DateTime<chrono::Utc>>,
    pub status: QuotaEntitlementStatus,
    pub idempotency_key: String,
    /// Distribution attribution (see `PointsCreditLedger`).
    pub distribution_event_id: Option<Uuid>,
    pub distribution_rule_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Quota window read view (配额窗口读视图) — computed per (user, bucket, credit_type)
/// window: used amount from the points_transactions covering index aggregation, plus
/// derived remaining / reset time / tightest-constraint / exhausted flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaWindowView {
    pub key: String,
    pub limit: i64,
    pub used: i64,
    pub remaining: i64,
    pub window_seconds: i64,
    /// When the sliding window's oldest consume ages out / next reset point
    pub resets_at: Option<chrono::DateTime<chrono::Utc>>,
    /// True if this window is the minimum-remaining (tightest) constraint
    pub is_tightest: bool,
    /// True if remaining == 0
    pub exhausted: bool,
    /// Earliest `effective_from` across the entitlements contributing to this
    /// window key. Surfaces the quota grant's validity period in wallet views
    /// (PRD points.md §4.1 管理端区分展示生效区间).
    pub effective_from: chrono::DateTime<chrono::Utc>,
    /// Latest `effective_until` across contributing entitlements (`None` ⟺
    /// at least one is ongoing — the window keeps refreshing until the last
    /// entitlement ends).
    pub effective_until: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_status_from_str() {
        assert_eq!(
            "active".parse::<WalletStatus>().unwrap(),
            WalletStatus::Active
        );
        assert_eq!(
            "frozen".parse::<WalletStatus>().unwrap(),
            WalletStatus::Frozen
        );
        assert_eq!(
            "closed".parse::<WalletStatus>().unwrap(),
            WalletStatus::Closed
        );
        assert!("invalid".parse::<WalletStatus>().is_err());
    }

    #[test]
    fn test_idempotency_status_from_str() {
        assert_eq!(
            "pending".parse::<IdempotencyStatus>().unwrap(),
            IdempotencyStatus::Pending
        );
        assert_eq!(
            "processing".parse::<IdempotencyStatus>().unwrap(),
            IdempotencyStatus::Processing
        );
        assert_eq!(
            "completed".parse::<IdempotencyStatus>().unwrap(),
            IdempotencyStatus::Completed
        );
        assert_eq!(
            "failed".parse::<IdempotencyStatus>().unwrap(),
            IdempotencyStatus::Failed
        );
        assert!("invalid".parse::<IdempotencyStatus>().is_err());
    }

    #[test]
    fn test_safe_arithmetics_add() {
        assert_eq!(5i64.safe_add(10).unwrap(), 15);
        assert!(100i64.safe_add(i64::MAX - 50).is_err());
    }

    #[test]
    fn test_safe_arithmetics_sub() {
        assert_eq!(20i64.safe_sub(5).unwrap(), 15);
        assert!(5i64.safe_sub(10).is_err());
    }

    #[test]
    fn test_safe_arithmetics_mul() {
        assert_eq!(5i64.safe_mul(10).unwrap(), 50);
        assert!(100i64.safe_mul(i64::MAX / 50 + 1).is_err());
    }
}
