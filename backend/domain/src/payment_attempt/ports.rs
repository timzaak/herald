// Payment Attempt repository ports

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use super::entities::PurchaseHistoryRow;
use super::entities::{PaymentAttempt, PaymentAttemptStatus};
use crate::billing::BillingType;
use crate::common::entities::app_errors::CoreError;
use crate::points::CapturedRuleRef;

/// Input for creating a payment attempt.
///
/// The single-target `bucket_id` has been removed with the multi-wallet rule
/// model: at creation time the repository resolves the distribution rules that
/// match the attempt's billing type (`topup` for `OneTime`,
/// `subscription_initial` for `Recurring` / `NonRenewing`) and atomically
/// writes both the attempt row and its rule/bucket snapshot
/// (`payment_attempt_point_rules`) in one transaction. First fulfillment then
/// replays that snapshot via the `CapturedPaymentRules` executor selection.
#[derive(Debug, Clone)]
pub struct CreatePaymentAttemptInput {
    pub realm_id: String,
    pub user_id: Uuid,
    pub payment_provider: String,
    pub target_type: String, // "entitlement_mapping" (legacy values "subscription_entitlement" and "points_package" are accepted)
    pub target_id: Uuid,
    /// Drives rule snapshot resolution: `OneTime` -> `topup`,
    /// `Recurring`/`NonRenewing` -> `subscription_initial`.
    pub billing_type: BillingType,
    pub amount: i64,
    pub currency: String,
    pub provider_reference: Option<String>,
    pub metadata: Option<Value>,
    /// Anti-repeat flag: TRUE only for one_time + role mappings.
    pub is_one_time_role: bool,
}

/// Input for recording a subscription renewal payment attempt (find-or-create).
///
/// Unlike checkout-driven `CreatePaymentAttemptInput`, this records an already-completed
/// renewal charge: status is `Succeeded`, `expires_at`/`completed_at` are set to
/// `input.completed_at`, and no fulfillment is triggered.
///
/// Precondition: `amount > 0` (enforced by `payment_attempts.amount CHECK(amount > 0)`,
/// `20260408_unified_purchase.sql:100`). Callers MUST skip the renewal attempt + invoice
/// write when `amount == 0` (zero-yuan cycle).
#[derive(Debug, Clone)]
pub struct RecordRenewalAttemptInput {
    pub realm_id: String,
    pub user_id: Uuid,
    pub payment_provider: String, // "stripe" | "creem"
    pub target_id: Uuid,          // entitlement mapping id
    pub amount: i64,              // smallest currency unit; caller guarantees > 0
    pub currency: String,
    pub provider_reference: String, // idempotency key
    pub completed_at: DateTime<Utc>,
}

/// Repository trait for PaymentAttempt operations
#[allow(async_fn_in_trait)]
pub trait PaymentAttemptRepository: Send + Sync {
    /// Create a new payment attempt
    async fn create_payment_attempt(
        &self,
        input: CreatePaymentAttemptInput,
    ) -> Result<PaymentAttempt, CoreError>;

    /// Direct-insert a renewal payment attempt as already-Succeeded.
    ///
    /// Dedicated port because `create_payment_attempt` hardcodes `status=Pending`/
    /// `completed_at=None`/`expires_at=now+2h` (checkout semantics) which cannot
    /// represent a renewal charge that has already completed. This port sets
    /// `status=Succeeded`, `completed_at=Some(input.completed_at)`,
    /// `expires_at=input.completed_at` (already-succeeded attempts have no expiry
    /// semantics), `target_type=entitlement_mapping` (matches DB CHECK
    /// chk_target_type from migration 20260609 and PurchasableTarget canonical
    /// value; legacy "subscription_entitlement" is a read-only FromStr alias).
    /// Does NOT trigger
    /// fulfillment (renewal does not create a subscription).
    async fn insert_succeeded_renewal_attempt(
        &self,
        input: RecordRenewalAttemptInput,
    ) -> Result<PaymentAttempt, CoreError>;

    /// Find a payment attempt by ID
    async fn find_payment_attempt_by_id(
        &self,
        realm_id: &str,
        attempt_id: Uuid,
    ) -> Result<Option<PaymentAttempt>, CoreError>;

    /// Find a payment attempt by ID only (without realm filter)
    /// Used for webhook handlers where realm is not known upfront
    async fn find_payment_attempt_by_id_only(
        &self,
        attempt_id: Uuid,
    ) -> Result<Option<PaymentAttempt>, CoreError>;

    /// Find payment attempts by user (paginated)
    async fn find_payment_attempts_by_user(
        &self,
        realm_id: &str,
        user_id: Uuid,
        limit: u64,
    ) -> Result<Vec<PaymentAttempt>, CoreError>;

    /// Find a payment attempt by provider reference (for webhooks)
    async fn find_payment_attempt_by_provider_reference(
        &self,
        provider: &str,
        reference: &str,
    ) -> Result<Option<PaymentAttempt>, CoreError>;

    /// Update a payment attempt
    async fn update_payment_attempt(
        &self,
        attempt: PaymentAttempt,
    ) -> Result<PaymentAttempt, CoreError>;

    /// Update a payment attempt only if its current status still matches the
    /// status observed by the caller.
    async fn update_payment_attempt_with_status_guard(
        &self,
        attempt: PaymentAttempt,
        expected_status: PaymentAttemptStatus,
    ) -> Result<PaymentAttempt, CoreError>;

    /// List expired attempts (for cleanup)
    async fn list_expired_attempts(
        &self,
        before: DateTime<Utc>,
    ) -> Result<Vec<PaymentAttempt>, CoreError>;

    /// List successful payments with filters and pagination. None selects all
    /// users in the realm; callers must authorize realm billing access first.
    async fn list_purchase_history(
        &self,
        realm_id: &str,
        user_id: Option<Uuid>,
        payment_provider: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<PurchaseHistoryRow>, i64), CoreError>;

    /// Check whether a user has at least one succeeded payment attempt for the
    /// given `target_id` (entitlement mapping id). Used by the M3 one-time+role
    /// ownership gate (design §5.4): a user who already succeeded a purchase for
    /// a one_time+role target is blocked from re-buying. `target_id` matches the
    /// `payment_attempts.target_id` column (FK to provider_entitlement_mappings).
    async fn has_succeeded_attempt(
        &self,
        user_id: Uuid,
        target_id: Uuid,
    ) -> Result<bool, CoreError>;

    /// Load the rule/bucket references captured for an attempt at purchase
    /// creation (`payment_attempt_point_rules`). Frozen at creation time: a rule
    /// disabled after capture is still returned here. First fulfillment feeds
    /// this into the `CapturedPaymentRules` executor selection so an already-paid
    /// attempt completes its captured grant set regardless of later rule
    /// enable/disable. Returns an empty vec for an
    /// attempt that matched no rules at creation (a valid zero-result event).
    async fn find_captured_rule_refs(
        &self,
        realm_id: &str,
        attempt_id: Uuid,
    ) -> Result<Vec<CapturedRuleRef>, CoreError>;
}
