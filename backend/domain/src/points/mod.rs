pub mod distribution_rules;
pub mod dtos;
pub mod entities;
pub mod errors;
pub mod expiration_service;
pub mod grant_schedule;
pub mod idempotency_service;
pub mod policies;
pub mod ports;
pub mod service;
pub mod services;
pub mod subscription_service;

pub use distribution_rules::{
    CapturedRuleRef, DistributionEvent, DistributionGrantResult, DistributionPolicy,
    DistributionReplayCorruption, DistributionRuleError, DistributionRuleOwner,
    DistributionRuleReference, DistributionRuleSelection, DistributionTrigger,
    PointsDistributionRule, ReplayResultRows, RuleUpsert, credit_pair_for_trigger,
    event_key_for_free_periodic, event_key_for_payment, event_key_for_registration,
    event_key_for_subscription_period, event_key_for_subscription_upgrade, fold_replay_results,
    quota_source_type_for_trigger, select_and_sort_rules, validate_rule_for_owner,
};
pub use entities::{
    CreditLedgerStatus, CreditSourceType, CreditType, IdempotencyKey, IdempotencyResult,
    IdempotencyStatus, PointsBalance, PointsConsumptionAllocation, PointsCreditLedger,
    PointsQuotaEntitlement, PointsRevocationRecord, PointsTransaction, PointsWallet,
    QuotaEntitlementStatus, QuotaSourceType, QuotaWindow, QuotaWindowView, RechargeType,
    RevocationType, SafeArithmetics, TransactionType, WalletStatus,
};
pub use errors::PointsErrorExt;
pub use expiration_service::{ExpirationService, ExpirationSummary};
pub use idempotency_service::{IdempotencyService, IdempotencyStore};
pub use policies::PointsPolicy;
pub use ports::{
    LedgerFilters, LedgerUpdate, Pagination, PointsRepository, ReclaimLocator,
    TopupRefundRevokeOutcome, TopupRefundRevokeRequest, TransactionFilters, WalletDelta,
    WalletFilters,
};
pub use service::PointsService;
/// Stable display-key derivation for quota windows (design §4.2.2 / §4.4.3).
/// Re-exported so the api layer can derive keys when materializing the domain
/// inputs from the request shape (`QuotaWindowInput` carries no key).
pub use service::derive_window_key;
pub use subscription_service::{CancelMode, SubscriptionService};

pub use grant_schedule::{
    GrantPeriodType, GrantSummary, PointsGrantRecord, PointsGrantSchedule, ProcessResult,
};
pub use services::{GrantScheduler, RegistrationService};
