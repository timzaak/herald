use crate::tests::helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_update_realm_totp_config_returns_wrapped_response(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "totp-config-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/realms/{}/config/totp", ctx._realm_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            json!({
                "enabled": true,
                "forceEnabled": true
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["message"], "Realm TOTP configuration updated");
    assert_eq!(body["enabled"], true);
    assert_eq!(body["forceEnabled"], true);
    assert!(body["meta"].is_null());

    // MFA-policy changes are "关键配置变更" (audit PRD): the PUT must leave an
    // audit trail naming the new policy values.
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE realm_id = $1 AND action = 'totp_config.update'
           AND details->>'enabled' = 'true' AND details->>'force_enabled' = 'true'",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        audit_count, 1,
        "TOTP config update must be audited with the new policy values"
    );
}

#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_get_realm_totp_config_returns_wrapped_response(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "totp-config-viewer@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, 'totp', 'settings', $2, false, true, '{}'::jsonb, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled, updated_at = NOW()",
    )
    .bind(&ctx._realm_id)
    .bind(r#"{"enabled":true,"force_enabled":false}"#)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/realms/{}/config/totp", ctx._realm_id))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["forceEnabled"], false);
    assert!(body["statistics"]["totalUsers"].is_number());
    assert!(body["statistics"]["enablementRate"].is_number());
    assert!(body["meta"].is_null());
}

/// ============================================================================
/// Scenario 3: Force Enable TOTP for All Users
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_force_enable_totp_for_all_users(ctx: &mut TestContext) {
    // ============================================================================
    // Given: A realm admin with TOTP enabled but not forced
    // ============================================================================
    println!("[Step 1] Setup realm with TOTP enabled");

    let app = ctx.create_unified_test_router();
    let (admin_token, user_id) =
        create_admin_session_with_user(ctx, "force-totp-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Enable TOTP without forcing
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, 'totp', 'settings', $2, false, true, '{}'::jsonb, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled, updated_at = NOW()",
    )
    .bind(&ctx._realm_id)
    .bind(r#"{"enabled":true,"force_enabled":false}"#)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    println!("[Step 1] ✓ TOTP enabled (not forced)");

    // ============================================================================
    // When: Admin sets force_enabled to true
    // ============================================================================
    println!("[Step 2] Force enable TOTP");

    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/realms/{}/config/totp", ctx._realm_id))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(Body::from(
            json!({
                "enabled": true,
                "forceEnabled": true
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    println!("[Step 2] ✓ Force enabled TOTP");

    // ============================================================================
    // Then: Verify force_enabled is persisted
    // ============================================================================
    println!("[Step 3] Verify force_enabled configuration");

    let config_value: String = sqlx::query_scalar(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = 'totp' AND config_key = 'settings'",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to fetch TOTP config");

    assert!(
        config_value.contains(r#""force_enabled":true"#),
        "force_enabled should be true"
    );

    println!("[Step 3] ✓ Configuration verified: force_enabled=true");
    println!("\n✅ Scenario 3 完成：Realm管理员强制启用TOTP成功");
}
