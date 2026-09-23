// =============================================================================
// Device Code test helpers
// =============================================================================
//
// Helper functions for device authorization grant scenario tests.

#![allow(dead_code)]

use crate::tests::schema_test_context::SchemaTestContext;
use axum::{body::Body, http::Request};
use redis::AsyncCommands;
use serde_json::json;
use tower::ServiceExt;

/// Create a Client App with device code grant settings via the API.
///
/// Sends POST `/api/client` with admin session cookie and JSON body
/// including `deviceCodeGrantEnabled` field.
///
/// Returns the raw HTTP response.
pub async fn create_client_app_with_device_code_grant(
    ctx: &mut SchemaTestContext,
    _realm_id: &str,
    admin_token: &str,
    client_id: &str,
    name: &str,
    enabled: bool,
    grant_enabled: bool,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let request = Request::builder()
        .method("POST")
        .uri("/api/client".to_string())
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", admin_token))
        .body(Body::from(
            json!({
                "clientId": client_id,
                "name": name,
                "description": format!("{} description", name),
                "redirectUris": ["https://example.com/callback"],
                "enabled": enabled,
                "browserRefreshAbsoluteTtlSeconds": 86400,
                "deviceCodeGrantEnabled": grant_enabled
            })
            .to_string(),
        ))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

/// Send a device authorization request.
///
/// Sends POST `/api/device/{realmId}/authorize` with form-urlencoded `client_id`.
pub async fn device_authorize(
    ctx: &SchemaTestContext,
    realm_id: &str,
    client_id: &str,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/device/{}/authorize", realm_id))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!("client_id={}", client_id)))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

/// Send a device token poll request.
///
/// Sends POST `/api/device/{realmId}/token` with form-urlencoded `grant_type` and `device_code`.
pub async fn device_token_poll(
    ctx: &SchemaTestContext,
    realm_id: &str,
    device_code: &str,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/device/{}/token", realm_id))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "grant_type=urn:ietf:params:oauth:grant-type:device_code&device_code={}",
            device_code
        )))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

/// Send a device verify request (authenticated).
///
/// Sends POST `/api/device/{realmId}/verify` with JSON body including `user_code`.
pub async fn device_verify(
    ctx: &SchemaTestContext,
    realm_id: &str,
    user_code: &str,
    session_token: &str,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/device/{}/verify", realm_id))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", session_token))
        .body(Body::from(json!({ "user_code": user_code }).to_string()))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

/// Send a device confirm request (authenticated).
///
/// Sends POST `/api/device/{realmId}/confirm` with JSON body including `user_code` and `approved`.
pub async fn device_confirm(
    ctx: &SchemaTestContext,
    realm_id: &str,
    user_code: &str,
    approved: bool,
    session_token: &str,
) -> axum::response::Response {
    let app = ctx.create_unified_test_router();

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/device/{}/confirm", realm_id))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", session_token))
        .body(Body::from(
            json!({
                "user_code": user_code,
                "approved": approved
            })
            .to_string(),
        ))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

/// Update the status (and optionally user_id) of a device code in Redis.
///
/// Reads the `device:{device_code}` JSON, merges `status` and `user_id`,
/// then writes it back with KEEPTTL so the original TTL is preserved.
pub async fn set_device_code_status_redis(
    ctx: &SchemaTestContext,
    device_code: &str,
    status: &str,
    user_id: Option<&str>,
) {
    let mut conn = ctx._app_state.redis_manager.get().await.unwrap();
    let key = format!("device:{}", device_code);

    // Read current state
    let raw: Option<String> = redis::cmd("GET")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .unwrap();
    let raw = raw.expect("device code key should exist in Redis");

    let mut state: serde_json::Value = serde_json::from_str(&raw).unwrap();
    state["status"] = json!(status);
    if let Some(uid) = user_id {
        state["user_id"] = json!(uid);
    }

    // Write back with KEEPTTL
    redis::cmd("SET")
        .arg(&key)
        .arg(state.to_string())
        .arg("KEEPTTL")
        .query_async::<String>(&mut conn)
        .await
        .unwrap();
}

/// Delete device code keys from Redis to simulate expiry.
///
/// Removes both `device:{device_code}` and `deviceUserCode:{user_code}` keys.
pub async fn delete_device_code_redis(ctx: &SchemaTestContext, device_code: &str, user_code: &str) {
    let mut conn = ctx._app_state.redis_manager.get().await.unwrap();
    let device_key = format!("device:{}", device_code);
    let user_code_key = format!("deviceUserCode:{}", user_code);

    let _: () = conn.del(&device_key).await.unwrap();
    let _: () = conn.del(&user_code_key).await.unwrap();
}
