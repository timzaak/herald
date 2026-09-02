//!
//! Periodically reconciles Apple + Google IAP state against Herald's view of
//! the world. Unlike Stripe/Creem compensation (which replays provider event
//! streams via `list_events` / `search_transactions`), IAP compensation is
//! "lookup + construct payload + replay through the same
//! [`WebhookEventProcessor`]". The constructed payloads are consumed by
//! [`iap_handlers::reprocess_apple_event`] /
//! contract).
//!
//!
//! - Apple notification compensation: default **1800s** (30 min) — Apple
//!   notification history retains ~30 days and this cadence bounds
//!   missed-notification latency well within that window.
//! - Google lifecycle polling: default **900s** (15 min) — Google is the
//!   *primary* lifecycle driver this period (no RTDN), so it runs on a tighter
//!   cadence than Apple.
//!
//! Both run inside the same `IapReconciliationJob::run` invocation; the worker
//! schedules the job on a single `iap_reconciliation_interval_secs` timer and
//! this file fans out per-realm + per-provider. The two intervals are surfaced
//! as independent `WorkerConfig` keys (see `lib.rs`) so operators can tune them
//! without touching code.
//!
//! # Failure isolation
//!
//! Each realm / transaction / token is reconciled independently. A single
//! failure (provider API error, malformed notification, stale token) is logged
//! object failure does not block others"). This mirrors
//! `WebhookCompensationJob::compensate_stripe` / `compensate_creem`.
//!
//! # Scope boundary
//!
//! Job-level integration tests (Apple missed-notification compensation, Google
//! state-change capture, voided refund recovery, single-token failure
//! production logic + skeleton-level unit tests for the pure helpers
//! (status mapping, page-token advance).

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use futures::FutureExt as _;
use herald_core::domain::billing::compensation::WebhookEventProcessor;
use herald_infra_iap::apple::models::{
    Environment as AppleEnvironment, NotificationHistoryRequest, NotificationHistoryResponse,
};
use herald_infra_iap::google::models::{SubscriptionPurchaseV2, VoidedPurchasesList};
use herald_infra_iap::google::service_account::GoogleServiceAccountAuth;
use herald_infra_iap::{AppleServerApiClient, GoogleDeveloperClient, IapError};
use sqlx::PgPool;
use tracing::{error, warn};

/// Lookback overlap factor shared by the Apple notification-history and Google
/// voided-purchases windows. Both providers retain ~30 days of history; 2× the
/// polling interval covers a single missed gap and mirrors the Stripe/Creem
/// compensation overlap policy.
const LOOKBACK_OVERLAP_FACTOR: i64 = 2;

/// Google voided-purchases lookback window. Google retains voided purchase
/// records for 30 days; the same 2× overlap factor bounds the missed-refund
/// gap.
const GOOGLE_VOIDED_LOOKBACK_SECS: i64 = 2 * 60 * 60;

/// How many realms' active subscriptions to load per page when polling Google
/// lifecycle. Bounded so a realm with many subscriptions does not hold a giant
/// result set in memory.
const GOOGLE_SUBSCRIPTIONS_PAGE_SIZE: u64 = 200;

/// How many Apple notification history pages to walk before giving up (defence
/// against an accidental infinite loop if Apple keeps returning `hasMore`).
const APPLE_HISTORY_MAX_PAGES: u32 = 50;

/// How many Google voided-purchase pages to walk before giving up.
const GOOGLE_VOIDED_MAX_PAGES: u32 = 50;

/// Result of a reconciliation run. Mirrors `CompensationResult` shape so logs
/// are comparable across compensation jobs.
#[derive(Debug, Default)]
pub struct IapReconciliationStats {
    pub realms_scanned: usize,
    /// Apple notification-history records fetched (post-pagination).
    pub apple_notifications_fetched: usize,
    /// Apple notifications that survived dedup (local payment_event missing)
    /// and were handed to `reprocess_event`.
    pub apple_replayed: usize,
    /// Apple failures (per-notification). Does NOT abort the sweep.
    pub apple_failed: usize,
    /// Google subscription tokens polled via `subscriptionsv2.get`.
    pub google_tokens_polled: usize,
    /// Google state-change replays handed to `reprocess_event`.
    pub google_replayed: usize,
    /// Google voided purchases fetched.
    pub google_voided_fetched: usize,
    /// Google failures (per-token / per-voided-row). Does NOT abort the sweep.
    pub google_failed: usize,
}

/// IAP reconciliation job.
///
/// Constructed once and held by the worker; per-run it scans the realms that
/// have Apple and/or Google IAP credentials configured and fans out
/// `compensate_apple` / `poll_google_lifecycle` per realm.
///
/// The Apple `AppleServerApiClient` and Google `GoogleDeveloperClient` /
/// `GoogleServiceAccountAuth` are constructed **per realm** inside each
/// `compensate_*` call (the clients embed realm-specific signing material).
pub struct IapReconciliationJob {
    pg_pool: PgPool,
    processor: Arc<dyn WebhookEventProcessor>,
    /// Apple compensation interval (seconds). Used to size the notification
    /// history lookback window (interval × overlap factor).
    apple_interval_secs: i64,
    /// Google lifecycle polling interval (seconds). Used to size the voided
    /// purchases lookback window.
    google_interval_secs: i64,
    http: reqwest::Client,
}

impl IapReconciliationJob {
    /// Construct the job.
    ///
    /// `apple_interval_secs` / `google_interval_secs` are the *configured*
    /// lookback windows; the actual firing cadence is owned by the worker
    /// `tokio::select!` arm in `lib.rs`.
    pub fn new(
        pg_pool: PgPool,
        processor: Arc<dyn WebhookEventProcessor>,
        apple_interval_secs: u64,
        google_interval_secs: u64,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to build reqwest::Client for iap reconciliation job");
        Self {
            pg_pool,
            processor,
            apple_interval_secs: apple_interval_secs as i64,
            google_interval_secs: google_interval_secs as i64,
            http,
        }
    }

    #[tracing::instrument(
        // Governance: root span — no inbound request context.
        // `self` carries the DB pool, the WebhookEventProcessor trait object
        // (which holds AppState handles) and a reqwest::Client — skip it. Only
        // the low-cardinality job name is recorded, mirroring the existing
        // webhook_compensation / payment_event_retry root spans.
        skip(self),
        fields(job.name = "iap_reconciliation")
    )]
    pub async fn run(&self) -> anyhow::Result<IapReconciliationStats> {
        let mut stats = IapReconciliationStats::default();
        let realms = self.fetch_iap_configured_realms().await?;
        stats.realms_scanned = realms.len();

        for realm in &realms {
            if realm.has_apple {
                match self.compensate_apple(realm).await {
                    Ok(apple_stats) => {
                        stats.apple_notifications_fetched += apple_stats.fetched;
                        stats.apple_replayed += apple_stats.replayed;
                        stats.apple_failed += apple_stats.failed;
                    }
                    Err(e) => {
                        // Realm-level failure (e.g. credentials malformed,
                        // Apple API fully unreachable). Log and continue —
                        // other realms must still be reconciled this cycle.
                        error!(
                            realm_id = %realm.realm_id,
                            error = %e,
                            "Apple compensation failed for realm"
                        );
                    }
                }
            }

            if realm.has_google {
                match self.poll_google_lifecycle(realm).await {
                    Ok(google_stats) => {
                        stats.google_tokens_polled += google_stats.tokens_polled;
                        stats.google_replayed += google_stats.replayed;
                        stats.google_voided_fetched += google_stats.voided_fetched;
                        stats.google_failed += google_stats.failed;
                    }
                    Err(e) => {
                        error!(
                            realm_id = %realm.realm_id,
                            error = %e,
                            "Google lifecycle polling failed for realm"
                        );
                    }
                }
            }
        }

        // Per-cycle completion is logged by the worker's `iap_reconciliation`
        // select arm (consistent with the other background jobs, which also
        // leave completion logging to the worker); the job just returns stats.
        Ok(stats)
    }

    /// Scan `realm_config` for realms with Apple and/or Google IAP credentials
    /// `issuer_id` row is present and non-empty (mirrors the `configured iff
    // config_key == issuer_id` rule in provider_handlers); "has_google" iff
    /// `service_account_json` is present.
    ///
    /// Also collects the optional per-provider `base_url` override rows
    /// (Stripe/Creem `base_url` realm-config injection pattern) so the
    /// per-realm Apple / Google clients can be pointed at a wiremock during
    /// tests. Production realms leave `base_url` unset → production endpoints.
    async fn fetch_iap_configured_realms(&self) -> anyhow::Result<Vec<IapRealmConfig>> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>)>(
            r#"
            SELECT realm_id, config_type, config_key, config_value
            FROM realm_config
            WHERE config_type IN ('apple', 'google')
              AND config_key IN
                  ('bundle_id', 'issuer_id', 'key_id', 'private_key_p8',
                   'environment', 'package_name', 'service_account_json',
                   'base_url')
              AND enabled = true
            "#,
        )
        .fetch_all(&self.pg_pool)
        .await?;

        let mut map: std::collections::HashMap<String, IapRealmConfig> =
            std::collections::HashMap::new();

        for (realm_id, config_type, config_key, config_value) in rows {
            let entry = map
                .entry(realm_id.clone())
                .or_insert_with(|| IapRealmConfig {
                    realm_id,
                    has_apple: false,
                    has_google: false,
                    apple_creds: AppleRealmCreds::default(),
                    google_creds: GoogleRealmCreds::default(),
                });

            let value_nonempty = config_value.as_deref().filter(|v| !v.is_empty());
            match (config_type.as_str(), config_key.as_str()) {
                ("apple", "bundle_id") => {
                    entry.apple_creds.bundle_id = value_nonempty.map(Into::into)
                }
                ("apple", "issuer_id") => {
                    entry.apple_creds.issuer_id = value_nonempty.map(Into::into);
                    if value_nonempty.is_some() {
                        entry.has_apple = true;
                    }
                }
                ("apple", "key_id") => entry.apple_creds.key_id = value_nonempty.map(Into::into),
                ("apple", "private_key_p8") => {
                    entry.apple_creds.private_key_p8 = value_nonempty.map(Into::into)
                }
                ("apple", "environment") => {
                    entry.apple_creds.environment = value_nonempty.map(Into::into)
                }
                ("apple", "base_url") => {
                    entry.apple_creds.base_url = value_nonempty.map(Into::into)
                }
                ("google", "package_name") => {
                    entry.google_creds.package_name = value_nonempty.map(Into::into)
                }
                ("google", "service_account_json") => {
                    entry.google_creds.service_account_json = value_nonempty.map(Into::into);
                    if value_nonempty.is_some() {
                        entry.has_google = true;
                    }
                }
                ("google", "base_url") => {
                    entry.google_creds.base_url = value_nonempty.map(Into::into)
                }
                _ => {}
            }
        }

        Ok(map.into_values().collect())
    }

    ///
    /// Walks Apple's `POST /inApps/v1/notifications/history` over the lookback
    /// window (`apple_interval_secs × overlap_factor`). For each historical
    /// notification whose `originalTransactionId` has no local `payment_event`,
    /// reconstructs a webhook-style payload and hands it to
    /// `reprocess_event(realm, "apple", type, payload)`. Single-notification
    /// failures are logged and skipped.
    async fn compensate_apple(&self, realm: &IapRealmConfig) -> anyhow::Result<AppleCompStats> {
        let mut stats = AppleCompStats::default();

        let client = match build_apple_client(realm, self.http.clone())? {
            Some(client) => client,
            // Configured-signal was true but individual keys are missing — skip
            // this realm silently rather than spamming the log every cycle.
            None => return Ok(stats),
        };

        let now = Utc::now();
        let start = now - Duration::seconds(self.apple_interval_secs * LOOKBACK_OVERLAP_FACTOR);
        let request = NotificationHistoryRequest {
            start_date: Some(start),
            end_date: Some(now),
            notification_type: None,
            notification_subtype: None,
            transaction_id: None,
            only_failures: Some(true),
        };

        let mut pagination_token = String::new();
        for _ in 0..APPLE_HISTORY_MAX_PAGES {
            // Panic guard: `app-store-server-library` mints its ES256 JWT inside
            // `ApiClient::generate_token` via
            // `EncodingKey::from_ec_pem(self.signing_key.as_slice()).unwrap()`
            // (and a second `.unwrap()` on the `encode` call). A realm whose
            // `.p8` is malformed / truncated / not a real EC key therefore
            // panics inside the API call rather than surfacing an `Err`.
            //
            // The job's contract is "single-realm failure must not abort the
            // and map it to a realm-level `anyhow::Error` — the outer `run()`
            // loop already logs + skips realm-level errors without aborting.
            // `catch_unwind` on the API-call future is the narrowest boundary
            // that covers the third-party panic source without widening the
            // guard over our own domain replay logic.
            let page_result =
                AssertUnwindSafe(client.get_notification_history(&pagination_token, &request))
                    .catch_unwind()
                    .await;
            let page: NotificationHistoryResponse = match page_result {
                Ok(Ok(page)) => page,
                Ok(Err(e)) => {
                    return Err(anyhow::anyhow!(
                        "apple notification history call failed: {e}"
                    ));
                }
                Err(panic_payload) => {
                    return Err(anyhow::anyhow!(
                        "apple API call panicked (likely malformed realm .p8 EC key inside app-store-server-library): {}",
                        panic_payload_downcast(&panic_payload)
                    ));
                }
            };

            stats.fetched += page
                .notification_history
                .as_ref()
                .map(Vec::len)
                .unwrap_or(0);

            if let Some(items) = page.notification_history.as_deref() {
                for item in items {
                    // Each history item carries the original signed JWS
                    // payload Apple attempted to deliver. We hand the raw
                    // signedPayload to reprocess_apple_event, which owns the
                    // JWS verification + domain replay.
                    let signed_payload = match item.signed_payload.as_deref() {
                        Some(s) if !s.is_empty() => s,
                        _ => {
                            stats.failed += 1;
                            warn!(
                                realm_id = %realm.realm_id,
                                "Apple notification history item missing signedPayload — skipping"
                            );
                            continue;
                        }
                    };

                    let payload = serde_json::json!({ "signedPayload": signed_payload });

                    // The event_type is derived inside reprocess_apple_event
                    // from the decoded notification; pass an empty string here
                    // (the reprocess body ignores this argument for Apple).
                    match self
                        .processor
                        .reprocess_event(&realm.realm_id, "apple", "", &payload)
                        .await
                    {
                        Ok(()) => {
                            stats.replayed += 1;
                        }
                        Err(e) => {
                            stats.failed += 1;
                            warn!(
                                realm_id = %realm.realm_id,
                                error = %e,
                                "Apple notification replay failed — skipping (non-blocking)"
                            );
                        }
                    }
                }
            }

            match page.pagination_token {
                Some(token) if !token.is_empty() => {
                    pagination_token = token;
                }
                _ => break,
            }
        }

        Ok(stats)
    }

    ///
    /// Two passes:
    /// 1. **Subscription refresh**: for each Herald `Subscription` whose
    ///    `payment_provider='google'` and status grants access (active /
    ///    trialing / scheduled_cancel / dispute / past_due), call
    ///    `subscriptionsv2.get` and map the returned state to an event_type.
    ///    State *changes* (renew / cancel / expire / grace) hand a constructed
    ///    payload to `reprocess_event(realm, "google", ...)`.
    /// 2. **Voided purchases**: page through `voidedpurchases.list` over the
    ///    lookback window and replay each voided row as a refund event.
    ///
    /// Single-token failures are logged and skipped.
    async fn poll_google_lifecycle(
        &self,
        realm: &IapRealmConfig,
    ) -> anyhow::Result<GooglePollStats> {
        let mut stats = GooglePollStats::default();

        let (client, auth, package_name) = match build_google_client(realm, self.http.clone())? {
            Some(triple) => triple,
            None => return Ok(stats),
        };

        // --- Pass 1: subscription lifecycle refresh --------------------------
        let mut page: u64 = 1;
        loop {
            let (subs, _total) = sqlx_client_list_active_google_subscriptions(
                &self.pg_pool,
                &realm.realm_id,
                page,
                GOOGLE_SUBSCRIPTIONS_PAGE_SIZE,
            )
            .await?;

            if subs.is_empty() {
                break;
            }

            for sub in &subs {
                stats.tokens_polled += 1;
                let token = &sub.external_subscription_id;
                match client.get_subscription(&auth, &package_name, token).await {
                    Ok(purchase) => {
                        if let Some((event_type, payload)) =
                            map_google_subscription_change(sub, &purchase)
                        {
                            match self
                                .processor
                                .reprocess_event(&realm.realm_id, "google", &event_type, &payload)
                                .await
                            {
                                Ok(()) => stats.replayed += 1,
                                Err(e) => {
                                    stats.failed += 1;
                                    warn!(
                                        realm_id = %realm.realm_id,
                                        error = %e,
                                        "Google subscription state-change replay failed — non-blocking"
                                    );
                                }
                            }
                        }
                    }
                    Err(IapError::GoogleApi { status: 404, .. }) => {
                        // Token no longer exists at Google — replay as an
                        // expiry so the local subscription moves to a terminal
                        // state. Single-token failure isolation: log + continue.
                        let (event_type, payload) = google_expired_payload(token);
                        match self
                            .processor
                            .reprocess_event(&realm.realm_id, "google", &event_type, &payload)
                            .await
                        {
                            Ok(()) => stats.replayed += 1,
                            Err(e) => {
                                stats.failed += 1;
                                warn!(
                                    realm_id = %realm.realm_id,
                                    error = %e,
                                    "Google 404-expire replay failed — non-blocking"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        stats.failed += 1;
                        warn!(
                            realm_id = %realm.realm_id,
                            error = %e,
                            "Google subscriptionsv2.get failed — skipping (non-blocking)"
                        );
                    }
                }
            }

            if subs.len() < GOOGLE_SUBSCRIPTIONS_PAGE_SIZE as usize {
                break;
            }
            page += 1;
        }

        // --- Pass 2: voided purchases (refund / chargeback) ------------------
        let lookback_secs = self.google_interval_secs.max(GOOGLE_VOIDED_LOOKBACK_SECS);
        let since = Utc::now() - Duration::seconds(lookback_secs * LOOKBACK_OVERLAP_FACTOR);

        let mut page_token = String::new();
        for _ in 0..GOOGLE_VOIDED_MAX_PAGES {
            let list: VoidedPurchasesList = client
                .list_voided_purchases(&auth, &package_name, &page_token)
                .await?;

            stats.voided_fetched += list.voided_purchases.len();

            for voided in &list.voided_purchases {
                let purchase_token = voided.purchase_token.clone().unwrap_or_default();
                let payload = serde_json::json!({
                    "purchaseToken": purchase_token,
                    "purchaseType": voided.purchase_type,
                    "voidedTimeMillis": voided.voided_time_millis,
                    "orderId": voided.order_id,
                    "voidedSince": since.timestamp_millis(),
                });

                match self
                    .processor
                    .reprocess_event(&realm.realm_id, "google", "subscription.refund", &payload)
                    .await
                {
                    Ok(()) => stats.replayed += 1,
                    Err(e) => {
                        stats.failed += 1;
                        warn!(
                            realm_id = %realm.realm_id,
                            error = %e,
                            "Google voided-purchase refund replay failed — non-blocking"
                        );
                    }
                }
            }

            // Google returns the next page token via either `tokenPagination`
            // (v3) or `pageInfo`-adjacent fields depending on endpoint version.
            // The client model exposes both; prefer `tokenPagination`.
            let next = list
                .token_pagination
                .as_ref()
                .and_then(|p| p.next_page_token.clone())
                .filter(|t| !t.is_empty());
            match next {
                Some(t) => page_token = t,
                None => break,
            }
        }

        Ok(stats)
    }
}

// ============================================================================
// Per-realm client construction
// ============================================================================

/// Build the per-realm Apple Server API client. Returns `None` (skip realm)
/// when the configured-signal row was present but individual keys are missing
/// or the environment string is unrecognised.
fn build_apple_client(
    realm: &IapRealmConfig,
    http: reqwest::Client,
) -> anyhow::Result<Option<AppleServerApiClient>> {
    let (issuer_id, key_id, bundle_id, private_key_p8) = match (
        realm.apple_creds.issuer_id.as_ref(),
        realm.apple_creds.key_id.as_ref(),
        realm.apple_creds.bundle_id.as_ref(),
        realm.apple_creds.private_key_p8.as_ref(),
    ) {
        (Some(issuer_id), Some(key_id), Some(bundle_id), Some(private_key_p8)) => {
            (issuer_id, key_id, bundle_id, private_key_p8)
        }
        _ => return Ok(None),
    };
    let environment = match realm.apple_creds.environment.as_deref() {
        Some("production") => AppleEnvironment::Production,
        Some("sandbox") => AppleEnvironment::Sandbox,
        // Apple's API also has Xcode / LocalTesting envs; the admin form only
        // writes production/sandbox, so treat anything else as "skip realm".
        other => {
            warn!(
                realm_id = %realm.realm_id,
                environment = ?other,
                "Apple reconciliation skipping realm with unsupported environment"
            );
            return Ok(None);
        }
    };

    let signing_key = private_key_p8.as_bytes().to_vec();
    let client = match realm.apple_creds.base_url.as_ref() {
        Some(base) if !base.is_empty() => AppleServerApiClient::with_base_url(
            signing_key,
            key_id.clone(),
            issuer_id.clone(),
            bundle_id.clone(),
            environment,
            http,
            base.clone(),
        ),
        _ => AppleServerApiClient::new(
            signing_key,
            key_id.clone(),
            issuer_id.clone(),
            bundle_id.clone(),
            environment,
            http,
        ),
    }
    .map_err(|e| anyhow::anyhow!("failed to build AppleServerApiClient: {e}"))?;

    Ok(Some(client))
}

/// Build the per-realm Google Developer client + service-account auth + the
/// resolved package name. Returns `None` (skip realm) when required keys are
/// missing or the service-account JSON is unparseable.
fn build_google_client(
    realm: &IapRealmConfig,
    http: reqwest::Client,
) -> anyhow::Result<Option<(GoogleDeveloperClient, GoogleServiceAccountAuth, String)>> {
    let Some(package_name) = realm.google_creds.package_name.as_ref() else {
        return Ok(None);
    };
    let Some(raw_sa) = realm.google_creds.service_account_json.as_ref() else {
        return Ok(None);
    };

    // Parse the two fields the auth needs out of the service-account JSON via
    // serde_json::Value (avoids adding a `serde` dependency to the worker
    // crate just for this two-field struct).
    let sa_value: serde_json::Value = serde_json::from_str(raw_sa)
        .map_err(|e| anyhow::anyhow!("failed to parse google service_account_json: {e}"))?;
    let client_email = sa_value
        .get("client_email")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("google service_account_json missing client_email"))?
        .to_string();
    let private_key = sa_value
        .get("private_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("google service_account_json missing private_key"))?
        .to_string();

    // Per-realm base-URL override (Stripe/Creem `base_url` pattern). When
    // present, both the Developer API client and the OAuth token endpoint are
    // rooted at the override (token URI = `{base}/token`); otherwise the
    // production Google endpoints are used (behaviour unchanged).
    let auth = match realm.google_creds.base_url.as_ref() {
        Some(base) if !base.is_empty() => GoogleServiceAccountAuth::with_token_uri(
            client_email,
            private_key.into_bytes(),
            format!("{}/token", base.trim_end_matches('/')),
        ),
        _ => GoogleServiceAccountAuth::new(client_email, private_key.into_bytes()),
    };
    let client = match realm.google_creds.base_url.as_ref() {
        Some(base) if !base.is_empty() => GoogleDeveloperClient::with_base_url(http, base.clone()),
        _ => GoogleDeveloperClient::new(http),
    };
    Ok(Some((client, auth, package_name.clone())))
}

// ============================================================================
// Google subscription state-change mapping (pure helpers — unit-tested)
// ============================================================================

/// Map a polled Google subscription state to a `(event_type, payload)` pair
/// when the state has changed relative to Herald's stored subscription. Returns
/// `None` when there is no actionable change (the common case — most polls see
/// no transition).
///
/// This is a deliberately conservative mapper: only the high-signal lifecycle
/// transitions produce a replay. Exact-match renewals (state + expiry
/// unchanged) are skipped to avoid no-op replays every cycle.
///
/// result is filtered by the subscription's snapshot `billing_type` *before*
/// any renewal-flow mapping. `non_renewing` subscriptions only ever emit an
/// expiry transition (Google `EXPIRED`) — ACTIVE / GRACE / CANCELED / PAUSED
/// and advancing-expiry (renewal) are ignored, so a non-renewing subscription
/// never enters the renewal state machine. `recurring` (and any future
/// subscription-shape billing type) retain the full mapping below.
fn map_google_subscription_change(
    stored: &StoredGoogleSubscription,
    purchase: &SubscriptionPurchaseV2,
) -> Option<(String, serde_json::Value)> {
    let new_state = purchase.subscription_state.as_deref().unwrap_or("");
    let mapped_status = google_state_to_herald_status(new_state);

    let token = &stored.external_subscription_id;
    let product_id = purchase
        .line_items
        .first()
        .and_then(|li| li.product_id.clone())
        .unwrap_or_else(|| stored.external_product_id.clone());

    // Non-renewing filter: only the EXPIRED transition is actionable from the
    // poll. The mapped `expired` status covers Google `SUBSCRIPTION_STATE_EXPIRED`
    // (and any future expiry-shaped state). Everything else — ACTIVE renewal
    // detection, grace / pause / cancel transitions — is dropped: a
    // non-renewing subscription has a fixed service window and does not
    // participate in the renewal flow. Role reclamation on natural expiry is
    if stored.billing_type == "non_renewing" {
        return if mapped_status == "expired" && stored.status != "expired" {
            Some((
                "subscription.expired".to_string(),
                google_state_change_payload(
                    token,
                    new_state,
                    "expired",
                    &stored.status,
                    &product_id,
                    purchase,
                ),
            ))
        } else {
            None
        };
    }

    // State transition: if Herald's recorded status differs from the mapped
    // Google state, emit the corresponding event.
    if mapped_status != stored.status {
        let event_type = match mapped_status.as_str() {
            "active" => "subscription.renewed",
            "canceled" | "expired" => "subscription.expired",
            "past_due" | "paused" => "subscription.past_due",
            _ => "subscription.updated",
        };
        return Some((
            event_type.to_string(),
            google_state_change_payload(
                token,
                new_state,
                &mapped_status,
                &stored.status,
                &product_id,
                purchase,
            ),
        ));
    }

    // Same status, but expiry advanced → treat as a renewal.
    if let Some(new_expiry) = purchase.line_items.first().and_then(|li| li.expiry_time)
        && stored
            .current_period_end
            .map(|stored_end| new_expiry > stored_end)
            .unwrap_or(true)
    {
        return Some((
            "subscription.renewed".to_string(),
            serde_json::json!({
                "purchaseToken": token,
                "subscriptionState": new_state,
                "productId": product_id,
                "expiryTime": new_expiry.to_rfc3339(),
            }),
        ));
    }

    None
}

/// Build the state-transition payload shared by every Google subscription
/// state change (recurring transition + non-renewing expiry). The caller picks
/// the `event_type`; only the payload `Value` is built here.
fn google_state_change_payload(
    token: &str,
    subscription_state: &str,
    herald_status: &str,
    previous_status: &str,
    product_id: &str,
    purchase: &SubscriptionPurchaseV2,
) -> serde_json::Value {
    serde_json::json!({
        "purchaseToken": token,
        "subscriptionState": subscription_state,
        "heraldStatus": herald_status,
        "previousStatus": previous_status,
        "productId": product_id,
        "expiryTime": purchase
            .line_items
            .first()
            .and_then(|li| li.expiry_time)
            .map(|t| t.to_rfc3339()),
    })
}

/// Construct the payload for a 404-induced expiry replay.
fn google_expired_payload(token: &str) -> (String, serde_json::Value) {
    (
        "subscription.expired".to_string(),
        serde_json::json!({
            "purchaseToken": token,
            "subscriptionState": "SUBSCRIPTION_STATE_EXPIRED",
            "heraldStatus": "expired",
            "reason": "google_token_not_found",
        }),
    )
}

/// Map a Google `subscriptionState` enum string to Herald's
/// `SubscriptionStatus` string form (lowercase, as stored in the DB).
///
/// Reference:
/// https://developers.google.com/android-publisher/api-ref/rest/v3/purchases.subscriptionsv2#subscriptionstate
fn google_state_to_herald_status(state: &str) -> String {
    match state {
        "SUBSCRIPTION_STATE_ACTIVE" => "active",
        "SUBSCRIPTION_STATE_IN_GRACE_PERIOD" => "past_due",
        "SUBSCRIPTION_STATE_ON_HOLD" => "paused",
        "SUBSCRIPTION_STATE_PAUSED" => "paused",
        "SUBSCRIPTION_STATE_CANCELED" => "scheduled_cancel",
        "SUBSCRIPTION_STATE_EXPIRED" => "expired",
        "SUBSCRIPTION_STATE_PENDING" => "incomplete",
        // Unknown / future values: treat as "updated" so the replay still
        // happens (the domain layer decides what to do).
        _ => "updated",
    }
    .to_string()
}

// ============================================================================
// Stored-row helpers
// ============================================================================

/// A Herald subscription row projected into the minimal fields the Google poll
/// needs to detect state changes. Kept separate from the domain `Subscription`
/// to avoid pulling the full entity into the worker's hot loop.
///
/// `billing_type` is the snapshot column written at fulfillment time
/// state transition (renew / cancel / expire / grace), while `non_renewing`
/// only acts on `EXPIRED` (and a Google 404) — other Google states (ACTIVE /
/// GRACE / CANCELED / …) are ignored so a non-renewing subscription never
struct StoredGoogleSubscription {
    external_subscription_id: String,
    external_product_id: String,
    status: String,
    billing_type: String,
    current_period_end: Option<DateTime<Utc>>,
}

/// Page through Herald's `subscription` table for Google subscriptions whose
/// status grants access (the set worth re-polling). A subscription that is
/// already `canceled` / `expired` has no actionable transition to discover
/// from `subscriptionsv2.get`, so we skip those to bound API calls.
///
/// This is a hand-written projection query (not a repository method) because
/// the worker must not depend on the Sea-ORM-based
/// `PostgresBillingRepository::list_subscriptions` (which returns the full
/// `Subscription` entity); we only need the projection below
/// (`external_subscription_id`, `external_product_id`, `status`,
/// `billing_type`, `current_period_end`).
///
/// `ActiveGoogleSubscriptionRow` is the row tuple alias sqlx decodes into
/// (kept as a named type to satisfy clippy's `type_complexity` lint once
/// `billing_type` made this a 5-tuple).
type ActiveGoogleSubscriptionRow = (String, String, String, String, Option<DateTime<Utc>>);

async fn sqlx_client_list_active_google_subscriptions(
    pg_pool: &PgPool,
    realm_id: &str,
    page: u64,
    page_size: u64,
) -> anyhow::Result<(Vec<StoredGoogleSubscription>, u64)> {
    let offset = page.saturating_sub(1) * page_size;
    let rows: Vec<ActiveGoogleSubscriptionRow> = sqlx::query_as(
        r#"
        SELECT external_subscription_id,
               external_product_id,
               status::text,
               billing_type,
               current_period_end
        FROM subscription
        WHERE realm_id = $1
          AND payment_provider = 'google'
          AND status IN ('active', 'trialing', 'scheduled_cancel', 'past_due', 'dispute', 'paused', 'incomplete')
        ORDER BY updated_at
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(realm_id)
    .bind(page_size as i64)
    .bind(offset as i64)
    .fetch_all(pg_pool)
    .await?;

    let more = rows.len() as u64 == page_size;
    let subs = rows
        .into_iter()
        .map(
            |(
                external_subscription_id,
                external_product_id,
                status,
                billing_type,
                current_period_end,
            )| StoredGoogleSubscription {
                external_subscription_id,
                external_product_id,
                status,
                billing_type,
                current_period_end,
            },
        )
        .collect();
    Ok((subs, if more { 1 } else { 0 }))
}

/// Best-effort stringification of a `catch_unwind` panic payload (`Box<dyn Any
/// + Send>`). `std::panic` payloads are typically `&'static str` or `String`;
/// anything else falls back to a generic placeholder so the resulting
/// `anyhow::Error` is always human-readable.
fn panic_payload_downcast(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string panic payload>".to_string()
}

// ============================================================================
// Internal structs
// ============================================================================

struct IapRealmConfig {
    realm_id: String,
    has_apple: bool,
    has_google: bool,
    apple_creds: AppleRealmCreds,
    google_creds: GoogleRealmCreds,
}

#[derive(Default)]
struct AppleRealmCreds {
    bundle_id: Option<String>,
    issuer_id: Option<String>,
    key_id: Option<String>,
    private_key_p8: Option<String>,
    environment: Option<String>,
    /// Optional App Store Server API base override (test injection /
    /// wiremock). Mirrors Stripe/Creem `base_url`. Production: `None`.
    base_url: Option<String>,
}

#[derive(Default)]
struct GoogleRealmCreds {
    package_name: Option<String>,
    service_account_json: Option<String>,
    /// Optional Play Developer API + OAuth token-endpoint base override
    /// (test injection / wiremock). Mirrors Stripe/Creem `base_url`.
    /// Production: `None`.
    base_url: Option<String>,
}

#[derive(Default)]
struct AppleCompStats {
    fetched: usize,
    replayed: usize,
    failed: usize,
}

#[derive(Default)]
struct GooglePollStats {
    tokens_polled: usize,
    replayed: usize,
    voided_fetched: usize,
    failed: usize,
}

#[cfg(test)]
mod tests {
    // Skeleton-level unit tests for the pure mapping helpers.
    //
    // compensation, Google state-change capture, voided refund recovery,
    // single-token failure isolation) require a live `WebhookEventProcessor`
    // plus DB + Apple/Google API stubs and are therefore owned by the test
    // slot. These unit tests cover the deterministic, side-effect-free helpers
    // that the integration tests would otherwise have to re-derive.

    use super::*;
    use chrono::TimeZone;

    fn stored(
        token: &str,
        status: &str,
        expiry: Option<DateTime<Utc>>,
    ) -> StoredGoogleSubscription {
        stored_with_billing(token, status, "recurring", expiry)
    }

    /// Same as [`stored`] but lets the caller pick the snapshot `billing_type`
    fn stored_with_billing(
        token: &str,
        status: &str,
        billing_type: &str,
        expiry: Option<DateTime<Utc>>,
    ) -> StoredGoogleSubscription {
        StoredGoogleSubscription {
            external_subscription_id: token.to_string(),
            external_product_id: "pro_monthly".to_string(),
            status: status.to_string(),
            billing_type: billing_type.to_string(),
            current_period_end: expiry,
        }
    }

    fn sub_state(state: &str, expiry: Option<DateTime<Utc>>) -> SubscriptionPurchaseV2 {
        let line_items = if expiry.is_some() {
            vec![herald_infra_iap::google::models::SubscriptionLineItem {
                product_id: Some("pro_monthly".to_string()),
                expiry_time: expiry,
                ..Default::default()
            }]
        } else {
            Vec::new()
        };
        SubscriptionPurchaseV2 {
            subscription_state: Some(state.to_string()),
            line_items,
            ..Default::default()
        }
    }

    #[test]
    fn google_state_maps_to_herald_status_strings() {
        assert_eq!(
            google_state_to_herald_status("SUBSCRIPTION_STATE_ACTIVE"),
            "active"
        );
        assert_eq!(
            google_state_to_herald_status("SUBSCRIPTION_STATE_IN_GRACE_PERIOD"),
            "past_due"
        );
        assert_eq!(
            google_state_to_herald_status("SUBSCRIPTION_STATE_ON_HOLD"),
            "paused"
        );
        assert_eq!(
            google_state_to_herald_status("SUBSCRIPTION_STATE_PAUSED"),
            "paused"
        );
        assert_eq!(
            google_state_to_herald_status("SUBSCRIPTION_STATE_CANCELED"),
            "scheduled_cancel"
        );
        assert_eq!(
            google_state_to_herald_status("SUBSCRIPTION_STATE_EXPIRED"),
            "expired"
        );
        // Unknown future values do not crash — they map to a generic "updated".
        assert_eq!(
            google_state_to_herald_status("SUBSCRIPTION_STATE_FUTURE"),
            "updated"
        );
    }

    #[test]
    fn map_returns_none_when_state_and_expiry_unchanged() {
        // Same active status, same expiry → no replay (avoids no-op churn
        // every cycle).
        let expiry = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let stored = stored("tok-1", "active", Some(expiry));
        let purchase = sub_state("SUBSCRIPTION_STATE_ACTIVE", Some(expiry));
        assert!(map_google_subscription_change(&stored, &purchase).is_none());
    }

    #[test]
    fn map_emits_expired_event_on_state_transition() {
        let expiry = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let stored = stored("tok-2", "active", Some(expiry));
        let purchase = sub_state("SUBSCRIPTION_STATE_EXPIRED", Some(expiry));

        let (event_type, payload) = map_google_subscription_change(&stored, &purchase)
            .expect("state transition must produce a replay");
        assert_eq!(event_type, "subscription.expired");
        assert_eq!(payload["heraldStatus"], "expired");
        assert_eq!(payload["previousStatus"], "active");
        assert_eq!(payload["purchaseToken"], "tok-2");
    }

    #[test]
    fn map_emits_renewed_event_when_expiry_advances() {
        // Same status but a later expiry → renewal.
        let old_expiry = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let new_expiry = Utc.timestamp_opt(1_700_086_400, 0).unwrap(); // +1d
        let stored = stored("tok-3", "active", Some(old_expiry));
        let purchase = sub_state("SUBSCRIPTION_STATE_ACTIVE", Some(new_expiry));

        let (event_type, payload) = map_google_subscription_change(&stored, &purchase)
            .expect("advancing expiry must produce a renewal replay");
        assert_eq!(event_type, "subscription.renewed");
        assert_eq!(payload["expiryTime"], new_expiry.to_rfc3339());
    }

    #[test]
    fn map_emits_renewed_event_when_stored_expiry_missing() {
        // No recorded expiry + active status + Google returns an expiry → treat
        // as renewal (first observation of an active subscription).
        let new_expiry = Utc.timestamp_opt(1_700_086_400, 0).unwrap();
        let stored = stored("tok-4", "active", None);
        let purchase = sub_state("SUBSCRIPTION_STATE_ACTIVE", Some(new_expiry));

        let (event_type, _payload) = map_google_subscription_change(&stored, &purchase)
            .expect("missing stored expiry must produce a replay");
        assert_eq!(event_type, "subscription.renewed");
    }

    #[test]
    fn google_expired_payload_carries_token_and_reason() {
        let (event_type, payload) = google_expired_payload("tok-404");
        assert_eq!(event_type, "subscription.expired");
        assert_eq!(payload["purchaseToken"], "tok-404");
        assert_eq!(payload["reason"], "google_token_not_found");
        assert_eq!(payload["heraldStatus"], "expired");
    }

    //
    // A non-renewing subscription has a fixed service window. The reconciliation
    // poll must only act on Google's EXPIRED transition (plus the 404 path); all
    // other Google states — ACTIVE renewal, GRACE, CANCELED, PAUSED, advancing
    // expiry — are dropped so the subscription never enters the renewal flow.

    #[test]
    fn non_renewing_emits_expired_when_google_reports_expired() {
        // The one transition that *does* fire for non_renewing: Google reports
        // EXPIRED while Herald still records the subscription as active.
        let expiry = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let stored = stored_with_billing("nr-1", "active", "non_renewing", Some(expiry));
        let purchase = sub_state("SUBSCRIPTION_STATE_EXPIRED", Some(expiry));

        let (event_type, payload) = map_google_subscription_change(&stored, &purchase)
            .expect("non_renewing EXPIRED transition must produce an expiry replay");
        assert_eq!(event_type, "subscription.expired");
        assert_eq!(payload["heraldStatus"], "expired");
        assert_eq!(payload["previousStatus"], "active");
        assert_eq!(payload["purchaseToken"], "nr-1");
    }

    #[test]
    fn non_renewing_ignores_active_state_transition() {
        // Non-renewing returning to ACTIVE from a non-active stored status must
        // NOT replay a "renewed" event — non_renewing never renews.
        let expiry = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let stored = stored_with_billing("nr-2", "past_due", "non_renewing", Some(expiry));
        let purchase = sub_state("SUBSCRIPTION_STATE_ACTIVE", Some(expiry));
        assert!(
            map_google_subscription_change(&stored, &purchase).is_none(),
            "non_renewing must ignore the ACTIVE transition"
        );
    }

    #[test]
    fn non_renewing_ignores_grace_pause_cancel_transitions() {
        // GRACE / PAUSED / CANCELED are renewal-flow states. Non-renewing must
        // not emit any replay for them.
        let expiry = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        for google_state in [
            "SUBSCRIPTION_STATE_IN_GRACE_PERIOD",
            "SUBSCRIPTION_STATE_PAUSED",
            "SUBSCRIPTION_STATE_CANCELED",
            "SUBSCRIPTION_STATE_ON_HOLD",
        ] {
            let stored = stored_with_billing("nr-3", "active", "non_renewing", Some(expiry));
            let purchase = sub_state(google_state, Some(expiry));
            assert!(
                map_google_subscription_change(&stored, &purchase).is_none(),
                "non_renewing must ignore state {google_state:?}"
            );
        }
    }

    #[test]
    fn non_renewing_ignores_advancing_expiry_renewal_detection() {
        // Same ACTIVE status but a later expiry would be a renewal for recurring.
        // For non_renewing it must be ignored — the service window is fixed.
        let old_expiry = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let new_expiry = Utc.timestamp_opt(1_700_086_400, 0).unwrap(); // +1d
        let stored = stored_with_billing("nr-4", "active", "non_renewing", Some(old_expiry));
        let purchase = sub_state("SUBSCRIPTION_STATE_ACTIVE", Some(new_expiry));
        assert!(
            map_google_subscription_change(&stored, &purchase).is_none(),
            "non_renewing must not treat advancing expiry as a renewal"
        );
    }

    #[test]
    fn non_renewing_skips_replay_when_already_expired() {
        // Idempotency: once Herald's stored status is already `expired`, a
        // subsequent EXPIRED poll must not re-emit (mirrors the recurring
        // "no no-op churn every cycle" contract).
        let expiry = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let stored = stored_with_billing("nr-5", "expired", "non_renewing", Some(expiry));
        let purchase = sub_state("SUBSCRIPTION_STATE_EXPIRED", Some(expiry));
        assert!(map_google_subscription_change(&stored, &purchase).is_none());
    }

    #[test]
    fn recurring_retains_full_state_mapping_after_filter_added() {
        // the non_renewing branch must not change recurring's behaviour. Same
        // ACTIVE→EXPIRED transition that produced `subscription.expired` before
        let expiry = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let stored = stored_with_billing("rec-1", "active", "recurring", Some(expiry));
        let purchase = sub_state("SUBSCRIPTION_STATE_EXPIRED", Some(expiry));

        let (event_type, payload) = map_google_subscription_change(&stored, &purchase)
            .expect("recurring EXPIRED must still replay under the new filter");
        assert_eq!(event_type, "subscription.expired");
        assert_eq!(payload["heraldStatus"], "expired");

        // And recurring still honours the renewal-detection path.
        let new_expiry = Utc.timestamp_opt(1_700_086_400, 0).unwrap(); // +1d
        let stored_renew = stored_with_billing("rec-2", "active", "recurring", Some(expiry));
        let purchase_renew = sub_state("SUBSCRIPTION_STATE_ACTIVE", Some(new_expiry));
        let (renew_type, _) = map_google_subscription_change(&stored_renew, &purchase_renew)
            .expect("recurring advancing-expiry renewal must still fire");
        assert_eq!(renew_type, "subscription.renewed");
    }
}
