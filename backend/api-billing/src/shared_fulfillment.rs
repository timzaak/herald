//! Shared provider-fulfillment dispatch (design support-iap §5.3).
//!
//! Single function used by the Stripe, Creem, Apple and Google webhook / receipt
//! paths to complete a succeeded payment attempt. Extracted from the previously
//! duplicated inline `complete_succeeded_payment_attempt` + `BillingType` match
//! that lived in `stripe_webhook_handlers.rs` (local `fulfill_payment_attempt`)
//! and `webhook_handlers.rs` (Creem inline match).
//!
//! ## Extraction boundary (hard regression constraint)
//!
//! The extraction is **strictly limited** to the
//! `complete_succeeded_payment_attempt` call plus the `BillingType` decision
//! that the caller already supplies via `billing_type_override`. It does NOT
//! touch:
//!
//! - webhook signature verification,
//! - provider metadata extraction,
//! - status projection / subscription sync logic.
//!
//! The Stripe / Creem call sites were refactored to delegate here **line-by-line
//! equivalent** (same `provider_status`, `provider_transaction_id`, `completed_at`
//! and `billing_type_override` values, same `PaymentCompletionSource::ProviderWebhook`
//! source). Full regression of the Stripe/Creem webhook behaviour is covered by
//! the test slot (design §6.3).
//!
//! ## Signature note
//!
//! Design §5.3 sketches the signature without a `provider_status` parameter.
//! The existing Stripe/Creem call sites, however, pass a per-event
//! `provider_status` (e.g. `"succeeded"`) into `CompletePaymentAttemptInput`,
//! and the extraction must stay line-equivalent. We therefore keep
//! `provider_status` as an explicit parameter; this is a faithful refinement of
//! the design, not a behavioural deviation.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use herald_api_base::application::http::state::AppState;
use herald_core::domain::billing::entities::BillingType;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::purchase::services::{
    CompletePaymentAttemptInput, PaymentCompletionSource,
};

/// Complete a succeeded payment attempt for a provider event.
///
/// This is the single shared dispatch point for all provider webhook / receipt
/// fulfillment paths (Stripe, Creem, Apple, Google). It does NOT perform receipt
/// verification, ownership checks or provider API calls — those remain the
/// caller's responsibility (and untouched by this extraction).
///
/// # Arguments
///
/// * `app_state` - Shared application state (carries the `PurchaseService`).
/// * `realm_id` - Realm the provider event was addressed to (the webhook path
///   realm). Passed through as `expected_realm_id` so an event signed for one
///   realm can never complete another realm's attempt.
/// * `attempt_id` - The payment attempt to mark succeeded + fulfill.
/// * `provider` - Provider short name (`"stripe"` / `"creem"` / `"apple"` /
///   `"google"`). Recorded on the `PaymentCompletionSource::ProviderWebhook`
///   source for auditability; no behavioural branching on the value happens
///   here.
/// * `provider_status` - Provider-side status string (e.g. `"succeeded"`).
/// * `provider_transaction_id` - The provider's transaction id (Stripe
///   checkout/session id, Creem checkout id, Apple `originalTransactionId`,
///   Google `purchaseToken`).
/// * `completed_at` - When the provider considers the payment completed.
/// * `billing_type_override` - When `Some`, takes precedence over the
///   entitlement mapping's `billing_type` (matches the pre-extraction
///   Stripe/Creem behaviour).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fulfill_provider_event(
    app_state: &AppState,
    realm_id: &str,
    attempt_id: Uuid,
    provider: &str,
    provider_status: &str,
    provider_transaction_id: String,
    completed_at: DateTime<Utc>,
    billing_type_override: Option<BillingType>,
) -> Result<(), CoreError> {
    app_state
        .purchase_service
        .complete_succeeded_payment_attempt(CompletePaymentAttemptInput {
            attempt_id,
            provider_status: provider_status.to_string(),
            provider_transaction_id,
            completed_at,
            source: PaymentCompletionSource::ProviderWebhook {
                provider: provider.to_string(),
            },
            billing_type_override,
            expected_realm_id: Some(realm_id.to_string()),
        })
        .await?;
    Ok(())
}
