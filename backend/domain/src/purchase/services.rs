use uuid::Uuid;

use crate::billing::entities::BillingType;
use crate::common::entities::app_errors::CoreError;
use crate::payment_attempt::PurchasableTarget;
use crate::payment_attempt::entities::{PaymentAttempt, PaymentContext};

/// Metadata keys written into provider checkout sessions and read back by webhook handlers.
/// Both sides must use these constants to prevent mismatches.
pub mod metadata_keys {
    pub const HERALD_REALM_ID: &str = "heraldRealmId";
    pub const HERALD_USER_ID: &str = "heraldUserId";
    pub const TARGET_TYPE: &str = "targetType";
    pub const TARGET_ID: &str = "targetId";
    pub const ATTEMPT_ID: &str = "attemptId";
}

/// Checkout flow a client requests on `create_payment_attempt`. `Hosted`
/// (default) returns a Stripe Hosted Checkout URL; `PaymentIntent` returns a
/// raw PaymentIntent `client_secret` so a mobile wallet SDK (Apple Pay /
/// Google Pay) can confirm the payment client-side. `PaymentIntent` is only
/// valid for `stripe` + one-time purchases — enforced in
/// `build_payment_context`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PaymentFlow {
    #[default]
    Hosted,
    PaymentIntent,
}

impl PaymentFlow {
    /// `None` / `""` / `"hosted"` → `Hosted`; `"payment_intent"` →
    /// `PaymentIntent`; anything else is a `BadRequest`.
    pub fn parse(s: Option<&str>) -> Result<Self, CoreError> {
        match s {
            None | Some("") | Some("hosted") => Ok(PaymentFlow::Hosted),
            Some("payment_intent") => Ok(PaymentFlow::PaymentIntent),
            Some(other) => Err(CoreError::BadRequest(format!(
                "invalid flow: {other}; expected 'hosted' or 'payment_intent'"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparePaymentAttemptInput {
    pub realm_id: String,
    pub user_id: Uuid,
    pub user_email: Option<String>,
    pub payment_provider: String,
    pub target_type: String,
    pub target_id: Uuid,
    pub metadata: Option<serde_json::Value>,
    /// Checkout flow requested by the client. `Hosted` (default) for the
    /// redirect-to-hosted-page journey; `PaymentIntent` for mobile wallet
    /// SDK confirmation (stripe + one-time only). Ignored by non-stripe
    /// providers — `PaymentIntent` is rejected for them at validation.
    pub flow: PaymentFlow,
    /// WeChat-only checkout scene: `"native"` (default) or `"jsapi"`. Ignored
    /// by other providers (DEC-wechat-support-009/010).
    pub payment_scene: Option<String>,
    /// WeChat JSAPI payer openid; required when `payment_scene = "jsapi"`
    /// (DEC-wechat-support-009). Obtained out-of-band via the WeChat OAuth
    /// login flow.
    pub openid: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PurchaseTargetSnapshot {
    pub target_type: PurchasableTarget,
    pub target_id: Uuid,
    pub amount: i64,
    pub currency: String,
    pub title: String,
    pub provider_external_product_id: Option<String>,
    /// Real provider Price ID (e.g. Stripe `price_...`). Populated from
    /// `EntitlementMapping.external_price_id`. None for price-less providers
    /// (Creem) or mappings without an external price.
    pub provider_external_price_id: Option<String>,
    pub billing_period: Option<String>,
    pub billing_type: Option<BillingType>,
    /// Anti-repeat flag: TRUE only for one_time + non-empty granted_role_ids.
    /// Drives the `payment_attempts.is_one_time_role` column + DB unique guard.
    pub is_one_time_role: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedPaymentAttempt {
    pub attempt: PaymentAttempt,
    pub target: PurchaseTargetSnapshot,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreatedPaymentAttempt {
    pub attempt: PaymentAttempt,
    pub context: PaymentContext,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaymentCompletionSource {
    InternalApi,
    ProviderWebhook { provider: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompletePaymentAttemptInput {
    pub attempt_id: Uuid,
    pub provider_status: String,
    pub provider_transaction_id: String,
    pub completed_at: chrono::DateTime<chrono::Utc>,
    pub source: PaymentCompletionSource,
    /// Override the billing_type read from the entitlement mapping.
    /// Used when the provider webhook carries billing_type metadata that
    /// should take precedence over the mapping's stored billing_type.
    pub billing_type_override: Option<BillingType>,
    /// Realm the completion event was addressed to (the webhook path realm).
    /// When `Some`, the attempt's own realm must match or the completion is
    /// rejected — a signature-valid event for realm A must never fulfill a
    /// realm-B attempt referenced through forged metadata. `None` is reserved
    /// for the internal-key fulfillment endpoint, which has no path realm.
    pub expected_realm_id: Option<String>,
}

/// Input for the IAP receipt submission path.
///
/// IAP (Apple App Store / Google Play) is a "purchase already happened on the
/// store -> client submits credential -> Herald verifies + fulfils" reverse
/// payment semantic. Unlike the Stripe/Creem forward path, IAP never returns a
/// checkout URL, so it reuses `prepare_payment_attempt`'s row creation but
/// **skips** `build_payment_context` (design §5.2). The store-side transaction
/// id (`originalTransactionId` / `purchaseToken`) is bound as the attempt's
/// `provider_reference` up-front.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateIapAttemptInput {
    pub realm_id: String,
    pub user_id: Uuid,
    /// Provider short name: `"apple"` or `"google"`.
    pub payment_provider: String,
    /// Purchasable target type. Always `EntitlementMapping` for IAP.
    pub target_type: PurchasableTarget,
    /// Entitlement mapping id (the store product's Herald mapping).
    pub target_id: Uuid,
    /// Store-side transaction id used as the attempt's `provider_reference`:
    /// Apple `originalTransactionId` / Google `purchaseToken`.
    pub provider_reference: String,
    /// Optional diagnostic metadata persisted on the attempt.
    pub metadata: Option<serde_json::Value>,
}

#[cfg(test)]
mod payment_flow_tests {
    use super::PaymentFlow;
    use crate::common::entities::app_errors::CoreError;

    #[test]
    fn parse_absent_or_hosted_yields_hosted() {
        // Absent/empty must mean hosted: the default web journey passes no
        // flow field at all, and existing clients must keep working.
        assert_eq!(PaymentFlow::parse(None).unwrap(), PaymentFlow::Hosted);
        assert_eq!(PaymentFlow::parse(Some("")).unwrap(), PaymentFlow::Hosted);
        assert_eq!(
            PaymentFlow::parse(Some("hosted")).unwrap(),
            PaymentFlow::Hosted
        );
    }

    #[test]
    fn parse_payment_intent_yields_payment_intent() {
        assert_eq!(
            PaymentFlow::parse(Some("payment_intent")).unwrap(),
            PaymentFlow::PaymentIntent
        );
    }

    #[test]
    fn parse_unknown_value_is_bad_request() {
        // Unknown flows must fail loud at parse time, not silently fall back
        // to hosted (a typo like "paymentintent" would otherwise hand a
        // mobile app a checkout URL it cannot open).
        let err = PaymentFlow::parse(Some("bogus")).unwrap_err();
        match err {
            CoreError::BadRequest(msg) => {
                assert!(msg.contains("invalid flow: bogus"), "unexpected: {msg}");
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    #[test]
    fn default_is_hosted() {
        assert_eq!(PaymentFlow::default(), PaymentFlow::Hosted);
    }
}
