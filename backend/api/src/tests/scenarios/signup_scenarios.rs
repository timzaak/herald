// =============================================================================
// Self-service Realm Signup - Scenario Tests
// =============================================================================
//
// 端到端验证公开自助开通端点 POST /api/auth/admin/signup 与公开开关
// GET /api/auth/admin/signup/status（design realm-create §4.2 / §6.1）。
//
// Covers:
// - US-SR-001: 访客一次提交即开通新 realm，成为 realm-admin
// - US-SR-002: 开通后立即获得新 realm 的 admin-web-console 会话
// - US-SR-003: 自助开通的 realm 与手动创建一致（复用 create_realm 链路）
// - US-SR-004: 平台开关控制入口可见性与开通（fail-closed）
//
// Environment behaviour (design §6.1 / §7, P2):
// - `RateLimitConfig.enforce_in_dev` defaults to `false`, so `rate_limit_hit`
//   is skipped in the test context. The IP-quota scenario below asserts the
//   *actual* (non-429) behaviour with a comment and MUST NOT be strengthened
//   to assert 429 by this item or the runner. The quota constant + call site
//   are covered by the domain unit tests and production code.
// - The admin realm's `admin-web-console` Client App is seeded with
//   `turnstile_enabled=false`, so Turnstile is never enforced here. The
//   Turnstile-enforced branch is verified by the existing
//   `client_app_turnstile_scenarios` coverage of `verify_turnstile_for_client_app`.
//
// 运行方式:
//   uv run scripts/backend-test.py -- -E 'package(herald-api) and test(/signup/)'
// =============================================================================

use crate::tests::response_json;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

/// Admin realm is the only host of the public signup entry (DEC-001).
const ADMIN_REALM: &str = "admin";

/// Toggle the platform self-service signup switch via SQL.
///
/// The signup read path is fail-closed (missing row ⇒ disabled), so scenarios
/// that exercise the open path must explicitly upsert an enabled row.
async fn set_platform_signup_enabled(ctx: &TestContext, enabled: bool) {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata)
         VALUES ('admin', 'platform_signup', 'enabled', $1, false, true, '{}'::jsonb)
         ON CONFLICT (realm_id, config_type, config_key) DO UPDATE
           SET config_value = EXCLUDED.config_value, updated_at = now()",
    )
    .bind(if enabled { "true" } else { "false" })
    .execute(&ctx.app_state.pool)
    .await
    .expect("failed to set platform signup toggle");
}

fn signup_body(realm_name: &str, realm_slug: Option<&str>, email: &str, password: &str) -> String {
    let mut payload = json!({
        "realmName": realm_name,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    if let Some(slug) = realm_slug {
        payload["realmSlug"] = json!(slug);
    } else {
        payload["realmSlug"] = json!(null);
    }
    payload.to_string()
}

/// DELETE FROM realm cleanup (realm deletion is otherwise unsupported; scenarios
/// own their fixtures). Cascading rows (client_app, roles, account, ...) are
/// removed by the schema's FK cascade.
async fn cleanup_realm(ctx: &TestContext, realm_id: &str) {
    // child tables first to avoid non-cascading FKs observed in some test fixtures
    let _ = sqlx::query("DELETE FROM user_roles WHERE realm_id = $1")
        .bind(realm_id)
        .execute(&ctx.app_state.pool)
        .await;
    let _ = sqlx::query("DELETE FROM roles WHERE realm_id = $1")
        .bind(realm_id)
        .execute(&ctx.app_state.pool)
        .await;
    let _ = sqlx::query("DELETE FROM permissions WHERE realm_id = $1")
        .bind(realm_id)
        .execute(&ctx.app_state.pool)
        .await;
    let _ = sqlx::query("DELETE FROM account WHERE realm_id = $1")
        .bind(realm_id)
        .execute(&ctx.app_state.pool)
        .await;
    let _ = sqlx::query("DELETE FROM client_app WHERE realm_id = $1")
        .bind(realm_id)
        .execute(&ctx.app_state.pool)
        .await;
    let _ = sqlx::query("DELETE FROM realm_config WHERE realm_id = $1")
        .bind(realm_id)
        .execute(&ctx.app_state.pool)
        .await;
    let _ = sqlx::query("DELETE FROM realm WHERE id = $1")
        .bind(realm_id)
        .execute(&ctx.app_state.pool)
        .await;
}

// =============================================================================
// Scenario 1 — US-SR-001 / US-SR-002: signup opens a realm and issues a session
// =============================================================================
//
// User Story: US-SR-001 自助注册开通新 Realm (P0)
//             US-SR-002 开通后立即管理新 Realm (P0)
// Source: docs/user-stories/core/realm-create.md
// Covers:
// - 平台开关开启时，访客一次提交即开通新 realm
// - 响应携带新 realm 的 first-party access/refresh token（DEC-012）
// - 新管理员账号为 Normal（DEC-006），可立即用返回 token 查询自身状态
// - 新 realm 在 DB 中存在并带有 realm-admin 角色（US-SR-003 复用既有链路）
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_opens_realm_and_issues_session(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    let app = ctx.create_unified_test_router();

    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap();
    let slug = format!("sr-success-{stamp}");
    let email = format!("owner-{stamp}@signup.test");

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{ADMIN_REALM}/signup"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.10")
        .body(Body::from(signup_body(
            "Signup Success Realm",
            Some(&slug),
            &email,
            "Password123",
        )))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "open toggle + valid payload should provision a realm"
    );

    let body: serde_json::Value = response_json(resp).await;
    assert_eq!(
        body["realmId"], slug,
        "response realm_id must match the slug"
    );
    assert_eq!(body["realmName"], "Signup Success Realm");
    assert!(
        body["accessToken"].as_str().is_some_and(|t| !t.is_empty()),
        "access token must be issued"
    );
    assert!(
        body["refreshToken"].as_str().is_some_and(|t| !t.is_empty()),
        "refresh token must be issued"
    );
    assert_eq!(body["tokenType"], "Bearer");
    let access_token = body["accessToken"].as_str().unwrap().to_string();

    // The issued session is bound to the NEW realm (DEC-012): status reports
    // the new realm's admin-web-console client and realm-admin permissions.
    let status_req = Request::builder()
        .method("GET")
        .uri("/api/auth/status")
        .header("authorization", format!("Bearer {access_token}"))
        .body(Body::empty())
        .unwrap();
    let status_resp = app.clone().oneshot(status_req).await.unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status: serde_json::Value = response_json(status_resp).await;
    assert_eq!(
        status["realmId"], slug,
        "session must be scoped to the new realm"
    );
    assert_eq!(status["clientId"], "admin-web-console");

    // The new realm + realm-admin role exist (US-SR-003: same chain as manual create).
    let has_realm: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM realm WHERE id = $1)")
        .bind(&slug)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
    assert!(has_realm, "new realm must be persisted");
    let has_admin_role: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM roles WHERE realm_id = $1 AND name = 'realm-admin')",
    )
    .bind(&slug)
    .fetch_one(&ctx.app_state.pool)
    .await
    .unwrap();
    assert!(
        has_admin_role,
        "new realm must receive the realm-admin role"
    );

    // Signup records consent to the effective ToS + Privacy for the new admin
    // (mirrors the register entrance): the issued session must not outrun the
    // user's consent state — otherwise the admin's next console login would be
    // consent-gated for agreements already accepted at signup.
    let admin_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&slug)
            .bind(&email)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
    let consented_types: Vec<String> = sqlx::query_scalar(
        "SELECT agreement_type FROM user_agreement_consent WHERE user_id = $1 ORDER BY agreement_type",
    )
    .bind(admin_id)
    .fetch_all(&ctx.app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        consented_types,
        vec!["privacy_policy".to_string(), "terms_of_service".to_string()],
        "signup must record register-consent for both agreement types"
    );

    cleanup_realm(ctx, &slug).await;
}

// =============================================================================
// Scenario 2 — DEC-001: only the admin realm hosts signup
// =============================================================================
//
// Covers: signup 强制 realmId="admin"，非 admin realm 不承载入口 → 404。
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_non_admin_realm_rejected(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    let app = ctx.create_unified_test_router();

    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/some-other-realm/signup")
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.11")
        .body(Body::from(signup_body(
            "Other Host",
            None,
            "x@signup.test",
            "Password123",
        )))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "non-admin realm must not host the signup entry"
    );
}

// =============================================================================
// Scenario 3 — US-SR-004: platform toggle gates the entry (fail-closed)
// =============================================================================
//
// User Story: US-SR-004 平台自助开通开关控制 (P0)
// Source: docs/user-stories/core/realm-create.md
// Covers:
// - 开关关闭 → signup 返回 403（DEC-009）
// - 公开状态查询返回 enabled=false（入口可见性，fail-closed）
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_disabled_when_toggle_off(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, false).await;
    let app = ctx.create_unified_test_router();

    // Public status reflects the closed toggle (frontend hides the entry).
    let status_req = Request::builder()
        .method("GET")
        .uri(format!("/api/auth/{ADMIN_REALM}/signup/status"))
        .body(Body::empty())
        .unwrap();
    let status_resp = app.clone().oneshot(status_req).await.unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status: serde_json::Value = response_json(status_resp).await;
    assert_eq!(
        status["enabled"], false,
        "status must report disabled when toggle is off"
    );

    // Provisioning is refused.
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{ADMIN_REALM}/signup"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.12")
        .body(Body::from(signup_body(
            "Blocked Realm",
            None,
            "blocked@signup.test",
            "Password123",
        )))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "disabled toggle must refuse provisioning"
    );
}

// =============================================================================
// Scenario 4 — DEC-007: same-IP 24h quota
// =============================================================================
//
// Covers: signup 在 create_realm 前对 rl:signup:ip:{ip} 做限流计数（DEC-011）。
//
// P2 NOTE: `RateLimitConfig.enforce_in_dev` defaults to `false`, and the signup
// handler uses `rate_limit_hit` (NOT `rate_limit_hit_forced`). In the test
// context the limit is therefore skipped and the 3rd attempt does NOT return
// 429. This scenario asserts the *actual* (non-429) behaviour with a comment
// and MUST NOT be strengthened to assert 429 by this item or the runner.
// The 2/24h quota constant and call site are verified by the domain unit tests
// and production code; the live 429 path is exercised in environments that
// opt into enforce_in_dev.
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_ip_limit_24h(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    let app = ctx.create_unified_test_router();
    let ip = "203.0.113.13";
    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap();

    let mut created_slugs = Vec::new();
    for i in 0..3 {
        let slug = format!("sr-limit-{stamp}-{i}");
        let email = format!("limit-{stamp}-{i}@signup.test");
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/auth/{ADMIN_REALM}/signup"))
            .header("content-type", "application/json")
            .header("x-forwarded-for", ip)
            .body(Body::from(signup_body(
                &format!("Limit Realm {i}"),
                Some(&slug),
                &email,
                "Password123",
            )))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        // enforce_in_dev=false → rate limiting skipped → all three succeed.
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "rate_limit_hit is skipped in test env; attempt {i} should provision"
        );
        created_slugs.push(slug);
    }

    for slug in &created_slugs {
        cleanup_realm(ctx, slug).await;
    }
}

// =============================================================================
// Scenario 5 — validation failures do not create a realm
// =============================================================================
//
// Covers: 邮箱/密码/realmName/realmSlug 非法 → 400，且不创建任何 realm。
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_validation_failures(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    let app = ctx.create_unified_test_router();

    // Password too short (< 8). `axum_valid::Valid` rejects with 400 before any
    // provisioning side-effect.
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{ADMIN_REALM}/signup"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.14")
        .body(Body::from(signup_body(
            "Short Pw Realm",
            None,
            "shortpw@signup.test",
            "short", // < 8 chars
        )))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "short password must be rejected with 400"
    );

    // No realm/email artefact created by the failed attempt.
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM account WHERE email = 'shortpw@signup.test')",
    )
    .fetch_one(&ctx.app_state.pool)
    .await
    .unwrap();
    assert!(!exists, "no account must be created on validation failure");
}

// =============================================================================
// Scenario 6 — realm slug conflict is rejected (400, codebase convention)
// =============================================================================
//
// Covers: realmSlug 已占用 → 被拒绝，不创建重复 realm。
//
// Status code note (design vs. codebase convention):
// Design §4.2.2 lists 409 for a slug conflict, but the realm repository
// returns `CoreError::BadRequest("Realm with ID '...' already exists")` on a
// duplicate id, and the existing admin `create_realm` handler documents this
// path as 400 (`backend/api/src/application/http/realm/crud.rs` OpenAPI:
// "400 - Bad request - invalid ID or ID already exists"). Self-service signup
// reuses the same repository, so it inherits the codebase-wide 400 convention
// rather than the design's idealized 409. Per Rule 10, the test follows the
// established convention; diverging only signup to 409 would split two callers
// of the same `create_realm` path. This is a D2 engineering call (conflict is
// still rejected; only the status code differs from the design note).
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_slug_conflict(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    let app = ctx.create_unified_test_router();
    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap();
    let slug = format!("sr-conflict-{stamp}");
    let email_a = format!("a-{stamp}@signup.test");
    let email_b = format!("b-{stamp}@signup.test");

    // First provisioning succeeds and occupies the slug.
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{ADMIN_REALM}/signup"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.15")
        .body(Body::from(signup_body(
            "First Realm",
            Some(&slug),
            &email_a,
            "Password123",
        )))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Second attempt reusing the slug is rejected (codebase convention: 400).
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{ADMIN_REALM}/signup"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.16")
        .body(Body::from(signup_body(
            "Second Realm",
            Some(&slug),
            &email_b,
            "Password123",
        )))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "duplicate realm slug is rejected with 400 (codebase convention; design §4.2.2 idealized this as 409)"
    );

    // Only one realm/account for the slug exists (the first one).
    let realm_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM realm WHERE id = $1")
        .bind(&slug)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
    assert_eq!(realm_count, 1, "no duplicate realm should be created");

    cleanup_realm(ctx, &slug).await;
}
