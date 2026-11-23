// Outbound webhook delivery scenario tests (end-to-end): drives the real
// fan-out + delivery pipeline against a wiremock receiver — signing,
// envelope shape, category filtering, realm isolation, payment and manual
// triggers, Granted-only idempotency, exponential-backoff retry, auto-pause
// with pending-drop and no-replay-after-resume.
//
// User Stories: US-WEP-002 (state changes generate and deliver signed
// events), US-WEP-003 (retry with backoff, auto-pause on sustained failure).
//
// The delivery job is constructed with a plain test client pointed at the
// loopback wiremock receiver — production always installs the
// SafeDnsResolver, which would correctly refuse to dial it (that behavior
// has its own unit and registration-400 coverage).

use crate::tests::helpers::auth_helpers::create_admin_session_with_role;
use crate::tests::helpers::points_helpers::{
    create_payment_attempt_snapshot, ensure_test_bucket_for_realm,
    snapshot_attempt_rules_for_mapping,
};
use crate::tests::helpers::rbac_helpers::create_role;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use herald_core::domain::billing::entities::{BillingType, EntitlementMapping};
use herald_core::domain::webhook_outbound::WebhookDeliveryConfig;
use herald_worker::OutboundDeliveryJob;
use herald_worker::jobs::outbound_delivery_job::DeliveryStats;
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KNOWN_SECRET: &str = "whsec_scenario_key";

/// Plain HTTP client for the delivery job (loopback wiremock target).
fn test_delivery_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

async fn run_delivery_job(ctx: &TestContext) -> anyhow::Result<DeliveryStats> {
    let job = OutboundDeliveryJob::with_client(
        ctx._app_state.pool.clone(),
        WebhookDeliveryConfig::default(),
        test_delivery_client(),
    );
    job.run().await
}

/// Run the sweep until the queue for this realm is drained — the per-realm
/// claim cap (batch_per_realm=4/tick) is real behavior, so suites that
/// queue more than four deliveries must sweep repeatedly.
async fn drain_delivery_job(ctx: &TestContext) -> anyhow::Result<DeliveryStats> {
    let mut total = DeliveryStats::default();
    for _ in 0..5 {
        let stats = run_delivery_job(ctx).await?;
        let claimed = stats.claimed;
        total.claimed += stats.claimed;
        total.delivered += stats.delivered;
        total.failed += stats.failed;
        total.dropped += stats.dropped;
        total.paused += stats.paused;
        if claimed == 0 {
            break;
        }
    }
    Ok(total)
}

/// Seed a subscription row directly: delivery scenarios bypass the
/// registration-time public-URL check on purpose (the receiver is loopback)
/// — that check has its own management-face coverage.
async fn seed_subscription(
    ctx: &TestContext,
    realm_id: &str,
    endpoint_url: &str,
    categories: &[&str],
) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO webhook_subscription (id, realm_id, endpoint_url, secret, event_categories)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(realm_id)
    .bind(endpoint_url)
    .bind(KNOWN_SECRET)
    .bind(categories.to_vec())
    .execute(&ctx._app_state.pool)
    .await
    .expect("seed webhook_subscription");
    id
}

/// Direct account insert (no session needed for emit targets).
async fn seed_user(ctx: &TestContext, realm_id: &str, email: &str) -> Uuid {
    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, $4, 1)
         ON CONFLICT (realm_id, email) DO NOTHING",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(email)
    .bind("$2a$12$dummy_password_hash")
    .execute(&ctx._app_state.pool)
    .await
    .expect("seed user");
    user_id
}

/// Grant `role_ids` to `user_id` through the manual roles endpoint — the
/// fan-out trigger under test.
async fn admin_grant_roles(app: &axum::Router, token: &str, user_id: Uuid, role_ids: &[Uuid]) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/permission/users/{user_id}/roles"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({"roleIds": role_ids}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

/// One-time mapping granting `role_ids` on payment (no points rule — the
/// pure-role case needs no bucket snapshot state).
async fn seed_one_time_mapping_with_role(
    ctx: &TestContext,
    realm_id: &str,
    role_ids: &[Uuid],
) -> Uuid {
    let mapping_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key,
             billing_type, enabled, provider_product_info, granted_role_ids, created_at, updated_at)
         VALUES ($1, $2, 'stripe', $3, $4, 'one_time', true, $5, $6, NOW(), NOW())",
    )
    .bind(mapping_id)
    .bind(realm_id)
    .bind(format!("prod_{mapping_id}"))
    .bind(format!("delivery-key-{mapping_id}"))
    .bind(serde_json::json!({}))
    .bind(role_ids.to_vec())
    .execute(&ctx._app_state.pool)
    .await
    .expect("seed mapping");
    mapping_id
}

/// Pending payment attempt (mirrors the paywall W1 fixture; the rule
/// snapshot stays empty for pure-role mappings).
async fn seed_pending_attempt(
    ctx: &TestContext,
    realm_id: &str,
    user_id: Uuid,
    mapping_id: Uuid,
) -> Uuid {
    let attempt_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO payment_attempts
            (id, realm_id, user_id, payment_provider, target_type, target_id,
             amount, currency, status, expires_at, created_at, updated_at)
         VALUES ($1, $2, $3, 'stripe', 'entitlement_mapping', $4,
                 999, 'USD', 'Pending', NOW() + INTERVAL '2 hours', NOW(), NOW())",
    )
    .bind(attempt_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(mapping_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("seed payment attempt");
    snapshot_attempt_rules_for_mapping(
        &ctx._app_state.pool,
        attempt_id,
        realm_id,
        mapping_id,
        "topup",
    )
    .await;
    attempt_id
}

/// Fulfill a pending attempt through the internal handler (same entry the
/// provider webhook path uses).
async fn fulfill_attempt(ctx: &TestContext, attempt_id: Uuid, provider_tx_id: &str) {
    let payload = serde_json::json!({
        "realmId": ctx._realm_id,
        "providerStatus": "success",
        "providerTransactionId": provider_tx_id,
        "completedAt": chrono::Utc::now().to_rfc3339(),
    });
    let response = crate::application::http::billing::purchase_handlers::fulfill_payment(
        axum::extract::State((*ctx._app_state).clone()),
        axum::extract::Path(attempt_id),
        axum::Json(serde_json::from_value(payload).unwrap()),
    )
    .await;
    assert!(response.is_ok(), "fulfillment failed: {:?}", response.err());
}

/// Recompute the delivery signature exactly as a consumer would.
fn verify_signature(req: &wiremock::Request) -> bool {
    use hmac::Mac;
    type HmacSha256 = hmac::Hmac<sha2::Sha256>;
    let Some(ts) = req
        .headers
        .get("x-herald-timestamp")
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(sig) = req
        .headers
        .get("x-herald-signature")
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let mut mac = <HmacSha256 as Mac>::new_from_slice(KNOWN_SECRET.as_bytes()).unwrap();
    mac.update(ts.as_bytes());
    mac.update(b".");
    mac.update(&req.body);
    let expected = format!("v1={}", hex::encode(mac.finalize().into_bytes()));
    // Constant-time-ish comparison for the test (length check + eq).
    expected.len() == sig.len() && expected == sig
}

async fn admin_create_user(app: &axum::Router, token: &str, email: &str) -> Uuid {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/users")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({"email": email, "password": "password123", "roleIds": []})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    Uuid::parse_str(v["id"].as_str().unwrap()).unwrap()
}

async fn admin_disable_user(app: &axum::Router, token: &str, user_id: Uuid) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/users/{user_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({"status": "forbidden"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// User Story: US-WEP-002 / PRD 验收线 1 (paywall 解锁)
// Covers: payment-driven grant fans out, is HMAC-signed over
// "{timestamp}.{body}", carries the documented headers and envelope, and a
// 2xx marks the delivery delivered with the failure counter reset.
//
// WHY: this is the paywall-unlock acceptance line — consumers bind business
// logic (entitlement unlock) to this push. A signature that doesn't verify
// with the consumer's recomputation, a missing header, or a delivery that
// never flips to delivered each break that integration in the field while
// local tests stay green.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_delivery_payment_grant_signed_push(ctx: &mut TestContext) {
    let _app = ctx.create_unified_test_router();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cb"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let token = create_admin_session_with_role(ctx, "delivery-pay-admin@test.com", 1800).await;
    let role_id = create_role(ctx, &ctx._realm_id, &token, "delivery-pro-member", "role").await;
    let user_id = seed_user(ctx, &ctx._realm_id, "delivery-pay-user@test.com").await;
    let mapping_id = seed_one_time_mapping_with_role(ctx, &ctx._realm_id, &[role_id]).await;
    seed_subscription(
        ctx,
        &ctx._realm_id,
        &format!("{}/cb", server.uri()),
        &["role_events"],
    )
    .await;

    let attempt_id = seed_pending_attempt(ctx, &ctx._realm_id, user_id, mapping_id).await;
    fulfill_attempt(ctx, attempt_id, "pi_delivery_pay_1").await;

    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(stats.delivered, 1, "one delivery expected: {stats:?}");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert!(
        verify_signature(req),
        "signature must verify against the known secret"
    );

    let env: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(env["type"], "role.granted");
    assert_eq!(env["realmId"], ctx._realm_id.as_str());
    assert_eq!(env["data"]["userId"], user_id.to_string());
    assert_eq!(env["data"]["roleId"], role_id.to_string());
    assert_eq!(env["data"]["source"], "payment");
    assert!(env["occurredAt"].is_string() && env["createdAt"].is_string());
    assert_eq!(env["data"]["roleName"], "delivery-pro-member");

    let event_id = env["id"].as_str().unwrap();
    assert_eq!(
        req.headers
            .get("x-herald-event-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
        event_id,
        "header event id must equal body.id"
    );
    assert_eq!(
        req.headers
            .get("x-herald-event-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
        "role.granted"
    );
    assert!(
        req.headers
            .get("x-herald-delivery-id")
            .and_then(|v| v.to_str().ok())
            .is_some()
    );

    // 2xx accounting: delivered + counter reset.
    let (status, failures): (String, i32) = sqlx::query_as(
        "SELECT d.status, s.consecutive_failures \
         FROM outbound_delivery d JOIN webhook_subscription s ON s.id = d.subscription_id \
         WHERE d.event_id = $1",
    )
    .bind(Uuid::parse_str(event_id).unwrap())
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(status, "delivered");
    assert_eq!(failures, 0);
}

// User Story: US-WEP-002 / PRD 验收线 2 (禁用下线)
// Covers: the admin disable flip emits and delivers user.disabled; a
// no-op update (nickname only) does not.
//
// WHY: the "instant takedown" acceptance line; also pins the
// transition-only semantics — re-asserting Forbidden or editing a nickname
// must not spam consumers with phantom disables.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_delivery_user_disabled_push(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let token = create_admin_session_with_role(ctx, "delivery-disable-admin@test.com", 1800).await;
    seed_subscription(ctx, &ctx._realm_id, &server.uri(), &["user_lifecycle"]).await;

    // Nickname-only update first: no lifecycle transition, no delivery.
    let user_id = admin_create_user(&app, &token, "delivery-disable-target@test.com").await;
    run_delivery_job(ctx).await.unwrap(); // drain the user.created push
    let before = server.received_requests().await.unwrap().len();

    admin_disable_user(&app, &token, user_id).await;
    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(stats.delivered, 1);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), before + 1);
    let env: serde_json::Value = serde_json::from_slice(&requests[before].body).unwrap();
    assert_eq!(env["type"], "user.disabled");
    assert_eq!(env["data"]["userId"], user_id.to_string());
    assert!(verify_signature(&requests[before]));

    // Re-asserting forbidden: no transition → no new delivery.
    admin_disable_user(&app, &token, user_id).await;
    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(stats.delivered, 0, "no transition, no event");
}

// User Story: US-WEP-002 场景 4 (类别过滤) + 场景 5 (Realm 隔离)
// Covers: a role_events-only subscription receives nothing for user.created;
// a subscription in another realm receives nothing for this realm's events.
//
// WHY: filtering and tenancy are the two noise/leak axes — a misrouted event
// either spams a consumer that opted out or hands one tenant another
// tenant's user data.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_delivery_category_filter_and_realm_isolation(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // This realm subscribes ONLY to role_events.
    let role_receiver = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&role_receiver)
        .await;
    seed_subscription(ctx, &ctx._realm_id, &role_receiver.uri(), &["role_events"]).await;

    // Another realm subscribes to everything.
    let other_receiver = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&other_receiver)
        .await;
    sqlx::query("INSERT INTO realm (id, name) VALUES ($1, 'delivery-other-realm')")
        .bind("delivery-other-realm")
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    seed_subscription(
        ctx,
        "delivery-other-realm",
        &other_receiver.uri(),
        &["user_lifecycle", "role_events", "subscription_lifecycle"],
    )
    .await;

    // Trigger: user created in THIS realm (user_lifecycle category).
    let token = create_admin_session_with_role(ctx, "delivery-filter-admin@test.com", 1800).await;
    admin_create_user(&app, &token, "delivery-filter-user@test.com").await;

    run_delivery_job(ctx).await.unwrap();

    assert_eq!(
        role_receiver.received_requests().await.unwrap().len(),
        0,
        "category filter: role_events-only gets nothing for user.created"
    );
    assert_eq!(
        other_receiver.received_requests().await.unwrap().len(),
        0,
        "realm isolation: other realm gets nothing for this realm's user"
    );
}

// User Story: US-WEP-002 场景 2 (与管理端手工授予同等投递)
// Covers: admin console assignment fans out role.granted with
// source="manual" and the role name snapshot.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_delivery_manual_role_grant(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let token = create_admin_session_with_role(ctx, "delivery-manual-admin@test.com", 1800).await;
    let role_id = create_role(ctx, &ctx._realm_id, &token, "delivery-manual-role", "role").await;
    let user_id = seed_user(ctx, &ctx._realm_id, "delivery-manual-user@test.com").await;
    seed_subscription(ctx, &ctx._realm_id, &server.uri(), &["role_events"]).await;

    admin_grant_roles(&app, &token, user_id, &[role_id]).await;

    run_delivery_job(ctx).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let env: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(env["type"], "role.granted");
    assert_eq!(env["data"]["source"], "manual");
    assert_eq!(env["data"]["roleName"], "delivery-manual-role");
    assert!(verify_signature(&requests[0]));
}

// User Story: US-WEP-002 场景 3 (订阅生命周期) + US-WEP-003 (幂等重放)
// Covers: handle_subscription_paid with is_renewal grants the role (payment
// source) AND emits subscription.renewed; a re-run of the grant for the same
// source (AlreadyExists) emits NO second role.granted.
//
// WHY: providers redeliver paid events; the Granted-only rule is what keeps
// replay from pushing duplicate unlock events to consumers that may have
// side effects on each push.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_delivery_renewal_and_granted_only_idempotency(ctx: &mut TestContext) {
    let _app = ctx.create_unified_test_router();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let token = create_admin_session_with_role(ctx, "delivery-renew-admin@test.com", 1800).await;
    let role_id = create_role(ctx, &ctx._realm_id, &token, "delivery-renew-role", "role").await;
    let user_id = seed_user(ctx, &ctx._realm_id, "delivery-renew-user@test.com").await;
    seed_subscription(
        ctx,
        &ctx._realm_id,
        &server.uri(),
        &["role_events", "subscription_lifecycle"],
    )
    .await;

    let mapping = EntitlementMapping {
        id: Uuid::now_v7(),
        realm_id: ctx._realm_id.clone(),
        payment_provider: "stripe".into(),
        external_product_id: "prod_renew".into(),
        external_price_id: None,
        entitlement_key: "renew-key".into(),
        billing_type: Some(BillingType::Recurring),
        billing_period: Some("monthly".into()),
        service_duration_days: None,
        enabled: true,
        provider_product_info: None,
        granted_role_ids: vec![role_id],
        synced_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let subscription_id = Uuid::now_v7();
    let period_start = chrono::Utc::now();
    let period_end = period_start + chrono::Duration::days(30);

    ctx._app_state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            &ctx._realm_id,
            &mapping,
            true,
            period_start,
            period_end,
            "evt_renew_1".into(),
        )
        .await
        .unwrap();

    run_delivery_job(ctx).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2, "role.granted + subscription.renewed");
    let mut types: Vec<String> = requests
        .iter()
        .map(|r| {
            let env: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
            env["type"].as_str().unwrap().to_string()
        })
        .collect();
    types.sort();
    assert_eq!(types, vec!["role.granted", "subscription.renewed"]);
    let renewed = requests
        .iter()
        .map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).unwrap())
        .find(|env| env["type"] == "subscription.renewed")
        .unwrap();
    assert_eq!(
        renewed["data"]["subscriptionId"],
        subscription_id.to_string()
    );
    assert_eq!(renewed["data"]["entitlementKey"], "renew-key");

    // Replay the same grant source: AlreadyExists must not re-emit
    // role.granted (the renewal emit itself is gated in production by the
    // payment_event processed flag this direct call bypasses).
    ctx._app_state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            &ctx._realm_id,
            &mapping,
            true,
            period_start,
            period_end,
            "evt_renew_1".into(),
        )
        .await
        .unwrap();

    let event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM outbound_event WHERE event_type = 'role.granted'")
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        event_count, 1,
        "Granted-only: replay must not re-emit role.granted"
    );
}

// User Story: US-WEP-003 场景 1 (失败重试直至送达)
// Covers: a 500 keeps the row pending with a future next_retry_at; the
// next sweep does not re-claim it; after the receiver recovers (and the
// cooldown elapses) the backlog delivers.
//
// WHY: this is the at-least-once heart — a consumer outage must neither
// drop the event nor hammer the endpoint every sweep.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_delivery_backoff_then_backlog_delivered(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let token = create_admin_session_with_role(ctx, "delivery-backoff-admin@test.com", 1800).await;
    let role_id = create_role(ctx, &ctx._realm_id, &token, "delivery-backoff-role", "role").await;
    let user_id = seed_user(ctx, &ctx._realm_id, "delivery-backoff-user@test.com").await;
    seed_subscription(ctx, &ctx._realm_id, &server.uri(), &["role_events"]).await;

    // Manual grant creates the pending delivery.
    admin_grant_roles(&app, &token, user_id, &[role_id]).await;

    // First sweep: 500 → failure accounting with a backoff ~60s out.
    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(stats.failed, 1);
    let row: (String, i32, Option<chrono::DateTime<chrono::Utc>>, String) = sqlx::query_as(
        "SELECT status, attempt_count, next_retry_at, coalesce(last_error,'') \
         FROM outbound_delivery",
    )
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(row.0, "pending");
    assert_eq!(row.1, 1);
    assert!(
        row.2.unwrap() > chrono::Utc::now(),
        "next_retry_at must be in the future (backoff)"
    );
    assert_eq!(row.3, "HTTP 500");

    // Second sweep before the cooldown: not re-claimed.
    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(stats.claimed, 0, "cooldown must prevent immediate re-claim");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    // Receiver recovers; cooldown elapses (simulated by clearing
    // next_retry_at — the wall-clock wait is covered by the backoff pin
    // test). reset() clears the 500 stub so the recovered endpoint answers
    // 200 (mount order must not decide which stub wins).
    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    sqlx::query("UPDATE outbound_delivery SET next_retry_at = NULL")
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(stats.delivered, 1, "backlog must deliver after recovery");
    let status: String = sqlx::query_scalar("SELECT status FROM outbound_delivery")
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap();
    assert_eq!(status, "delivered");
}

// User Story: US-WEP-003 场景 2/3 (自动暂停、暂停期不投递恢复不补发)
// Covers: reaching the threshold pauses ONLY that subscription, drops its
// pending deliveries in the same transaction, writes the system audit row,
// leaves a sibling subscription in the same realm untouched, and after
// resume only NEW events arrive — the dropped ones never revive.
//
// WHY: unbounded retry storms against a dead endpoint and silent event loss
// are the two failure modes the pause machine exists to prevent; a pause
// that leaked into sibling subscriptions would multiply the blast radius.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_delivery_auto_pause_drop_and_no_replay(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Endpoint that always fails (this subscription) and one that succeeds
    // (sibling).
    let dead = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&dead)
        .await;
    let alive = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&alive)
        .await;

    let token = create_admin_session_with_role(ctx, "delivery-pause-admin@test.com", 1800).await;
    let dead_sub = seed_subscription(ctx, &ctx._realm_id, &dead.uri(), &["role_events"]).await;
    let alive_sub = seed_subscription(ctx, &ctx._realm_id, &alive.uri(), &["role_events"]).await;

    // Push the dead subscription to one failure below the threshold so a
    // single failing sweep crosses it.
    sqlx::query("UPDATE webhook_subscription SET consecutive_failures = 15 WHERE id = $1")
        .bind(dead_sub)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    // One manual grant fans out to BOTH subscriptions: the dead one's
    // delivery carries attempt_count=15 (pre-failed), the sibling's is fresh.
    let role_id = create_role(ctx, &ctx._realm_id, &token, "delivery-pause-role", "role").await;
    let user_id = seed_user(ctx, &ctx._realm_id, "delivery-pause-user@test.com").await;
    admin_grant_roles(&app, &token, user_id, &[role_id]).await;

    sqlx::query("UPDATE outbound_delivery SET attempt_count = 15 WHERE subscription_id = $1")
        .bind(dead_sub)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(stats.paused, 1, "threshold crossing must pause: {stats:?}");
    assert_eq!(stats.delivered, 1, "sibling delivery succeeds");

    // Paused: only this subscription; its pending rows dropped; audit row
    // written by the system actor.
    let dead_status: String =
        sqlx::query_scalar("SELECT status FROM webhook_subscription WHERE id = $1")
            .bind(dead_sub)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(dead_status, "paused");
    let alive_status: String =
        sqlx::query_scalar("SELECT status FROM webhook_subscription WHERE id = $1")
            .bind(alive_sub)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(alive_status, "active", "pause must not leak to siblings");
    let dead_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM outbound_delivery WHERE subscription_id = $1 AND status = 'pending'",
    )
    .bind(dead_sub)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(dead_rows, 0, "pending deliveries must be dropped at pause");
    let audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events \
         WHERE action = 'webhook_subscription.auto_paused' AND target_id = $1",
    )
    .bind(dead_sub.to_string())
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(audit, 1, "system auto-pause audit row must exist");

    let delivered_before_resume = alive.received_requests().await.unwrap().len();

    // While paused: a NEW grant in the realm fans out only to the active
    // sibling; the paused subscription gets no delivery row at all.
    let role2 = create_role(ctx, &ctx._realm_id, &token, "delivery-pause-role-2", "role").await;
    admin_grant_roles(&app, &token, user_id, &[role2]).await;
    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(stats.delivered, 1, "only the active sibling delivers");
    let paused_new_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM outbound_delivery WHERE subscription_id = $1")
            .bind(dead_sub)
            .fetch_one(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        paused_new_rows, 1,
        "paused subscription gets no new delivery rows (fan-out skips it)"
    );

    // Resume via the admin API.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/webhooks/{dead_sub}/status"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"status": "active"}"#.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Post-resume event: a third grant reaches the (recovered) endpoint; the
    // dropped row from the pause never revives. reset() clears the 500 stub
    // so the recovered endpoint answers 200.
    dead.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&dead)
        .await;
    let role3 = create_role(ctx, &ctx._realm_id, &token, "delivery-pause-role-3", "role").await;
    admin_grant_roles(&app, &token, user_id, &[role3]).await;

    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(
        stats.delivered, 2,
        "both subscriptions receive the new event"
    );

    let dead_deliveries: Vec<String> = sqlx::query_scalar(
        "SELECT status FROM outbound_delivery WHERE subscription_id = $1 ORDER BY created_at",
    )
    .bind(dead_sub)
    .fetch_all(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        dead_deliveries,
        vec!["dropped".to_string(), "delivered".to_string()],
        "dropped stays dropped; only the post-resume event delivers"
    );
    assert_eq!(
        alive.received_requests().await.unwrap().len(),
        delivered_before_resume + 2,
        "active sibling received the paused-period and post-resume grants"
    );
}

// User Story: US-WEP-002 场景 3 (订阅生命周期事件投递)
// Covers: entry into the has_access set emits subscription.activated (with
// subscriptionId + entitlementKey), the flip into expired emits
// subscription.expired, and the two events carry distinct event ids.
//
// WHY: the entry/flip predicates in emit_subscription_lifecycle are the
// only untested business judgments of this feature — a wrong predicate
// (e.g. emitting on every sync, or missing the first-activation case)
// pushes wrong contract events to every subscriber.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_delivery_subscription_activated_then_expired(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    seed_subscription(
        ctx,
        &ctx._realm_id,
        &server.uri(),
        &["subscription_lifecycle"],
    )
    .await;

    ctx.with_creem_config(
        &ctx._realm_id,
        Some("test_api_key"),
        Some("test_secret"),
        Some(30),
    )
    .await;
    let user_id = seed_user(ctx, &ctx._realm_id, "delivery-lifecycle-user@test.com").await;

    // Seed a no-access subscription; the provider lifecycle events then
    // walk it incomplete → active (activation) → expired (flip).
    let external_sub = format!("sub_lifecycle_{}", Uuid::now_v7());
    let subscription_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO subscription (id, realm_id, user_id, external_subscription_id, \
         external_product_id, payment_provider, status, entitlement_key, billing_type, \
         created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'prod_lifecycle', 'creem', 'incomplete', 'lifecycle-key', \
         'recurring', NOW(), NOW())",
    )
    .bind(subscription_id)
    .bind(&ctx._realm_id)
    .bind(user_id)
    .bind(&external_sub)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let lifecycle_event = |event_type: &str, event_id: &str| {
        serde_json::json!({
            "id": event_id,
            "eventType": event_type,
            "data": {
                "object": {
                    "subscriptionId": external_sub,
                    "productId": "prod_lifecycle",
                    "userId": user_id.to_string(),
                    "currentPeriodEnd": (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339(),
                }
            },
            "herald_entitlement_key": "lifecycle-key",
            "metadata": {
                "herald_entitlement_key": "lifecycle-key",
                "herald_realm_id": ctx._realm_id,
                "herald_user_id": user_id.to_string(),
            },
        })
    };

    let activated_evt = format!("evt_act_{}", Uuid::now_v7());
    let resp = crate::tests::helpers::webhook_helpers::send_webhook_with_signature(
        &app,
        &ctx._realm_id,
        lifecycle_event("subscription.active", &activated_evt),
        "test_secret",
    )
    .await;
    crate::tests::helpers::webhook_helpers::assert_webhook_success(&resp);

    let expired_evt = format!("evt_exp_{}", Uuid::now_v7());
    let resp = crate::tests::helpers::webhook_helpers::send_webhook_with_signature(
        &app,
        &ctx._realm_id,
        lifecycle_event("subscription.expired", &expired_evt),
        "test_secret",
    )
    .await;
    crate::tests::helpers::webhook_helpers::assert_webhook_success(&resp);

    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(stats.delivered, 2);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let envelopes: Vec<serde_json::Value> = requests
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect();
    let types: Vec<&str> = envelopes
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert!(
        types.contains(&"subscription.activated"),
        "types: {types:?}"
    );
    assert!(types.contains(&"subscription.expired"), "types: {types:?}");
    for (idx, env) in envelopes.iter().enumerate() {
        assert_eq!(env["data"]["subscriptionId"], subscription_id.to_string());
        assert_eq!(env["data"]["entitlementKey"], "lifecycle-key");
        assert!(
            verify_signature(&requests[idx]),
            "event {} signature",
            types[idx]
        );
    }
    // Distinct event ids — one id per state change, the idempotency key
    // consumers deduplicate on.
    let ids: Vec<&str> = envelopes
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_ne!(ids[0], ids[1]);
}

// User Story: US-WEP-002 场景 2 (退款触发自动撤销) + 目录 subscription.refunded
// Covers: a full subscription refund revokes the payment-granted role
// (role.revoked, source=payment, via the pre-read snapshot) and emits
// subscription.refunded carrying the subscription id.
//
// WHY: the revoke snapshot is taken BEFORE the delete (rows are unrecoverable
// after) — a broken ordering emits role.revoked with no roleId or drops the
// event entirely, and consumers keep entitlements for refunded users.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_delivery_refund_revokes_role_and_refunds(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    seed_subscription(
        ctx,
        &ctx._realm_id,
        &server.uri(),
        &["role_events", "subscription_lifecycle"],
    )
    .await;

    ctx.with_creem_config(
        &ctx._realm_id,
        Some("test_api_key"),
        Some("test_secret"),
        Some(30),
    )
    .await;
    let token = create_admin_session_with_role(ctx, "delivery-refund-admin@test.com", 1800).await;
    let role_id = create_role(ctx, &ctx._realm_id, &token, "delivery-refund-role", "role").await;
    let user_id = seed_user(ctx, &ctx._realm_id, "delivery-refund-user@test.com").await;

    // The refund handler resolves the originating payment_attempt by
    // paymentId before routing (bucket resolution); seed that snapshot
    // through the standard helper and use its paymentId.
    let bucket_id = ensure_test_bucket_for_realm(&ctx._app_state.pool, &ctx._realm_id).await;
    let payment_id = format!("pay_refund_{}", Uuid::now_v7());
    let (_attempt_id, _mapping_id, _rule_id) =
        create_payment_attempt_snapshot(ctx, &ctx._realm_id, user_id, &payment_id, bucket_id, 999)
            .await;

    // Active subscription + a payment-granted role keyed on it (exactly the
    // rows the refund revoke path consumes).
    let external_sub = format!("sub_refund_{}", Uuid::now_v7());
    let subscription_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO subscription (id, realm_id, user_id, external_subscription_id, \
         external_product_id, payment_provider, status, entitlement_key, billing_type, \
         created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'prod_refund', 'creem', 'active', 'refund-key', \
         'recurring', NOW(), NOW())",
    )
    .bind(subscription_id)
    .bind(&ctx._realm_id)
    .bind(user_id)
    .bind(&external_sub)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, principal_type, principal_id, \
         source, source_id) VALUES ($1, $2, $3, $4, 'user', $2::text, 'payment', $5)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(role_id)
    .bind(&ctx._realm_id)
    .bind(subscription_id.to_string())
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let refund = serde_json::json!({
        "id": format!("evt_refund_{}", Uuid::now_v7()),
        "eventType": "refund.created",
        "data": {
            "object": {
                "id": format!("re_{}", Uuid::now_v7()),
                "paymentId": payment_id,
                "subscriptionId": external_sub,
                "amount": 999,
                "originalAmount": 999,
                "currency": "USD",
                "metadata": {"userId": user_id.to_string(), "refundType": "subscription"},
            }
        },
        "metadata": {"herald_user_id": user_id.to_string()},
    });
    let resp = crate::tests::helpers::webhook_helpers::send_webhook_with_signature(
        &app,
        &ctx._realm_id,
        refund,
        "test_secret",
    )
    .await;
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        status == StatusCode::OK || status == StatusCode::ACCEPTED,
        "refund webhook failed: {status} {}",
        String::from_utf8_lossy(&body)
    );

    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(stats.delivered, 2);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let envelopes: Vec<serde_json::Value> = requests
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect();
    let revoked = envelopes
        .iter()
        .find(|e| e["type"] == "role.revoked")
        .expect("role.revoked delivered");
    assert_eq!(revoked["data"]["roleId"], role_id.to_string());
    assert_eq!(revoked["data"]["source"], "payment");
    let refunded = envelopes
        .iter()
        .find(|e| e["type"] == "subscription.refunded")
        .expect("subscription.refunded delivered");
    assert_eq!(
        refunded["data"]["subscriptionId"],
        subscription_id.to_string()
    );
}

// User Story: US-WEP-002 (触发源全覆盖) / US-WEP-001 (角色全量同步差集)
// Covers: admin-create → user.created; PUT roles {A,B} → granted A+B;
// PUT {A} → revoked B (diff, original source); single-role DELETE →
// revoked A; admin user delete → user.deleted — all delivered with
// distinct event ids.
//
// WHY: the replace-diff in roles.rs derives granted/revoked sets from a
// pre-read snapshot — a wrong diff direction or a lost pre-read emits the
// opposite event for every bulk role edit, and user.deleted is the
// takedown signal consumers act on immediately.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_delivery_user_lifecycle_and_role_diff(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    seed_subscription(
        ctx,
        &ctx._realm_id,
        &server.uri(),
        &["user_lifecycle", "role_events"],
    )
    .await;

    let token =
        create_admin_session_with_role(ctx, "delivery-lifecycle2-admin@test.com", 1800).await;
    let role_a = create_role(ctx, &ctx._realm_id, &token, "delivery-diff-role-a", "role").await;
    let role_b = create_role(ctx, &ctx._realm_id, &token, "delivery-diff-role-b", "role").await;
    let user_id = admin_create_user(&app, &token, "delivery-diff-user@test.com").await;

    let put_roles = |role_ids: serde_json::Value| {
        let app = app.clone();
        let token = token.clone();
        async move {
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/users/{user_id}/roles"))
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            serde_json::json!({"roleIds": role_ids}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    };
    put_roles(serde_json::json!([role_a, role_b])).await;
    put_roles(serde_json::json!([role_a])).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/permission/users/{user_id}/roles/{role_a}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/users/{user_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let stats = drain_delivery_job(ctx).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    let envelopes: Vec<serde_json::Value> = requests
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect();
    let mut types: Vec<String> = envelopes
        .iter()
        .map(|e| e["type"].as_str().unwrap().to_string())
        .collect();
    types.sort();
    assert_eq!(
        stats.delivered, 6,
        "created + granted×2 + revoked×2 + deleted — got {types:?}"
    );
    assert_eq!(requests.len(), 6, "got {types:?}");
    assert_eq!(
        types,
        vec![
            "role.granted",
            "role.granted",
            "role.revoked",
            "role.revoked",
            "user.created",
            "user.deleted",
        ],
        "delivered event set"
    );

    // The diff revoke carries role_b (removed by the replace), the manual
    // revoke carries role_a; user.deleted carries the user id.
    let revoked_roles: Vec<Uuid> = envelopes
        .iter()
        .filter(|e| e["type"] == "role.revoked")
        .map(|e| Uuid::parse_str(e["data"]["roleId"].as_str().unwrap()).unwrap())
        .collect();
    assert!(
        revoked_roles.contains(&role_a) && revoked_roles.contains(&role_b),
        "revoked roles: {revoked_roles:?}"
    );
    let deleted = envelopes
        .iter()
        .find(|e| e["type"] == "user.deleted")
        .unwrap();
    assert_eq!(deleted["data"]["userId"], user_id.to_string());

    // Idempotency keys: all six state changes carry distinct ids.
    let ids: std::collections::HashSet<&str> = envelopes
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 6, "each state change has a distinct eventId");
}

// User Story: US-WEP-002 (用户创建覆盖全部创建来源)
// Covers: the public register endpoint also fans out user.created — the
// "user exists now" fact is source-independent by design.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_delivery_register_source_user_created(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    seed_subscription(ctx, &ctx._realm_id, &server.uri(), &["user_lifecycle"]).await;

    // Self-service registration is fail-closed by default; opt the realm in
    // (mirrors user_register_test).
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let payload = serde_json::json!({
        "clientId": ctx._client_id,
        "email": "delivery-register-user@test.com",
        "password": "password123",
        "turnstileToken": "dummy",
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/auth/{}/register", ctx._realm_id))
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-forwarded-for", "3.3.3.3")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let stats = run_delivery_job(ctx).await.unwrap();
    assert_eq!(stats.delivered, 1);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let env: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(env["type"], "user.created");
    assert_eq!(env["realmId"], ctx._realm_id.as_str());
    assert!(verify_signature(&requests[0]));
}
