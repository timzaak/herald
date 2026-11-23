use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::Instrument;
use uuid::Uuid;

use herald_core::domain::telemetry::external_http::timed_external_http_span;
use herald_core::domain::webhook_outbound::{
    OutboundEventEnvelope, SafeDnsResolver, WebhookDeliveryConfig, sign_delivery,
};
use herald_core::third::email::EmailService;

/// Result of one outbound-delivery sweep.
#[derive(Debug, Default)]
pub struct DeliveryStats {
    pub claimed: usize,
    pub delivered: usize,
    pub failed: usize,
    pub dropped: usize,
    pub paused: usize,
}

struct ClaimedDelivery {
    delivery_id: Uuid,
    event_id: Uuid,
    subscription_id: Uuid,
    attempt_count: i32,
}

struct LoadedContext {
    realm_id: String,
    endpoint_url: String,
    secret: String,
    subscription_status: String,
    event_type: String,
    payload: serde_json::Value,
    occurred_at: chrono::DateTime<chrono::Utc>,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Failure summary for `last_error`: status code or error category only —
/// never response-body content (it is attacker-controlled text that would
/// be echoed into the admin console).
fn failure_summary(status: Option<reqwest::StatusCode>, err: Option<&reqwest::Error>) -> String {
    if let Some(status) = status {
        return format!("HTTP {}", status.as_u16());
    }
    if let Some(err) = err {
        if err.is_timeout() {
            return "timeout".to_string();
        }
        if err.is_connect() {
            return "connect error".to_string();
        }
        if err.is_redirect() {
            return "redirect rejected".to_string();
        }
        return "request failed".to_string();
    }
    "unknown".to_string()
}

fn truncate_500(s: &str) -> String {
    s.chars().take(500).collect()
}

/// Sweeps `outbound_delivery WHERE status = 'pending'`, delivers each
/// claimed row over HTTP with per-subscription HMAC signing, and runs the
/// auto-pause state machine on sustained failure. The sweep is the only
/// delivery path — the API layer never sends inline.
///
/// Concurrency safety: claims use `FOR UPDATE SKIP LOCKED` plus a 5-minute
/// lease, so multiple workers (or a crashed worker's stale lease) cannot
/// double-send except within the lease-expiry window — an at-least-once
/// contract behavior consumers deduplicate by eventId.
pub struct OutboundDeliveryJob {
    pool: PgPool,
    http: reqwest::Client,
    cfg: WebhookDeliveryConfig,
}

impl OutboundDeliveryJob {
    pub fn new(pool: PgPool, cfg: WebhookDeliveryConfig) -> Self {
        // Dedicated client: no redirects, short timeout, and the SafeDnsResolver
        // re-checks every resolved IP at connect time (registration-time
        // validation alone is TOCTOU-vulnerable to DNS rebinding).
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.http_timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .dns_resolver(Arc::new(SafeDnsResolver))
            .build()
            .expect("failed to build outbound delivery reqwest client");
        Self { pool, http, cfg }
    }

    /// Test seam: deliver through an externally built client. Scenario
    /// tests point it at a loopback wiremock receiver, which the
    /// production-only SafeDnsResolver would (correctly) refuse to dial.
    /// The delivery state machine (claim, sign, backoff, pause) is what
    /// those tests exercise — DNS filtering has its own unit + registration
    /// coverage. Production always constructs via `new`.
    pub fn with_client(pool: PgPool, cfg: WebhookDeliveryConfig, http: reqwest::Client) -> Self {
        Self { pool, http, cfg }
    }

    #[tracing::instrument(
        skip(self),
        fields(job.name = "outbound_delivery")
    )]
    pub async fn run(&self) -> anyhow::Result<DeliveryStats> {
        let mut stats = DeliveryStats::default();

        let realms: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT DISTINCT realm_id FROM outbound_delivery
            WHERE status = 'pending' AND (next_retry_at IS NULL OR next_retry_at <= NOW())
            LIMIT $1
            "#,
        )
        .bind(self.cfg.realms_per_tick)
        .fetch_all(&self.pool)
        .await?;

        let mut claims: Vec<ClaimedDelivery> = Vec::new();
        for realm_id in realms {
            // A single realm's claim error must not abort the sweep: rows
            // claimed by earlier iterations are already leased (5-minute
            // reclaim guard) and would sit frozen until the lease expires.
            let claimed = match self.claim_realm(&realm_id).await {
                Ok(claimed) => claimed,
                Err(error) => {
                    tracing::warn!(
                        realm_id = %realm_id,
                        error = %error,
                        "claim_realm failed; skipping realm for this sweep"
                    );
                    continue;
                }
            };
            claims.extend(claimed);
        }
        stats.claimed = claims.len();

        let semaphore = Arc::new(Semaphore::new(self.cfg.http_concurrency));
        let mut join_set = tokio::task::JoinSet::new();
        for claim in claims {
            let permit = semaphore.clone().acquire_owned().await?;
            join_set.spawn(deliver_claimed(
                self.pool.clone(),
                self.http.clone(),
                self.cfg,
                claim,
                permit,
            ));
        }
        // Keep the permit alive until its task completes; the guard must
        // outlive the request (this no-op bind documents the intent).
        drop(semaphore);

        while let Some(joined) = join_set.join_next().await {
            match joined {
                Ok(Ok(outcome)) => match outcome {
                    DeliveryOutcome::Delivered => stats.delivered += 1,
                    DeliveryOutcome::Failed => stats.failed += 1,
                    DeliveryOutcome::Dropped => stats.dropped += 1,
                    DeliveryOutcome::Paused => {
                        stats.failed += 1;
                        stats.paused += 1;
                    }
                },
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "outbound delivery task failed");
                    stats.failed += 1;
                }
                Err(e) => {
                    tracing::error!(error = %e, "outbound delivery task panicked");
                    stats.failed += 1;
                }
            }
        }

        tracing::info!(
            claimed = stats.claimed,
            delivered = stats.delivered,
            failed = stats.failed,
            dropped = stats.dropped,
            paused = stats.paused,
            "Outbound delivery sweep completed"
        );

        Ok(stats)
    }

    /// Lease-claim up to `batch_per_realm` due pending rows for one realm.
    async fn claim_realm(&self, realm_id: &str) -> anyhow::Result<Vec<ClaimedDelivery>> {
        let rows: Vec<(Uuid, Uuid, Uuid, i32)> = sqlx::query_as(
            r#"
            UPDATE outbound_delivery d SET processing_started_at = NOW()
            WHERE d.id IN (
                SELECT id FROM outbound_delivery
                WHERE realm_id = $1 AND status = 'pending'
                  AND (next_retry_at IS NULL OR next_retry_at <= NOW())
                  AND (processing_started_at IS NULL
                       OR processing_started_at <= NOW() - INTERVAL '5 minutes')
                ORDER BY next_retry_at NULLS FIRST, id
                LIMIT $2
                FOR UPDATE SKIP LOCKED
            )
            RETURNING d.id, d.event_id, d.subscription_id, d.attempt_count
            "#,
        )
        .bind(realm_id)
        .bind(self.cfg.batch_per_realm)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(delivery_id, event_id, subscription_id, attempt_count)| ClaimedDelivery {
                    delivery_id,
                    event_id,
                    subscription_id,
                    attempt_count,
                },
            )
            .collect())
    }
}

enum DeliveryOutcome {
    Delivered,
    Failed,
    Dropped,
    Paused,
}

/// Exponential backoff: `min(base × 2^(attempt-1), max)`, no jitter.
fn backoff_secs(cfg: &WebhookDeliveryConfig, attempt_count: i32) -> i64 {
    let base = cfg.backoff_base_secs;
    let max = cfg.backoff_max_secs;
    let exp = (attempt_count as u32 - 1).min(30);
    base.saturating_mul(1i64 << exp).min(max)
}

async fn deliver_claimed(
    pool: PgPool,
    http: reqwest::Client,
    cfg: WebhookDeliveryConfig,
    claim: ClaimedDelivery,
    permit: OwnedSemaphorePermit,
) -> anyhow::Result<DeliveryOutcome> {
    let _permit = permit;
    let Some(ctx) = load_context(&pool, &claim).await? else {
        // Subscription disappeared between claim and load (delete cascades
        // usually remove the delivery first; a raced row is dropped).
        mark_dropped(&pool, claim.delivery_id).await?;
        return Ok(DeliveryOutcome::Dropped);
    };

    // A paused/disabled subscription delivers nothing; the pause
    // transaction already dropped the bulk of its pending rows — this
    // covers rows claimed in the race window just before the flip.
    if ctx.subscription_status != "active" {
        mark_dropped(&pool, claim.delivery_id).await?;
        return Ok(DeliveryOutcome::Dropped);
    }

    let envelope = OutboundEventEnvelope {
        id: claim.event_id,
        r#type: &ctx.event_type,
        realm_id: &ctx.realm_id,
        created_at: ctx.created_at,
        occurred_at: ctx.occurred_at,
        data: &ctx.payload,
    };
    let body = serde_json::to_vec(&envelope)?;

    let host = url::Url::parse(&ctx.endpoint_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default();
    let timing = timed_external_http_span(&host, "POST");

    let unix_ts = chrono::Utc::now().timestamp();
    let signature = sign_delivery(&ctx.secret, unix_ts, &body);

    let response = http
        .post(&ctx.endpoint_url)
        .header("Content-Type", "application/json")
        .header("X-Herald-Event-Id", claim.event_id.to_string())
        .header("X-Herald-Event-Type", &ctx.event_type)
        .header("X-Herald-Delivery-Id", claim.delivery_id.to_string())
        .header("X-Herald-Timestamp", unix_ts.to_string())
        .header("X-Herald-Signature", signature)
        .body(body)
        .send()
        // `Entered` guards are !Send and this future is spawned; instrument
        // instead so the span still covers the round-trip.
        .instrument(timing.span().clone())
        .await;

    // Success is judged on the status line alone — the response body is
    // never read (bounded memory, no attacker-controlled echo).
    let outcome = match response {
        Ok(resp) if resp.status().is_success() => {
            record_success(&pool, claim.delivery_id, claim.subscription_id).await?;
            DeliveryOutcome::Delivered
        }
        Ok(resp) => {
            let error = truncate_500(&failure_summary(Some(resp.status()), None));
            record_failure(&pool, &claim, &error, cfg).await?
        }
        Err(e) => {
            let error = truncate_500(&failure_summary(None, Some(&e)));
            record_failure(&pool, &claim, &error, cfg).await?
        }
    };
    Ok(outcome)
}

/// Loads the event payload and the subscription row for a claim. `None`
/// when the subscription row is gone.
async fn load_context(
    pool: &PgPool,
    claim: &ClaimedDelivery,
) -> anyhow::Result<Option<LoadedContext>> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            String,
            serde_json::Value,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
        ),
    >(
        r#"
        SELECT e.realm_id, s.endpoint_url, s.secret, s.status,
               e.event_type, e.payload, e.occurred_at, e.created_at
        FROM outbound_event e
        JOIN webhook_subscription s ON s.id = $1
        WHERE e.id = $2
        "#,
    )
    .bind(claim.subscription_id)
    .bind(claim.event_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(
            realm_id,
            endpoint_url,
            secret,
            subscription_status,
            event_type,
            payload,
            occurred_at,
            created_at,
        )| LoadedContext {
            realm_id,
            endpoint_url,
            secret,
            subscription_status,
            event_type,
            payload,
            occurred_at,
            created_at,
        },
    ))
}

async fn mark_dropped(pool: &PgPool, delivery_id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE outbound_delivery SET status = 'dropped', processing_started_at = NULL WHERE id = $1",
    )
    .bind(delivery_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn record_success(
    pool: &PgPool,
    delivery_id: Uuid,
    subscription_id: Uuid,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    // Status guards keep a concurrent in-flight outcome from rewriting a
    // pause result: a dropped row must not resurrect as delivered, and a
    // paused subscription keeps its failure counter (the pause reason
    // surface).
    sqlx::query(
        r#"
        UPDATE outbound_delivery
        SET status = 'delivered', delivered_at = NOW(), processing_started_at = NULL
        WHERE id = $1 AND status = 'pending'
        "#,
    )
    .bind(delivery_id)
    .execute(&mut *tx)
    .await?;
    // Any success resets the auto-pause progress — a recovered endpoint
    // starts from a clean slate.
    sqlx::query(
        r#"
        UPDATE webhook_subscription SET consecutive_failures = 0, updated_at = NOW()
        WHERE id = $1 AND status = 'active'
        "#,
    )
    .bind(subscription_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Failure accounting plus the conditional auto-pause flip. The flip's
/// `WHERE status = 'active' AND consecutive_failures >= threshold` guard
/// makes it effective exactly once under concurrency; when it wins, every
/// remaining pending delivery of the subscription is dropped in the same
/// transaction (the persisted "no delivery, no replay after resume"
/// semantics).
async fn record_failure(
    pool: &PgPool,
    claim: &ClaimedDelivery,
    error: &str,
    cfg: WebhookDeliveryConfig,
) -> anyhow::Result<DeliveryOutcome> {
    let next_attempt = claim.attempt_count + 1;
    let backoff_secs = backoff_secs(&cfg, next_attempt);

    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"
        UPDATE outbound_delivery
        SET attempt_count = $2,
            next_retry_at = NOW() + ($3 * INTERVAL '1 second'),
            last_attempt_at = NOW(),
            last_error = $4,
            processing_started_at = NULL
        WHERE id = $1 AND status = 'pending'
        "#,
    )
    .bind(claim.delivery_id)
    .bind(next_attempt)
    .bind(backoff_secs)
    .bind(error)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE webhook_subscription
        SET consecutive_failures = consecutive_failures + 1,
            last_error = $2,
            updated_at = NOW()
        WHERE id = $1 AND status = 'active'
        "#,
    )
    .bind(claim.subscription_id)
    .bind(error)
    .execute(&mut *tx)
    .await?;

    let paused = sqlx::query(
        r#"
        UPDATE webhook_subscription
        SET status = 'paused', paused_at = NOW(), updated_at = NOW()
        WHERE id = $1 AND status = 'active' AND consecutive_failures >= $2
        "#,
    )
    .bind(claim.subscription_id)
    .bind(cfg.pause_threshold)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;

    let mut outcome = DeliveryOutcome::Failed;
    if paused {
        // Drop every still-pending delivery of this subscription —
        // including this one's retry.
        sqlx::query(
            r#"
            UPDATE outbound_delivery
            SET status = 'dropped', processing_started_at = NULL
            WHERE subscription_id = $1 AND status = 'pending'
            "#,
        )
        .bind(claim.subscription_id)
        .execute(&mut *tx)
        .await?;
        outcome = DeliveryOutcome::Paused;
    }
    tx.commit().await?;

    if paused {
        notify_auto_paused(pool, claim, error, next_attempt).await;
    }
    Ok(outcome)
}

/// Best-effort post-commit pause notification: system audit row + email to
/// the realm's builtin realm-admin holders when the realm has a configured
/// email channel (unconfigured realms skip silently — the admin console
/// status surface is the always-on channel).
async fn notify_auto_paused(pool: &PgPool, claim: &ClaimedDelivery, error: &str, failures: i32) {
    let (realm_id, endpoint_url) = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT realm_id, endpoint_url FROM webhook_subscription WHERE id = $1",
    )
    .bind(claim.subscription_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    let Some(realm_id) = realm_id else {
        return;
    };

    let details = serde_json::json!({
        "consecutiveFailures": failures,
        "lastError": error,
        "endpointUrl": endpoint_url,
    });
    if let Err(e) = sqlx::query(
        r#"
        INSERT INTO audit_events
            (realm_id, category, action, actor_id, actor_type, target_type, target_id, result, details)
        VALUES ($1, 'realm_management', 'webhook_subscription.auto_paused', 'system', 'system',
                'realm', $2, 'success', $3)
        "#,
    )
    .bind(&realm_id)
    .bind(claim.subscription_id.to_string())
    .bind(&details)
    .execute(pool)
    .await
    {
        tracing::warn!(error = %e, "failed to record webhook auto-pause audit");
    }

    let recipients: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT DISTINCT a.email
        FROM user_roles ur
        JOIN roles ro ON ro.id = ur.role_id AND ro.name = 'realm-admin' AND ro.is_builtin = true
        JOIN account a ON a.id = ur.user_id AND a.status = 1
        WHERE ur.realm_id = $1
        LIMIT 10
        "#,
    )
    .bind(&realm_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for to in recipients {
        let subject = "Webhook subscription auto-paused";
        let text = format!(
            "Your webhook subscription stopped responding and has been automatically paused after \
             {failures} consecutive failed delivery attempts.\n\nLast error: {error}\n\nThe \
             subscription will not receive further events while paused, and paused events are not \
             replayed after you resume it. Resume or edit it in the admin console under Webhooks."
        );
        let html = format!(
            "<p>Your webhook subscription stopped responding and has been automatically paused \
             after <strong>{failures}</strong> consecutive failed delivery attempts.</p>\
             <p>Last error: {error}</p>\
             <p>The subscription will not receive further events while paused, and paused events \
             are not replayed after you resume it. Resume or edit it in the admin console under \
             Webhooks.</p>"
        );
        // Silently no-ops when the realm has no email channel configured.
        if let Err(e) = EmailService::send_email(pool, &realm_id, &to, subject, &text, &html).await
        {
            tracing::warn!(error = %e, "failed to send webhook auto-pause notification email");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// User Story: US-WEP-003 (retry semantics — exponential backoff).
    /// Covers: 60s base, doubling per attempt, 8h cap.
    ///
    /// WHY: the backoff curve is the retry-storm brake and a documented
    /// consumer-facing behavior ("retries for up to ~2.5 days before the
    /// subscription pauses"); an accidental base change or an off-by-one in
    /// the exponent silently shifts the whole schedule (e.g. attempt 1
    /// already waiting 2 minutes, or the cap never applying).
    #[test]
    fn backoff_sequence_is_pinned() {
        let cfg = WebhookDeliveryConfig::default();
        // 60 × 2^(n-1) seconds, capped at 28800 (8h). The 10th attempt's raw
        // value (30720) already exceeds the cap, so the tail flattens.
        let expected = [
            60,     // 1st failure
            120,    // 2nd
            240,    // 3rd
            480,    // 4th
            960,    // 5th
            1_920,  // 6th
            3_840,  // 7th
            7_680,  // 8th
            15_360, // 9th — cumulative ≈ 8.5h
            28_800, // 10th onwards: cap
            28_800, 28_800, 28_800, 28_800, 28_800,
            28_800, // 16th (the pause threshold attempt)
            28_800, // 17th — would only run for non-paused retry loops
        ];
        for (idx, want) in expected.iter().enumerate() {
            let attempt = (idx + 1) as i32;
            assert_eq!(backoff_secs(&cfg, attempt), *want, "attempt {attempt}");
        }
        // The cap holds at large attempts and never overflows.
        assert_eq!(backoff_secs(&cfg, 30), 28_800);
        assert_eq!(backoff_secs(&cfg, 1000), 28_800);
    }

    /// User Story: US-WEP-003 (runtime parameters are the configured
    /// defaults).
    /// Covers: DEC-006 numeric set (sweep 10s, 25×4/tick, 16 concurrency,
    /// 10s timeout, 60s/8h backoff, threshold 16).
    ///
    /// WHY: these numbers were chosen as a coherent anti-storm bundle (a
    /// single endpoint's worst-case throughput ≤ 0.4 deliveries/sec, pause
    /// after ≈2.5 days of sustained failure); changing one value without
    /// revisiting the bundle breaks that reasoning silently.
    #[test]
    fn delivery_runtime_defaults_are_pinned() {
        let cfg = WebhookDeliveryConfig::default();
        assert_eq!(cfg.sweep_interval_secs, 10);
        assert_eq!(cfg.realms_per_tick, 25);
        assert_eq!(cfg.batch_per_realm, 4);
        assert_eq!(cfg.http_concurrency, 16);
        assert_eq!(cfg.http_timeout_secs, 10);
        assert_eq!(cfg.backoff_base_secs, 60);
        assert_eq!(cfg.backoff_max_secs, 28_800);
        assert_eq!(cfg.pause_threshold, 16);
    }

    /// User Story: US-WEP-003 (failure summaries must not echo response
    /// bodies).
    /// Covers: HTTP status → "HTTP xxx"; error kinds → category words.
    ///
    /// WHY: `last_error` is rendered in the admin console; a leaked
    /// attacker-controlled body would make every subscriber endpoint an XSS
    /// / content-injection vector into the console.
    #[test]
    fn failure_summary_is_category_only() {
        let status = reqwest::StatusCode::INTERNAL_SERVER_ERROR;
        assert_eq!(failure_summary(Some(status), None), "HTTP 500");
        assert_eq!(failure_summary(None, None), "unknown");
        let long = "x".repeat(600);
        assert_eq!(truncate_500(&long).len(), 500);
    }
}
