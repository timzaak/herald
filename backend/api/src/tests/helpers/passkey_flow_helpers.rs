// =============================================================================
// Passkey Flow Test Helpers
// =============================================================================
//
// Shared orchestration for the WebAuthn registration ceremony, the realm
// passkey configuration, and the per-user rate-limit key. Used by both the
// passkey API scenarios and the OIDC scenarios, so the ceremony (begin →
// clear rate limit → finish, one reauth token per step) and the
// `rl:passkey:user:{id}` Redis key live in exactly one place. They MUST stay
// mechanically in sync with `backend/api-auth/src/user_passkey.rs`.
//
// Not exported via `pub use` — imported explicitly by the scenario files,
// mirroring the `otp_helpers` / `passkey_authenticator` pattern.

#![allow(dead_code)]

use crate::tests::helpers::auth_helpers::obtain_reauth_token;
use crate::tests::helpers::passkey_authenticator::Es256Authenticator;
use crate::tests::schema_test_context::SchemaTestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use redis::AsyncCommands;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

/// Origin the passkey RP declares in tests; mirrors the `RP_ORIGIN` env var
/// the scenarios set before exercising passkey endpoints.
pub const RP_ORIGIN: &str = "https://localhost";

/// Enable/disable the realm passkey login entrance (`realm_config` upsert).
pub async fn setup_realm_passkey_config(ctx: &SchemaTestContext, realm_id: &str, enabled: bool) {
    let config_value = json!({ "enabled": enabled });

    sqlx::query(
        "INSERT INTO realm_config
            (id, realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, $2, 'passkey', 'settings', $3, false, $4, NULL, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value,
                       enabled = EXCLUDED.enabled,
                       updated_at = NOW()",
    )
    .bind(Uuid::now_v7())
    .bind(realm_id)
    .bind(config_value.to_string())
    .bind(enabled)
    .execute(&ctx._app_state.pool)
    .await
    .expect("passkey realm config should upsert");
}

/// Delete the per-user passkey rate-limit counter so a test ceremony is not
/// judged as a retry storm.
pub async fn clear_passkey_user_rate_limit(ctx: &SchemaTestContext, user_id: &str) {
    let mut conn = ctx._app_state.redis_manager.get().await.unwrap();
    let _: () = conn
        .del(format!("rl:passkey:user:{user_id}"))
        .await
        .expect("passkey user rate limit key should clear");
}

/// POST /api/user/passkey/registration/begin. Returns
/// `(options, regToken)` for the matching finish call.
pub async fn begin_registration(
    ctx: &SchemaTestContext,
    session_token: &str,
    password: &str,
    nickname: Option<&str>,
) -> (Value, String) {
    let reauth_token =
        obtain_reauth_token(ctx, session_token, "bind_authenticator", password).await;
    let mut payload = json!({ "reauthToken": reauth_token });
    if let Some(name) = nickname {
        payload["nickname"] = json!(name);
    }

    let req = Request::builder()
        .method("POST")
        .uri("/api/user/passkey/registration/begin")
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {session_token}"))
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = crate::tests::response_json(response).await;

    (
        body["options"].clone(),
        body["regToken"]
            .as_str()
            .expect("regToken should be present")
            .to_string(),
    )
}

/// POST /api/user/passkey/registration/finish. Returns the response body.
pub async fn finish_registration(
    ctx: &SchemaTestContext,
    session_token: &str,
    password: &str,
    authenticator: &mut Es256Authenticator,
    reg_token: &str,
    options: Value,
) -> Value {
    let attestation = authenticator.register(&options, RP_ORIGIN);
    let reauth_token =
        obtain_reauth_token(ctx, session_token, "bind_authenticator", password).await;
    let payload = json!({
        "reauthToken": reauth_token,
        "regToken": reg_token,
        "attestation": attestation
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/user/passkey/registration/finish")
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {session_token}"))
        .body(Body::from(payload.to_string()))
        .unwrap();
    let response = ctx.create_unified_test_router().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    crate::tests::response_json(response).await
}

/// Drive the full registration ceremony for a signed-in user and return the
/// new credential id. The rate-limit counter is cleared before and after the
/// finish call — the ceremony legitimately makes several attempts per second.
pub async fn register_one_passkey(
    ctx: &SchemaTestContext,
    session_token: &str,
    user_id: &str,
    password: &str,
    nickname: Option<&str>,
    authenticator: &mut Es256Authenticator,
) -> String {
    let (options, reg_token) = begin_registration(ctx, session_token, password, nickname).await;
    clear_passkey_user_rate_limit(ctx, user_id).await;
    let body = finish_registration(
        ctx,
        session_token,
        password,
        authenticator,
        &reg_token,
        options,
    )
    .await;
    clear_passkey_user_rate_limit(ctx, user_id).await;

    body["credentialId"]
        .as_str()
        .expect("credentialId should be present")
        .to_string()
}
