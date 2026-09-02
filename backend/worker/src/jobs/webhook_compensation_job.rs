use std::sync::Arc;

use sqlx::PgPool;
use tracing::{error, info, warn};

use herald_core::domain::billing::compensation::WebhookEventProcessor;
use herald_core::{CreemClient, StripeClient};

/// Stripe event types that Herald can process.
const STRIPE_EVENT_TYPES: &[&str] = &[
    "checkout.session.completed",
    "checkout.session.expired",
    "checkout.session.async_payment_succeeded",
    "checkout.session.async_payment_failed",
    "customer.subscription.created",
    "customer.subscription.updated",
    "customer.subscription.paused",
    "customer.subscription.resumed",
    "customer.subscription.deleted",
    "charge.refunded",
    "charge.dispute.created",
    "charge.dispute.closed",
    "credit_note.created",
    "credit_note.voided",
    "invoice.payment_succeeded",
    "invoice.payment_action_required",
    "invoice.payment_failed",
    "invoice.created",
    "invoice.finalized",
    "invoice.paid",
    "invoice.voided",
    "payment_intent.succeeded",
    "payment_intent.payment_failed",
];

/// Creem subscription statuses that map to routeable event types in process_creem_event_once.
/// Unknown statuses are skipped (not counted as compensated or failed) to avoid the
/// handler's catch-all branch returning a placeholder that inflates the compensated count.
const KNOWN_CREEM_SUBSCRIPTION_STATUSES: &[&str] = &[
    "paid",
    "active",
    "trialing",
    "update",
    "canceled",
    "paused",
    "past_due",
    "scheduled_cancel",
    "expired",
];

/// Overlap factor applied to the lookback window.
///
/// Using 2x the interval ensures consecutive runs overlap, preventing
/// permanent event loss when worker downtime exceeds the interval.
const LOOKBACK_OVERLAP_FACTOR: u64 = 2;

/// Result of a compensation run.
#[derive(Debug, Default)]
pub struct CompensationResult {
    pub realms_scanned: usize,
    pub events_fetched: usize,
    pub events_compensated: usize,
    pub events_failed: usize,
}

pub struct WebhookCompensationJob {
    pg_pool: PgPool,
    processor: Arc<dyn WebhookEventProcessor>,
    interval_secs: u64,
    http: reqwest::Client,
}

impl WebhookCompensationJob {
    pub fn new(
        pg_pool: PgPool,
        processor: Arc<dyn WebhookEventProcessor>,
        interval_secs: u64,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to build reqwest::Client for compensation job");
        Self {
            pg_pool,
            processor,
            interval_secs,
            http,
        }
    }

    #[tracing::instrument(
        // Governance: root span — no inbound request context.
        // `self` carries provider api keys / DB pool handles (compensation
        // fetches Stripe/Creem API keys from realm_config), so it is skipped.
        // Only the low-cardinality job name is recorded.
        skip(self),
        fields(job.name = "webhook_compensation")
    )]
    pub async fn run(&self) -> anyhow::Result<CompensationResult> {
        let mut result = CompensationResult::default();
        let now = chrono::Utc::now();
        let window_start =
            now - chrono::Duration::seconds((self.interval_secs * LOOKBACK_OVERLAP_FACTOR) as i64);
        let created_gte = window_start.timestamp();
        let created_lte = now.timestamp();

        // Query realms that have Stripe and/or Creem API keys configured.
        let realms = self.fetch_configured_realms().await?;
        result.realms_scanned = realms.len();

        for realm in &realms {
            if let Some(ref stripe_api_key) = realm.stripe_api_key {
                match self
                    .compensate_stripe(
                        &realm.realm_id,
                        stripe_api_key,
                        realm.stripe_base_url.as_deref(),
                        created_gte,
                        created_lte,
                    )
                    .await
                {
                    Ok(stats) => {
                        result.events_fetched += stats.events_fetched;
                        result.events_compensated += stats.events_compensated;
                        result.events_failed += stats.events_failed;
                    }
                    Err(e) => {
                        error!(
                            realm_id = %realm.realm_id,
                            error = %e,
                            "Stripe compensation failed for realm"
                        );
                    }
                }
            }

            if let Some(ref creem_api_key) = realm.creem_api_key {
                match self
                    .compensate_creem(
                        &realm.realm_id,
                        creem_api_key,
                        realm.creem_base_url.as_deref(),
                        created_gte,
                    )
                    .await
                {
                    Ok(stats) => {
                        result.events_fetched += stats.events_fetched;
                        result.events_compensated += stats.events_compensated;
                        result.events_failed += stats.events_failed;
                    }
                    Err(e) => {
                        error!(
                            realm_id = %realm.realm_id,
                            error = %e,
                            "Creem compensation failed for realm"
                        );
                    }
                }
            }
        }

        info!(
            realms_scanned = result.realms_scanned,
            events_fetched = result.events_fetched,
            events_compensated = result.events_compensated,
            events_failed = result.events_failed,
            "Webhook compensation completed"
        );

        Ok(result)
    }

    async fn fetch_configured_realms(&self) -> anyhow::Result<Vec<RealmConfig>> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>)>(
            r#"
            SELECT realm_id, config_type, config_key, config_value
            FROM realm_config
            WHERE config_type IN ('stripe', 'creem')
              AND config_key IN ('api_key', 'base_url')
              AND enabled = true
            "#,
        )
        .fetch_all(&self.pg_pool)
        .await?;

        // Group by realm_id to build RealmConfig with both providers.
        let mut map: std::collections::HashMap<String, RealmConfig> =
            std::collections::HashMap::new();

        for (realm_id, config_type, config_key, config_value) in rows {
            let entry = map.entry(realm_id.clone()).or_insert_with(|| RealmConfig {
                realm_id,
                stripe_api_key: None,
                stripe_base_url: None,
                creem_api_key: None,
                creem_base_url: None,
            });

            match (config_type.as_str(), config_key.as_str()) {
                ("stripe", "api_key") => entry.stripe_api_key = config_value,
                ("stripe", "base_url") => entry.stripe_base_url = config_value,
                ("creem", "api_key") => entry.creem_api_key = config_value,
                ("creem", "base_url") => entry.creem_base_url = config_value,
                _ => {}
            }
        }

        Ok(map.into_values().collect())
    }

    async fn compensate_stripe(
        &self,
        realm_id: &str,
        api_key: &str,
        base_url: Option<&str>,
        created_gte: i64,
        created_lte: i64,
    ) -> anyhow::Result<CompensationStats> {
        let base_url = base_url
            .map(|u| u.to_string())
            .unwrap_or_else(|| "https://api.stripe.com".to_string());
        let client =
            StripeClient::with_http_client(self.http.clone(), api_key.to_string(), base_url);
        let mut stats = CompensationStats::default();
        let mut starting_after: Option<String> = None;

        loop {
            let params = herald_core::infrastructure::stripe::ListEventsParams {
                created_gte,
                created_lte,
                event_types: STRIPE_EVENT_TYPES.iter().map(|s| s.to_string()).collect(),
                limit: 100,
                starting_after: starting_after.clone(),
            };

            let event_list = client.list_events(&params).await?;
            stats.events_fetched += event_list.data.len();

            for event in &event_list.data {
                let payload = serde_json::json!({
                    "id": event.id,
                    "type": event.event_type,
                    "data": event.data,
                });
                match self
                    .processor
                    .reprocess_event(realm_id, "stripe", &event.event_type, &payload)
                    .await
                {
                    Ok(()) => {
                        stats.events_compensated += 1;
                    }
                    Err(e) => {
                        stats.events_failed += 1;
                        error!(
                            realm_id = %realm_id,
                            event_id = %event.id,
                            event_type = %event.event_type,
                            error = %e,
                            "Failed to compensate Stripe event"
                        );
                    }
                }
            }

            if event_list.has_more {
                if let Some(last) = event_list.data.last() {
                    starting_after = Some(last.id.clone());
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(stats)
    }

    async fn compensate_creem(
        &self,
        realm_id: &str,
        api_key: &str,
        base_url: Option<&str>,
        created_gte: i64,
    ) -> anyhow::Result<CompensationStats> {
        let base_url = base_url.map(|u| u.to_string()).unwrap_or_else(|| {
            if api_key.starts_with("ck_test_") || api_key.starts_with("creem_test_") {
                "https://test-api.creem.io".to_string()
            } else {
                "https://api.creem.io".to_string()
            }
        });
        let client =
            CreemClient::with_http_client(self.http.clone(), api_key.to_string(), base_url);
        let mut stats = CompensationStats::default();
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        let mut page_number = 1;
        let page_size = 100;

        loop {
            let params = herald_core::infrastructure::creem::SearchTransactionsParams {
                page_number,
                page_size,
                created_after: Some(created_gte),
            };

            let tx_list = client.search_transactions(&params).await?;
            stats.events_fetched += tx_list.data.len();

            for tx in &tx_list.data {
                // Deduplicate across transactions and subscriptions.
                if !seen_ids.insert(tx.id.clone()) {
                    continue;
                }

                let event_type = Self::creem_event_type_from_transaction(tx);
                let event_id = tx.id.clone();

                // Build subscription/customer with camelCase keys explicitly,
                // since Serialize on CreemTransactionSub/Customer uses snake_case
                // but the webhook handler expects camelCase.
                let subscription = tx
                    .subscription
                    .as_ref()
                    .map(|s| serde_json::json!({ "subscriptionId": s.subscription_id }));
                let customer = tx
                    .customer
                    .as_ref()
                    .map(|c| serde_json::json!({ "customerId": c.customer_id }));
                let order = tx
                    .order
                    .as_ref()
                    .map(|o| serde_json::json!({ "orderId": o.order_id }));

                let payload = serde_json::json!({
                    "id": tx.id,
                    "eventType": event_type,
                    "object": {
                        "id": tx.id,
                        "type": tx.r#type,
                        "status": tx.status,
                        "amount": tx.amount,
                        "currency": tx.currency,
                        "order": order,
                        "subscription": subscription,
                        "customer": customer,
                    },
                });
                match self
                    .processor
                    .reprocess_event(realm_id, "creem", &event_type, &payload)
                    .await
                {
                    Ok(()) => {
                        stats.events_compensated += 1;
                    }
                    Err(e) => {
                        stats.events_failed += 1;
                        error!(
                            realm_id = %realm_id,
                            event_id = %event_id,
                            event_type = %event_type,
                            error = %e,
                            "Failed to compensate Creem transaction"
                        );
                    }
                }
            }

            if let Some(next) = tx_list.pagination.next_page {
                page_number = next;
            } else {
                break;
            }
        }

        let mut page_number = 1;
        loop {
            let params = herald_core::infrastructure::creem::SearchSubscriptionsParams {
                page_number,
                page_size,
                created_after: Some(created_gte),
            };

            let sub_list = client.search_subscriptions(&params).await?;
            stats.events_fetched += sub_list.data.len();

            for sub in &sub_list.data {
                // Deduplicate: subscription IDs may overlap with transaction-subscription IDs.
                if !seen_ids.insert(sub.id.clone()) {
                    continue;
                }

                let event_type = format!("subscription.{}", sub.status);
                let event_id = sub.id.clone();

                // Skip unknown subscription statuses -- the handler's catch-all branch
                // would silently return a placeholder, inflating compensated count.
                if !KNOWN_CREEM_SUBSCRIPTION_STATUSES.contains(&sub.status.as_str()) {
                    warn!(
                        realm_id = %realm_id,
                        subscription_id = %sub.id,
                        status = %sub.status,
                        "Skipping Creem subscription with unknown status"
                    );
                    continue;
                }

                let payload = serde_json::json!({
                    "id": sub.id,
                    "eventType": event_type,
                    "object": {
                        "id": sub.id,
                        "status": sub.status,
                        "customer": sub.customer,
                        "product": sub.product,
                        "canceled_at": sub.canceled_at,
                        "current_period_start_date": sub.current_period_start_date,
                        "current_period_end_date": sub.current_period_end_date,
                    },
                });
                match self
                    .processor
                    .reprocess_event(realm_id, "creem", &event_type, &payload)
                    .await
                {
                    Ok(()) => {
                        stats.events_compensated += 1;
                    }
                    Err(e) => {
                        stats.events_failed += 1;
                        error!(
                            realm_id = %realm_id,
                            event_id = %event_id,
                            event_type = %event_type,
                            error = %e,
                            "Failed to compensate Creem subscription"
                        );
                    }
                }
            }

            if let Some(next) = sub_list.pagination.next_page {
                page_number = next;
            } else {
                break;
            }
        }

        Ok(stats)
    }

    fn creem_event_type_from_transaction(
        tx: &herald_core::infrastructure::creem::CreemTransaction,
    ) -> String {
        if tx.status == "chargeback" {
            return "dispute.created".to_string();
        }
        if let Some(refunded) = tx.refunded_amount
            && refunded > 0
        {
            return "refund.created".to_string();
        }
        match tx.r#type.as_str() {
            "payment" => {
                if tx.subscription.is_some() {
                    "subscription.paid".to_string()
                } else {
                    "checkout.completed".to_string()
                }
            }
            "invoice" => "subscription.paid".to_string(),
            _ => "checkout.completed".to_string(),
        }
    }
}

struct RealmConfig {
    realm_id: String,
    stripe_api_key: Option<String>,
    stripe_base_url: Option<String>,
    creem_api_key: Option<String>,
    creem_base_url: Option<String>,
}

/// Internal stats accumulator per provider path.
#[derive(Default)]
struct CompensationStats {
    events_fetched: usize,
    events_compensated: usize,
    events_failed: usize,
}

// Governance tests.
// Covers: worker jobs `WebhookCompensationJob::run`,
// `PointsExpirationJob::run`, `PointsQuotaExpirationJob::run` instrument skip
// correctness.
// WHY: these are root spans with no inbound request context, and `self`
// carries provider API keys / DB pool / repository handles. If the
// `#[instrument]` macro ever stops skipping `self`, those handles/keys may be
// recorded as span fields. Source-scan baseline, anchored per
// method to the immediately-preceding `#[tracing::instrument(...)]`.
#[cfg(test)]
mod instrument_skip_tests {
    const COMP_SRC: &str = include_str!("webhook_compensation_job.rs");
    const EXP_SRC: &str = include_str!("points_expiration_job.rs");
    const PREGRANT_SRC: &str = include_str!("points_pre_grant_job.rs");

    fn instrument_body_preceding(src: &str, fn_name: &str) -> String {
        let needle = format!("fn {fn_name}");
        let fn_pos = src
            .find(&needle)
            .unwrap_or_else(|| panic!("fn {fn_name} not found in source"));
        let attr_start = src[..fn_pos]
            .rfind("#[tracing::instrument(")
            .unwrap_or_else(|| panic!("no #[tracing::instrument( preceding fn {fn_name}"));
        let body_start = attr_start + "#[tracing::instrument(".len();
        // Find the attribute close: the first line at/after body_start whose
        // trimmed content is exactly `)]`. This handles indented closes (e.g.
        // inside an `impl` block) and ignores inline `))]` sequences such as
        // `#[validate(length(...))]` that appear on struct fields.
        let tail = &src[body_start..];
        let mut consumed = 0usize;
        for line in tail.lines() {
            let prev = consumed;
            consumed += line.len() + 1; // +1 for the line separator
            if line.trim() == ")]" {
                return tail[..prev].to_string();
            }
        }
        panic!("unterminated #[tracing::instrument( for fn {fn_name}")
    }

    #[test]
    fn instrument_skip_worker_webhook_compensation_run_is_root_span_skipping_self() {
        let body = instrument_body_preceding(COMP_SRC, "run");
        assert!(
            body.contains("skip(self)"),
            "WebhookCompensationJob::run must skip(self) (carries provider API keys / DB pool); body was:\n{body}"
        );
        for banned in ["token", "password", "email", "secret", "api_key", "apikey"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "webhook_compensation span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }

    #[test]
    fn instrument_skip_worker_points_expiration_run_is_root_span_skipping_self() {
        let body = instrument_body_preceding(EXP_SRC, "run");
        assert!(
            body.contains("skip(self)"),
            "PointsExpirationJob::run must skip(self) (holds repository/DB handles); body was:\n{body}"
        );
        for banned in ["token", "password", "email", "secret", "api_key", "apikey"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "points_expiration span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }

    #[test]
    fn instrument_skip_worker_points_quota_expiration_run_is_root_span_skipping_self() {
        let body = instrument_body_preceding(PREGRANT_SRC, "run");
        assert!(
            body.contains("skip(self)"),
            "PointsQuotaExpirationJob::run must skip(self) (holds GrantScheduler); body was:\n{body}"
        );
        for banned in ["token", "password", "email", "secret", "api_key", "apikey"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "points_quota_expiration span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }
}
