//
// End-to-end HTTP scenario tests for the admin user-management guards:
//   PUT  /api/users/{userId}                 (edit)
//   POST /api/users/{userId}/reset-password  (password reset)
//   PUT  /api/users/{userId}/roles           (role replace)
//   POST /api/users                          (create)
//   GET  /api/users/{userId}                 (read)
//
// Guards:
//   - Deleted(3) is the anonymizing self-deletion terminal state
//     (users.md §4.2 「不可恢复」). The account row survives only as a
//     compliance record: admin edits must not mutate or revive the
//     tombstone (a status flip back to Normal or a password reset would
//     produce a live account shell).
//   - GET /api/users/{userId} cross-realm targets must 404 — the
//     same id-oracle convention as the module's require_target_user_in_realm
//     helper (update/delete/reset-password already 404).
//   - PUT /api/users/{userId}/roles follows the same id-oracle for missing /
//     cross-realm targets (404, not the pre-fix catch-all 500 that its own
//     OpenAPI 404 declaration already promised).
//   - Duplicate email on admin create returns 409 Conflict, unified
//     with the ext user-creation and self-service change-email faces
//     (users.md §4.1: 邮箱冲突返回 409).
//

use crate::tests::helpers::*;
use crate::tests::response_json;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;

/// Seed an account row directly with the given status (0..=3), bypassing the
/// admin update endpoint. Self-delete is the only legal producer of status=3
/// (the anonymization pipeline); seeding it directly is the test stand-in.
/// Returns (user_id, email).
async fn seed_user_with_status(ctx: &TestContext, status: i16, tag: &str) -> (Uuid, String) {
    let user_id = Uuid::now_v7();
    let email = format!("{}-{}@test.com", tag, user_id.simple());
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, NOW(), NOW())",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .bind(&email)
    .bind("$2a$12$dummy_password_hash")
    .bind(status)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed test user");
    (user_id, email)
}

/// Seed an account row in a foreign realm (account.realm_id is free text with
/// no FK), representing another tenant's user for cross-realm assertions.
async fn seed_user_in_foreign_realm(ctx: &TestContext, tag: &str) -> (Uuid, String) {
    let user_id = Uuid::now_v7();
    let email = format!("{}-{}@test.com", tag, user_id.simple());
    let foreign_realm = format!("foreign-realm-{}", Uuid::now_v7().simple());
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 1, NOW(), NOW())",
    )
    .bind(user_id)
    .bind(foreign_realm)
    .bind(&email)
    .bind("$2a$12$dummy_password_hash")
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed foreign-realm test user");
    (user_id, email)
}

async fn read_account_status(ctx: &TestContext, user_id: Uuid) -> i16 {
    sqlx::query_scalar("SELECT status FROM account WHERE id = $1")
        .bind(user_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("Failed to read account status")
}

/// PUT `/api/users/{userId}` with an arbitrary JSON body.
async fn put_update_user(
    ctx: &TestContext,
    admin_token: &str,
    user_id: Uuid,
    body: serde_json::Value,
) -> StatusCode {
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/users/{}", user_id))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::from(body.to_string()))
        .unwrap();
    app.oneshot(req).await.unwrap().status()
}

/// PUT `/api/users/{userId}/roles` with the given role ids.
async fn put_user_roles(
    ctx: &TestContext,
    admin_token: &str,
    user_id: Uuid,
    role_ids: &[Uuid],
) -> StatusCode {
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/users/{}/roles", user_id))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::from(json!({ "roleIds": role_ids }).to_string()))
        .unwrap();
    app.oneshot(req).await.unwrap().status()
}

/// GET `/api/users/{userId}`.
async fn get_user(ctx: &TestContext, admin_token: &str, user_id: Uuid) -> axum::response::Response {
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/users/{}", user_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// POST `/api/users` (admin create). Returns the raw response.
async fn post_create_user(
    ctx: &TestContext,
    admin_token: &str,
    body: serde_json::Value,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("POST")
        .uri("/api/users".to_string())
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::from(body.to_string()))
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// WHY: users.md §4.2 marks Deleted(3) as the anonymizing terminal state
///      (不可恢复). Before the guard, PUT could flip a Deleted tombstone back
///      to Normal(1)/Forbidden(2) or edit its nickname — breaking the state
///      machine invariant and the compliance record. Both edit shapes must be
///      refused with 409 and the stored status must stay 3.
#[test_context(TestContext)]
#[tokio::test]
async fn test_update_deleted_user_returns_409_and_tombstone_unchanged(ctx: &mut TestContext) {
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "deleted-edit-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_user_with_status(ctx, 3, "deleted-edit").await;

    // Attempted revival: flip status back to Normal.
    let status_attempt = put_update_user(
        ctx,
        &admin_token,
        user_id,
        json!({ "status": "normal", "nickname": "revived" }),
    )
    .await;
    assert_eq!(
        status_attempt,
        StatusCode::CONFLICT,
        "editing a Deleted user must be refused with 409"
    );

    // Even a "harmless" nickname-only edit must not touch the tombstone.
    let nickname_attempt = put_update_user(
        ctx,
        &admin_token,
        user_id,
        json!({ "nickname": "tombstone-nick" }),
    )
    .await;
    assert_eq!(
        nickname_attempt,
        StatusCode::CONFLICT,
        "nickname-only edits on a Deleted user must also be refused"
    );

    // The stored state is untouched — the terminal state is not recoverable
    // through the admin edit surface.
    assert_eq!(
        read_account_status(ctx, user_id).await,
        3,
        "Deleted status must survive rejected edit attempts"
    );
}

/// WHY: the Deleted guard is a terminal-state guard, not a blanket edit ban —
///      routine admin edits of Normal(1) users (enable/disable, nickname) are
///      the module's primary function and must keep working.
#[test_context(TestContext)]
#[tokio::test]
async fn test_update_normal_user_returns_200(ctx: &mut TestContext) {
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "normal-edit-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_user_with_status(ctx, 1, "normal-edit").await;

    let status = put_update_user(
        ctx,
        &admin_token,
        user_id,
        json!({ "nickname": "legit-nick" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "Normal user edit must succeed");

    assert_eq!(
        read_account_status(ctx, user_id).await,
        1,
        "Normal user must stay Normal after a nickname edit"
    );
}

/// WHY: an admin password reset writes a fresh credential hash onto the
///      account row. On a Deleted tombstone (whose own credentials were
///      destroyed by anonymization) that is the strongest revival vector —
///      the account would become login-capable again.
#[test_context(TestContext)]
#[tokio::test]
async fn test_reset_password_deleted_user_returns_409(ctx: &mut TestContext) {
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "deleted-reset-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_user_with_status(ctx, 3, "deleted-reset").await;

    let app = ctx.create_unified_test_router();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/users/{}/reset-password", user_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "password reset on a Deleted user must be refused with 409"
    );
}

/// WHY: role assignment (users-module PUT replace) rewrites the target's
///      user_roles rows. On a Deleted tombstone that mutates the compliance
///      record and dresses the shell with privileges.
#[test_context(TestContext)]
#[tokio::test]
async fn test_update_roles_deleted_user_returns_409(ctx: &mut TestContext) {
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "deleted-roles-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let (user_id, _email) = seed_user_with_status(ctx, 3, "deleted-roles").await;
    let role_id = create_role(ctx, &ctx._realm_id, &admin_token, "role-deleted-guard", "R").await;

    let status = put_user_roles(ctx, &admin_token, user_id, &[role_id]).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "role update on a Deleted user must be refused with 409"
    );

    let assigned: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_roles WHERE user_id = $1 AND realm_id = $2")
            .bind(user_id)
            .bind(&ctx._realm_id)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to count user_roles");
    assert_eq!(assigned, 0, "no role row may be written for a Deleted user");
}

/// WHY: the module's require_target_user_in_realm helper deliberately returns
///      UserNotFound for a wrong-realm target so a foreign id is
///      indistinguishable from a missing one (no id oracle). GET used to map
///      the same case to 403, leaking "this id exists in another realm". It
///      must 404 like its sibling write paths; an in-realm target must keep
///      returning 200 with the full detail payload.
#[test_context(TestContext)]
#[tokio::test]
async fn test_get_user_cross_realm_returns_404_in_realm_returns_200(ctx: &mut TestContext) {
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "get-cross-realm-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    // Cross-realm target: same shape as a missing id (404).
    let (foreign_id, _foreign_email) = seed_user_in_foreign_realm(ctx, "get-cross").await;
    let resp = get_user(ctx, &admin_token, foreign_id).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "cross-realm GET must 404 (indistinguishable from missing, no id oracle)"
    );

    // In-realm behavior is unchanged: 200 + detail fields.
    let (local_id, local_email) = seed_user_with_status(ctx, 1, "get-local").await;
    let resp = get_user(ctx, &admin_token, local_id).await;
    assert_eq!(resp.status(), StatusCode::OK, "in-realm GET must succeed");
    let body: serde_json::Value = response_json(resp).await;
    assert_eq!(body["email"].as_str(), Some(local_email.as_str()));
    assert_eq!(body["status"].as_str(), Some("normal"));
    assert!(
        body["providerIds"].is_array(),
        "in-realm detail payload must still include providerIds"
    );
}

/// WHY: PUT roles' error mapping dropped UserNotFound into the catch-all 500
///      arm, so a missing or cross-realm target id — the exact id-oracle case
///      every sibling face maps to 404 and this endpoint's own OpenAPI
///      declaration promises — surfaced as "Unexpected error". A 5xx on a
///      client addressing error is wrong on both ends: it breaks the uniform
///      404 convention (no id oracle across realms) and pollutes 5xx
///      monitoring with a 4xx-class condition.
#[test_context(TestContext)]
#[tokio::test]
async fn test_update_roles_missing_or_cross_realm_user_returns_404(ctx: &mut TestContext) {
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "roles-404-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let role_id = create_role(ctx, &ctx._realm_id, &admin_token, "role-404-guard", "R").await;

    // Missing id: a random UUID with no account row anywhere.
    let missing_status = put_user_roles(ctx, &admin_token, Uuid::now_v7(), &[role_id]).await;
    assert_eq!(
        missing_status,
        StatusCode::NOT_FOUND,
        "role update on a missing user must 404 (uniform id-oracle, not the catch-all 500)"
    );

    // Cross-realm target: same shape as a missing id (404, no id oracle).
    let (foreign_id, _foreign_email) = seed_user_in_foreign_realm(ctx, "roles-cross").await;
    let foreign_status = put_user_roles(ctx, &admin_token, foreign_id, &[role_id]).await;
    assert_eq!(
        foreign_status,
        StatusCode::NOT_FOUND,
        "role update on a cross-realm user must 404 (indistinguishable from missing)"
    );
}

/// WHY: the same duplicate-email condition used to be a 400 on the admin
///      face while the ext face and self-service change-email already
///      returned 409 (users.md §4.1). One condition, one code: the second
///      create must 409 so API consumers can distinguish "conflict with an
///      existing account" from "malformed request".
#[test_context(TestContext)]
#[tokio::test]
async fn test_create_user_duplicate_email_returns_409(ctx: &mut TestContext) {
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "dup-email-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let email = format!("dup-create-{}@test.com", Uuid::now_v7().simple());
    let body = json!({
        "email": email,
        "password": "Password123!",
        "roleIds": [],
    });

    let first = post_create_user(ctx, &admin_token, body.clone()).await;
    assert_eq!(
        first.status(),
        StatusCode::CREATED,
        "first create must succeed"
    );

    let second = post_create_user(ctx, &admin_token, body).await;
    assert_eq!(
        second.status(),
        StatusCode::CONFLICT,
        "duplicate email on admin create must return 409 Conflict"
    );
}
