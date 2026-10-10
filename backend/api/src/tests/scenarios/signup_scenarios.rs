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
// - US-SR-005: 自助开通邮箱验证（前置验证码）——email_code 端点门控
//   （404/403/400 无通道）、发码 newest-wins、限流 429、signup 码校验
//   （必填/错码/跨邮箱/单次消费/TTL）、无通道 fail-open、status 的
//   emailVerificationRequired 组合翻转（docs/user-stories/core/realm-create.md 故事 5）。
//
// Environment behaviour (design §6.1 / §7, P2):
// - `RateLimitConfig.enforce_in_dev` defaults to `false`, so `rate_limit_hit`
//   is skipped in the default test context. The IP-quota scenario below asserts
//   the *actual* (non-429) behaviour with a comment and MUST NOT be strengthened
//   to assert 429 by this item or the runner. The quota constant is pinned by
//   the `signup_ip_quota_is_two_realms_per_24h` domain unit test, and the
//   enforced 429 path is covered by
//   `test_signup_ip_limit_enforced_returns_429_on_third_attempt` (scenario 4b
//   below, production app_env override).
// - The admin realm's `admin-web-console` Client App is seeded with
//   `turnstile_enabled=false`, so Turnstile is never enforced here. The
//   Turnstile-enforced branch is verified by the existing
//   `client_app_turnstile_scenarios` coverage of `verify_turnstile_for_client_app`.
// - Email-verification scenarios provision the admin realm's mail channel as
//   the unreachable loopback SMTP probe (127.0.0.1:9 —
//   tests/helpers/email_config_helpers): the code sender reaches the send
//   step offline and deterministically, and the send fails honestly AFTER
//   the code row is persisted — the same posture as the change-email
//   scenarios. A code request therefore wires out as 500 here (not the
//   production 200 "ok"); issuance, persistence, newest-wins and consumption
//   are the behaviours these scenarios pin.
//
// 运行方式:
//   uv run scripts/backend-test.py -- -E 'package(herald-api) and test(/signup/)'
// =============================================================================

use crate::tests::helpers::email_config_helpers::{
    delete_email_config_direct, insert_unreachable_smtp_email_config_direct,
};
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
    build_signup_body(realm_name, realm_slug, email, password, None)
}

/// `signup_body` plus the mailbox verification code consumed by the signup
/// gate. The field is absent (not `null`) in the no-code shape above, so both
/// request forms stay byte-identical to what the frontend sends.
fn signup_body_with_code(
    realm_name: &str,
    realm_slug: Option<&str>,
    email: &str,
    password: &str,
    email_verification_code: &str,
) -> String {
    build_signup_body(
        realm_name,
        realm_slug,
        email,
        password,
        Some(email_verification_code),
    )
}

fn build_signup_body(
    realm_name: &str,
    realm_slug: Option<&str>,
    email: &str,
    password: &str,
    email_verification_code: Option<&str>,
) -> String {
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
    if let Some(code) = email_verification_code {
        payload["emailVerificationCode"] = json!(code);
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

/// Toggle the admin realm's registration email-verification switch via SQL.
///
/// The flag alone does not enable the gate: `is_email_verification_required`
/// fail-opens to false while the admin realm has no usable mail channel, so
/// scenarios that exercise the enforced path must also provision a channel.
async fn set_email_verification_required(ctx: &TestContext, enabled: bool) {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata)
         VALUES ('admin', 'registration', 'require_email_verification', $1, false, true, '{}'::jsonb)
         ON CONFLICT (realm_id, config_type, config_key) DO UPDATE
           SET config_value = EXCLUDED.config_value, updated_at = now()",
    )
    .bind(if enabled { "true" } else { "false" })
    .execute(&ctx.app_state.pool)
    .await
    .expect("failed to set signup email verification toggle");
}

/// POST /api/auth/{realm}/signup/email_code for a mailbox. Caller owns the
/// response.
async fn post_signup_email_code(
    app: &axum::Router,
    realm: &str,
    ip: &str,
    email: &str,
) -> axum::response::Response {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{realm}/signup/email_code"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", ip)
        .body(Body::from(json!({ "email": email }).to_string()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap()
}

/// GET /api/auth/admin/signup/status, parsed.
async fn get_signup_status(app: &axum::Router) -> serde_json::Value {
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/auth/{ADMIN_REALM}/signup/status"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    response_json(resp).await
}

/// Number of stored 'signup' codes for a mailbox in the admin realm.
async fn signup_code_count(pool: &sqlx::PgPool, email: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM email_verification_code
         WHERE realm_id = 'admin' AND type = 'signup' AND email = $1",
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// The newest stored 'signup' code row `(id, code)` for a mailbox in the
/// admin realm.
async fn latest_signup_code(pool: &sqlx::PgPool, email: &str) -> Option<(uuid::Uuid, String)> {
    sqlx::query_as(
        "SELECT id, verification_code FROM email_verification_code
         WHERE realm_id = 'admin' AND type = 'signup' AND email = $1
         ORDER BY id DESC LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
    .unwrap()
}

/// Drive POST email_code and read the freshly issued code back from the
/// table — the code is stored as 6-digit plaintext (same policy as the
/// password-reset codes), so scenarios consume the real generated value
/// instead of seeding a second format.
///
/// The response asserts as 500, not the production 200 "ok": with the
/// unreachable loopback SMTP channel the send fails offline AFTER the code
/// row is persisted (see the file-header environment note).
async fn issue_signup_code_and_read(
    ctx: &TestContext,
    app: &axum::Router,
    ip: &str,
    email: &str,
) -> String {
    let resp = post_signup_email_code(app, ADMIN_REALM, ip, email).await;
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "the send fails on the unreachable relay after the code row is persisted"
    );
    latest_signup_code(&ctx.app_state.pool, email)
        .await
        .expect("the code row must be persisted before the send is attempted")
        .1
}

/// A wrong code that can never collide with the issued one. A fixed test
/// constant matches the real code once per 10^6 runs; `+1 (mod 10^6)` of the
/// actually issued value is deterministically different.
fn other_six_digit_code(code: &str) -> String {
    let n: u32 = code
        .parse()
        .expect("stored signup codes are six ASCII digits");
    format!("{:06}", (n + 1) % 1_000_000)
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
// The 2/24h quota constant is pinned by the `signup_ip_quota_is_two_realms_per_24h`
// domain unit test; the enforced 429 path is covered by scenario 4b below
// (production app_env override).
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

// =============================================================================
// Scenario 4b — DEC-011: same-IP 24h quota, ENFORCED path (positive test)
// =============================================================================
//
// The sibling `test_signup_ip_limit_24h` runs in the default test environment
// where `rate_limit_hit` is skipped, so it can only pin the non-429 shape.
// This scenario flips the router's `app_env` to production (per-test state
// override; no process-wide mutation), which is the ONLY branch the limiter
// keys on, and drives the real 2/24h quota end to end: two provisions from
// one IP succeed, the third is refused with 429 before any realm row is
// created. A unique per-run IP keeps the enforced Redis counter from leaking
// into other scenarios.
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_ip_limit_enforced_returns_429_on_third_attempt(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    let app = ctx.create_unified_test_router_with_state(|s| {
        s.app_env = "production".to_string();
    });
    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap();
    let ip = format!("203.0.{}.{}", 200 + (stamp % 40), (stamp / 41) % 250);

    let mut created_slugs = Vec::new();
    for i in 0..2 {
        let slug = format!("sr-429-{stamp}-{i}");
        let email = format!("enforced-{stamp}-{i}@signup.test");
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/auth/{ADMIN_REALM}/signup"))
            .header("content-type", "application/json")
            .header("x-forwarded-for", &ip)
            .body(Body::from(signup_body(
                &format!("Enforced Realm {i}"),
                Some(&slug),
                &email,
                "Password123",
            )))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "attempts 1-2 are within the 2/24h quota and must provision"
        );
        created_slugs.push(slug);
    }

    // Third attempt, same IP: refused 429 BEFORE create_realm — the quota is
    // the P0 anti-abuse acceptance value (DEC-011), so a realm row must not
    // appear.
    let slug_third = format!("sr-429-{stamp}-2");
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{ADMIN_REALM}/signup"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", &ip)
        .body(Body::from(signup_body(
            "Third Realm",
            Some(&slug_third),
            &format!("enforced-{stamp}-2@signup.test"),
            "Password123",
        )))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "the 3rd signup from one IP within 24h must be refused with 429"
    );
    let refused_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM realm WHERE id = $1")
        .bind(&slug_third)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
    assert_eq!(
        refused_count, 0,
        "a quota-refused signup must not leave a realm row behind"
    );

    for slug in &created_slugs {
        cleanup_realm(ctx, slug).await;
    }
}

// =============================================================================
// Scenario 7 — the code sender is admin-realm-only (404)
// =============================================================================
//
// Covers: POST /api/auth/{realmId}/signup/email_code 非 admin realm → 404。
// 码发送入口与开通入口共享同一托管边界，不得比 signup 本体更宽（否则任意
// realm 都能探测/触发 admin 平台的出站邮件）。
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_email_code_non_admin_realm_rejected(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    let app = ctx.create_unified_test_router();

    let resp =
        post_signup_email_code(&app, "some-other-realm", "203.0.113.17", "x@signup.test").await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "non-admin realms must not host the signup code sender"
    );
}

// =============================================================================
// Scenario 8 — US-SR-004: the platform toggle gates the code sender (403)
// =============================================================================
//
// User Story: US-SR-004 平台自助开通开关控制 (P0)
// Source: docs/user-stories/core/realm-create.md
// Covers: 平台开关关闭 → email_code 403（fail-closed，先于通道检查），
// 且不落任何码行——码发送入口不得比它服务的开通入口活得更久。
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_email_code_gated_by_platform_toggle(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, false).await;
    let app = ctx.create_unified_test_router();

    let resp = post_signup_email_code(&app, ADMIN_REALM, "203.0.113.18", "off@signup.test").await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "the code sender must be refused while the signup toggle is off"
    );
    assert_eq!(
        signup_code_count(&ctx.app_state.pool, "off@signup.test").await,
        0,
        "no code may be issued while the entry is closed"
    );
}

// =============================================================================
// Scenario 9 — a code request without a mail channel is refused (400)
// =============================================================================
//
// Covers: admin realm 邮件通道未配置 → 400 "Email is not configured for this
// realm"，且零码行。无通道时发码是死路：邮件永不到达，静默假成功只会让
// 访客死等（与 change_email request 的同形门控一致）。
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_email_code_rejected_without_email_channel(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    delete_email_config_direct(&ctx.app_state.pool, ADMIN_REALM).await;
    let app = ctx.create_unified_test_router();

    let resp =
        post_signup_email_code(&app, ADMIN_REALM, "203.0.113.19", "nochannel@signup.test").await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a code request without a mail channel must be rejected, not faked"
    );
    let body: serde_json::Value = response_json(resp).await;
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("Email is not configured for this realm"),
        "error message should name the missing channel, got: {:?}",
        body
    );
    assert_eq!(
        signup_code_count(&ctx.app_state.pool, "nochannel@signup.test").await,
        0,
        "no code may be issued on rejection"
    );
}

// =============================================================================
// Scenario 10 — a successful request persists one plaintext code, newest-wins
// =============================================================================
//
// Covers:
// - 发码在 admin realm 落一行 type='signup' 的 6 位数字明文码，行键是规范化
//   （trim + lowercase）后的邮箱——与 consume 端的再规范化一致
// - 重发 newest-wins：旧码行被删、只留最新一行，一个邮箱永远只有一个可消费
//   的码（与 change-email 请求路径同策略）
//
// 500 而非生产 200：见文件头 Environment behaviour 注释（不可达回环 SMTP，
// 码先行落库后发送如实失败）。
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_email_code_persists_code_newest_wins(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    insert_unreachable_smtp_email_config_direct(&ctx.app_state.pool, ADMIN_REALM).await;
    let app = ctx.create_unified_test_router();

    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap();
    // Mixed case on purpose: the row must be keyed on the normalized address.
    let email = format!("MixedCase-{stamp}@Example.TEST");
    let normalized = email.to_ascii_lowercase();

    let resp = post_signup_email_code(&app, ADMIN_REALM, "203.0.113.20", &email).await;
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "the send fails on the unreachable relay after the code row is persisted"
    );

    // One row, scoped to the admin realm + 'signup' type, keyed on the
    // normalized mailbox, carrying the 6-digit plaintext the consume step
    // matches on.
    let rows: Vec<(uuid::Uuid, String, String, String)> = sqlx::query_as(
        "SELECT id, realm_id, type, verification_code FROM email_verification_code WHERE email = $1",
    )
    .bind(&normalized)
    .fetch_all(&ctx.app_state.pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1, "one code row per mailbox after issuance");
    assert_eq!(rows[0].1, "admin", "signup codes live in the admin realm");
    assert_eq!(
        rows[0].2, "signup",
        "the row must use the 'signup' code type"
    );
    assert!(
        rows[0].3.chars().count() == 6 && rows[0].3.chars().all(|c| c.is_ascii_digit()),
        "the stored code is the zero-padded 6-digit plaintext, got: {}",
        rows[0].3
    );
    let first_id = rows[0].0;

    // Resend replaces: delete-then-insert leaves exactly one row whose id is
    // the new issuance's.
    let resp = post_signup_email_code(&app, ADMIN_REALM, "203.0.113.20", &email).await;
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "the resend reaches the send step as well"
    );
    assert_eq!(
        signup_code_count(&ctx.app_state.pool, &normalized).await,
        1,
        "resend must delete the previous code row (newest-wins)"
    );
    let (second_id, _) = latest_signup_code(&ctx.app_state.pool, &normalized)
        .await
        .expect("the newest code row must exist");
    assert_ne!(
        first_id, second_id,
        "the surviving row must be the newly issued one"
    );
}

// =============================================================================
// Scenario 11 — code-request rate limit, ENFORCED path (1 per 120s per IP)
// =============================================================================
//
// 同 scenario 4 的 P2 NOTE：默认测试上下文 enforce_in_dev=false，限流被跳过；
// 这里复用 4b 的 app_env production 手法（per-test 状态覆盖，不改进程全局）
// 驱动真实 (1,120) per-IP 配额：第一次请求放行（走到发送步骤），同 IP 第二次
// 120s 内 → 429。唯一 IP（TEST-NET-2 段，避开 4b 的 203.0.x.y）防止 enforced
// Redis 计数器泄漏进其他场景。
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_email_code_rate_limit_enforced_returns_429_on_second_request(
    ctx: &mut TestContext,
) {
    set_platform_signup_enabled(ctx, true).await;
    insert_unreachable_smtp_email_config_direct(&ctx.app_state.pool, ADMIN_REALM).await;
    let app = ctx.create_unified_test_router_with_state(|s| {
        s.app_env = "production".to_string();
    });
    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap();
    let ip = format!("198.51.100.{}", (stamp % 250) + 1);
    let email = format!("rl-{stamp}@signup.test");

    let resp = post_signup_email_code(&app, ADMIN_REALM, &ip, &email).await;
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "the first request is within the 1/120s budget and reaches the send step"
    );

    // The limiter is the only anti-bombing defense on this Turnstile-less
    // unauthenticated sender, so the 2nd request must be refused BEFORE any
    // code is issued or replaced.
    let resp = post_signup_email_code(&app, ADMIN_REALM, &ip, &email).await;
    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "the 2nd code request per IP within 120s must be refused with 429"
    );
    assert_eq!(
        signup_code_count(&ctx.app_state.pool, &email).await,
        1,
        "a rate-refused request must not issue or replace a code row"
    );
}

// =============================================================================
// Scenario 12 — US-SR-001: the mailbox-code gate on provisioning
// =============================================================================
//
// User Story: US-SR-001 自助注册开通新 Realm (P0)
// Source: docs/user-stories/core/realm-create.md
// Covers（验证开关开启 + 邮件通道已配置时）:
// - 无码 → 400 "email verification code is required"
// - 错码 → 400 "invalid email verification code"
// - A 邮箱的码 + B 邮箱提交 → 同一 400（consume 按 (code, email) 对匹配，
//   错码/错邮箱/过期同文案，响应不得成为区分依据）
// - 失败尝试不烧码：前三次 400 后，正确码仍能开通（单次消费只在匹配时发生）
// - 正确码 → 200（realm 建成、token 照发），码被消费：行删除、同码再用 → 400
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_email_code_gate_when_verification_enabled(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    set_email_verification_required(ctx, true).await;
    insert_unreachable_smtp_email_config_direct(&ctx.app_state.pool, ADMIN_REALM).await;
    let app = ctx.create_unified_test_router();

    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap();
    let slug = format!("sr-code-{stamp}");
    let email = format!("owner-code-{stamp}@signup.test");
    let code = issue_signup_code_and_read(ctx, &app, "203.0.113.21", &email).await;

    let signup_req = |slug: &str, email: &str, code: Option<&str>| {
        Request::builder()
            .method("POST")
            .uri(format!("/api/auth/{ADMIN_REALM}/signup"))
            .header("content-type", "application/json")
            .header("x-forwarded-for", "203.0.113.21")
            .body(Body::from(match code {
                Some(code) => {
                    signup_body_with_code("Coded Realm", Some(slug), email, "Password123", code)
                }
                None => signup_body("Coded Realm", Some(slug), email, "Password123"),
            }))
            .unwrap()
    };

    // 1) No code at all → explicitly demanded.
    let resp = app
        .clone()
        .oneshot(signup_req(&slug, &email, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response_json(resp).await;
    assert_eq!(
        body["message"], "email verification code is required",
        "the gate must name the missing requirement"
    );

    // 2) Wrong code → the unified invalid message.
    let wrong = other_six_digit_code(&code);
    let resp = app
        .clone()
        .oneshot(signup_req(&slug, &email, Some(&wrong)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response_json(resp).await;
    assert_eq!(body["message"], "invalid email verification code");

    // 3) Mailbox A's code submitted under mailbox B → the same message, so
    // the response cannot reveal which half of the (code, email) pair was off.
    let email_b = format!("owner-code-b-{stamp}@signup.test");
    let resp = app
        .clone()
        .oneshot(signup_req(&format!("{slug}-b"), &email_b, Some(&code)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response_json(resp).await;
    assert_eq!(
        body["message"], "invalid email verification code",
        "cross-mailbox consumption must be indistinguishable from a wrong code"
    );

    // 4) The correct code still works: the failed attempts above never
    // burned it (consumption is an atomic match-and-delete).
    let resp = app
        .clone()
        .oneshot(signup_req(&slug, &email, Some(&code)))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the issued code must provision the realm after refused attempts"
    );
    let body: serde_json::Value = response_json(resp).await;
    assert_eq!(body["realmId"], slug);
    assert!(
        body["accessToken"].as_str().is_some_and(|t| !t.is_empty()),
        "the gated path still issues the first-party session"
    );
    let has_realm: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM realm WHERE id = $1)")
        .bind(&slug)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
    assert!(has_realm, "the verified signup must create the realm");
    assert_eq!(
        signup_code_count(&ctx.app_state.pool, &email).await,
        0,
        "a successful signup consumes the single-use code"
    );
    cleanup_realm(ctx, &slug).await;

    // 5) The consumed code is dead: resubmitting it (fresh realm, same
    // mailbox) is just another invalid code.
    let resp = app
        .clone()
        .oneshot(signup_req(&format!("{slug}-reuse"), &email, Some(&code)))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a consumed code must not provision a second realm"
    );
    let body: serde_json::Value = response_json(resp).await;
    assert_eq!(body["message"], "invalid email verification code");
}

// =============================================================================
// Scenario 13 — a code older than the TTL is refused (1800s)
// =============================================================================
//
// Covers: 把码行 created_at 回拨到 TTL（1800s）之外后提交正确码 → 400
// "invalid email verification code"，且行不被消费（TTL 拒绝不是删除——
// 行仍在，区别于消费路径）。时间回拨 UPDATE 沿用 client_api/mcp 场景的既有
// 手法（直接 UPDATE 自有 fixture 行，确定性）。
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_email_code_ttl_expired_rejected(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    set_email_verification_required(ctx, true).await;
    insert_unreachable_smtp_email_config_direct(&ctx.app_state.pool, ADMIN_REALM).await;
    let app = ctx.create_unified_test_router();

    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap();
    let slug = format!("sr-ttl-{stamp}");
    let email = format!("owner-ttl-{stamp}@signup.test");
    let code = issue_signup_code_and_read(ctx, &app, "203.0.113.22", &email).await;

    // 1801s = EMAIL_VERIFICATION_CODE_TTL_SECONDS (1800) + 1: the row falls
    // outside the consume cutoff.
    sqlx::query(
        "UPDATE email_verification_code SET created_at = NOW() - INTERVAL '1801 seconds'
         WHERE realm_id = 'admin' AND email = $1 AND type = 'signup'",
    )
    .bind(&email)
    .execute(&ctx.app_state.pool)
    .await
    .expect("failed to age the signup code row");

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{ADMIN_REALM}/signup"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.22")
        .body(Body::from(signup_body_with_code(
            "Expired Realm",
            Some(&slug),
            &email,
            "Password123",
            &code,
        )))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "an expired code must not provision a realm"
    );
    let body: serde_json::Value = response_json(resp).await;
    assert_eq!(body["message"], "invalid email verification code");
    assert_eq!(
        signup_code_count(&ctx.app_state.pool, &email).await,
        1,
        "an expired code is refused, not consumed — the row survives"
    );
}

// =============================================================================
// Scenario 14 — fail-open: verification on, mail channel missing
// =============================================================================
//
// Covers: 验证开关开启但 admin realm 邮件通道未配置 →
// `is_email_verification_required` 强制返回 false（fail-open）：不带码也能
// 开通 200，status 同样上报 emailVerificationRequired=false。通道缺配是管理员
// 配置错误，不得变相封锁整个自助开通入口；此场景固化该语义。
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_verification_fail_open_without_email_channel(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    set_email_verification_required(ctx, true).await;
    delete_email_config_direct(&ctx.app_state.pool, ADMIN_REALM).await;
    let app = ctx.create_unified_test_router();

    let status = get_signup_status(&app).await;
    assert_eq!(status["enabled"], true);
    assert_eq!(
        status["emailVerificationRequired"], false,
        "the fail-open combo must report the gate as absent"
    );

    let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap();
    let slug = format!("sr-failopen-{stamp}");
    let email = format!("owner-failopen-{stamp}@signup.test");
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{ADMIN_REALM}/signup"))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.23")
        .body(Body::from(signup_body(
            "Fail-open Realm",
            Some(&slug),
            &email,
            "Password123",
        )))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "verification requested but no channel configured must fail open, not block signup"
    );

    cleanup_realm(ctx, &slug).await;
}

// =============================================================================
// Scenario 15 — US-SR-004: status reports emailVerificationRequired per combo
// =============================================================================
//
// User Story: US-SR-004 平台自助开通开关控制 (P0)
// Source: docs/user-stories/core/realm-create.md
// Covers: GET /api/auth/admin/signup/status 的 emailVerificationRequired 随
// registration 开关 × 邮件通道组合翻转：默认关；开而未配通道 → false
// （fail-open）；开且配通道 → true；显式关 → false。status 与 signup 门控读
// 同一 helper，标志不得与强制侧不一致。
#[test_context(TestContext)]
#[tokio::test]
async fn test_signup_status_email_verification_required_matrix(ctx: &mut TestContext) {
    set_platform_signup_enabled(ctx, true).await;
    // Guarantee the pristine default regardless of the template schema's state.
    sqlx::query(
        "DELETE FROM realm_config
         WHERE realm_id = 'admin' AND config_type = 'registration' AND config_key = 'require_email_verification'",
    )
    .execute(&ctx.app_state.pool)
    .await
    .expect("failed to clear the verification config row");
    delete_email_config_direct(&ctx.app_state.pool, ADMIN_REALM).await;
    let app = ctx.create_unified_test_router();

    let status = get_signup_status(&app).await;
    assert_eq!(status["enabled"], true);
    assert_eq!(
        status["emailVerificationRequired"], false,
        "default: no registration row → not required"
    );

    set_email_verification_required(ctx, true).await;
    let status = get_signup_status(&app).await;
    assert_eq!(
        status["emailVerificationRequired"], false,
        "verification on but no mail channel → fail-open false"
    );

    insert_unreachable_smtp_email_config_direct(&ctx.app_state.pool, ADMIN_REALM).await;
    let status = get_signup_status(&app).await;
    assert_eq!(
        status["emailVerificationRequired"], true,
        "verification on + channel configured → required"
    );

    set_email_verification_required(ctx, false).await;
    let status = get_signup_status(&app).await;
    assert_eq!(
        status["emailVerificationRequired"], false,
        "verification explicitly off (channel present) → not required"
    );
}
