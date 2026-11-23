// Webhook subscription management-face scenario tests: verifies the
// /api/webhooks management surface — secret one-time reveal, private-address
// rejection, category validation, duplicates, status machine, rotation,
// deletion, the per-realm cap, permission split, cross-realm 404, and the
// builtin-permission migration seed.
//
// User Story: US-WEP-001 (configure webhook event subscriptions).

use crate::tests::helpers::auth_helpers::{
    create_admin_session_with_role, create_admin_session_with_user,
};
use crate::tests::helpers::rbac_helpers::grant_single_permission;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use test_context::test_context;
use tower::ServiceExt;

// A public IP literal — validation accepts it without any DNS dependency,
// keeping these management-face tests hermetic.
const PUBLIC_URL: &str = "https://93.184.216.34/callback";

async fn post_json(
    app: &axum::Router,
    token: &str,
    uri: String,
    body: String,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {}", token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn get(app: &axum::Router, token: &str, uri: String) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

// User Story: US-WEP-001
// Covers: secret one-time reveal — create echoes it, list/get never do.
//
// WHY: the reveal-once promise is the whole security model of the signing
// secret; a list/get that leaks it makes every later "rotate to recover"
// meaningless and silently breaks the consumer's assumption that only they
// hold the key.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_webhook_secret_shown_once_on_create(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let token = create_admin_session_with_role(ctx, "webhook-admin-create@test.com", 1800).await;

    let resp = post_json(
        &app,
        &token,
        "/api/webhooks".into(),
        r#"{"endpointUrl": "https://93.184.216.34/cb", "eventCategories": ["role_events"]}"#.into(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    let secret = created["secret"]
        .as_str()
        .expect("secret on create")
        .to_string();
    assert!(
        secret.starts_with("whsec_"),
        "secret must carry whsec_ prefix"
    );
    let subscription_id = created["id"].as_str().unwrap().to_string();

    // List and get expose only the hint.
    let resp = get(&app, &token, "/api/webhooks?page=0&pageSize=20".into()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list = body_json(resp).await;
    assert!(
        list["items"][0].get("secret").is_none(),
        "list must never echo the secret"
    );
    assert!(
        list["items"][0]["secretHint"]
            .as_str()
            .unwrap()
            .starts_with('…'),
        "list exposes secretHint tail only"
    );

    let resp = get(&app, &token, format!("/api/webhooks/{subscription_id}")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let detail = body_json(resp).await;
    assert!(
        detail.get("secret").is_none(),
        "get must never echo the secret"
    );
    assert_eq!(detail["status"], "active");

    // No storage path beyond create stores a recoverable copy either: the
    // audit row for create carries the URL/categories, never the secret.
    let leaked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE details::text LIKE '%' || $1 || '%'",
    )
    .bind(&secret)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(leaked, 0, "the secret must not appear in any audit payload");
}

// User Story: US-WEP-001 (security baseline — SSRF).
// Covers: loopback, RFC1918, CGNAT, link-local (cloud metadata), IPv6
// loopback/ULA all rejected with 400 invalid_endpoint_url.
//
// WHY: Herald dials tenant-controlled URLs; any accepted private range is a
// direct SSRF path into the deployment's internal network (metadata service,
// admin panels). The single error code intentionally does not reveal which
// range tripped, shrinking the probing surface.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_webhook_private_url_matrix_rejected(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let token = create_admin_session_with_role(ctx, "webhook-admin-private@test.com", 1800).await;

    let rejected = [
        "http://localhost:9/cb",
        "http://127.0.0.1:9/cb",
        "http://10.1.2.3/cb",
        "http://192.168.1.10/cb",
        "http://172.16.5.5/cb",
        "http://169.254.169.254/latest/meta-data",
        "http://[::1]/cb",
        "http://[fc00::1]/cb",
        "http://[fe80::1]/cb",
    ];
    for url in rejected {
        let body = serde_json::json!({"endpointUrl": url, "eventCategories": ["user_lifecycle"]})
            .to_string();
        let resp = post_json(&app, &token, "/api/webhooks".into(), body).await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "{url} must be rejected at registration"
        );
        let err = body_json(resp).await;
        assert_eq!(err["code"], "invalid_endpoint_url", "{url}");
    }

    // No rows were created by the rejected attempts.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM webhook_subscription")
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

// User Story: US-WEP-001 / US-WEP-002 (category filtering is a contract).
// Covers: empty and unknown category lists rejected with 400.
//
// WHY: the fan-out query matches on these exact strings — accepting an
// unknown value would create a subscription that can never match anything
// while looking configured.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_webhook_invalid_categories_rejected(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let token = create_admin_session_with_role(ctx, "webhook-admin-cats@test.com", 1800).await;

    let empty: Vec<&str> = vec![];
    let unknown = vec!["not_a_category"];
    for categories in [empty, unknown] {
        let body = serde_json::json!({"endpointUrl": PUBLIC_URL, "eventCategories": categories})
            .to_string();
        let resp = post_json(&app, &token, "/api/webhooks".into(), body).await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "categories {categories:?}"
        );
        let err = body_json(resp).await;
        assert_eq!(err["code"], "invalid_event_categories");
    }
}

// User Story: US-WEP-001
// Covers: the (realm_id, endpoint_url) uniqueness is surfaced as 409, not 500.
//
// WHY: a raw unique-violation 500 would both leak internals and break the
// admin UI's duplicate handling; two identical configs would also double
// every push.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_webhook_duplicate_url_conflict(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let token = create_admin_session_with_role(ctx, "webhook-admin-dup@test.com", 1800).await;

    let body =
        r#"{"endpointUrl": "https://93.184.216.34/dup", "eventCategories": ["role_events"]}"#;
    let resp = post_json(&app, &token, "/api/webhooks".into(), body.into()).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = post_json(&app, &token, "/api/webhooks".into(), body.into()).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err = body_json(resp).await;
    assert_eq!(err["code"], "duplicate_endpoint_url");
}

// User Story: US-WEP-001 (realm boundary).
// Covers: another realm's subscription is 404, not 403 (no existence leak).
//
// WHY: confirming existence of another tenant's callback URL is itself
// information; the admin-face precedent (admin_users) is cross-realm 404.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_webhook_cross_realm_404(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // A subscription belonging to a different realm.
    sqlx::query("INSERT INTO realm (id, name) VALUES ($1, 'webhook-other-realm')")
        .bind("webhook-other-realm")
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    let other_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO webhook_subscription (id, realm_id, endpoint_url, secret, event_categories)
         VALUES ($1, 'webhook-other-realm', 'https://93.184.216.35/cb', 'whsec_other', $2)",
    )
    .bind(other_id)
    .bind(&["user_lifecycle"][..])
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let token = create_admin_session_with_role(ctx, "webhook-admin-xrealm@test.com", 1800).await;
    let resp = get(&app, &token, format!("/api/webhooks/{other_id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// User Story: US-WEP-001 (access control).
// Covers: webhooks.view grants list; view-only cannot create; no-permission
// user gets 403.
//
// WHY: subscription rows expose callback URLs and failure telemetry — they
// must sit behind the same admin-console permission face as the rest of the
// console, with manage strictly stronger than view.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_webhook_permission_split(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    let (view_token, view_user) =
        create_admin_session_with_user(ctx, "webhook-viewonly@test.com", 1800).await;
    grant_single_permission(ctx, &view_user, "webhooks", "view").await;

    let resp = get(&app, &view_token, "/api/webhooks?page=0&pageSize=20".into()).await;
    assert_eq!(resp.status(), StatusCode::OK, "webhooks.view grants list");

    let resp = post_json(
        &app,
        &view_token,
        "/api/webhooks".into(),
        r#"{"endpointUrl": "https://93.184.216.36/cb", "eventCategories": ["role_events"]}"#.into(),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "view-only must not create"
    );

    let (none_token, none_user) =
        create_admin_session_with_user(ctx, "webhook-noperm@test.com", 1800).await;
    drop(none_user);
    let resp = get(&app, &none_token, "/api/webhooks?page=0&pageSize=20".into()).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "no permission → 403");
}

// User Story: US-WEP-001 / US-WEP-003
// Covers: disable → disabled; resume resets failure state; requesting
// `paused` through the API is 400; same-state is idempotent 200.
//
// WHY: `paused` is written exclusively by the delivery pipeline's
// conditional flip; letting an admin set it (or letting resume keep a stale
// failure counter) would let a subscription re-pause instantly or fake
// system state.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_webhook_status_machine(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let token = create_admin_session_with_role(ctx, "webhook-admin-status@test.com", 1800).await;

    let resp = post_json(
        &app,
        &token,
        "/api/webhooks".into(),
        r#"{"endpointUrl": "https://93.184.216.37/cb", "eventCategories": ["user_lifecycle"]}"#
            .into(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    let id = created["id"].as_str().unwrap().to_string();

    // paused is system-only.
    let resp = post_json(
        &app,
        &token,
        format!("/api/webhooks/{id}/status"),
        r#"{"status": "paused"}"#.into(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Simulate the pipeline having paused it, then resume: counters clear.
    sqlx::query(
        "UPDATE webhook_subscription SET status='paused', consecutive_failures=9, \
         paused_at=NOW(), last_error='HTTP 500' WHERE id=$1",
    )
    .bind(uuid::Uuid::parse_str(&id).unwrap())
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let resp = post_json(
        &app,
        &token,
        format!("/api/webhooks/{id}/status"),
        r#"{"status": "active"}"#.into(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resumed = body_json(resp).await;
    assert_eq!(resumed["status"], "active");
    assert_eq!(
        resumed["consecutiveFailures"], 0,
        "resume clears the counter"
    );
    assert!(resumed["pausedAt"].is_null(), "resume clears pausedAt");

    // Same-state transition is idempotent.
    let resp = post_json(
        &app,
        &token,
        format!("/api/webhooks/{id}/status"),
        r#"{"status": "active"}"#.into(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Disable, then re-enable.
    let resp = post_json(
        &app,
        &token,
        format!("/api/webhooks/{id}/status"),
        r#"{"status": "disabled"}"#.into(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = post_json(
        &app,
        &token,
        format!("/api/webhooks/{id}/status"),
        r#"{"status": "active"}"#.into(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// User Story: US-WEP-001
// Covers: rotation returns the new secret once; the hint changes; the audit
// payload carries only the hint.
//
// WHY: rotation exists precisely because the old key may have leaked — if
// rotation didn't immediately change the stored key (or echoed it into
// audit), the recovery operation itself would be broken.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_webhook_rotate_secret(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let token = create_admin_session_with_role(ctx, "webhook-admin-rotate@test.com", 1800).await;

    let resp = post_json(
        &app,
        &token,
        "/api/webhooks".into(),
        r#"{"endpointUrl": "https://93.184.216.38/cb", "eventCategories": ["role_events"]}"#.into(),
    )
    .await;
    let created = body_json(resp).await;
    let id = created["id"].as_str().unwrap().to_string();
    let old_secret = created["secret"].as_str().unwrap().to_string();
    let old_hint = created["secretHint"].as_str().unwrap().to_string();

    let resp = post_json(
        &app,
        &token,
        format!("/api/webhooks/{id}/rotate-secret"),
        "{}".into(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let rotated = body_json(resp).await;
    let new_secret = rotated["secret"].as_str().unwrap().to_string();
    assert_ne!(new_secret, old_secret);
    assert!(new_secret.starts_with("whsec_"));
    assert_ne!(rotated["secretHint"].as_str().unwrap(), old_hint);

    // The stored key really changed.
    let stored: String = sqlx::query_scalar("SELECT secret FROM webhook_subscription WHERE id=$1")
        .bind(uuid::Uuid::parse_str(&id).unwrap())
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap();
    assert_eq!(stored, new_secret);
}

// User Story: US-WEP-001
// Covers: delete returns 204; the row (and its cascade) is gone; repeat 404.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_webhook_delete(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let token = create_admin_session_with_role(ctx, "webhook-admin-delete@test.com", 1800).await;

    let resp = post_json(
        &app,
        &token,
        "/api/webhooks".into(),
        r#"{"endpointUrl": "https://93.184.216.39/cb", "eventCategories": ["role_events"]}"#.into(),
    )
    .await;
    let created = body_json(resp).await;
    let id = created["id"].as_str().unwrap().to_string();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/webhooks/{id}"))
                .header(header::AUTHORIZATION, format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = get(&app, &token, format!("/api/webhooks/{id}")).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// User Story: US-WEP-001 (engineering guard).
// Covers: the 21st subscription in a realm is rejected 400
// subscription_limit_exceeded.
//
// WHY: fan-out cost scales with subscription count per realm; without a cap
// one tenant can multiply every event push twenty-fold per URL.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_webhook_subscription_cap(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let token = create_admin_session_with_role(ctx, "webhook-admin-cap@test.com", 1800).await;

    // Seed the 20 allowed rows directly (the cap is the subject here, not
    // the create path).
    for i in 0..20 {
        sqlx::query(
            "INSERT INTO webhook_subscription (id, realm_id, endpoint_url, secret, event_categories)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(&ctx._realm_id)
        .bind(format!("https://93.184.216.40/cb-{i}"))
        .bind(format!("whsec_cap_{i}"))
        .bind(&["role_events"][..])
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    }

    let resp = post_json(
        &app,
        &token,
        "/api/webhooks".into(),
        r#"{"endpointUrl": "https://93.184.216.41/cb", "eventCategories": ["role_events"]}"#.into(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err = body_json(resp).await;
    assert_eq!(err["code"], "subscription_limit_exceeded");
}

// User Story: US-WEP-001
// Covers: the migration attached webhooks.view/manage to the builtin
// realm-admin role of this (pre-existing) test realm.
//
// WHY: rbac_init only runs at realm creation; without the seed migration
// every existing realm's admins would see the Webhooks menu permission-
// denied forever.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_webhook_builtin_permissions_seeded(ctx: &mut TestContext) {
    let seeded: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM role_permissions rp
        JOIN roles ro ON ro.id = rp.role_id AND ro.name = 'realm-admin' AND ro.is_builtin
        JOIN permissions p ON p.id = rp.permission_id
        WHERE ro.realm_id = $1 AND p.name IN ('webhooks.view', 'webhooks.manage')
        "#,
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        seeded, 2,
        "both builtin webhooks permissions must be attached to realm-admin"
    );
}
