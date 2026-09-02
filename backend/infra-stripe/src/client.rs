use crate::models::{
    CancelSubscriptionRequest, CancelSubscriptionResponse, CheckoutSession, CreateCheckoutRequest,
    CreatePaymentIntentRequest, ListEventsParams, PaymentIntent, StripeEventList,
};
use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::telemetry::external_http::timed_external_http_span;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct StripeClient {
    pub(crate) http: reqwest::Client,
    pub(crate) api_key: String,
    pub(crate) base_url: String,
}

impl StripeClient {
    /// Create a new Stripe API client
    ///
    /// # Arguments
    ///
    /// * `api_key` - Stripe API key (sk_test_... or sk_live_...)
    /// * `timeout_seconds` - HTTP request timeout in seconds
    ///
    /// # Note
    ///
    /// Uses Stripe API endpoint: https://api.stripe.com
    pub fn new(api_key: String, timeout_seconds: u64) -> Result<Self, CoreError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| {
                CoreError::InternalServerError(format!("Failed to create HTTP client: {e}"))
            })?;

        Ok(Self {
            http,
            api_key,
            base_url: "https://api.stripe.com".to_string(),
        })
    }

    /// Create a new Stripe API client with a custom base URL
    ///
    /// This is primarily useful for testing with mock servers.
    ///
    /// # Arguments
    ///
    /// * `api_key` - Stripe API key
    /// * `base_url` - Custom base URL for the API
    /// * `timeout_seconds` - HTTP request timeout in seconds
    pub fn with_base_url(
        api_key: String,
        base_url: String,
        timeout_seconds: u64,
    ) -> Result<Self, CoreError> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(timeout_seconds))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| {
                CoreError::InternalServerError(format!("Failed to create HTTP client: {e}"))
            })?;

        Ok(Self {
            http,
            api_key,
            base_url,
        })
    }

    /// Create a Stripe API client reusing an existing `reqwest::Client`.
    ///
    /// Avoids per-realm `reqwest::Client` reconstruction in batch jobs that
    /// iterate over many realms with different API keys but can share the
    /// underlying connection pool.
    pub fn with_http_client(http: reqwest::Client, api_key: String, base_url: String) -> Self {
        Self {
            http,
            api_key,
            base_url,
        }
    }

    /// Create a checkout session for a product
    ///
    /// # Arguments
    ///
    /// * `request` - Checkout session creation request
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The API request fails
    /// - Invalid API key
    /// - Network connectivity issues
    /// - Stripe returns an error response
    pub async fn create_checkout_session(
        &self,
        request: &CreateCheckoutRequest,
    ) -> Result<CheckoutSession, CoreError> {
        let is_payment_mode = request.mode.as_deref() == Some("payment");

        let url = format!("{}/v1/checkout/sessions", self.base_url);

        let mode_value = if is_payment_mode {
            "payment"
        } else {
            "subscription"
        };

        // Build form-encoded fields (Stripe requires application/x-www-form-urlencoded)
        let mut form_fields: Vec<(String, String)> = vec![
            ("success_url".to_string(), request.success_url.clone()),
            ("cancel_url".to_string(), request.cancel_url.clone()),
            ("mode".to_string(), mode_value.to_owned()),
            // Metadata fields
            (
                "metadata[herald_realm_id]".to_string(),
                request.realm_id.clone(),
            ),
            (
                "metadata[herald_client_app_id]".to_string(),
                request.client_app_id.to_string(),
            ),
            (
                "metadata[herald_mapping_id]".to_string(),
                request.mapping_id.to_string(),
            ),
            (
                "metadata[herald_billing_period]".to_string(),
                request.billing_period.clone(),
            ),
            (
                "metadata[herald_plan_name]".to_string(),
                request.plan_name.clone(),
            ),
        ];

        if let Some(user_id) = request.user_id {
            form_fields.push(("metadata[herald_user_id]".to_string(), user_id.to_string()));
        }

        if let Some(extra_metadata) = &request.metadata {
            for (key, value) in extra_metadata {
                form_fields.push((format!("metadata[{key}]"), value.clone()));
            }
        }

        if let Some(customer_email) = &request.customer_email {
            form_fields.push(("customer_email".to_string(), customer_email.clone()));
        }

        // Line items[0] — reference the REAL Stripe Price when available
        // (real-price semantics); otherwise fall back to rebuilding
        // an ad-hoc Price via price_data (price-less providers / no external
        // price). The price-less branch is explicit price-less-provider
        // semantics, NOT a compatibility layer.
        if let Some(pid) = request.price_id.as_deref().filter(|s| !s.is_empty()) {
            form_fields.push(("line_items[0][price]".to_string(), pid.to_string()));
            form_fields.push(("line_items[0][quantity]".to_string(), "1".to_string()));
        } else {
            // price-less fallback: rebuild an ad-hoc Price via price_data
            form_fields.push((
                "line_items[0][price_data][currency]".to_string(),
                request.currency.clone(),
            ));
            form_fields.push((
                "line_items[0][price_data][product_data][name]".to_string(),
                request.plan_name.clone(),
            ));
            form_fields.push((
                "line_items[0][price_data][product_data][metadata][herald_mapping_id]".to_string(),
                request.mapping_id.to_string(),
            ));
            form_fields.push((
                "line_items[0][price_data][unit_amount]".to_string(),
                request.price_amount.to_string(),
            ));

            // Recurring interval only for subscription mode
            if !is_payment_mode {
                let interval = if request.billing_period == "monthly" {
                    "month"
                } else {
                    "year"
                };
                form_fields.push((
                    "line_items[0][price_data][recurring][interval]".to_string(),
                    interval.to_string(),
                ));
            }

            form_fields.push(("line_items[0][quantity]".to_string(), "1".to_string()));
        }

        if is_payment_mode {
            // For one-time payments, propagate metadata to payment_intent_data
            // so the metadata is available on the PaymentIntent object
            form_fields.push((
                "payment_intent_data[metadata][herald_realm_id]".to_string(),
                request.realm_id.clone(),
            ));
            form_fields.push((
                "payment_intent_data[metadata][herald_client_app_id]".to_string(),
                request.client_app_id.to_string(),
            ));
            form_fields.push((
                "payment_intent_data[metadata][herald_mapping_id]".to_string(),
                request.mapping_id.to_string(),
            ));
            if let Some(user_id) = request.user_id {
                form_fields.push((
                    "payment_intent_data[metadata][herald_user_id]".to_string(),
                    user_id.to_string(),
                ));
            }
            if let Some(extra_metadata) = &request.metadata {
                for (key, value) in extra_metadata {
                    form_fields.push((
                        format!("payment_intent_data[metadata][{key}]"),
                        value.clone(),
                    ));
                }
            }

            // Enable Stripe invoice creation for payment-mode checkout so that
            // one-time payments produce an `in_*` invoice (and `invoice.*` webhook
            // events) instead of just a Charge. Without this, the invoices table
            // never records provider=stripe one-time payments.
            //
            // Reference: https://docs.stripe.com/api/checkout/sessions/create
            // (invoice_creation.enabled + invoice_creation.invoice_data.metadata)
            form_fields.push(("invoice_creation[enabled]".to_string(), "true".to_string()));
            form_fields.push((
                "invoice_creation[invoice_data][metadata][herald_realm_id]".to_string(),
                request.realm_id.clone(),
            ));
            form_fields.push((
                "invoice_creation[invoice_data][metadata][herald_client_app_id]".to_string(),
                request.client_app_id.to_string(),
            ));
            form_fields.push((
                "invoice_creation[invoice_data][metadata][herald_mapping_id]".to_string(),
                request.mapping_id.to_string(),
            ));
            // Stripe webhook handler (`handle_stripe_invoice_event`) reads
            // `metadata.userId` to resolve account_id; include it so the
            // resulting invoice can be linked to the purchasing user.
            if let Some(user_id) = request.user_id {
                form_fields.push((
                    "invoice_creation[invoice_data][metadata][herald_user_id]".to_string(),
                    user_id.to_string(),
                ));
                form_fields.push((
                    "invoice_creation[invoice_data][metadata][userId]".to_string(),
                    user_id.to_string(),
                ));
            }
            if let Some(extra_metadata) = &request.metadata {
                for (key, value) in extra_metadata {
                    form_fields.push((
                        format!("invoice_creation[invoice_data][metadata][{key}]"),
                        value.clone(),
                    ));
                }
            }
        } else {
            // Propagate all metadata keys to subscription_data[metadata] so that
            // when Stripe creates the subscription from the checkout session, the
            // subscription object carries the same herald_ metadata.  Without this,
            // customer.subscription.created events have empty metadata and the
            // webhook handler cannot resolve userId.
            form_fields.push((
                "subscription_data[metadata][herald_realm_id]".to_string(),
                request.realm_id.clone(),
            ));
            form_fields.push((
                "subscription_data[metadata][herald_client_app_id]".to_string(),
                request.client_app_id.to_string(),
            ));
            form_fields.push((
                "subscription_data[metadata][herald_mapping_id]".to_string(),
                request.mapping_id.to_string(),
            ));
            if let Some(user_id) = request.user_id {
                form_fields.push((
                    "subscription_data[metadata][herald_user_id]".to_string(),
                    user_id.to_string(),
                ));
            }
            if let Some(extra_metadata) = &request.metadata {
                for (key, value) in extra_metadata {
                    form_fields
                        .push((format!("subscription_data[metadata][{key}]"), value.clone()));
                }
            }

            // Add trial period if specified
            if let Some(trial_days) = request.trial_days
                && trial_days > 0
            {
                form_fields.push((
                    "subscription_data[trial_period_days]".to_string(),
                    trial_days.to_string(),
                ));
            }
        }

        tracing::info!(
            "Creating Stripe checkout session for mapping: {}",
            request.mapping_id
        );

        // external.http span + duration histogram. Host-only attribute
        // (no path/query, no api key, no body) per governance.
        let timing = timed_external_http_span(&self.base_url, "POST");
        let _span_enter = timing.span().enter();

        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .form(&form_fields)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("Stripe API error: {} - {}", status, text);
            return Err(CoreError::InternalServerError(format!(
                "Stripe API error: {} - {}",
                status.as_u16(),
                text
            )));
        }

        let stripe_response: serde_json::Value = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse Stripe response: {}", e);
            CoreError::InternalServerError(format!("Invalid Stripe response: {}", e))
        })?;

        Ok(CheckoutSession {
            id: stripe_response["id"]
                .as_str()
                .ok_or_else(|| {
                    CoreError::InternalServerError(
                        "Missing 'id' in Stripe checkout session response".to_string(),
                    )
                })?
                .to_string(),
            url: stripe_response["url"]
                .as_str()
                .ok_or_else(|| {
                    CoreError::InternalServerError(
                        "Missing 'url' in Stripe checkout session response".to_string(),
                    )
                })?
                .to_string(),
            customer: stripe_response["customer"].as_str().map(|s| s.to_string()),
            status: stripe_response["status"].as_str().map(|s| s.to_string()),
            payment_intent: stripe_response["payment_intent"]
                .as_str()
                .map(|s| s.to_string()),
            subscription: stripe_response["subscription"]
                .as_str()
                .map(|s| s.to_string()),
            metadata: serde_json::from_value(stripe_response["metadata"].clone())
                .unwrap_or_default(),
        })
    }

    /// Create a payment intent for one-off payments such as points package purchases.
    pub async fn create_payment_intent(
        &self,
        request: &CreatePaymentIntentRequest,
    ) -> Result<PaymentIntent, CoreError> {
        if request.amount <= 0 {
            return Err(CoreError::BadRequest(
                "Payment intent amount must be greater than 0".to_string(),
            ));
        }

        if request.currency.len() != 3 {
            return Err(CoreError::BadRequest(
                "Payment intent currency must be a 3-letter ISO code".to_string(),
            ));
        }

        let url = format!("{}/v1/payment_intents", self.base_url);

        let mut form_fields = vec![
            ("amount".to_string(), request.amount.to_string()),
            ("currency".to_string(), request.currency.clone()),
            (
                "automatic_payment_methods[enabled]".to_string(),
                "true".to_string(),
            ),
        ];
        if let Some(receipt_email) = &request.receipt_email {
            form_fields.push(("receipt_email".to_string(), receipt_email.clone()));
        }
        form_fields.extend(
            request
                .metadata
                .iter()
                .map(|(key, value)| (format!("metadata[{key}]"), value.clone())),
        );

        // external.http span + duration histogram.
        let timing = timed_external_http_span(&self.base_url, "POST");
        let _span_enter = timing.span().enter();

        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .form(&form_fields)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("Stripe payment intent API error: {} - {}", status, text);
            return Err(CoreError::InternalServerError(format!(
                "Stripe payment intent API error: {} - {}",
                status.as_u16(),
                text
            )));
        }

        let stripe_response: serde_json::Value = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse Stripe payment intent response: {}", e);
            CoreError::InternalServerError(format!("Invalid Stripe response: {}", e))
        })?;

        Ok(PaymentIntent {
            id: stripe_response["id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            client_secret: stripe_response["client_secret"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            amount: stripe_response["amount"].as_i64().unwrap_or_default(),
            currency: stripe_response["currency"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            status: stripe_response["status"].as_str().map(str::to_string),
            metadata: stripe_response["metadata"].clone(),
        })
    }

    /// Cancel a Stripe subscription.
    ///
    /// - `cancel_at_period_end = false` → `DELETE /v1/subscriptions/{id}` (immediate cancel).
    /// - `cancel_at_period_end = true`  → `POST /v1/subscriptions/{id}` with form
    ///   `cancel_at_period_end=true` (stays active until period end).
    ///
    /// Only the provider side is touched; local subscription state is expected to be
    /// updated by the subsequent Stripe webhook (`customer.subscription.deleted`).
    pub async fn cancel_subscription(
        &self,
        request: &CancelSubscriptionRequest,
    ) -> Result<CancelSubscriptionResponse, CoreError> {
        if request.subscription_id.is_empty() {
            return Err(CoreError::BadRequest(
                "Stripe subscription id is required".to_string(),
            ));
        }

        let url = format!(
            "{}/v1/subscriptions/{}",
            self.base_url, request.subscription_id
        );

        // external.http span + duration histogram. Verb differs by mode but the
        // span label only carries the host; both branches share one timing site.
        let timing = timed_external_http_span(&self.base_url, "POST");
        let _span_enter = timing.span().enter();

        let response = if request.cancel_at_period_end {
            self.http
                .post(&url)
                .bearer_auth(&self.api_key)
                .form(&[("cancel_at_period_end".to_string(), "true".to_string())])
                .send()
                .await?
        } else {
            self.http
                .delete(&url)
                .bearer_auth(&self.api_key)
                .send()
                .await?
        };

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            tracing::error!(
                "Stripe cancel subscription API error: {} - {}",
                status,
                text
            );
            return Err(CoreError::InternalServerError(format!(
                "Stripe cancel subscription API error: {} - {}",
                status.as_u16(),
                text
            )));
        }

        let stripe_response: serde_json::Value = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse Stripe cancel response: {}", e);
            CoreError::InternalServerError(format!("Invalid Stripe response: {}", e))
        })?;

        Ok(CancelSubscriptionResponse {
            id: stripe_response["id"]
                .as_str()
                .unwrap_or(&request.subscription_id)
                .to_string(),
            status: stripe_response["status"].as_str().map(str::to_string),
            cancel_at_period_end: stripe_response["cancel_at_period_end"].as_bool(),
            canceled_at: stripe_response["canceled_at"].as_i64(),
        })
    }

    /// List Stripe events via GET /v1/events
    ///
    /// Used for webhook compensation — polling Stripe for events that may have been
    /// missed by webhook delivery.
    ///
    /// # Arguments
    ///
    /// * `params` - Query parameters (time range, event types, pagination)
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or returns a non-success status.
    pub async fn list_events(
        &self,
        params: &ListEventsParams,
    ) -> Result<StripeEventList, CoreError> {
        let url = format!("{}/v1/events", self.base_url);

        // Stripe's /v1/events endpoint accepts an array of event-type filters
        // under the `types` query parameter (repeated `types[]=...` keys),
        // capped at 20 values per request. The previous code used `type[]`,
        // which Stripe parses as the scalar `type` parameter; when multiple
        // repeated `type[]` keys arrived, Stripe aggregated them into a single
        // JSON-array value and rejected the request with
        // `400 Invalid string: [...] param=type`, causing the webhook
        // compensation job to fetch zero events.
        //
        // Use the documented `types[]` array parameter, and cap at 20 to stay
        // within the API limit when callers (e.g. the worker) supply more.
        const STRIPE_TYPES_MAX: usize = 20;
        let types_to_send = &params.event_types[..params.event_types.len().min(STRIPE_TYPES_MAX)];
        if params.event_types.len() > STRIPE_TYPES_MAX {
            tracing::warn!(
                requested = params.event_types.len(),
                sent = STRIPE_TYPES_MAX,
                "Stripe /v1/events accepts at most {} event types per request; \
                 extra types dropped — callers should chunk requests to cover them",
                STRIPE_TYPES_MAX
            );
        }

        let mut query: Vec<(&str, String)> = vec![
            ("created[gte]", params.created_gte.to_string()),
            ("created[lte]", params.created_lte.to_string()),
            ("limit", params.limit.to_string()),
        ];
        for et in types_to_send {
            query.push(("types[]", et.clone()));
        }
        if let Some(sa) = &params.starting_after {
            query.push(("starting_after", sa.clone()));
        }

        tracing::info!(
            "Listing Stripe events: {} types, limit {}, range {}-{}",
            types_to_send.len(),
            params.limit,
            params.created_gte,
            params.created_lte
        );

        // external.http span + duration histogram.
        let timing = timed_external_http_span(&self.base_url, "GET");
        let _span_enter = timing.span().enter();

        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .query(&query)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            tracing::error!("Stripe list events API error: {} - {}", status, text);
            return Err(CoreError::InternalServerError(format!(
                "Stripe list events API error: {} - {}",
                status.as_u16(),
                text
            )));
        }

        let event_list: StripeEventList = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse Stripe events response: {}", e);
            CoreError::InternalServerError(format!("Invalid Stripe events response: {}", e))
        })?;

        Ok(event_list)
    }

    /// Verify a Stripe webhook signature
    ///
    /// # Arguments
    ///
    /// * `payload` - Raw webhook payload bytes
    /// * `signature` - Stripe-Signature header value
    /// * `secret` - Webhook signing secret
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Signature is invalid
    /// - Timestamp is too old (replay attack protection)
    /// - Secret is invalid
    ///
    /// # Implementation Details
    ///
    /// Stripe uses HMAC-SHA256 to sign webhook payloads.
    /// The signature is sent in the `stripe-signature` header with format: "t=...,v1=..."
    ///
    /// # Note
    ///
    /// This is a static method (doesn't require &self) because webhook signature verification
    /// doesn't need any client state (api_key, http, etc.). This allows per-realm webhook
    /// verification without creating a StripeClient instance.
    pub fn verify_webhook_signature(
        payload: &[u8],
        signature: &str,
        secret: &str,
    ) -> Result<(), CoreError> {
        // Parse signature header
        let signature_elements: Vec<&str> = signature.split(',').collect();
        let mut timestamp = None;
        let mut expected_signature = None;

        for element in signature_elements {
            let parts: Vec<&str> = element.split('=').collect();
            if parts.len() != 2 {
                continue;
            }
            match parts[0] {
                "t" => timestamp = Some(parts[1]),
                "v1" => expected_signature = Some(parts[1]),
                _ => {}
            }
        }

        let timestamp = timestamp.ok_or_else(|| {
            CoreError::BadRequest("Missing timestamp in webhook signature".to_string())
        })?;

        let expected_signature = expected_signature.ok_or_else(|| {
            CoreError::BadRequest("Missing signature in webhook signature".to_string())
        })?;

        // Check timestamp age (replay attack protection - 15 minutes)
        let timestamp_i64: i64 = timestamp.parse().map_err(|_| {
            CoreError::BadRequest("Invalid timestamp in webhook signature".to_string())
        })?;

        let now = chrono::Utc::now().timestamp();
        let age_seconds = now - timestamp_i64;

        if age_seconds > 900 {
            // 15 minutes = 900 seconds
            return Err(CoreError::BadRequest(format!(
                "Webhook timestamp is too old: {} seconds",
                age_seconds
            )));
        }

        if age_seconds < -900 {
            return Err(CoreError::BadRequest(
                "Webhook timestamp is in the future".to_string(),
            ));
        }

        // Build signed payload
        let signed_payload = format!("{}.{}", timestamp, String::from_utf8_lossy(payload));

        // Compute HMAC-SHA256
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|_| CoreError::InternalServerError("Invalid webhook secret".to_string()))?;

        mac.update(signed_payload.as_bytes());
        let computed_signature = hex::encode(mac.finalize().into_bytes());

        // Compare signatures in constant time — a byte-wise `==` on the hex
        // strings would short-circuit and leak the matching prefix length.
        if constant_time_eq(computed_signature.as_bytes(), expected_signature.as_bytes()) {
            Ok(())
        } else {
            Err(CoreError::BadRequest(
                "Invalid webhook signature".to_string(),
            ))
        }
    }
}

/// Constant-time byte-slice comparison: XOR-accumulates instead of
/// short-circuiting, so timing does not leak the first mismatch position.
/// Only the length mismatch returns early, which reveals no secret material.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (byte_a, byte_b) in a.iter().zip(b.iter()) {
        result |= byte_a ^ byte_b;
    }
    result == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_verify_webhook_signature_valid() {
        let payload = b"test_payload";
        let secret = "whsec_test_secret";

        // Create a valid signature
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let signed_payload = format!("{}.{}", timestamp, String::from_utf8_lossy(payload));

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(signed_payload.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());

        let signature_header = format!("t={},v1={}", timestamp, signature);

        let result = StripeClient::verify_webhook_signature(payload, &signature_header, secret);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_webhook_signature_invalid() {
        let payload = b"test_payload";
        let secret = "whsec_test_secret";

        let timestamp = chrono::Utc::now().timestamp().to_string();
        let signature_header = format!("t={},v1=invalid_signature", timestamp);

        let result = StripeClient::verify_webhook_signature(payload, &signature_header, secret);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_webhook_signature_old_timestamp() {
        let payload = b"test_payload";
        let secret = "whsec_test_secret";

        // Use a timestamp from 20 minutes ago
        let old_timestamp = (chrono::Utc::now().timestamp() - 1200).to_string();
        let signature_header = format!("t={},v1=some_signature", old_timestamp);

        let result = StripeClient::verify_webhook_signature(payload, &signature_header, secret);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_payment_intent_sends_form_encoded_metadata() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "pi_test_123",
                "client_secret": "pi_test_123_secret_abc",
                "amount": 1299,
                "currency": "usd",
                "status": "requires_payment_method",
                "metadata": {
                    "attemptId": "attempt-123",
                    "targetType": "points_package"
                }
            })))
            .mount(&mock_server)
            .await;

        let result = client
            .create_payment_intent(&CreatePaymentIntentRequest {
                amount: 1299,
                currency: "usd".to_string(),
                receipt_email: Some("buyer@example.com".to_string()),
                metadata: std::collections::HashMap::from([
                    ("attemptId".to_string(), "attempt-123".to_string()),
                    ("targetType".to_string(), "points_package".to_string()),
                ]),
            })
            .await
            .expect("payment intent should be created");

        assert_eq!(result.id, "pi_test_123");
        assert_eq!(result.client_secret, "pi_test_123_secret_abc");
        assert_eq!(result.amount, 1299);
        assert_eq!(result.currency, "usd");
        assert_eq!(result.metadata["attemptId"], "attempt-123");

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording should be enabled");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method.as_str(), "POST");
        assert_eq!(requests[0].url.path(), "/v1/payment_intents");
        let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(&requests[0].body)
            .into_owned()
            .collect();

        assert_eq!(
            requests[0]
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk_test_123")
        );
        assert_eq!(form.get("amount"), Some(&"1299".to_string()));
        assert_eq!(form.get("currency"), Some(&"usd".to_string()));
        assert_eq!(
            form.get("receipt_email"),
            Some(&"buyer@example.com".to_string())
        );
        assert_eq!(
            form.get("automatic_payment_methods[enabled]"),
            Some(&"true".to_string())
        );
        assert_eq!(
            form.get("metadata[attemptId]"),
            Some(&"attempt-123".to_string())
        );
        assert_eq!(
            form.get("metadata[targetType]"),
            Some(&"points_package".to_string())
        );
    }

    /// Verifies that create_checkout_session sends form-encoded data (not JSON),
    /// matching Stripe's requirement for application/x-www-form-urlencoded.
    #[tokio::test]
    async fn test_create_checkout_session_sends_form_encoded() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "cs_test_123",
                "url": "https://checkout.stripe.com/test",
                "customer": null,
                "status": "open",
                "payment_intent": "pi_test_123",
                "subscription": null,
                "metadata": { "realm_id": "realm-1", "source": "web" }
            })))
            .mount(&mock_server)
            .await;

        let mapping_id = uuid::Uuid::now_v7();
        let result = client
            .create_checkout_session(&CreateCheckoutRequest {
                client_app_id: uuid::Uuid::now_v7(),
                mapping_id,
                user_id: Some(uuid::Uuid::now_v7()),
                customer_email: Some("buyer@example.com".to_string()),
                success_url: "https://example.com/success".to_string(),
                cancel_url: "https://example.com/cancel".to_string(),
                billing_period: "monthly".to_string(),
                trial_days: Some(14),
                price_amount: 1999,
                currency: "usd".to_string(),
                plan_name: "Pro Plan".to_string(),
                price_id: None, // price-less fallback -> price_data
                realm_id: "realm-1".to_string(),
                webhook_url: None,
                metadata: Some(std::collections::HashMap::from([(
                    "source".to_string(),
                    "web".to_string(),
                )])),
                mode: None, // subscription mode (default)
            })
            .await
            .expect("checkout session should be created");

        assert_eq!(result.id, "cs_test_123");
        assert_eq!(result.url, "https://checkout.stripe.com/test");
        assert_eq!(result.status.as_deref(), Some("open"));

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording should be enabled");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method.as_str(), "POST");
        assert_eq!(requests[0].url.path(), "/v1/checkout/sessions");

        // Parse form body to verify form-encoding (not JSON)
        let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(&requests[0].body)
            .into_owned()
            .collect();

        assert_eq!(
            requests[0]
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk_test_123")
        );
        // Verify key fields are form-encoded
        assert_eq!(form.get("mode"), Some(&"subscription".to_string()));
        assert_eq!(
            form.get("success_url"),
            Some(&"https://example.com/success".to_string())
        );
        assert_eq!(
            form.get("cancel_url"),
            Some(&"https://example.com/cancel".to_string())
        );
        assert_eq!(
            form.get("customer_email"),
            Some(&"buyer@example.com".to_string())
        );
        // Metadata fields
        assert_eq!(
            form.get("metadata[herald_realm_id]"),
            Some(&"realm-1".to_string())
        );
        assert_eq!(
            form.get("metadata[herald_plan_name]"),
            Some(&"Pro Plan".to_string())
        );
        assert_eq!(
            form.get("metadata[herald_mapping_id]"),
            Some(&mapping_id.to_string())
        );
        assert_eq!(form.get("metadata[source]"), Some(&"web".to_string()));
        assert_eq!(
            form.get("line_items[0][price_data][product_data][metadata][herald_mapping_id]"),
            Some(&mapping_id.to_string())
        );
        // Line items
        assert_eq!(
            form.get("line_items[0][price_data][currency]"),
            Some(&"usd".to_string())
        );
        assert_eq!(
            form.get("line_items[0][price_data][unit_amount]"),
            Some(&"1999".to_string())
        );
        assert_eq!(
            form.get("line_items[0][price_data][recurring][interval]"),
            Some(&"month".to_string())
        );
        assert_eq!(form.get("line_items[0][quantity]"), Some(&"1".to_string()));
        // Trial period
        assert_eq!(
            form.get("subscription_data[trial_period_days]"),
            Some(&"14".to_string())
        );
    }

    #[tokio::test]
    async fn test_create_payment_intent_rejects_invalid_amount_before_request() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        let result = client
            .create_payment_intent(&CreatePaymentIntentRequest {
                amount: 0,
                currency: "usd".to_string(),
                receipt_email: None,
                metadata: std::collections::HashMap::new(),
            })
            .await;

        assert!(matches!(result, Err(CoreError::BadRequest(_))));
    }

    /// Immediate cancel issues `DELETE /v1/subscriptions/{id}`.
    #[tokio::test]
    async fn test_cancel_subscription_immediate_uses_delete() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "sub_test_123",
                "status": "canceled",
                "cancel_at_period_end": false,
                "canceled_at": 1700000000
            })))
            .mount(&mock_server)
            .await;

        let result = client
            .cancel_subscription(&CancelSubscriptionRequest {
                subscription_id: "sub_test_123".to_string(),
                cancel_at_period_end: false,
            })
            .await
            .expect("immediate cancel should succeed");

        assert_eq!(result.id, "sub_test_123");
        assert_eq!(result.status.as_deref(), Some("canceled"));
        assert_eq!(result.cancel_at_period_end, Some(false));
        assert_eq!(result.canceled_at, Some(1700000000));

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording should be enabled");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method.as_str(), "DELETE");
        assert_eq!(requests[0].url.path(), "/v1/subscriptions/sub_test_123");
        assert_eq!(
            requests[0]
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk_test_123")
        );
    }

    /// Scheduled cancel issues `POST /v1/subscriptions/{id}` with form
    /// `cancel_at_period_end=true`.
    #[tokio::test]
    async fn test_cancel_subscription_at_period_end_uses_post_with_form() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "sub_test_456",
                "status": "active",
                "cancel_at_period_end": true,
                "canceled_at": null
            })))
            .mount(&mock_server)
            .await;

        let result = client
            .cancel_subscription(&CancelSubscriptionRequest {
                subscription_id: "sub_test_456".to_string(),
                cancel_at_period_end: true,
            })
            .await
            .expect("scheduled cancel should succeed");

        assert_eq!(result.id, "sub_test_456");
        assert_eq!(result.status.as_deref(), Some("active"));
        assert_eq!(result.cancel_at_period_end, Some(true));
        assert!(result.canceled_at.is_none());

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording should be enabled");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method.as_str(), "POST");
        assert_eq!(requests[0].url.path(), "/v1/subscriptions/sub_test_456");
        let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(&requests[0].body)
            .into_owned()
            .collect();
        assert_eq!(form.get("cancel_at_period_end"), Some(&"true".to_string()));
    }

    /// Provider error (non-2xx) is surfaced as InternalServerError and not swallowed.
    #[tokio::test]
    async fn test_cancel_subscription_surfaces_provider_error() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(402).set_body_string("no such subscription"))
            .mount(&mock_server)
            .await;

        let result = client
            .cancel_subscription(&CancelSubscriptionRequest {
                subscription_id: "sub_missing".to_string(),
                cancel_at_period_end: false,
            })
            .await;

        assert!(
            matches!(result, Err(CoreError::InternalServerError(_))),
            "provider error must surface, got {:?}",
            result
        );
    }

    /// Verify that payment mode sends payment_intent_data[metadata] and skips
    /// recurring interval and subscription_data.
    #[tokio::test]
    async fn test_create_checkout_session_payment_mode_uses_payment_intent_data() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "cs_test_payment",
                "url": "https://checkout.stripe.com/payment",
                "customer": null,
                "status": "open",
                "payment_intent": "pi_test_payment",
                "subscription": null,
                "metadata": { "realm_id": "realm-2", "source": "one-time" }
            })))
            .mount(&mock_server)
            .await;

        let mapping_id = uuid::Uuid::now_v7();
        let user_id = uuid::Uuid::now_v7();
        let result = client
            .create_checkout_session(&CreateCheckoutRequest {
                client_app_id: uuid::Uuid::now_v7(),
                mapping_id,
                user_id: Some(user_id),
                customer_email: Some("buyer@example.com".to_string()),
                success_url: "https://example.com/success".to_string(),
                cancel_url: "https://example.com/cancel".to_string(),
                billing_period: "monthly".to_string(), // irrelevant for payment mode
                trial_days: None,
                price_amount: 500,
                currency: "usd".to_string(),
                plan_name: "Points Pack 100".to_string(),
                price_id: None, // price-less fallback -> price_data
                realm_id: "realm-2".to_string(),
                webhook_url: None,
                metadata: Some(std::collections::HashMap::from([(
                    "source".to_string(),
                    "one-time".to_string(),
                )])),
                mode: Some("payment".to_string()),
            })
            .await
            .expect("checkout session should be created in payment mode");

        assert_eq!(result.id, "cs_test_payment");

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording should be enabled");
        assert_eq!(requests.len(), 1);

        let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(&requests[0].body)
            .into_owned()
            .collect();

        // Mode should be "payment" (not "subscription")
        assert_eq!(form.get("mode"), Some(&"payment".to_string()));

        // Should NOT have recurring interval
        assert!(
            !form.contains_key("line_items[0][price_data][recurring][interval]"),
            "payment mode should not include recurring interval"
        );

        // Should NOT have subscription_data
        assert!(
            form.keys().all(|k| !k.starts_with("subscription_data[")),
            "payment mode should not include subscription_data fields"
        );

        // Should have payment_intent_data[metadata] with herald_ keys
        assert_eq!(
            form.get("payment_intent_data[metadata][herald_realm_id]"),
            Some(&"realm-2".to_string())
        );
        assert_eq!(
            form.get("payment_intent_data[metadata][herald_mapping_id]"),
            Some(&mapping_id.to_string())
        );
        assert_eq!(
            form.get("payment_intent_data[metadata][herald_user_id]"),
            Some(&user_id.to_string())
        );
        assert_eq!(
            form.get("payment_intent_data[metadata][source]"),
            Some(&"one-time".to_string())
        );

        // Should still have line item price data (without recurring)
        assert_eq!(
            form.get("line_items[0][price_data][currency]"),
            Some(&"usd".to_string())
        );
        assert_eq!(
            form.get("line_items[0][price_data][unit_amount]"),
            Some(&"500".to_string())
        );

        // Should enable invoice creation for one-time payments
        assert_eq!(
            form.get("invoice_creation[enabled]"),
            Some(&"true".to_string()),
            "payment mode should enable invoice_creation"
        );
        assert_eq!(
            form.get("invoice_creation[invoice_data][metadata][herald_realm_id]"),
            Some(&"realm-2".to_string()),
            "invoice_creation metadata should include herald_realm_id"
        );
        assert_eq!(
            form.get("invoice_creation[invoice_data][metadata][herald_mapping_id]"),
            Some(&mapping_id.to_string()),
            "invoice_creation metadata should include herald_mapping_id"
        );
        assert_eq!(
            form.get("invoice_creation[invoice_data][metadata][herald_user_id]"),
            Some(&user_id.to_string()),
            "invoice_creation metadata should include herald_user_id"
        );
        assert_eq!(
            form.get("invoice_creation[invoice_data][metadata][userId]"),
            Some(&user_id.to_string()),
            "invoice_creation metadata should include userId for webhook handler"
        );
        assert_eq!(
            form.get("invoice_creation[invoice_data][metadata][source]"),
            Some(&"one-time".to_string()),
            "invoice_creation metadata should include extra metadata"
        );
    }

    /// Verify that subscription mode (default/None) still includes
    /// subscription_data and recurring interval.
    #[tokio::test]
    async fn test_create_checkout_session_subscription_mode_includes_subscription_data() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "cs_test_sub",
                "url": "https://checkout.stripe.com/sub",
                "customer": null,
                "status": "open",
                "payment_intent": null,
                "subscription": "sub_test_sub",
                "metadata": {}
            })))
            .mount(&mock_server)
            .await;

        let mapping_id = uuid::Uuid::now_v7();
        let user_id = uuid::Uuid::now_v7();
        let result = client
            .create_checkout_session(&CreateCheckoutRequest {
                client_app_id: uuid::Uuid::now_v7(),
                mapping_id,
                user_id: Some(user_id),
                customer_email: Some("buyer@example.com".to_string()),
                success_url: "https://example.com/success".to_string(),
                cancel_url: "https://example.com/cancel".to_string(),
                billing_period: "yearly".to_string(),
                trial_days: None,
                price_amount: 9999,
                currency: "usd".to_string(),
                plan_name: "Annual Plan".to_string(),
                price_id: None, // price-less fallback -> price_data
                realm_id: "realm-3".to_string(),
                webhook_url: None,
                metadata: None,
                mode: None, // subscription mode (default)
            })
            .await
            .expect("checkout session should be created");

        assert_eq!(result.id, "cs_test_sub");

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording should be enabled");
        assert_eq!(requests.len(), 1);

        let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(&requests[0].body)
            .into_owned()
            .collect();

        // Mode should be "subscription"
        assert_eq!(form.get("mode"), Some(&"subscription".to_string()));

        // Should have recurring interval
        assert_eq!(
            form.get("line_items[0][price_data][recurring][interval]"),
            Some(&"year".to_string())
        );

        // Should have subscription_data[metadata]
        assert_eq!(
            form.get("subscription_data[metadata][herald_realm_id]"),
            Some(&"realm-3".to_string())
        );
        assert_eq!(
            form.get("subscription_data[metadata][herald_mapping_id]"),
            Some(&mapping_id.to_string())
        );
        assert_eq!(
            form.get("subscription_data[metadata][herald_user_id]"),
            Some(&user_id.to_string())
        );

        // Should NOT have payment_intent_data
        assert!(
            form.keys().all(|k| !k.starts_with("payment_intent_data[")),
            "subscription mode should not include payment_intent_data fields"
        );

        // Should NOT have invoice_creation (only for payment mode)
        assert!(
            !form.contains_key("invoice_creation[enabled]"),
            "subscription mode should not include invoice_creation fields"
        );
    }

    // --- price_id branch unit tests ---
    // Real-price semantics: `CreateCheckoutRequest.price_id` selects
    // between two mutually exclusive line-item shapes:
    //   - Some(non-empty) -> reference the real Stripe Price
    //     (`line_items[0][price]` + `quantity`); MUST NOT emit `price_data`.
    //   - None            -> price-less fallback, rebuild an ad-hoc Price via
    //     `line_items[0][price_data]`; this is explicit price-less-provider
    //     semantics (NOT a compatibility layer); MUST NOT emit `line_items[0][price]`.
    // These two tests encode WHY the distinction matters: referencing the real
    // Price keeps Stripe-side analytics, coupons, and webhook `price` fields
    // consistent with the catalog, while the None branch preserves price-less
    // provider support. Each test fails if the branch is removed or inverted.

    /// User Story: US-EM-009 — as a billing operator I need checkout to
    /// reference the real Stripe Price (configured per entitlement mapping)
    /// when one exists, so that Stripe-side analytics, coupons, and webhook
    /// `price` fields stay consistent with the catalog.
    /// Covers: price_id=Some -> `line_items[0][price]` + `quantity`; no
    /// `price_data` fields leak onto the wire (mutual exclusivity of the two
    /// branches). Fails if the Some-branch is removed or if price_data is
    /// accidentally emitted alongside a real Price reference.
    #[tokio::test]
    async fn create_checkout_session_with_price_id_references_real_price() {
        let mock_server = MockServer::start().await;

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "cs_test_price",
                "url": "https://checkout.stripe.com/price",
                "customer": null,
                "status": "open",
                "payment_intent": null,
                "subscription": "sub_test_price",
                "metadata": {}
            })))
            .mount(&mock_server)
            .await;

        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        let request = CreateCheckoutRequest {
            client_app_id: uuid::Uuid::now_v7(),
            mapping_id: uuid::Uuid::now_v7(),
            user_id: Some(uuid::Uuid::now_v7()),
            customer_email: Some("buyer@example.com".to_string()),
            success_url: "https://example.com/success".to_string(),
            cancel_url: "https://example.com/cancel".to_string(),
            billing_period: "monthly".to_string(),
            trial_days: None,
            price_amount: 1999,
            currency: "usd".to_string(),
            plan_name: "Pro Plan".to_string(),
            price_id: Some("price_real_abc".to_string()), // real Price reference
            realm_id: "realm-x".to_string(),
            webhook_url: None,
            metadata: None,
            mode: None, // subscription mode
        };

        client
            .create_checkout_session(&request)
            .await
            .expect("checkout with real price_id should succeed");

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording should be enabled");
        assert_eq!(requests.len(), 1, "exactly one checkout call");
        let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(&requests[0].body)
            .into_owned()
            .collect();

        // Real Price is referenced verbatim...
        assert_eq!(
            form.get("line_items[0][price]"),
            Some(&"price_real_abc".to_string()),
            "Some(price_id) must populate line_items[0][price] with the real Price ID"
        );
        assert_eq!(
            form.get("line_items[0][quantity]"),
            Some(&"1".to_string()),
            "Some(price_id) must set line_items[0][quantity]=1"
        );
        // ...and NO price_data fields may leak onto the wire. Mutual
        // exclusivity is the load-bearing assertion: emitting price_data
        // alongside a real Price reference would let Stripe silently override
        // the catalog Price, breaking analytics/webhook consistency.
        assert!(
            !form
                .keys()
                .any(|k| k.starts_with("line_items[0][price_data]")),
            "Some(price_id) must NOT emit any price_data fields; got: {:?}",
            form.keys()
                .filter(|k| k.starts_with("line_items[0][price_data]"))
                .collect::<Vec<_>>()
        );
    }

    /// User Story: A2 price-less provider — as a billing operator I need
    /// checkout to keep working for price-less providers (no external Stripe
    /// Price) by rebuilding an ad-hoc Price via `price_data`, so that
    /// amount-based checkout remains a first-class path (NOT a compatibility
    /// layer that could be deleted).
    /// Covers: price_id=None -> `line_items[0][price_data]` (currency /
    /// product_data.name / unit_amount) is present and `line_items[0][price]`
    /// is absent. Fails if the None-branch price_data fallback is removed or
    /// if a stray `line_items[0][price]` is emitted.
    #[tokio::test]
    async fn create_checkout_session_without_price_id_falls_back_to_price_data() {
        let mock_server = MockServer::start().await;

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "cs_test_priceless",
                "url": "https://checkout.stripe.com/priceless",
                "customer": null,
                "status": "open",
                "payment_intent": null,
                "subscription": "sub_test_priceless",
                "metadata": {}
            })))
            .mount(&mock_server)
            .await;

        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        let request = CreateCheckoutRequest {
            client_app_id: uuid::Uuid::now_v7(),
            mapping_id: uuid::Uuid::now_v7(),
            user_id: Some(uuid::Uuid::now_v7()),
            customer_email: Some("buyer@example.com".to_string()),
            success_url: "https://example.com/success".to_string(),
            cancel_url: "https://example.com/cancel".to_string(),
            billing_period: "monthly".to_string(),
            trial_days: None,
            price_amount: 1999,
            currency: "usd".to_string(),
            plan_name: "Pro Plan".to_string(),
            price_id: None, // price-less fallback -> price_data
            realm_id: "realm-x".to_string(),
            webhook_url: None,
            metadata: None,
            mode: None, // subscription mode
        };

        client
            .create_checkout_session(&request)
            .await
            .expect("checkout with price_id=None should succeed");

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording should be enabled");
        assert_eq!(requests.len(), 1, "exactly one checkout call");
        let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(&requests[0].body)
            .into_owned()
            .collect();

        // price_data fallback is present (amount-based price-less checkout).
        assert_eq!(
            form.get("line_items[0][price_data][currency]"),
            Some(&"usd".to_string()),
            "None price_id must fall back to price_data[currency]"
        );
        assert_eq!(
            form.get("line_items[0][price_data][product_data][name]"),
            Some(&"Pro Plan".to_string()),
            "None price_id must fall back to price_data[product_data][name]"
        );
        assert_eq!(
            form.get("line_items[0][price_data][unit_amount]"),
            Some(&"1999".to_string()),
            "None price_id must fall back to price_data[unit_amount]"
        );
        // ...and line_items[0][price] must NOT be set. Emitting a real-Price
        // reference when none was configured would point checkout at an
        // undefined Stripe object and break price-less providers.
        assert!(
            !form.contains_key("line_items[0][price]"),
            "None price_id must NOT emit line_items[0][price]; got: {:?}",
            form.get("line_items[0][price]")
        );
    }

    // --- list_events wiremock tests ---
    // User Story: As a billing operator I need StripeClient to faithfully query
    // the Stripe Events API so that webhook compensation can recover missed events.
    // Covers: query parameter encoding, Bearer auth, single-page parsing,
    // has_more passthrough, and empty-response handling.

    /// Verifies that `list_events` sends the correct GET request with query
    /// parameters (`created[gte]`, `created[lte]`, `types[]`, `limit`) and
    /// `Bearer` auth to `/v1/events`.
    ///
    /// Regression guard: an earlier version sent each event type under the
    /// unsupported `type[]` key, which Stripe parses as the scalar `type`
    /// parameter and rejects with `400 Invalid string: [...] param=type` when
    /// more than one value is supplied. This test asserts the documented
    /// `types[]` array parameter is used, that each event type becomes its own
    /// repeated query key (never a single JSON-array-encoded value), and that
    /// no scalar `type` key leaks onto the wire.
    #[tokio::test]
    async fn test_list_events_sends_correct_query_params() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        let event_types = vec![
            "checkout.session.completed".to_string(),
            "customer.subscription.*".to_string(),
        ];

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [],
                "has_more": false
            })))
            .mount(&mock_server)
            .await;

        let result = client
            .list_events(&ListEventsParams {
                created_gte: 1_700_000_000,
                created_lte: 1_700_001_000,
                event_types: event_types.clone(),
                limit: 100,
                starting_after: None,
            })
            .await
            .expect("list_events should succeed");

        assert!(result.data.is_empty());
        assert!(!result.has_more);

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording should be enabled");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method.as_str(), "GET");
        assert_eq!(requests[0].url.path(), "/v1/events");

        // Verify Bearer auth
        assert_eq!(
            requests[0]
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk_test_123")
        );

        // The raw query string must contain one `types[]=<value>` occurrence
        // per event type as genuinely separate keys. If reqwest ever
        // regressed to collapsing repeated keys into a single JSON-array
        // literal (the failure mode observed in production), this raw-string
        // assertion would catch it — `query_pairs()` would otherwise paper
        // over the difference.
        let raw_query = requests[0].url.query().unwrap_or("");
        assert!(
            !raw_query.contains("%5B%22") && !raw_query.contains("%5D%22"),
            "raw query must not embed event types as a JSON-array string literal: {}",
            raw_query
        );
        for et in &event_types {
            // Each event type must appear as its own `types[]=<value>` key.
            // For plain alphanumeric+dot event-type strings reqwest emits them
            // verbatim (no percent-encoding), so we look for the literal
            // `types%5B%5D=<value>` segment in the raw query.
            let expected = format!("types%5B%5D={et}");
            assert!(
                raw_query.contains(&expected),
                "raw query should contain a separate `types[]={}` key; got: {}",
                et,
                raw_query
            );
        }
        assert!(
            !raw_query.contains("type%5B%5D=")
                && !raw_query.contains("&type=")
                && !raw_query.starts_with("type="),
            "legacy scalar `type` / unsupported `type[]` keys must not appear; got: {}",
            raw_query
        );

        // Parse query string from the URL
        let query_pairs: std::collections::HashMap<String, String> = requests[0]
            .url
            .query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        assert_eq!(
            query_pairs.get("created[gte]"),
            Some(&"1700000000".to_string()),
            "created[gte] query param"
        );
        assert_eq!(
            query_pairs.get("created[lte]"),
            Some(&"1700001000".to_string()),
            "created[lte] query param"
        );
        assert_eq!(
            query_pairs.get("limit"),
            Some(&"100".to_string()),
            "limit query param"
        );

        // types[] is repeated — collect all values
        let type_values: Vec<String> = requests[0]
            .url
            .query_pairs()
            .filter(|(k, _)| k == "types[]")
            .map(|(_, v)| v.to_string())
            .collect();
        assert_eq!(
            type_values.len(),
            event_types.len(),
            "should have one types[] query param per event type"
        );
        for et in &event_types {
            assert!(type_values.contains(et), "types[] should contain {}", et);
        }
        assert!(
            !query_pairs.contains_key("type") && !query_pairs.contains_key("type[]"),
            "scalar `type` / unsupported `type[]` keys must not be present"
        );

        // No starting_after since starting_after was None
        assert!(
            !query_pairs.contains_key("starting_after"),
            "starting_after should not be present when None"
        );
    }

    /// Regression guard: Stripe's /v1/events endpoint documents a maximum of
    /// 20 `types[]` values per request. The production webhook-compensation
    /// worker currently supplies 23 event types; the client must cap the
    /// outgoing array at 20 so the request is accepted instead of rejected
    /// with 400. Callers are responsible for chunking to cover the remainder.
    #[tokio::test]
    async fn test_list_events_caps_types_at_stripe_maximum() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [],
                "has_more": false
            })))
            .mount(&mock_server)
            .await;

        // 23 types — mirrors the production webhook-compensation configuration.
        let event_types: Vec<String> = (0..23).map(|i| format!("event.type.{i}")).collect();

        client
            .list_events(&ListEventsParams {
                created_gte: 1_700_000_000,
                created_lte: 1_700_001_000,
                event_types,
                limit: 100,
                starting_after: None,
            })
            .await
            .expect("list_events should succeed");

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording should be enabled");
        assert_eq!(requests.len(), 1);

        let type_values: Vec<String> = requests[0]
            .url
            .query_pairs()
            .filter(|(k, _)| k == "types[]")
            .map(|(_, v)| v.to_string())
            .collect();
        assert_eq!(
            type_values.len(),
            20,
            "Stripe /v1/events accepts at most 20 types[] values; client must cap the array"
        );
        // The first 20 requested types are sent in order.
        for i in 0..20 {
            assert!(
                type_values.contains(&format!("event.type.{i}")),
                "expected event.type.{} to be sent",
                i
            );
        }
    }

    /// Verifies that `list_events` parses a single-page response into
    /// `StripeEventList` with correct event fields and `has_more = false`.
    #[tokio::test]
    async fn test_list_events_parses_single_page_response() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {
                        "id": "evt_001",
                        "type": "checkout.session.completed",
                        "created": 1700000050,
                        "data": { "object": { "id": "cs_test_abc" } }
                    },
                    {
                        "id": "evt_002",
                        "type": "customer.subscription.created",
                        "created": 1700000080,
                        "data": { "object": { "id": "sub_test_xyz" } }
                    }
                ],
                "has_more": false
            })))
            .mount(&mock_server)
            .await;

        let result = client
            .list_events(&ListEventsParams {
                created_gte: 1_700_000_000,
                created_lte: 1_700_001_000,
                event_types: vec!["checkout.session.completed".to_string()],
                limit: 100,
                starting_after: None,
            })
            .await
            .expect("list_events should succeed");

        assert_eq!(result.data.len(), 2);
        assert!(!result.has_more);

        // First event
        assert_eq!(result.data[0].id, "evt_001");
        assert_eq!(result.data[0].event_type, "checkout.session.completed");
        assert_eq!(result.data[0].created, 1_700_000_050);
        assert_eq!(result.data[0].data["object"]["id"], "cs_test_abc");

        // Second event
        assert_eq!(result.data[1].id, "evt_002");
        assert_eq!(result.data[1].event_type, "customer.subscription.created");
        assert_eq!(result.data[1].created, 1_700_000_080);
        assert_eq!(result.data[1].data["object"]["id"], "sub_test_xyz");
    }

    /// Verifies that `list_events` makes exactly one HTTP request (single-page
    /// method) and passes `has_more: true` through to the caller, who is
    /// responsible for external pagination.
    #[tokio::test]
    async fn test_list_events_follows_pagination() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {
                        "id": "evt_100",
                        "type": "checkout.session.completed",
                        "created": 1700000050,
                        "data": {}
                    },
                    {
                        "id": "evt_101",
                        "type": "checkout.session.completed",
                        "created": 1700000060,
                        "data": {}
                    }
                ],
                "has_more": true
            })))
            .mount(&mock_server)
            .await;

        let result = client
            .list_events(&ListEventsParams {
                created_gte: 1_700_000_000,
                created_lte: 1_700_001_000,
                event_types: vec!["checkout.session.completed".to_string()],
                limit: 2,
                starting_after: None,
            })
            .await
            .expect("list_events should succeed");

        // Single-page method: exactly one HTTP request
        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording should be enabled");
        assert_eq!(
            requests.len(),
            1,
            "list_events should make exactly one request"
        );

        // Returns the page as-is; caller handles pagination
        assert_eq!(result.data.len(), 2);
        assert!(
            result.has_more,
            "has_more must be true so the caller can paginate externally"
        );
    }

    /// Verifies that `list_events` returns an empty list when the API
    /// responds with `data: []` and `has_more: false`.
    #[tokio::test]
    async fn test_list_events_empty_response() {
        let mock_server = MockServer::start().await;
        let client =
            StripeClient::with_base_url("sk_test_123".to_string(), mock_server.uri(), 30).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [],
                "has_more": false
            })))
            .mount(&mock_server)
            .await;

        let result = client
            .list_events(&ListEventsParams {
                created_gte: 1_700_000_000,
                created_lte: 1_700_001_000,
                event_types: vec!["checkout.session.completed".to_string()],
                limit: 100,
                starting_after: None,
            })
            .await
            .expect("list_events should succeed");

        assert!(result.data.is_empty(), "data should be empty");
        assert!(!result.has_more, "has_more should be false");
    }
}
