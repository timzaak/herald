use serde::{Deserialize, Serialize};

/// Request to create a Creem checkout session
#[derive(Debug, Clone, Serialize)]
pub struct CreateCheckoutRequest {
    pub product_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success_url: Option<String>,
    pub customer: CreemCheckoutCustomer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
    /// App locale tag (e.g. `en`, `zh-CN`); never part of the JSON body — the
    /// client maps it to the undocumented-but-observed `X-Creem-Language`
    /// request header so the hosted checkout page follows the app language.
    /// Creem localizes automatically by customer language when absent.
    /// Unrecognized values are dropped (fail-open).
    #[serde(skip)]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreemCheckoutCustomer {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// Response from Creem checkout session creation
#[derive(Debug, Clone, Deserialize)]
pub struct CheckoutSession {
    pub id: String,
    pub checkout_url: String,
    pub status: String,
}

/// Creem webhook event
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemWebhookEvent {
    pub id: String,
    #[serde(rename = "eventType")]
    pub event_type: String,
    pub object: serde_json::Value,
}

/// Creem subscription object from webhook
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemSubscription {
    pub id: String,
    pub customer: CreemCustomer,
    pub product: CreemProduct,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period_start: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period_end: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_at_period_end: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemCustomer {
    pub email: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemProduct {
    pub id: String,
    pub name: String,
    pub price: i64,
    pub currency: String,
    #[serde(rename = "billing_type")]
    pub billing_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_period: Option<String>,
}

/// Creem dispute object from webhook
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemDispute {
    pub id: String,
    pub subscription_id: String,
    pub status: String,
    pub amount: i64,
    pub currency: String,
    pub reason: String,
    pub created_at: i64,
}

/// Creem refund object from webhook
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemRefund {
    pub id: String,
    pub subscription_id: String,
    pub amount: i64,
    pub currency: String,
    pub reason: Option<String>,
    pub created_at: i64,
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

/// Pagination metadata returned by Creem list/search endpoints
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemPagination {
    pub total_records: i64,
    pub total_pages: i64,
    pub current_page: i32,
    pub next_page: Option<i32>,
    pub prev_page: Option<i32>,
}

// ---------------------------------------------------------------------------
// Transaction search
// ---------------------------------------------------------------------------

/// Parameters for `GET /v1/transactions/search`
#[derive(Debug, Clone, Serialize)]
pub struct SearchTransactionsParams {
    pub page_number: i32,
    pub page_size: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_after: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemTransactionOrder {
    pub order_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemTransactionSub {
    pub subscription_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemTransactionCustomer {
    pub customer_id: String,
}

/// A single Creem transaction returned by the search endpoint
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemTransaction {
    pub id: String,
    pub mode: String,
    pub object: String,
    pub amount: i64,
    pub currency: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub status: String,
    pub created_at: i64,
    pub amount_paid: i64,
    pub refunded_amount: Option<i64>,
    pub order: Option<CreemTransactionOrder>,
    pub subscription: Option<CreemTransactionSub>,
    pub customer: Option<CreemTransactionCustomer>,
}

/// Paginated list of Creem transactions
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemTransactionList {
    pub data: Vec<CreemTransaction>,
    pub pagination: CreemPagination,
}

// ---------------------------------------------------------------------------
// Subscription search
// ---------------------------------------------------------------------------

/// Parameters for `GET /v1/subscriptions/search`
#[derive(Debug, Clone, Serialize)]
pub struct SearchSubscriptionsParams {
    pub page_number: i32,
    pub page_size: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_after: Option<i64>,
}

/// A single Creem subscription returned by the search endpoint
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemSubscriptionSearchResult {
    pub id: String,
    pub status: String,
    pub customer: Option<CreemCustomer>,
    pub product: Option<CreemProduct>,
    pub canceled_at: Option<String>,
    pub current_period_start_date: Option<String>,
    pub current_period_end_date: Option<String>,
    pub next_transaction_date: Option<String>,
    pub last_transaction_date: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Paginated list of Creem subscriptions
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreemSubscriptionList {
    pub data: Vec<CreemSubscriptionSearchResult>,
    pub pagination: CreemPagination,
}

/// Cancel timing for a Creem subscription.
///
/// Maps to the Creem cancel endpoint's `mode` field (`"immediate"` / `"scheduled"`).
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum CreemCancelMode {
    #[serde(rename = "immediate")]
    Immediate,
    #[serde(rename = "scheduled")]
    Scheduled,
}

/// Request body for `POST /v1/subscriptions/{id}/cancel`.
#[derive(Debug, Clone, Serialize)]
pub struct CreemCancelSubscriptionRequest {
    pub mode: CreemCancelMode,
    /// `"cancel"` or `"pause"`; only honored when `mode = scheduled`.
    /// We always request a terminal cancel (never pause).
    #[serde(rename = "onExecute", skip_serializing_if = "Option::is_none")]
    pub on_execute: Option<String>,
}

/// Response from canceling a Creem subscription. Only the fields we read back.
#[derive(Debug, Clone, Deserialize)]
pub struct CreemCancelSubscriptionResponse {
    pub id: String,
    pub status: Option<String>,
    pub canceled_at: Option<String>,
}
