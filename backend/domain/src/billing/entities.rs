use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::common::entities::app_errors::CoreError;

/// Subscription entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: Uuid,
    pub realm_id: String,
    pub user_id: Uuid,
    /// External subscription ID from payment provider (Stripe, Creem, etc.)
    pub external_subscription_id: String,
    /// External product ID from payment provider
    /// Required field - every subscription must have a product/pricing plan
    pub external_product_id: String,
    /// Payment provider type (stripe, creem)
    /// Used to determine which external IDs are populated
    pub payment_provider: String,
    pub status: SubscriptionStatus,
    /// Herald entitlement key replacing plan_id + tier
    pub entitlement_key: String,
    /// Billing type snapshot taken from the entitlement mapping at fulfillment
    /// here: `Recurring` or `NonRenewing` (one-time purchases never create a
    /// subscription row). Lets reconciliation/views/api-ext filter without
    /// joining the mapping.
    pub billing_type: BillingType,
    /// External price ID from the payment provider
    pub external_price_id: Option<String>,
    /// Provider-specific metadata as JSON
    pub provider_metadata: Option<serde_json::Value>,
    /// Last sync timestamp from provider
    pub synced_at: Option<chrono::DateTime<chrono::Utc>>,
    pub current_period_start: Option<chrono::DateTime<chrono::Utc>>,
    pub current_period_end: Option<chrono::DateTime<chrono::Utc>>,
    pub cancel_at_period_end: bool,
    pub client_app_id: Option<Uuid>,
    pub cancel_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Subscription status - Creem official states
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionStatus {
    // Creem official states
    Active,          // Subscription active, normally billed - has access
    Canceled,        // Canceled - no access
    Expired,         // Subscription expired - no access
    Incomplete,      // Payment needs to be completed within 23 hours - no access
    Paused,          // Paused - no access
    Trialing,        // Trial period - has access
    PastDue,         // Payment failed or overdue - no access
    ScheduledCancel, // Scheduled to cancel at period end - has access (until cancel date)
    Dispute, // Payment dispute/refund chargeback in progress - has access (during investigation)

    // Local extension state
    Pending, // Reserved for local extension
}

/// Subscription statuses that still count as granting entitlements for the
/// raw SQL protection guards (entitlement-mapping disable guards in
/// herald-infra, provider-config deletion guard in herald-api), spelled as an
/// `IN (...)` list. Keep in sync with `SubscriptionStatus` above. Note this
/// set includes `past_due` and is therefore wider than
/// `SubscriptionStatus::has_access`, which governs account-level access.
pub const ACCESS_GRANTING_SUBSCRIPTION_STATUSES_SQL: &str =
    "'active','trialing','past_due','scheduled_cancel','dispute'";

impl SubscriptionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SubscriptionStatus::Active => "active",
            SubscriptionStatus::Canceled => "canceled",
            SubscriptionStatus::Expired => "expired",
            SubscriptionStatus::Incomplete => "incomplete",
            SubscriptionStatus::Pending => "pending",
            SubscriptionStatus::Trialing => "trialing",
            SubscriptionStatus::Paused => "paused",
            SubscriptionStatus::PastDue => "past_due",
            SubscriptionStatus::ScheduledCancel => "scheduled_cancel",
            SubscriptionStatus::Dispute => "dispute",
        }
    }

    /// Check if subscription has access based on status
    pub fn has_access(&self) -> bool {
        matches!(
            self,
            SubscriptionStatus::Active
                | SubscriptionStatus::Trialing
                | SubscriptionStatus::ScheduledCancel
                | SubscriptionStatus::Dispute
        )
    }

    pub fn can_transition_to(&self, target: &Self) -> bool {
        match (self, target) {
            // From Pending
            (SubscriptionStatus::Pending, SubscriptionStatus::Active)
            | (SubscriptionStatus::Pending, SubscriptionStatus::Trialing)
            | (SubscriptionStatus::Pending, SubscriptionStatus::Incomplete)
            | (SubscriptionStatus::Pending, SubscriptionStatus::Canceled)
            | (SubscriptionStatus::Pending, SubscriptionStatus::Expired)
            | (SubscriptionStatus::Pending, SubscriptionStatus::Paused)
            | (SubscriptionStatus::Pending, SubscriptionStatus::PastDue)
            // From Incomplete
            | (SubscriptionStatus::Incomplete, SubscriptionStatus::Active)
            | (SubscriptionStatus::Incomplete, SubscriptionStatus::Canceled)
            | (SubscriptionStatus::Incomplete, SubscriptionStatus::Expired)
            // From Trialing
            | (SubscriptionStatus::Trialing, SubscriptionStatus::Active)
            | (SubscriptionStatus::Trialing, SubscriptionStatus::Canceled)
            | (SubscriptionStatus::Trialing, SubscriptionStatus::Expired)
            | (SubscriptionStatus::Trialing, SubscriptionStatus::Paused)
            | (SubscriptionStatus::Trialing, SubscriptionStatus::PastDue)
            | (SubscriptionStatus::Trialing, SubscriptionStatus::Dispute)
            // From Active
            | (SubscriptionStatus::Active, SubscriptionStatus::Canceled)
            | (SubscriptionStatus::Active, SubscriptionStatus::Expired)
            | (SubscriptionStatus::Active, SubscriptionStatus::Paused)
            | (SubscriptionStatus::Active, SubscriptionStatus::PastDue)
            | (SubscriptionStatus::Active, SubscriptionStatus::ScheduledCancel)
            | (SubscriptionStatus::Active, SubscriptionStatus::Dispute)
            // From Paused
            | (SubscriptionStatus::Paused, SubscriptionStatus::Active)
            | (SubscriptionStatus::Paused, SubscriptionStatus::Canceled)
            | (SubscriptionStatus::Paused, SubscriptionStatus::Expired)
            | (SubscriptionStatus::Paused, SubscriptionStatus::PastDue)
            | (SubscriptionStatus::Paused, SubscriptionStatus::Dispute)
            // From PastDue
            | (SubscriptionStatus::PastDue, SubscriptionStatus::Active)
            | (SubscriptionStatus::PastDue, SubscriptionStatus::Canceled)
            | (SubscriptionStatus::PastDue, SubscriptionStatus::Expired)
            | (SubscriptionStatus::PastDue, SubscriptionStatus::Dispute)
            | (SubscriptionStatus::PastDue, SubscriptionStatus::ScheduledCancel)
            // From Dispute
            | (SubscriptionStatus::Dispute, SubscriptionStatus::Active)
            | (SubscriptionStatus::Dispute, SubscriptionStatus::Canceled)
            | (SubscriptionStatus::Dispute, SubscriptionStatus::Expired)
            | (SubscriptionStatus::Dispute, SubscriptionStatus::PastDue)
            // From ScheduledCancel
            | (SubscriptionStatus::ScheduledCancel, SubscriptionStatus::Active)
            | (SubscriptionStatus::ScheduledCancel, SubscriptionStatus::Canceled)
            | (SubscriptionStatus::ScheduledCancel, SubscriptionStatus::Expired)
            // From Canceled
            | (SubscriptionStatus::Canceled, SubscriptionStatus::Expired) => true,
            _ => self == target,
        }
    }
}

impl std::str::FromStr for SubscriptionStatus {
    type Err = crate::common::entities::app_errors::CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "active" => Ok(SubscriptionStatus::Active),
            "canceled" => Ok(SubscriptionStatus::Canceled),
            "expired" => Ok(SubscriptionStatus::Expired),
            "incomplete" => Ok(SubscriptionStatus::Incomplete),
            "pending" => Ok(SubscriptionStatus::Pending),
            "trialing" => Ok(SubscriptionStatus::Trialing),
            "paused" => Ok(SubscriptionStatus::Paused),
            "past_due" => Ok(SubscriptionStatus::PastDue),
            "scheduled_cancel" => Ok(SubscriptionStatus::ScheduledCancel),
            "dispute" => Ok(SubscriptionStatus::Dispute),
            _ => Err(
                crate::common::entities::app_errors::CoreError::InvalidSubscriptionStatus(
                    s.to_string(),
                ),
            ),
        }
    }
}

/// Payment event entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentEvent {
    pub id: Uuid,
    pub realm_id: String,
    /// External event ID from payment provider (unique per provider)
    pub external_event_id: String,
    /// Payment provider type (creem, stripe, etc.)
    pub payment_provider: String,
    pub event_type: String,
    pub subscription_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub processed: bool,
    pub processing_started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// EntitlementMapping domain entity - maps provider products to Herald entitlement keys.
///
/// Points distribution is configured via a rule set (`points_distribution_rules`)
/// owned by this mapping; the old scalar credit-strategy fields have been removed and are
/// no longer carried here. Rules are loaded separately via the rule query ports
/// and surfaced as `point_rules` on the management responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementMapping {
    pub id: Uuid,
    pub realm_id: String,
    pub payment_provider: String,
    pub external_product_id: String,
    pub external_price_id: Option<String>,
    pub entitlement_key: String,
    pub billing_type: Option<BillingType>,
    pub billing_period: Option<String>,
    /// Fixed service-period length (days) for non-renewing mappings
    /// `None` otherwise. Kept semantically isolated from rule `validity_days`
    /// (one-time credit expiry) — never overloaded by billing_type.
    pub service_duration_days: Option<i64>,
    pub enabled: bool,
    pub provider_product_info: Option<serde_json::Value>,
    /// Cross-cutting config dimension orthogonal to `billing_type` / points.
    /// Empty ⟺ no role grant (pure points / pure payment record). Non-empty ⟺
    pub granted_role_ids: Vec<Uuid>,
    pub synced_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl EntitlementMapping {
    /// Returns the billing kind string for use in provider metadata.
    ///
    /// Maps billing_type to the PRD-defined values: "subscription" for recurring,
    /// "one_time" for one-time. Defaults to "subscription" when unset.
    pub fn billing_kind(&self) -> String {
        match self.billing_type {
            Some(BillingType::OneTime) => "one_time".to_string(),
            _ => "subscription".to_string(),
        }
    }
}

/// Billing type enum for entitlement mappings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BillingType {
    Recurring,
    OneTime,
    /// Non-renewing subscription: a single payment for a fixed service period
    /// `"non_renewing"`; maps to billing_kind `"subscription"` (fulfilled as a
    /// `Subscription` row with a fixed `current_period_end`).
    NonRenewing,
}

impl BillingType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BillingType::Recurring => "recurring",
            BillingType::OneTime => "one_time",
            BillingType::NonRenewing => "non_renewing",
        }
    }
}

impl std::str::FromStr for BillingType {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "recurring" => Ok(BillingType::Recurring),
            "one_time" => Ok(BillingType::OneTime),
            "non_renewing" => Ok(BillingType::NonRenewing),
            _ => Err(CoreError::BadRequest(format!(
                "Invalid billing type: {}",
                s
            ))),
        }
    }
}

/// Map a Creem-native billing-type word form onto the domain `BillingType`:
/// Creem reports `"onetime"` for one-time products and `"subscription"` for
/// recurring ones, while Herald's canonical forms are `"one_time"` /
/// `"recurring"`. Returns `None` for unrecognized forms so each caller applies
/// its own fallback — the webhook checkout path defaults to recurring, the
/// product sync preserves the mapping's existing value.
pub fn normalize_creem_billing_type_word_form(raw: &str) -> Option<BillingType> {
    match raw.to_ascii_lowercase().as_str() {
        "onetime" | "one_time" => Some(BillingType::OneTime),
        "recurring" | "subscription" => Some(BillingType::Recurring),
        _ => None,
    }
}

/// Aggregated feature availability facts for a realm.
/// Used to determine which billing UI features should be shown.
#[derive(Debug, Clone)]
pub struct FeatureFacts {
    pub has_payment_providers: bool,
    pub has_entitlement_mappings: bool,
    pub has_enabled_mappings: bool,
    pub has_one_time_mappings: bool,
    pub has_recurring_mappings: bool,
    pub has_invoice_seller_config: bool,
    pub has_invoices: bool,
    pub has_subscription_history: bool,
}

#[cfg(test)]
mod billing_type_tests {
    use super::*;

    // The DB↔domain boundary stores `BillingType` as TEXT (postgres_repository
    // round-trips via `as_str` / `FromStr`). These tests pin the wire shape so a
    // rename of the enum variant or its serialized form does not silently break

    #[test]
    fn non_renewing_serializes_to_snake_case_string() {
        assert_eq!(BillingType::NonRenewing.as_str(), "non_renewing");
        // Existing variants keep their shape (regression guard).
        assert_eq!(BillingType::Recurring.as_str(), "recurring");
        assert_eq!(BillingType::OneTime.as_str(), "one_time");
    }

    #[test]
    fn non_renewing_parses_from_string() {
        assert_eq!(
            "non_renewing".parse::<BillingType>().unwrap(),
            BillingType::NonRenewing
        );
        // Case-insensitive (matches the existing to_ascii_lowercase normalization).
        assert_eq!(
            "Non_Renewing".parse::<BillingType>().unwrap(),
            BillingType::NonRenewing
        );
    }

    #[test]
    fn unknown_billing_type_is_rejected_as_bad_request() {
        let err = "lifetime".parse::<BillingType>().unwrap_err();
        assert!(matches!(err, CoreError::BadRequest(_)), "got {err:?}");
    }

    // Creem reports native word forms the strict `FromStr` rejects; an unknown
    // form must stay `None` (the product sync relies on that to preserve the
    // mapping's existing billing_type instead of clobbering it).
    #[test]
    fn creem_word_form_normalization_maps_onto_billing_type() {
        assert_eq!(
            normalize_creem_billing_type_word_form("onetime"),
            Some(BillingType::OneTime)
        );
        assert_eq!(
            normalize_creem_billing_type_word_form("subscription"),
            Some(BillingType::Recurring)
        );
        // Canonical forms and case-insensitivity pass through unchanged in
        // meaning; unknown forms yield None.
        assert_eq!(
            normalize_creem_billing_type_word_form("OneTime"),
            Some(BillingType::OneTime)
        );
        assert_eq!(
            normalize_creem_billing_type_word_form("recurring"),
            Some(BillingType::Recurring)
        );
        assert_eq!(normalize_creem_billing_type_word_form("lifetime"), None);
    }

    #[test]
    fn billing_kind_classifies_non_renewing_as_subscription() {
        // billing_kind drives provider metadata classification; non_renewing is
        // fulfilled as a Subscription, so it must classify as "subscription"
        let mapping = EntitlementMapping {
            billing_type: Some(BillingType::NonRenewing),
            // Only the fields read by billing_kind matter here.
            ..all_fields_default_mapping()
        };
        assert_eq!(mapping.billing_kind(), "subscription");
    }

    /// Minimal `EntitlementMapping` with non-collection fields defaulted, for the
    /// `billing_kind` test. Kept local so the test does not depend on a builder.
    fn all_fields_default_mapping() -> EntitlementMapping {
        EntitlementMapping {
            id: Uuid::nil(),
            realm_id: String::new(),
            payment_provider: String::new(),
            external_product_id: String::new(),
            external_price_id: None,
            entitlement_key: String::new(),
            billing_type: None,
            billing_period: None,
            service_duration_days: None,
            enabled: false,
            provider_product_info: None,
            granted_role_ids: Vec::new(),
            synced_at: None,
            created_at: chrono::DateTime::parse_from_rfc3339("1970-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339("1970-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        }
    }
}
