use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stripe Checkout Session response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CheckoutSession {
    /// Stripe checkout session ID
    pub id: String,
    /// URL to redirect user to for payment
    pub url: String,
    /// Customer ID (if created)
    pub customer: Option<String>,
    /// Payment status
    pub status: Option<String>,
    /// Payment intent ID
    pub payment_intent: Option<String>,
    /// Subscription ID (for subscription checkout)
    pub subscription: Option<String>,
    /// Metadata
    pub metadata: serde_json::Value,
}

/// Request to create a Stripe checkout session
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateCheckoutRequest {
    /// Client app ID
    #[serde(rename = "clientAppId")]
    pub client_app_id: Uuid,
    /// Herald entitlement mapping ID
    #[serde(rename = "mappingId")]
    pub mapping_id: Uuid,
    /// User ID (for subscription tracking)
    #[serde(rename = "userId")]
    pub user_id: Option<Uuid>,
    /// Customer email for checkout prefill and payment provider requirements
    #[serde(rename = "customerEmail", skip_serializing_if = "Option::is_none")]
    pub customer_email: Option<String>,
    /// Success URL
    #[serde(rename = "successUrl")]
    pub success_url: String,
    /// Cancel URL
    #[serde(rename = "cancelUrl")]
    pub cancel_url: String,
    /// Billing period (monthly/yearly)
    #[serde(rename = "billingPeriod")]
    pub billing_period: String,
    /// Trial days (optional)
    #[serde(rename = "trialDays")]
    pub trial_days: Option<u32>,
    /// Plan price amount (in cents)
    #[serde(rename = "priceAmount")]
    pub price_amount: i64,
    /// Plan currency (e.g., "usd")
    #[serde(rename = "currency")]
    pub currency: String,
    /// Plan name
    #[serde(rename = "planName")]
    pub plan_name: String,
    /// Real Stripe Price ID to reference (e.g. "price_...").
    /// - Some(non-empty) -> checkout references the existing Price via
    ///   `line_items[0][price]` (real-price semantics).
    /// - None -> price-less fallback: rebuild an ad-hoc Price via
    ///   `line_items[0][price_data]` (price-less providers / no external price).
    #[serde(rename = "priceId", skip_serializing_if = "Option::is_none")]
    pub price_id: Option<String>,
    /// Realm ID (for webhook routing)
    #[serde(rename = "realmId")]
    pub realm_id: String,
    /// Webhook URL (optional, for realm-specific webhooks)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    /// Extra metadata to merge into the Stripe checkout session metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
    /// Checkout mode: "subscription" (default) or "payment" (one-time)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// App locale tag (e.g. `en`, `zh-CN`) mapped to Stripe's `locale`
    /// checkout parameter so the hosted page follows the app language.
    /// Mapped by the client; unrecognized values are dropped (Stripe then
    /// auto-detects from the buyer's browser).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

/// Request to create a Stripe payment intent.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreatePaymentIntentRequest {
    /// Amount in the smallest currency unit (for example cents)
    pub amount: i64,
    /// ISO 4217 currency code
    pub currency: String,
    /// Customer email for receipt and payment provider requirements
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_email: Option<String>,
    /// Provider metadata stored on the payment intent
    pub metadata: std::collections::HashMap<String, String>,
}

/// Stripe Payment Intent response.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaymentIntent {
    pub id: String,
    pub client_secret: String,
    pub amount: i64,
    pub currency: String,
    pub status: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Request to cancel a Stripe subscription.
///
/// `cancel_at_period_end = false` issues `DELETE /v1/subscriptions/{id}` (immediate);
/// `true` issues `POST /v1/subscriptions/{id}` with `cancel_at_period_end=true` (scheduled).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CancelSubscriptionRequest {
    /// Stripe subscription id (`sub_...`).
    pub subscription_id: String,
    /// When true, the subscription stays active until period end then cancels.
    pub cancel_at_period_end: bool,
}

/// Stripe subscription object after a cancel call. Only the fields we read back.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CancelSubscriptionResponse {
    pub id: String,
    pub status: Option<String>,
    pub cancel_at_period_end: Option<bool>,
    /// Unix timestamp the subscription was canceled, if any.
    pub canceled_at: Option<i64>,
}

/// Stripe webhook event envelope
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StripeWebhookEvent {
    /// Event ID
    pub id: String,
    /// Event object (contains the actual data)
    pub object: serde_json::Value,
    /// Event type (e.g., "checkout.session.completed")
    #[serde(rename = "type")]
    pub event_type: String,
    /// API version
    #[serde(rename = "api_version")]
    pub api_version: Option<String>,
    /// Created timestamp
    pub created: i64,
    /// Request ID (for idempotency)
    pub request: Option<serde_json::Value>,
}

/// Parameters for listing Stripe events via GET /v1/events
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ListEventsParams {
    /// Unix timestamp — only return events created at or after this time
    pub created_gte: i64,
    /// Unix timestamp — only return events created at or before this time
    pub created_lte: i64,
    /// Filter to specific event types (e.g., "checkout.session.completed")
    pub event_types: Vec<String>,
    /// Maximum number of events to return per page (max 100)
    pub limit: i64,
    /// Pagination cursor — event ID to start after
    pub starting_after: Option<String>,
}

/// A single Stripe event returned by the Events API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StripeEvent {
    /// Event ID (e.g., "evt_1P...")
    pub id: String,
    /// Event type (e.g., "checkout.session.completed")
    #[serde(rename = "type")]
    pub event_type: String,
    /// Created timestamp (Unix)
    pub created: i64,
    /// Event payload (the actual object data)
    pub data: serde_json::Value,
}

/// Paginated list of Stripe events
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StripeEventList {
    /// List of events in this page
    pub data: Vec<StripeEvent>,
    /// Whether more events are available beyond this page
    pub has_more: bool,
}
