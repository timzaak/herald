// Payment Attempt domain entities

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::common::entities::app_errors::CoreError;

/// Unified payment attempt for initiator-based payment platforms
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaymentAttempt {
    pub id: Uuid,
    pub realm_id: String,
    pub user_id: Uuid,
    pub payment_provider: String, // "stripe", "creem"
    pub target_type: PurchasableTarget,
    pub target_id: Uuid,
    pub amount: i64,
    pub currency: String,
    pub status: PaymentAttemptStatus,
    pub is_one_time_role: bool, // anti-repeat flag (one_time + role mappings)
    pub provider_reference: Option<String>, // checkout session ID for Stripe/Creem
    pub provider_status: Option<String>, // Raw status from provider
    pub metadata: Option<serde_json::Value>,
    pub expires_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Type of purchasable target
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PurchasableTarget {
    EntitlementMapping,
}

impl std::fmt::Display for PurchasableTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EntitlementMapping => write!(f, "entitlement_mapping"),
        }
    }
}

impl std::str::FromStr for PurchasableTarget {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "entitlement_mapping" | "subscription_entitlement" | "points_package" => {
                Ok(Self::EntitlementMapping)
            }
            _ => Err(CoreError::BadRequest(format!(
                "Invalid purchasable target: {s}"
            ))),
        }
    }
}

/// Payment attempt status
///
/// # State Transition Matrix
///
/// | From \ To   | Pending | RequiresAction | Succeeded | Failed | Cancelled | Expired |
/// |-------------|---------|----------------|-----------|--------|-----------|---------|
/// | Pending     | -       | ✅             | ✅        | ✅     | ✅        | ✅      |
/// | RequiresAction | -   | -              | ✅        | ✅     | ✅        | ✅      |
/// | Succeeded   | ❌     | ❌             | ✅*       | ❌     | ❌        | ❌      |
/// | Failed      | ❌     | ❌             | ❌        | ✅*    | ❌        | ❌      |
/// | Cancelled   | ❌     | ❌             | ❌        | ❌     | ✅*       | ❌      |
/// | Expired     | ❌     | ❌             | ❌        | ❌     | ❌        | ✅*     |
///
/// \* = Idempotent (no-op if already in target state)
///
/// # Invalid Transitions (Blocked)
///
/// - ❌ `Expired → Succeeded` - Expired payments cannot succeed
/// - ❌ `Failed → Succeeded` - Failed payments cannot succeed
/// - ❌ `Succeeded → Pending` - Completed payments cannot revert to pending
/// - ❌ `Cancelled → Succeeded` - Cancelled payments cannot succeed
/// - ❌ `Succeeded → Failed` - Success cannot become failure (general `can_transition_to` blocks this)
/// - ❌ `Succeeded → Cancelled` - Success cannot be cancelled
/// - ❌ `Succeeded → Expired` - Success cannot expire
///
/// Note: `Succeeded -> Failed` is allowed ONLY via `mark_failed_for_async_recovery` for async payment
/// recovery (eager strategy). The general `can_transition_to` method does NOT allow this transition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PaymentAttemptStatus {
    Pending,
    RequiresAction,
    Succeeded,
    Failed,
    Cancelled,
    Expired,
}

impl PaymentAttemptStatus {
    /// Check if transition to target status is allowed
    ///
    /// # Valid Transitions
    ///
    /// From `Pending`:
    /// - → `RequiresAction` (user action required)
    /// - → `Succeeded` (payment completed)
    /// - → `Failed` (payment failed)
    /// - → `Cancelled` (user cancelled)
    /// - → `Expired` (timeout)
    ///
    /// From `RequiresAction`:
    /// - → `Succeeded` (payment completed)
    /// - → `Failed` (payment failed)
    /// - → `Cancelled` (user cancelled)
    /// - → `Expired` (timeout)
    ///
    /// From `Succeeded`, `Failed`, `Cancelled`, `Expired`:
    /// - Only idempotent transitions to same state allowed
    ///
    /// # Examples
    ///
    /// ```rust
    /// assert!(PaymentAttemptStatus::Pending.can_transition_to(&PaymentAttemptStatus::Succeeded));
    /// assert!(!PaymentAttemptStatus::Expired.can_transition_to(&PaymentAttemptStatus::Succeeded));
    /// assert!(PaymentAttemptStatus::Succeeded.can_transition_to(&PaymentAttemptStatus::Succeeded)); // idempotent
    /// ```
    pub fn can_transition_to(&self, target: &Self) -> bool {
        // Idempotent: always allow transition to same state
        if self == target {
            return true;
        }

        // Valid state transitions
        matches!(
            (self, target),
            // Pending can transition to any state
            (PaymentAttemptStatus::Pending, PaymentAttemptStatus::RequiresAction)
            | (PaymentAttemptStatus::Pending, PaymentAttemptStatus::Succeeded)
            | (PaymentAttemptStatus::Pending, PaymentAttemptStatus::Failed)
            | (PaymentAttemptStatus::Pending, PaymentAttemptStatus::Cancelled)
            | (PaymentAttemptStatus::Pending, PaymentAttemptStatus::Expired)
            |
            // RequiresAction can transition to terminal states
            (PaymentAttemptStatus::RequiresAction, PaymentAttemptStatus::Succeeded)
            | (PaymentAttemptStatus::RequiresAction, PaymentAttemptStatus::Failed)
            | (PaymentAttemptStatus::RequiresAction, PaymentAttemptStatus::Cancelled)
            | (PaymentAttemptStatus::RequiresAction, PaymentAttemptStatus::Expired)
        )
    }

    /// Check if this status can be transitioned to Failed for async payment recovery.
    /// Only `Succeeded` allows this special transition (for eager strategy revocation).
    pub fn can_transition_to_failed_for_async_recovery(&self) -> bool {
        matches!(self, PaymentAttemptStatus::Succeeded)
    }

    /// Check if this is a terminal state (cannot transition out)
    ///
    /// Terminal states: `Succeeded`, `Failed`, `Cancelled`, `Expired`
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            PaymentAttemptStatus::Succeeded
                | PaymentAttemptStatus::Failed
                | PaymentAttemptStatus::Cancelled
                | PaymentAttemptStatus::Expired
        )
    }

    /// Check if this is an active state (can still be completed)
    ///
    /// Active states: `Pending`, `RequiresAction`
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            PaymentAttemptStatus::Pending | PaymentAttemptStatus::RequiresAction
        )
    }
}

impl std::fmt::Display for PaymentAttemptStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::RequiresAction => write!(f, "RequiresAction"),
            Self::Succeeded => write!(f, "Succeeded"),
            Self::Failed => write!(f, "Failed"),
            Self::Cancelled => write!(f, "Cancelled"),
            Self::Expired => write!(f, "Expired"),
        }
    }
}

impl std::str::FromStr for PaymentAttemptStatus {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Pending" => Ok(Self::Pending),
            "RequiresAction" => Ok(Self::RequiresAction),
            "Succeeded" => Ok(Self::Succeeded),
            "Failed" => Ok(Self::Failed),
            "Cancelled" => Ok(Self::Cancelled),
            "Expired" => Ok(Self::Expired),
            _ => Err(CoreError::BadRequest(format!(
                "Invalid payment attempt status: {s}"
            ))),
        }
    }
}

/// Platform-specific payment context for initiating payment
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentContext {
    pub stripe_checkout_url: Option<String>, // Checkout URL for Stripe
    pub creem_checkout_url: Option<String>,  // Checkout URL for Creem
    pub client_secret: Option<String>,       // For Stripe elements
    /// WeChat Native (PC scan) `code_url` rendered as a QR code.
    pub wechat_code_url: Option<String>,
    /// WeChat JSAPI invocation params for in-WeChat-browser payment.
    pub wechat_jsapi_params: Option<WechatJsapiParams>,
}

/// Parameters returned to the browser for the WeChat JSAPI
/// `WeixinJSBridge.invoke('getBrandWCPayRequest', ...)` call. Flat provider
/// field on `PaymentContext` per DEC-wechat-support-011 (no generic payload).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WechatJsapiParams {
    pub app_id: String,
    pub time_stamp: String,
    pub nonce_str: String,
    /// `prepay_id=...`
    pub package: String,
    pub sign_type: String,
    pub pay_sign: String,
}

/// Purchase history row returned by the repository's list_purchase_history query.
#[derive(Debug, Clone)]
pub struct PurchaseHistoryRow {
    pub user_id: Uuid,
    pub attempt_id: Uuid,
    pub target_mapping_id: Uuid,
    pub product_name: Option<String>,
    pub points: Option<i64>,
    pub amount: i64,
    pub currency: String,
    pub payment_provider: String,
    pub status: String,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
