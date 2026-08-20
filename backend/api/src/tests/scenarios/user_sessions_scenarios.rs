// =============================================================================
// User Sessions Management Scenario Tests
// =============================================================================
//
// End-to-end HTTP scenario tests for the admin session management endpoints:
//   GET    /api/users/{realmId}/{userId}/sessions
//   DELETE /api/users/{realmId}/{userId}/sessions
//   DELETE /api/users/{realmId}/{userId}/sessions/{familyId}
// and the Forbidden-status side-effect (PUT /api/users/{realmId}/{userId}) that
// revokes all of a user's active sessions.
//
// Covers user stories in docs/user-stories/core/realm-admin.md:
//   - US-RA-020 (list / revoke single / revoke all / permissions / realm /
//     cross-user / 404 / concurrent no-op)
//   - US-RA-021 (Forbidden linkage revocation)
//
// =============================================================================

use crate::tests::helpers::auth_helpers::{create_admin_session_with_user, grant_realm_admin_role};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use herald_core::domain::authentication::BrowserTokenService;
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::client::ports::ClientService;
use herald_core::domain::legal::entities::AgreementType;
use herald_core::domain::user::UserRepository;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;

// =============================================================================
// Local helpers
// =============================================================================

/// Seed a Normal (status=1) user with a bcrypt password, username, provider_ids
/// and a profile nickname, plus the consent records required to pass the login
/// consent gate. Returns `(user_id, email)`. Mirrors the
/// `account_self_delete_scenarios.rs` seeding pattern.
async fn seed_normal_user_with_password(
    ctx: &TestContext,
    realm_id: &str,
    password: &str,
) -> (Uuid, String) {
    let user_id = Uuid::now_v7();
    let email = format!("sessions-{}@test.com", user_id.simple());
    let username = format!("user_{}", user_id.simple());
    let password_hash =
        bcrypt::hash(password, bcrypt::DEFAULT_COST).expect("Failed to hash password");
    let provider_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, username, provider_ids, status, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, 1, NOW(), NOW())",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(&email)
    .bind(&password_hash)
    .bind(&username)
    .bind(vec![provider_id])
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create test user");

    sqlx::query(
        "INSERT INTO profile (id, realm_id, nickname, created_at, updated_at)
         VALUES ($1, $2, $3, NOW(), NOW())",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(format!("nick_{}", user_id.simple()))
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create test profile");

    // Consent records so the login consent gate (BE-D08) lets the user in.
    let tos_id = ctx
        .app_state
        .legal_service
        .current_effective(realm_id, AgreementType::TermsOfService)
        .await
        .expect("Failed to resolve effective ToS")
        .map(|v| v.id)
        .expect("No effective ToS version exists");
    let pp_id = ctx
        .app_state
        .legal_service
        .current_effective(realm_id, AgreementType::PrivacyPolicy)
        .await
        .expect("Failed to resolve effective PrivacyPolicy")
        .map(|v| v.id)
        .expect("No effective PrivacyPolicy version exists");

    sqlx::query(
        "INSERT INTO user_agreement_consent (user_id, realm_id, agreement_type, consented_version_id)
         VALUES ($1, $2, 'terms_of_service', $3),
                ($1, $2, 'privacy_policy', $4)
         ON CONFLICT (user_id, agreement_type) DO NOTHING",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(tos_id)
    .bind(pp_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to seed user consent");

    (user_id, email)
}

/// Set the account row status directly via SQL (bypassing the admin update
/// endpoint). Useful for seeding a user that is already Forbidden without
/// triggering the Forbidden linkage revoke. `status` is the raw i16
/// (WaitVerified=0, Normal=1, Forbidden=2, ...).
async fn set_user_status_directly(ctx: &TestContext, user_id: Uuid, status: i16) {
    sqlx::query("UPDATE account SET status = $1, updated_at = NOW() WHERE id = $2")
        .bind(status)
        .bind(user_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to set user status");
}

/// Password login returning the Bearer access token. Mirrors
/// `account_self_delete_scenarios.rs::login_and_get_token`.
async fn login_and_get_token(
    ctx: &TestContext,
    realm_id: &str,
    email: &str,
    password: &str,
) -> String {
    let app = ctx.create_unified_test_router();
    let payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.10")
        .header("user-agent", "test-ua-login")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    body["accessToken"]
        .as_str()
        .expect("Login should return accessToken")
        .to_owned()
}

/// Create a FirstParty browser-token family directly via the token service,
/// returning the access token. UA/IP are passed so the session metadata index
/// (BE-D01) records non-null values for list assertions. Mirrors
/// `account_self_delete_scenarios.rs::create_extra_session`, updated for the
/// BE-D01 `create_first_party_token_family` signature.
async fn create_extra_session(
    ctx: &TestContext,
    user_id: Uuid,
    user_agent: Option<String>,
    client_ip: Option<String>,
) -> String {
    let user = ctx
        .app_state
        .user_repository
        .get_user_by_id(user_id)
        .await
        .expect("Failed to load test user");
    let client_app = ctx
        .app_state
        .service
        .client_service()
        .get_client_app_by_client_id(&ctx._realm_id, &ctx._client_id)
        .await
        .expect("Failed to load test client app");
    RedisBrowserTokenService::new(ctx.app_state.redis_manager.clone())
        .create_first_party_token_family(&user, &client_app, user_agent, client_ip)
        .await
        .expect("Failed to create extra token family")
        .access_token
}

/// Grant a single `(resource, action)` permission to a user via a dedicated
/// role. Mirrors `api_keys_permission_scenarios.rs::grant_single_permission`.
async fn grant_single_permission(ctx: &TestContext, user_id: Uuid, resource: &str, action: &str) {
    let role_uuid = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
         VALUES ($1, $2, $3, $4, $5, false)",
    )
    .bind(role_uuid)
    .bind(format!("test-role-{}-{}", resource, action))
    .bind(format!("Test role for {}.{} only", resource, action))
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create single-permission role");

    let policy_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(policy_id)
    .bind(role_uuid)
    .bind(&ctx._realm_id)
    .bind(resource)
    .bind(action)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to add single permission to role");

    let user_role_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
         VALUES ($1, $2, $3, $4, $5, $6, $2::text)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_role_id)
    .bind(user_id)
    .bind(role_uuid)
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .bind(herald_core::domain::authorization::principal_types::USER)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to assign single-permission role to user");

    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_user_role_cache(&ctx._realm_id, &user_id.to_string())
        .await;
}

/// GET `/api/users/{realmId}/{userId}/sessions` with the admin Bearer token.
async fn list_sessions(
    ctx: &TestContext,
    admin_token: &str,
    realm_id: &str,
    user_id: Uuid,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/users/{}/{}/sessions", realm_id, user_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// DELETE `/api/users/{realmId}/{userId}/sessions/{familyId}` (revoke single).
async fn revoke_one_session(
    ctx: &TestContext,
    admin_token: &str,
    realm_id: &str,
    user_id: Uuid,
    family_id: Uuid,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("DELETE")
        .uri(format!(
            "/api/users/{}/{}/sessions/{}",
            realm_id, user_id, family_id
        ))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// DELETE `/api/users/{realmId}/{userId}/sessions` (revoke all).
async fn revoke_all_sessions(
    ctx: &TestContext,
    admin_token: &str,
    realm_id: &str,
    user_id: Uuid,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/users/{}/{}/sessions", realm_id, user_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// PUT `/api/users/{realmId}/{userId}` with `{"status": <i16>}`. Forbidden=2,
/// Normal=1. Mirrors `user_list_scenarios.rs` PUT pattern and
/// `types.rs::UserUpdateRequest.status: Option<i16>`.
async fn update_user_status(
    ctx: &TestContext,
    admin_token: &str,
    realm_id: &str,
    user_id: Uuid,
    status_i16: i16,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();
    let payload = json!({ "status": status_i16 });
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/users/{}/{}", realm_id, user_id))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::from(payload.to_string()))
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// Hit `/api/user/profile` with a Bearer token and return the status code. Used
/// to assert that a revoked session yields 401 on the next request. Mirrors
/// `account_self_delete_scenarios.rs::protected_endpoint_status`.
async fn protected_endpoint_status(ctx: &TestContext, token: &str) -> StatusCode {
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("GET")
        .uri("/api/user/profile")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.unwrap().status()
}

// =============================================================================
// US-RA-020 — list scenarios
// =============================================================================

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — list returns 200 + [] when the user has no sessions
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_user_sessions_list_empty_when_no_sessions(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-list-empty-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-Empty1!").await;

    let resp = list_sessions(ctx, &admin_token, &realm_id, user_id).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let arr = body
        .as_array()
        .expect("list response should be a JSON array");
    assert!(arr.is_empty(), "session list should be empty");
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — list returns active sessions with metadata fields;
///         meta index (UA/IP/createdAt) populated from login/create_family.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_user_sessions_list_returns_active_sessions(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-list-active-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, email) = seed_normal_user_with_password(ctx, &realm_id, "PW-Active1!").await;
    // Session 1: from a real login (writes UA/IP via login flow).
    let _login_token = login_and_get_token(ctx, &realm_id, &email, "PW-Active1!").await;
    // Session 2: direct family creation with explicit UA/IP.
    let _extra_token = create_extra_session(
        ctx,
        user_id,
        Some("test-ua-extra".into()),
        Some("203.0.113.20".into()),
    )
    .await;

    let resp = list_sessions(ctx, &admin_token, &realm_id, user_id).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let arr = body
        .as_array()
        .expect("list response should be a JSON array");
    assert_eq!(arr.len(), 2, "expected two active sessions");

    for entry in arr {
        assert!(
            entry["familyId"].is_string(),
            "familyId must be present and non-null"
        );
        assert!(
            entry["clientAppId"].is_string(),
            "clientAppId must be present and non-null"
        );
        assert!(
            entry["credentialClass"].is_string(),
            "credentialClass must be present and non-null"
        );
        assert!(
            entry["absoluteExpiresAt"].is_string(),
            "absoluteExpiresAt must be present and non-null"
        );
    }

    // At least one entry (the extra session) must surface the UA/IP we wrote.
    let has_extra = arr.iter().any(|e| {
        e["userAgent"].as_str() == Some("test-ua-extra")
            && e["clientIp"].as_str() == Some("203.0.113.20")
            && e["createdAt"].is_string()
    });
    assert!(
        has_extra,
        "expected the explicitly-seeded extra session (test-ua-extra / 203.0.113.20) to surface in the list"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — list count decreases by one after revoking a single
///         family, and the revoked family no longer appears.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_user_sessions_list_decreases_after_revoke_one(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-dec-one-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-Dec1!").await;
    let _t1 = create_extra_session(
        ctx,
        user_id,
        Some("ua-1".into()),
        Some("203.0.113.1".into()),
    )
    .await;
    let _t2 = create_extra_session(
        ctx,
        user_id,
        Some("ua-2".into()),
        Some("203.0.113.2".into()),
    )
    .await;

    let before = list_sessions(ctx, &admin_token, &realm_id, user_id).await;
    let before_body: serde_json::Value = crate::tests::response_json(before).await;
    let before_arr = before_body.as_array().expect("array");
    assert_eq!(before_arr.len(), 2, "expected two sessions before revoke");

    let first_family_id = before_arr[0]["familyId"]
        .as_str()
        .expect("familyId is a string")
        .parse::<Uuid>()
        .expect("familyId is a valid UUID");

    let revoke_resp =
        revoke_one_session(ctx, &admin_token, &realm_id, user_id, first_family_id).await;
    assert_eq!(revoke_resp.status(), StatusCode::NO_CONTENT);

    let after = list_sessions(ctx, &admin_token, &realm_id, user_id).await;
    let after_body: serde_json::Value = crate::tests::response_json(after).await;
    let after_arr = after_body.as_array().expect("array");
    assert_eq!(after_arr.len(), 1, "expected one session after revoke");
    assert!(
        !after_arr
            .iter()
            .any(|e| { e["familyId"].as_str() == Some(first_family_id.to_string().as_str()) }),
        "revoked family must not appear in the list"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — list is empty after revoking all sessions.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_user_sessions_list_empty_after_revoke_all(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-empty-all-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-All1!").await;
    let _t1 = create_extra_session(
        ctx,
        user_id,
        Some("ua-a".into()),
        Some("203.0.113.3".into()),
    )
    .await;
    let _t2 = create_extra_session(
        ctx,
        user_id,
        Some("ua-b".into()),
        Some("203.0.113.4".into()),
    )
    .await;

    let revoke_resp = revoke_all_sessions(ctx, &admin_token, &realm_id, user_id).await;
    assert_eq!(revoke_resp.status(), StatusCode::OK);

    let list_resp = list_sessions(ctx, &admin_token, &realm_id, user_id).await;
    let body: serde_json::Value = crate::tests::response_json(list_resp).await;
    let arr = body.as_array().expect("array");
    assert!(
        arr.is_empty(),
        "session list should be empty after revoke-all"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 / §5.1 — legacy families without a metadata index
///         surface UA/IP/createdAt as null while remaining in the list.
///
/// NOTE: this is marked `#[ignore]` because constructing a family record
/// without the accompanying `bt:meta:{familyId}` hash requires bypassing the
/// BE-D01 `create_family` write path. The legacy/missing-meta behaviour is
/// already covered by the BE-D01 infra unit test
/// `list_user_sessions_handles_missing_meta_for_legacy_families` (see
/// `backend/infra/src/authentication/mod.rs`). Leaving the test in place as a
/// scenario-level marker; runner does not need to execute it.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
#[ignore]
async fn test_user_sessions_list_handles_missing_meta_as_null(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-missing-meta-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-Meta1!").await;

    // TODO(BE-T02): construct a family record via the low-level Redis API
    // without writing the meta hash, then assert UA/IP/createdAt are null.
    // Currently blocked on exposing the family-write Lua privately; the BE-D01
    // unit test already covers this semantics.
    let _ = user_id;

    // Placeholder assertion that the list endpoint returns OK for this user.
    let resp = list_sessions(ctx, &admin_token, &realm_id, user_id).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// =============================================================================
// US-RA-020 — revoke scenarios
// =============================================================================

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — revoke single returns 204 No Content.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_revoke_user_session_returns_204(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-revoke-204-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-2041!").await;
    let _token = create_extra_session(
        ctx,
        user_id,
        Some("ua-x".into()),
        Some("203.0.113.5".into()),
    )
    .await;

    let list_resp = list_sessions(ctx, &admin_token, &realm_id, user_id).await;
    let body: serde_json::Value = crate::tests::response_json(list_resp).await;
    let family_id = body[0]["familyId"]
        .as_str()
        .expect("familyId present")
        .parse::<Uuid>()
        .expect("valid UUID");

    let resp = revoke_one_session(ctx, &admin_token, &realm_id, user_id, family_id).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — revoke all returns 200 with revokedCount == N.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_revoke_all_sessions_returns_revoked_count(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-revoke-count-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-Count1!").await;
    let _t1 = create_extra_session(
        ctx,
        user_id,
        Some("ua-c1".into()),
        Some("203.0.113.6".into()),
    )
    .await;
    let _t2 = create_extra_session(
        ctx,
        user_id,
        Some("ua-c2".into()),
        Some("203.0.113.7".into()),
    )
    .await;

    let resp = revoke_all_sessions(ctx, &admin_token, &realm_id, user_id).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(
        body["revokedCount"].as_i64(),
        Some(2),
        "revokedCount must equal the number of active sessions"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 / §4.5 — revoke single takes effect immediately: the
///         revoked user token is rejected with 401 on the next request.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_revoke_user_session_revoked_token_fails_next_request(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-immediate-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, email) = seed_normal_user_with_password(ctx, &realm_id, "PW-Imm1!").await;
    let user_token = login_and_get_token(ctx, &realm_id, &email, "PW-Imm1!").await;
    assert_eq!(
        protected_endpoint_status(ctx, &user_token).await,
        StatusCode::OK,
        "token must work before revoke"
    );

    let list_resp = list_sessions(ctx, &admin_token, &realm_id, user_id).await;
    let body: serde_json::Value = crate::tests::response_json(list_resp).await;
    let family_id = body[0]["familyId"]
        .as_str()
        .expect("familyId present")
        .parse::<Uuid>()
        .expect("valid UUID");

    let revoke_resp = revoke_one_session(ctx, &admin_token, &realm_id, user_id, family_id).await;
    assert_eq!(revoke_resp.status(), StatusCode::NO_CONTENT);

    assert_eq!(
        protected_endpoint_status(ctx, &user_token).await,
        StatusCode::UNAUTHORIZED,
        "revoked user token must fail with 401 on the next request"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — revoke all invalidates every active token.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_revoke_all_sessions_all_tokens_fail(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-all-fail-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, email) = seed_normal_user_with_password(ctx, &realm_id, "PW-AllFail1!").await;
    let token_a = login_and_get_token(ctx, &realm_id, &email, "PW-AllFail1!").await;
    let token_b = create_extra_session(
        ctx,
        user_id,
        Some("ua-b".into()),
        Some("203.0.113.9".into()),
    )
    .await;

    assert_eq!(
        protected_endpoint_status(ctx, &token_a).await,
        StatusCode::OK
    );
    assert_eq!(
        protected_endpoint_status(ctx, &token_b).await,
        StatusCode::OK
    );

    let resp = revoke_all_sessions(ctx, &admin_token, &realm_id, user_id).await;
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        protected_endpoint_status(ctx, &token_a).await,
        StatusCode::UNAUTHORIZED,
        "token A must fail after revoke-all"
    );
    assert_eq!(
        protected_endpoint_status(ctx, &token_b).await,
        StatusCode::UNAUTHORIZED,
        "token B must fail after revoke-all"
    );
}

// =============================================================================
// US-RA-020 — permission / realm / cross-user scenarios
// =============================================================================

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — list requires users.view; an admin without it is
///         rejected with 403.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_list_sessions_forbidden_without_users_view(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    // Admin with only roles.view (no users.view, no users.manage).
    let (no_view_token, no_view_admin_id) =
        create_admin_session_with_user(ctx, "sessions-no-view-admin@test.com", 1800).await;
    grant_single_permission(
        ctx,
        no_view_admin_id.parse().expect("uuid"),
        "roles",
        "view",
    )
    .await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-NoView1!").await;

    let resp = list_sessions(ctx, &no_view_token, &realm_id, user_id).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "admin without users.view must get 403"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — revoke single requires users.manage; users.view
///         alone does not imply manage (403).
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_revoke_one_forbidden_without_users_manage(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (view_only_token, view_only_admin_id) =
        create_admin_session_with_user(ctx, "sessions-view-only-admin@test.com", 1800).await;
    grant_single_permission(
        ctx,
        view_only_admin_id.parse().expect("uuid"),
        "users",
        "view",
    )
    .await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-VOnly1!").await;
    let _token = create_extra_session(
        ctx,
        user_id,
        Some("ua-vo".into()),
        Some("203.0.113.11".into()),
    )
    .await;

    let random_family_id = Uuid::now_v7();
    let resp =
        revoke_one_session(ctx, &view_only_token, &realm_id, user_id, random_family_id).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "admin with only users.view must get 403 on revoke single"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — revoke all requires users.manage; users.view alone
///         does not imply manage (403).
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_revoke_all_forbidden_without_users_manage(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (view_only_token, view_only_admin_id) =
        create_admin_session_with_user(ctx, "sessions-view-only-all-admin@test.com", 1800).await;
    grant_single_permission(
        ctx,
        view_only_admin_id.parse().expect("uuid"),
        "users",
        "view",
    )
    .await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-VOnlyAll1!").await;

    let resp = revoke_all_sessions(ctx, &view_only_token, &realm_id, user_id).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "admin with only users.view must get 403 on revoke all"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 / §4.5 — list is realm-bound: an admin of one realm
///         querying another realm's path is rejected with 403.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_list_sessions_cross_realm_returns_403(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-cross-realm-list-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-CR1!").await;

    // Hit a different (non-existent) realm's path with this realm's admin.
    let other_realm = "other-realm-sessions";
    let resp = list_sessions(ctx, &admin_token, other_realm, user_id).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "cross-realm list must return 403"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 / §4.5 — revoke single is realm-bound (403).
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_revoke_one_cross_realm_returns_403(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-cross-realm-revoke-admin@test.com", 1800)
            .await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-CRRev1!").await;

    let other_realm = "other-realm-sessions";
    let resp = revoke_one_session(ctx, &admin_token, other_realm, user_id, Uuid::now_v7()).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "cross-realm revoke single must return 403"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — list returns 404 when the target user does not
///         exist in the realm.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_list_sessions_target_user_not_found_returns_404(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-nf-list-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let random_user_id = Uuid::now_v7();
    let resp = list_sessions(ctx, &admin_token, &realm_id, random_user_id).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "list for non-existent user must return 404"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — revoke single returns 404 when the target user
///         does not exist in the realm (checked before family ownership).
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_revoke_one_target_user_not_found_returns_404(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-nf-revoke-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let random_user_id = Uuid::now_v7();
    let resp =
        revoke_one_session(ctx, &admin_token, &realm_id, random_user_id, Uuid::now_v7()).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "revoke single for non-existent user must return 404"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — revoking another user's family under this user's
///         path returns 404 (no cross-user leak). User A owns familyIdA;
///         admin revokes familyIdA under user B's path -> 404.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_revoke_one_cross_user_family_returns_404(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-cross-user-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    // User A creates a session.
    let (user_a, _email_a) = seed_normal_user_with_password(ctx, &realm_id, "PW-CU1!").await;
    let _t_a = create_extra_session(
        ctx,
        user_a,
        Some("ua-cua".into()),
        Some("203.0.113.12".into()),
    )
    .await;
    let list_a = list_sessions(ctx, &admin_token, &realm_id, user_a).await;
    let body_a: serde_json::Value = crate::tests::response_json(list_a).await;
    let family_a = body_a[0]["familyId"]
        .as_str()
        .expect("familyId present")
        .parse::<Uuid>()
        .expect("valid UUID");

    // User B exists but does not own family_a.
    let (user_b, _email_b) = seed_normal_user_with_password(ctx, &realm_id, "PW-CU2!").await;

    let resp = revoke_one_session(ctx, &admin_token, &realm_id, user_b, family_a).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "revoking family A under user B's path must return 404 (no cross-user leak)"
    );
}

/// ============================================================================
/// User Story: US-RA-020
/// Covers: Design §4.2.2 — concurrent/no-op revoke of a non-existent family
///         returns 204 (successful empty operation). If the implementation
///         instead returns 404, that is a production-vs-design semantics
///         conflict and the runner must stop and report
///         `requires_test_semantics_change`; this assertion is written per the
///         design's 204 semantics.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_revoke_one_nonexistent_family_returns_204(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "sessions-noop-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-Noop1!").await;

    // Note: per design §4.2.2 a family that is absent from the user's active
    // set yields 404 (ownership guard). A family that exists and belongs to the
    // user but is already revoked/expired is the concurrent no-op case that
    // returns 204. We construct the latter by creating a family, revoking it,
    // then revoking it again.
    let _token = create_extra_session(
        ctx,
        user_id,
        Some("ua-noop".into()),
        Some("203.0.113.13".into()),
    )
    .await;
    let list_resp = list_sessions(ctx, &admin_token, &realm_id, user_id).await;
    let body: serde_json::Value = crate::tests::response_json(list_resp).await;
    let family_id = body[0]["familyId"]
        .as_str()
        .expect("familyId present")
        .parse::<Uuid>()
        .expect("valid UUID");

    // First revoke: real effect.
    let first = revoke_one_session(ctx, &admin_token, &realm_id, user_id, family_id).await;
    assert_eq!(first.status(), StatusCode::NO_CONTENT);

    // Second revoke of the same (now-revoked) family: concurrent no-op must be 204.
    // The family is no longer in the active list, so the ownership guard returns
    // 404 here; the 204 semantics apply to the lower-level idempotent
    // `revoke_family` primitive which is exercised by the revoke-all path and
    // the BE-D01 unit tests. We assert the design's 204 contract against the
    // handler; if the handler returns 404, runner classifies as a semantics
    // conflict (requires_test_semantics_change).
    let second = revoke_one_session(ctx, &admin_token, &realm_id, user_id, family_id).await;
    assert_eq!(
        second.status(),
        StatusCode::NO_CONTENT,
        "concurrent/already-revoked family revoke must be a 204 no-op per design §4.2.2"
    );
}

// =============================================================================
// US-RA-021 — Forbidden linkage scenarios
// =============================================================================

/// ============================================================================
/// User Story: US-RA-021
/// Covers: Design §5.3 — setting a user's status to Forbidden revokes their
///         sessions; the user's token is rejected with 401 on the next request.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_update_user_to_forbidden_revokes_sessions(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "forbidden-revoke-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, email) = seed_normal_user_with_password(ctx, &realm_id, "PW-Forb1!").await;
    let user_token = login_and_get_token(ctx, &realm_id, &email, "PW-Forb1!").await;
    assert_eq!(
        protected_endpoint_status(ctx, &user_token).await,
        StatusCode::OK
    );

    // PUT status=2 (Forbidden) triggers session revocation.
    let resp = update_user_status(ctx, &admin_token, &realm_id, user_id, 2).await;
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        protected_endpoint_status(ctx, &user_token).await,
        StatusCode::UNAUTHORIZED,
        "user token must be revoked after Forbidden status change"
    );
}

/// ============================================================================
/// User Story: US-RA-021
/// Covers: Design §5.3 — setting status to Forbidden revokes ALL of the user's
///         sessions, not just one.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_update_user_to_forbidden_revokes_all_sessions(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "forbidden-revoke-all-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, email) = seed_normal_user_with_password(ctx, &realm_id, "PW-ForbAll1!").await;
    let token_a = login_and_get_token(ctx, &realm_id, &email, "PW-ForbAll1!").await;
    let token_b = create_extra_session(
        ctx,
        user_id,
        Some("ua-fb".into()),
        Some("203.0.113.14".into()),
    )
    .await;

    assert_eq!(
        protected_endpoint_status(ctx, &token_a).await,
        StatusCode::OK
    );
    assert_eq!(
        protected_endpoint_status(ctx, &token_b).await,
        StatusCode::OK
    );

    let resp = update_user_status(ctx, &admin_token, &realm_id, user_id, 2).await;
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        protected_endpoint_status(ctx, &token_a).await,
        StatusCode::UNAUTHORIZED,
        "token A must be revoked after Forbidden change"
    );
    assert_eq!(
        protected_endpoint_status(ctx, &token_b).await,
        StatusCode::UNAUTHORIZED,
        "token B must be revoked after Forbidden change"
    );
}

/// ============================================================================
/// User Story: US-RA-021
/// Covers: Design §5.3 — a status change to Normal (non-Forbidden target) does
///         NOT revoke the user's sessions; the token remains valid.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_update_user_to_normal_does_not_revoke(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "normal-no-revoke-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, email) = seed_normal_user_with_password(ctx, &realm_id, "PW-Norm1!").await;
    let user_token = login_and_get_token(ctx, &realm_id, &email, "PW-Norm1!").await;
    assert_eq!(
        protected_endpoint_status(ctx, &user_token).await,
        StatusCode::OK
    );

    // PUT status=1 (Normal) — target is non-Forbidden, so no revocation.
    let resp = update_user_status(ctx, &admin_token, &realm_id, user_id, 1).await;
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        protected_endpoint_status(ctx, &user_token).await,
        StatusCode::OK,
        "token must remain valid when status changes to Normal"
    );
}

/// ============================================================================
/// User Story: docs/user-stories/core/realm-admin.md - US-RA-021
/// Covers: re-saving a Forbidden user as Forbidden (idempotent) does NOT
///         re-revoke; an existing session created via the back door
///         (bypassing the login gate) stays unrevoked at the token-family
///         level (bearer auth rejects Forbidden users regardless). The
///         linkage fires only on a transition INTO Forbidden.
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_update_user_keep_forbidden_no_revoke(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "keep-forbidden-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_normal_user_with_password(ctx, &realm_id, "PW-Keep1!").await;
    // Seed the user as already Forbidden via direct SQL (avoids triggering the
    // linkage, which only fires from the update endpoint).
    set_user_status_directly(ctx, user_id, 2).await;

    // Forbidden users cannot log in, so create the session directly.
    let token = create_extra_session(
        ctx,
        user_id,
        Some("ua-kf".into()),
        Some("203.0.113.15".into()),
    )
    .await;
    // Bearer auth rejects Forbidden users regardless of session state
    // (defense in depth), so "session alive" must be observed at the token
    // family level: the access token stays resolvable in Redis until the
    // family is revoked.
    let browser_tokens = RedisBrowserTokenService::new(ctx.app_state.redis_manager.clone());
    assert!(
        browser_tokens
            .lookup_access_token(&token)
            .await
            .unwrap()
            .is_some(),
        "back-door session must start unrevoked"
    );

    // PUT status=2 (Forbidden again) — idempotent, no transition INTO Forbidden.
    let resp = update_user_status(ctx, &admin_token, &realm_id, user_id, 2).await;
    assert_eq!(resp.status(), StatusCode::OK);

    assert!(
        browser_tokens
            .lookup_access_token(&token)
            .await
            .unwrap()
            .is_some(),
        "token family must stay unrevoked when re-saving Forbidden as Forbidden (no re-revoke)"
    );
}
