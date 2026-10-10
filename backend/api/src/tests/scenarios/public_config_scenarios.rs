// =============================================================================
// Public Configuration API Endpoint Scenarios
// =============================================================================
//
// 测试 GET /api/{realmId}/public-config 端点的各种场景
//
// **测试目标**：
// 1. 验证端点可以在不同配置下正确返回公共配置
// 2. 验证端点无需认证即可访问
// 3. 验证响应格式符合 OpenAPI 规范
// 4. 验证边界情况（不存在的 realm）
//
// **运行方式**：
// ```bash
// cargo nextest run --workspace public_config_scenarios
// ```
//
// =============================================================================

use crate::application::http::public_config::PublicConfigResponse;
use crate::tests::helpers::email_config_helpers::{
    delete_email_config_direct, insert_resend_email_config_direct,
};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

async fn upsert_public_realm_config(
    ctx: &TestContext,
    realm_id: &str,
    config_type: &str,
    config_key: &str,
    config_value: &str,
) {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, $2, $3, $4, true)
         ON CONFLICT (realm_id, config_type, config_key) DO UPDATE
         SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled",
    )
    .bind(realm_id)
    .bind(config_type)
    .bind(config_key)
    .bind(config_value)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();
}

async fn get_public_config(
    ctx: &TestContext,
    realm_id: &str,
) -> (StatusCode, serde_json::Value, PublicConfigResponse) {
    let app = ctx.create_unified_test_router();
    let request = Request::builder()
        .uri(format!("/api/public-config/{}", realm_id))
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let response_value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let response_json: PublicConfigResponse =
        serde_json::from_value(response_value.clone()).unwrap();

    (status, response_value, response_json)
}

fn assert_empty_white_label(response: &PublicConfigResponse) {
    assert!(response.white_label.logo_url.is_none());
    assert!(response.white_label.accent_color.is_none());
    assert!(response.white_label.background.is_none());
    assert!(response.white_label.footer_text.is_none());
    assert!(response.white_label.login_title.is_none());
    assert!(response.white_label.login_subtitle.is_none());
    assert!(response.white_label.register_title.is_none());
    assert!(response.white_label.register_subtitle.is_none());
}

/// ============================================================================
/// User Story: Public config with registration enabled, no OAuth providers
///
/// **场景描述**：
/// 当 realm 配置了允许注册，但未配置 OAuth 提供商时，端点应返回正确的注册配置和空的 OAuth 提供商列表。
///
/// **验收标准**：
/// - registration.enabled = true
/// - registration.requireEmailVerification = false (默认值)
/// - oauth_providers 为空数组
/// - 响应状态码为 200
/// - 无需认证即可访问
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_public_config_registration_enabled_no_oauth
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_public_config_registration_enabled_no_oauth(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();

    // 配置 realm: 允许注册
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)
         ON CONFLICT (realm_id, config_type, config_key) DO UPDATE
         SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled",
    )
    .bind(&realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    // 创建路由（无需认证）
    let app = ctx.create_unified_test_router();

    // 发送请求（无需认证）
    let request = Request::builder()
        .uri(format!("/api/public-config/{}", realm_id))
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 解析响应
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let response_value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let response_json: PublicConfigResponse =
        serde_json::from_value(response_value.clone()).unwrap();

    // 验证响应
    assert!(response_json.registration.enabled);
    assert!(!response_json.registration.require_email_verification);
    assert!(response_json.oauth_providers.is_empty());

    println!("✅ Test passed: registration enabled, no OAuth providers");
}

/// ============================================================================
/// User Story: Public config with registration disabled, OAuth providers present
///
/// **场景描述**：
/// 当 realm 禁止注册，但配置了 OAuth 提供商时，端点应返回正确的配置。
///
/// **验收标准**：
/// - registration.enabled = false
/// - registration.requireEmailVerification = false (默认值)
/// - oauth_providers 包含配置的提供商
/// - 响应状态码为 200
/// - 无需认证即可访问
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_public_config_registration_disabled_with_oauth
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_public_config_registration_disabled_with_oauth(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();

    // 配置 realm: 禁止注册
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'false', true)
         ON CONFLICT (realm_id, config_type, config_key) DO UPDATE
         SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled",
    )
    .bind(&realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    // 添加 OAuth 提供商
    let google_provider_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO oauth_provider_config (id, realm_id, provider_type, client_id, client_secret, scopes, enabled)
         VALUES ($1, $2, 'google', 'test-google-client-id', 'test-google-secret', ARRAY['openid', 'email'], true)",
    )
    .bind(google_provider_id)
    .bind(&realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    // 创建路由
    let app = ctx.create_unified_test_router();

    // 发送请求（无需认证）
    let request = Request::builder()
        .uri(format!("/api/public-config/{}", realm_id))
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 解析响应
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let response_value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let response_json: PublicConfigResponse =
        serde_json::from_value(response_value.clone()).unwrap();

    // 验证响应
    assert!(!response_json.registration.enabled);
    assert!(!response_json.registration.require_email_verification);
    assert_eq!(response_json.oauth_providers.len(), 1);
    assert_eq!(response_json.oauth_providers[0].name, "google");
    assert_eq!(response_json.oauth_providers[0].display_name, "Google");
    assert!(response_json.oauth_providers[0].enabled);

    println!("✅ Test passed: registration disabled, OAuth providers present");
}
/// ============================================================================
/// User Story: Public config with non-existent realm
///
/// **场景描述**：
/// 当请求的 realm 不存在时，端点应返回空配置或 404 错误。
///
/// **验收标准**：
/// - 响应状态码为 200（端点默认返回空配置）或 404（可选）
/// - registration.enabled = false (默认值)
/// - registration.requireEmailVerification = false (默认值)
/// - oauth_providers 为空数组
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_public_config_non_existent_realm
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_public_config_non_existent_realm(ctx: &mut TestContext) {
    // 使用不存在的 realm ID
    let non_existent_realm_id = "non-existent-realm-12345";

    // 创建路由
    let app = ctx.create_unified_test_router();

    // 发送请求
    let request = Request::builder()
        .uri(format!("/api/public-config/{}", non_existent_realm_id))
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();

    // 当前实现返回 200，即使 realm 不存在（因为查询返回空结果，使用默认值）
    assert_eq!(response.status(), StatusCode::OK);

    // 解析响应
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let response_value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let response_json: PublicConfigResponse =
        serde_json::from_value(response_value.clone()).unwrap();

    // 验证响应（默认值）
    assert!(!response_json.registration.enabled);
    assert!(!response_json.registration.require_email_verification);
    assert!(response_json.oauth_providers.is_empty());

    println!("✅ Test passed: non-existent realm returns default config");
}

/// ============================================================================
/// User Story: Public config response format validation
///
/// **场景描述**：
/// 验证端点返回的响应格式符合 OpenAPI 规范和 camelCase 约定。
///
/// **验收标准**：
/// - 响应是有效的 JSON
/// - 字段名称使用 camelCase (registration, enabled, requireEmailVerification, oauthProviders)
/// - 字段类型正确（enabled/requireEmailVerification 为 boolean，oauth_providers 为数组）
/// - 响应可以反序列化为 PublicConfigResponse 结构体
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_public_config_response_format
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_public_config_response_format(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();

    // 配置 realm
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)",
    )
    .bind(&realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    // 添加 OAuth 提供商
    let google_provider_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO oauth_provider_config (id, realm_id, provider_type, client_id, client_secret, scopes, enabled)
         VALUES ($1, $2, 'google', 'test-google-client-id', 'test-google-secret', ARRAY['openid', 'email'], true)",
    )
    .bind(google_provider_id)
    .bind(&realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    // 创建路由
    let app = ctx.create_unified_test_router();

    // 发送请求
    let request = Request::builder()
        .uri(format!("/api/public-config/{}", realm_id))
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 获取响应字符串
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = std::str::from_utf8(&body).unwrap();

    // 验证 JSON 格式
    assert!(serde_json::from_str::<serde_json::Value>(body_str).is_ok());

    // 验证 camelCase 字段名称
    assert!(body_str.contains("\"registration\""));
    assert!(body_str.contains("\"enabled\""));
    assert!(body_str.contains("\"requireEmailVerification\""));
    assert!(body_str.contains("\"oauthProviders\""));

    // 验证可以反序列化为强类型结构体
    let response_value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let response_json: PublicConfigResponse =
        serde_json::from_value(response_value.clone()).unwrap();

    // 验证类型
    assert!(response_json.registration.enabled);
    assert!(!response_json.registration.require_email_verification);
    assert_eq!(response_json.oauth_providers.len(), 1);

    let provider = &response_json.oauth_providers[0];
    assert_eq!(provider.name, "google");
    assert_eq!(provider.display_name, "Google");
    assert!(provider.enabled);

    println!("✅ Test passed: response format validation");
}

/// User Story: US-WL-002 — auth pages can render with default Herald branding when a Realm has not configured white-label settings.
/// Covers: Design §5.3 — missing `white_label/settings` returns an empty `whiteLabel` object without regressing registration or OAuth public config.
#[test_context(TestContext)]
#[tokio::test]
async fn public_config_white_label_empty_when_unconfigured(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();

    upsert_public_realm_config(ctx, &realm_id, "registration", "enabled", "true").await;

    let google_provider_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO oauth_provider_config (id, realm_id, provider_type, client_id, client_secret, scopes, enabled)
         VALUES ($1, $2, 'google', 'test-google-client-id', 'test-google-secret', ARRAY['openid', 'email'], true)",
    )
    .bind(google_provider_id)
    .bind(&realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let (status, response_value, response_json) = get_public_config(ctx, &realm_id).await;

    assert_eq!(status, StatusCode::OK);
    assert!(response_value.get("whiteLabel").is_some());
    assert_empty_white_label(&response_json);
    assert!(response_json.registration.enabled);
    assert_eq!(response_json.oauth_providers.len(), 1);
}

/// User Story: US-WL-002 — unauthenticated auth pages see the published Realm brand.
/// Covers: Design §4.2.2 / §5.3 — public config exposes published white-label fields using the camelCase wire contract.
#[test_context(TestContext)]
#[tokio::test]
async fn public_config_white_label_returns_published_camel_case_fields(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let config_value = json!({
        "logoUrl": "https://cdn.example.com/acme-logo.svg",
        "accentColor": "#2563eb",
        "background": {
            "type": "image",
            "value": "https://cdn.example.com/auth-bg.jpg"
        },
        "footerText": "Acme Inc.",
        "loginTitle": "Sign in to Acme",
        "loginSubtitle": "Use your Acme account",
        "registerTitle": "Create your Acme account",
        "registerSubtitle": "Start with Acme"
    })
    .to_string();

    upsert_public_realm_config(ctx, &realm_id, "white_label", "settings", &config_value).await;

    let (status, response_value, response_json) = get_public_config(ctx, &realm_id).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response_value["whiteLabel"]["logoUrl"],
        "https://cdn.example.com/acme-logo.svg"
    );
    assert_eq!(response_value["whiteLabel"]["accentColor"], "#2563eb");
    assert_eq!(response_value["whiteLabel"]["background"]["type"], "image");
    assert_eq!(
        response_value["whiteLabel"]["background"]["value"],
        "https://cdn.example.com/auth-bg.jpg"
    );
    assert_eq!(response_value["whiteLabel"]["footerText"], "Acme Inc.");
    assert_eq!(
        response_value["whiteLabel"]["loginTitle"],
        "Sign in to Acme"
    );
    assert_eq!(
        response_value["whiteLabel"]["loginSubtitle"],
        "Use your Acme account"
    );
    assert_eq!(
        response_value["whiteLabel"]["registerTitle"],
        "Create your Acme account"
    );
    assert_eq!(
        response_value["whiteLabel"]["registerSubtitle"],
        "Start with Acme"
    );
    assert_eq!(
        response_json.white_label.logo_url.as_deref(),
        Some("https://cdn.example.com/acme-logo.svg")
    );
    assert_eq!(
        response_json.white_label.accent_color.as_deref(),
        Some("#2563eb")
    );
}

/// User Story: US-WL-002 — terminal auth pages only use published branding.
/// Covers: Design §4.1 / §5.5 — `white_label/draft` is not exposed by public config before publish.
#[test_context(TestContext)]
#[tokio::test]
async fn public_config_white_label_draft_does_not_leak(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let draft_value = json!({
        "logoUrl": "https://cdn.example.com/draft-logo.svg",
        "accentColor": "#dc2626",
        "footerText": "Draft brand"
    })
    .to_string();

    upsert_public_realm_config(ctx, &realm_id, "white_label", "draft", &draft_value).await;

    let (status, response_value, response_json) = get_public_config(ctx, &realm_id).await;

    assert_eq!(status, StatusCode::OK);
    assert_empty_white_label(&response_json);
    assert!(response_value["whiteLabel"]["logoUrl"].is_null());
    assert!(response_value["whiteLabel"]["footerText"].is_null());
}

/// User Story: US-WL-002 — auth pages remain usable when stored branding JSON is corrupt.
/// Covers: Design §5.3 — invalid `white_label/settings` JSON falls back to empty `whiteLabel` and still returns 200.
#[test_context(TestContext)]
#[tokio::test]
async fn public_config_white_label_invalid_json_falls_back_to_empty(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();

    upsert_public_realm_config(ctx, &realm_id, "white_label", "settings", "{invalid json").await;

    let (status, response_value, response_json) = get_public_config(ctx, &realm_id).await;

    assert_eq!(status, StatusCode::OK);
    assert!(response_value.get("whiteLabel").is_some());
    assert_empty_white_label(&response_json);
}

/// User Story: US-CD-002 / US-CD-005 — terminal auth pages must never see an unpublished custom-domain hostname.
/// Covers: Design §4.2.2 (public-config does not surface draft), §4.5 (no leak of draft state) — a
/// `custom_domain/draft` row (unpublished hostname) must not be reflected in the public-config response,
/// mirroring the white-label draft non-leak guarantee (design §5.3 public config reads only `settings`).
#[test_context(TestContext)]
#[tokio::test]
async fn public_config_custom_domain_draft_does_not_leak(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let draft_value = json!({ "hostname": "draft.example.com" }).to_string();

    // Seed an unpublished custom-domain draft config row. No published
    // `custom_domain/settings` row exists, and no `custom_domain_mapping` row.
    upsert_public_realm_config(ctx, &realm_id, "custom_domain", "draft", &draft_value).await;

    let (status, response_value, _response_json) = get_public_config(ctx, &realm_id).await;

    assert_eq!(status, StatusCode::OK);
    // The public-config response (PublicConfigResponse) intentionally carries no
    // custom-domain field — draft hostname must never surface in the body.
    let body_str = response_value.to_string();
    assert!(
        !body_str.contains("draft.example.com"),
        "public-config must not leak the unpublished custom-domain draft hostname; got: {body_str}"
    );
    assert!(
        response_value.get("customDomain").is_none(),
        "public-config must not expose a customDomain field (draft or otherwise)"
    );
}

/// User Story: US-WL-002 — login and registration pages can fetch branding before a user signs in.
/// Covers: Design §4.2.1 / §5.3 — public config with white-label data does not require authentication.
#[test_context(TestContext)]
#[tokio::test]
async fn public_config_white_label_remains_unauthenticated(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let config_value = json!({
        "logoUrl": "https://cdn.example.com/public-logo.svg",
        "accentColor": "#0f766e"
    })
    .to_string();

    upsert_public_realm_config(ctx, &realm_id, "white_label", "settings", &config_value).await;

    let (status, _, response_json) = get_public_config(ctx, &realm_id).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response_json.white_label.logo_url.as_deref(),
        Some("https://cdn.example.com/public-logo.svg")
    );
}

/// emailChannelConfigured 是前端"修改邮箱"入口的可见性来源：必须如实反映
/// realm 是否具备可用邮件通道——未配置时 false，插入完整 Resend 配置后 true。
/// 与 registration.requireEmailVerification 的未配置门控口径对称：能力不存在
/// 时入口不应出现，而不是让用户走进一条必然死等的流程。
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_public_config_email_channel_flag(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();

    delete_email_config_direct(&ctx._app_state.pool, &realm_id).await;
    let (status, response_value, response_json) = get_public_config(ctx, &realm_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response_value["emailChannelConfigured"],
        json!(false),
        "unconfigured realm must report emailChannelConfigured=false on the wire"
    );
    assert!(!response_json.email_channel_configured);

    insert_resend_email_config_direct(&ctx._app_state.pool, &realm_id).await;
    let (status, response_value, response_json) = get_public_config(ctx, &realm_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        response_value["emailChannelConfigured"],
        json!(true),
        "a complete Resend config must flip emailChannelConfigured to true"
    );
    assert!(response_json.email_channel_configured);
}
