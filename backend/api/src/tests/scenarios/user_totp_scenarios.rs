// =============================================================================
// User TOTP API Scenarios Tests
// =============================================================================
//
// 测试 /api/user/totp API endpoints
//
// **测试目标**：
// 1. 验证 TOTP 启用流程（setup + verify）
// 2. 验证 TOTP 状态查询
// 3. 验证 TOTP 禁用功能
// 4. 验证 TOTP 重新生成功能
// 5. 验证 backup codes 统计
//
// **运行方式**：
// ```bash
// cargo nextest run --workspace totp
// ```
//
// =============================================================================

use crate::tests::helpers::auth_helpers::{attempt_reauth_verify, obtain_reauth_token};
use crate::tests::helpers::test_setup_helpers::record_test_user_consent;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base32;
use bcrypt;
use redis;
use serde_json::json;
use test_context::test_context;
use totp_lite::Sha256;
use tower::ServiceExt;

// ============================================================================
// Test Helper Functions
// ============================================================================

/// Create a test user and return their credentials
async fn create_test_user(ctx: &TestContext, email: &str, password: &str) -> String {
    let user_uuid = uuid::Uuid::now_v7();
    let password_hash =
        bcrypt::hash(password, bcrypt::DEFAULT_COST).expect("Failed to hash password");

    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, $4, 1)",
    )
    .bind(user_uuid)
    .bind(&ctx._realm_id)
    .bind(email)
    .bind(&password_hash)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create test user");

    record_test_user_consent(&ctx._app_state.pool, user_uuid, &ctx._realm_id).await;

    user_uuid.to_string()
}

/// Create a temporary TOTP session (simulating login step 1)
/// Returns (temp_token_or_session_token, user_id)
async fn create_temp_totp_session(
    ctx: &TestContext,
    user_email: &str,
    password: &str,
) -> (String, String) {
    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": user_email,
        "password": password,
        "turnstileToken": "dummy"
    });

    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();

    let app = ctx.create_unified_test_router();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    assert!(login_response.status().is_success());

    // Extract response body before moving login_response
    let (_login_status, _login_headers, login_response_body) = (
        login_response.status(),
        login_response.headers().clone(),
        axum::body::to_bytes(login_response.into_body(), usize::MAX)
            .await
            .expect("Failed to read response body"),
    );

    let login_body: serde_json::Value =
        serde_json::from_slice(&login_response_body).expect("Failed to parse JSON");

    let user_id = login_body["userId"]
        .as_str()
        .expect("User ID should exist")
        .to_string();

    // Check if user has TOTP enabled
    if let Some(true) = login_body["requiresTotp"].as_bool() {
        // User has TOTP enabled - return temp_token
        let temp_token = login_body["tempToken"]
            .as_str()
            .expect("Temp token should exist")
            .to_string();
        println!("  - User has TOTP enabled, temp_token={}", temp_token);
        (temp_token, user_id)
    } else {
        let session_token = login_body["accessToken"]
            .as_str()
            .expect("Login should return accessToken")
            .to_owned();
        println!(
            "  - User does not have TOTP enabled, session_token={}",
            session_token
        );
        (session_token, user_id)
    }
}

/// Complete TOTP login verification
/// Returns (session_token, response_body) or error status code
async fn complete_totp_login(
    ctx: &TestContext,
    realm_id: &str,
    temp_token: &str,
    code: Option<&str>,
    backup_code: Option<&str>,
) -> Result<(String, serde_json::Value), StatusCode> {
    let app = ctx.create_unified_test_router();
    let mut payload = json!({ "tempToken": temp_token });

    if let Some(c) = code {
        payload["code"] = json!(c);
    }
    if let Some(bc) = backup_code {
        payload["backupCode"] = json!(bc);
    }

    let verify_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login/verify-totp", realm_id))
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let verify_response = app.clone().oneshot(verify_request).await.unwrap();

    if !verify_response.status().is_success() {
        return Err(verify_response.status());
    }

    let verify_body_bytes = axum::body::to_bytes(verify_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let verify_body: serde_json::Value =
        serde_json::from_slice(&verify_body_bytes).expect("Failed to parse JSON");
    let session_token = verify_body["accessToken"]
        .as_str()
        .expect("TOTP verification should return accessToken")
        .to_owned();

    Ok((session_token, verify_body))
}

/// Generate expired TOTP code (using 31 seconds ago timestamp)
fn generate_expired_totp_code(secret: &str) -> String {
    let secret_bytes = base32::decode(base32::Alphabet::Rfc4648 { padding: true }, secret)
        .expect("Failed to decode secret");
    let expired_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 31;
    totp_lite::totp_custom::<Sha256>(30, 6, &secret_bytes, expired_time)
}

/// Setup Realm TOTP configuration
async fn setup_realm_totp_config(ctx: &TestContext, enabled: bool, force_enabled: bool) {
    let config_uuid = uuid::Uuid::now_v7();
    let config_value = json!({ "enabled": enabled, "force_enabled": force_enabled });
    let metadata = json!({ "force_enabled": force_enabled });

    // Check if config exists
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id::text FROM realm_config
         WHERE realm_id = $1 AND config_type = 'totp' AND config_key = 'settings'",
    )
    .bind(&ctx._realm_id)
    .fetch_optional(&ctx._app_state.pool)
    .await
    .unwrap();

    if let Some(id) = existing {
        // Update existing
        sqlx::query(
            "UPDATE realm_config
             SET enabled = $1, metadata = $2::jsonb, updated_at = NOW()
             WHERE id = $3",
        )
        .bind(enabled)
        .bind(metadata.to_string())
        .bind(&id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to update realm TOTP config");
    } else {
        // Create new
        sqlx::query(
            "INSERT INTO realm_config (id, realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
             VALUES ($1, $2, 'totp', 'settings', $3, false, $4, $5::jsonb, NOW(), NOW())",
        )
        .bind(config_uuid)
        .bind(&ctx._realm_id)
        .bind(config_value.to_string())
        .bind(enabled)
        .bind(metadata.to_string())
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to create realm TOTP config");
    }
}

/// Generate TOTP code from secret
fn generate_totp_code(secret: &str) -> String {
    let secret_bytes = base32::decode(base32::Alphabet::Rfc4648 { padding: true }, secret)
        .expect("Failed to decode secret");
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    totp_lite::totp_custom::<Sha256>(30, 6, &secret_bytes, current_time)
}

/// ============================================================================
/// User Story: 完整 TOTP 启用流程
///
/// **场景描述**：
/// 用户首次启用 TOTP，完成 setup 和 verify 流程。
///
/// **测试步骤**：
/// 1. 创建测试用户
/// 2. 配置 Realm TOTP
/// 3. 登录获取 token
/// 4. POST /api/user/totp - 启动 TOTP setup
/// 5. 生成 TOTP code
/// 6. POST /api/user/totp/verify - 验证 TOTP
/// 7. GET /api/user/totp/status - 检查状态
///
/// **验收标准**：
/// - Setup 返回 secret, qr_code_url, backup_codes, temp_token
/// - Verify 成功后 TOTP 状态为 enabled=true
/// - backup_codes 统计为 total=10, used=0
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_full_enable_flow
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_full_enable_flow(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Setup environment variable for TOTP encryption
    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    // ============================================================================
    // Step 1: 创建测试用户
    // ============================================================================
    println!("[Step 1] 创建测试用户");
    let email = "totp_user1@cas.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;
    println!("[Step 1] ✓ 用户创建成功: user_id={}", user_id);

    // ============================================================================
    // Step 2: 配置 Realm TOTP
    // ============================================================================
    println!("[Step 2] 配置 Realm TOTP");
    setup_realm_totp_config(ctx, true, false).await;
    println!("[Step 2] ✓ Realm TOTP 配置完成");

    // ============================================================================
    // Step 3: 登录获取 token
    // ============================================================================
    println!("[Step 3] 用户登录");
    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });

    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();

    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");
    println!("[Step 3] ✓ 登录成功, token={}", login_token);

    // ============================================================================
    // Step 4: POST /api/user/totp - 启动 TOTP setup
    // ============================================================================
    println!("[Step 4] 启动 TOTP setup");
    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();

    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let secret = enable_body["secret"].as_str().expect("Secret should exist");
    let qr_code_url = enable_body["qrCodeUrl"]
        .as_str()
        .expect("QR code URL should exist");
    let backup_codes: Vec<String> = enable_body["backupCodes"]
        .as_array()
        .expect("Backup codes should be array")
        .iter()
        .map(|v| {
            v.as_str()
                .expect("Backup code should be string")
                .to_string()
        })
        .collect();
    let temp_token = enable_body["tempToken"]
        .as_str()
        .expect("Temp token should exist");

    assert_eq!(backup_codes.len(), 10, "Should generate 10 backup codes");
    assert!(
        qr_code_url.contains(secret),
        "QR code URL should contain secret"
    );
    println!("[Step 4] ✓ TOTP setup 启动成功");
    println!("  - Secret: {}", secret);
    println!("  - Backup codes: {} codes", backup_codes.len());
    println!("  - Temp token: {}", temp_token);

    // ============================================================================
    // Step 5: 生成 TOTP code
    // ============================================================================
    println!("[Step 5] 生成 TOTP code");
    let totp_code = generate_totp_code(secret);
    println!("[Step 5] ✓ TOTP code: {}", totp_code);

    // ============================================================================
    // Step 6: POST /api/user/totp/verify - 验证 TOTP
    // ============================================================================
    println!("[Step 6] 验证 TOTP code");
    let verify_payload = json!({
        "tempToken": temp_token,
        "code": totp_code
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();

    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);

    let verify_body_bytes = axum::body::to_bytes(verify_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let verify_body: serde_json::Value =
        serde_json::from_slice(&verify_body_bytes).expect("Failed to parse JSON");

    assert_eq!(
        verify_body["message"], "TOTP enabled successfully",
        "Should return success message"
    );
    assert!(
        verify_body["enabledAt"].is_string(),
        "Should return enabled_at timestamp"
    );
    println!("[Step 6] ✓ TOTP 验证成功");

    // ============================================================================
    // Step 7: GET /api/user/totp/status - 检查状态
    // ============================================================================
    println!("[Step 7] 检查 TOTP status");
    let status_request = Request::builder()
        .method("GET")
        .uri("/api/user/totp/status")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::empty())
        .unwrap();

    let status_response = app.clone().oneshot(status_request).await.unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);

    let status_body_bytes = axum::body::to_bytes(status_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let status_body: serde_json::Value =
        serde_json::from_slice(&status_body_bytes).expect("Failed to parse JSON");

    assert_eq!(status_body["enabled"], true, "TOTP should be enabled");
    assert!(
        status_body["enabledAt"].is_string(),
        "Should return enabled_at timestamp"
    );
    assert_eq!(
        status_body["backupCodes"]["total"], 10,
        "Should have 10 backup codes total"
    );
    assert_eq!(
        status_body["backupCodes"]["remaining"], 10,
        "Should have 10 backup codes remaining"
    );
    assert_eq!(
        status_body["backupCodes"]["used"], 0,
        "Should have 0 backup codes used"
    );
    println!("[Step 7] ✓ TOTP status 正确");
    println!("  - enabled: {}", status_body["enabled"]);
    println!(
        "  - backup_codes: total={}, remaining={}, used={}",
        status_body["backupCodes"]["total"],
        status_body["backupCodes"]["remaining"],
        status_body["backupCodes"]["used"]
    );

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：完整 TOTP 启用流程");
}

/// ============================================================================
/// User Story: 重新启用 TOTP（未验证状态）
///
/// **场景描述**：
/// 用户启动 TOTP setup 但未验证，再次启动 setup 应该删除旧记录并创建新的。
///
/// **测试步骤**：
/// 1. 创建测试用户
/// 2. 配置 Realm TOTP
/// 3. 登录
/// 4. POST /api/user/totp - 第一次启动 setup（不验证）
/// 5. POST /api/user/totp - 第二次启动 setup（应删除旧记录）
/// 6. 验证 TOTP
/// 7. 检查状态
///
/// **验收标准**：
/// - 第二次 setup 应该成功（409 "already enabled" 错误）
/// - 最终 TOTP 应该成功启用
/// - 没有 duplicate key 错误
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_restart_unverified_setup
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_restart_unverified_setup(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    // ============================================================================
    // Step 1-3: 创建用户、配置 Realm、登录
    // ============================================================================
    println!("[Setup] 创建测试用户和 Realm 配置");
    let email = "totp_user2@cas.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    // Login
    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");
    println!("[Setup] ✓ 准备完成");

    // ============================================================================
    // Step 4: 第一次启动 TOTP setup（不验证）
    // ============================================================================
    println!("[Step 4] 第一次启动 TOTP setup");
    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();

    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);
    println!("[Step 4] ✓ 第一次 setup 成功");

    // Check that config exists but is not enabled
    let config_enabled: Option<bool> =
        sqlx::query_scalar("SELECT enabled FROM user_totp_config WHERE user_id = $1::uuid")
            .bind(&user_id)
            .fetch_optional(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(
        config_enabled,
        Some(false),
        "Config should exist but not enabled"
    );

    // Check backup codes exist
    let backup_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_totp_backup_codes
         WHERE user_totp_config_id = (SELECT id FROM user_totp_config WHERE user_id = $1::uuid)",
    )
    .bind(&user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        backup_count, 10,
        "Should have 10 backup codes after first setup"
    );
    println!("  - Backup codes count: {}", backup_count);

    // ============================================================================
    // Step 5: 第二次启动 TOTP setup（应删除旧记录）
    // ============================================================================
    println!("[Step 5] 第二次启动 TOTP setup（应该删除旧记录）");
    let reauth_token2 =
        obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload2 = json!({ "reauth_token": reauth_token2 });
    let enable_request2 = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload2.to_string()))
        .unwrap();

    let enable_response2 = app.clone().oneshot(enable_request2).await.unwrap();
    assert_eq!(
        enable_response2.status(),
        StatusCode::OK,
        "Second setup should succeed (delete old unverified config)"
    );

    let enable_body_bytes = axum::body::to_bytes(enable_response2.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let secret2 = enable_body["secret"].as_str().unwrap();
    let backup_codes2: Vec<String> = enable_body["backupCodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let temp_token2 = enable_body["tempToken"].as_str().unwrap();

    println!("[Step 5] ✓ 第二次 setup 成功");
    println!("  - Secret: {}", secret2);
    println!("  - Backup codes: {} codes", backup_codes2.len());

    // ============================================================================
    // Step 6: 验证 TOTP
    // ============================================================================
    println!("[Step 6] 验证 TOTP code");
    let totp_code = generate_totp_code(secret2);
    let verify_payload = json!({
        "tempToken": temp_token2,
        "code": totp_code
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();

    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    println!("[Step 6] ✓ TOTP 验证成功");

    // ============================================================================
    // Step 7: 检查状态
    // ============================================================================
    println!("[Step 7] 检查 TOTP status");
    let status_request = Request::builder()
        .method("GET")
        .uri("/api/user/totp/status")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::empty())
        .unwrap();

    let status_response = app.clone().oneshot(status_request).await.unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);

    let status_body_bytes = axum::body::to_bytes(status_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let status_body: serde_json::Value =
        serde_json::from_slice(&status_body_bytes).expect("Failed to parse JSON");

    assert_eq!(status_body["enabled"], true, "TOTP should be enabled");
    assert_eq!(
        status_body["backupCodes"]["total"], 10,
        "Should have 10 backup codes"
    );
    println!("[Step 7] ✓ TOTP status 正确");
    println!("  - enabled: {}", status_body["enabled"]);

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：重新启用 TOTP（未验证状态）");
}

/// ============================================================================
/// User Story: 禁用 TOTP
///
/// **场景描述**：
/// 用户成功启用 TOTP 后，可以禁用 TOTP。
///
/// **测试步骤**：
/// 1. 创建测试用户
/// 2. 配置 Realm TOTP（非 force_enabled）
/// 3. 登录
/// 4. 启用并验证 TOTP
/// 5. 禁用 TOTP
/// 6. 检查状态
///
/// **验收标准**：
/// - 禁用成功，返回 disabled_at 时间戳
/// - 状态查询显示 enabled=false
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_disable
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_disable(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    // ============================================================================
    // Setup: 创建用户、配置 Realm、登录
    // ============================================================================
    println!("[Setup] 准备测试环境");
    let email = "totp_user3@cas.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");
    println!("[Setup] ✓ 准备完成");

    // ============================================================================
    // Step 1-2: 启用并验证 TOTP
    // ============================================================================
    println!("[Step 1] 启用 TOTP setup");
    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();

    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let secret = enable_body["secret"].as_str().unwrap();
    let temp_token = enable_body["tempToken"].as_str().unwrap();
    println!("[Step 1] ✓ TOTP setup 成功");

    println!("[Step 2] 验证 TOTP code");
    let totp_code = generate_totp_code(secret);
    let verify_payload = json!({
        "tempToken": temp_token,
        "code": totp_code
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();

    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    println!("[Step 2] ✓ TOTP 验证成功");

    // ============================================================================
    // Step 3: 禁用 TOTP
    // ============================================================================
    println!("[Step 3] 禁用 TOTP");
    let reauth_token =
        obtain_reauth_token(ctx, &login_token, "remove_authenticator", password).await;
    let disable_payload = json!({ "reauth_token": reauth_token });
    let disable_request = Request::builder()
        .method("DELETE")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(disable_payload.to_string()))
        .unwrap();

    let disable_response = app.clone().oneshot(disable_request).await.unwrap();
    assert_eq!(disable_response.status(), StatusCode::OK);

    let disable_body_bytes = axum::body::to_bytes(disable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let disable_body: serde_json::Value =
        serde_json::from_slice(&disable_body_bytes).expect("Failed to parse JSON");

    assert_eq!(
        disable_body["message"], "TOTP disabled successfully",
        "Should return success message"
    );
    assert!(
        disable_body["disabledAt"].is_string(),
        "Should return disabled_at timestamp"
    );
    println!("[Step 3] ✓ TOTP 禁用成功");

    // ============================================================================
    // Step 4: 检查状态
    // ============================================================================
    println!("[Step 4] 检查 TOTP status");
    let status_request = Request::builder()
        .method("GET")
        .uri("/api/user/totp/status")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::empty())
        .unwrap();

    let status_response = app.clone().oneshot(status_request).await.unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);

    let status_body_bytes = axum::body::to_bytes(status_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let status_body: serde_json::Value =
        serde_json::from_slice(&status_body_bytes).expect("Failed to parse JSON");

    assert_eq!(status_body["enabled"], false, "TOTP should be disabled");
    assert_eq!(
        status_body["enabledAt"],
        serde_json::Value::Null,
        "enabled_at should be null"
    );
    assert_eq!(
        status_body["backupCodes"]["total"], 0,
        "Should have 0 backup codes after disable"
    );
    println!("[Step 4] ✓ TOTP 状态正确");
    println!("  - enabled: {}", status_body["enabled"]);

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：禁用 TOTP");
}

/// ============================================================================
/// User Story: 重新生成 TOTP Secret
///
/// **场景描述**：
/// 用户可以重新生成 TOTP secret 和 backup codes。
///
/// **测试步骤**：
/// 1. 创建测试用户
/// 2. 配置 Realm TOTP
/// 3. 登录
/// 4. 启用并验证 TOTP
/// 5. 重新生成 TOTP secret
/// 6. 使用新 secret 验证
///
/// **验收标准**：
/// - 重新生成返回新的 secret 和 backup codes
/// - 新 secret 可以成功验证
/// - 旧 secret 不再有效
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_regenerate_secret
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_regenerate_secret(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    // ============================================================================
    // Setup: 创建用户、配置 Realm、登录
    // ============================================================================
    println!("[Setup] 准备测试环境");
    let email = "totp_user4@cas.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");
    println!("[Setup] ✓ 准备完成");

    // ============================================================================
    // Step 1-2: 启用并验证 TOTP
    // ============================================================================
    println!("[Step 1] 启用 TOTP setup");
    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();

    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let old_secret = enable_body["secret"].as_str().unwrap();
    let temp_token = enable_body["tempToken"].as_str().unwrap();
    println!("[Step 1] ✓ TOTP setup 成功");

    println!("[Step 2] 验证 TOTP code");
    let totp_code = generate_totp_code(old_secret);
    let verify_payload = json!({
        "tempToken": temp_token,
        "code": totp_code
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();

    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    println!("[Step 2] ✓ TOTP 验证成功");

    // ============================================================================
    // Step 3: 重新生成 TOTP secret
    // ============================================================================
    println!("[Step 3] 重新生成 TOTP secret");
    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let regenerate_payload = json!({ "reauth_token": reauth_token });
    let regenerate_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/regenerate")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(regenerate_payload.to_string()))
        .unwrap();

    let regenerate_response = app.clone().oneshot(regenerate_request).await.unwrap();
    assert_eq!(regenerate_response.status(), StatusCode::OK);

    let regenerate_body_bytes = axum::body::to_bytes(regenerate_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let regenerate_body: serde_json::Value =
        serde_json::from_slice(&regenerate_body_bytes).expect("Failed to parse JSON");

    let new_secret = regenerate_body["secret"].as_str().unwrap();
    let new_backup_codes: Vec<String> = regenerate_body["backupCodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let new_temp_token = regenerate_body["tempToken"].as_str().unwrap();

    assert_ne!(new_secret, old_secret, "Secret should be different");
    assert_eq!(
        new_backup_codes.len(),
        10,
        "Should have 10 new backup codes"
    );
    println!("[Step 3] ✓ TOTP secret 重新生成成功");
    println!("  - Old secret: {}", old_secret);
    println!("  - New secret: {}", new_secret);

    // ============================================================================
    // Step 4: 使用新 secret 验证
    // ============================================================================
    println!("[Step 4] 使用新 secret 验证");
    let new_totp_code = generate_totp_code(new_secret);
    let new_verify_payload = json!({
        "tempToken": new_temp_token,
        "code": new_totp_code
    });
    let new_verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(new_verify_payload.to_string()))
        .unwrap();

    let new_verify_response = app.clone().oneshot(new_verify_request).await.unwrap();
    assert_eq!(new_verify_response.status(), StatusCode::OK);
    println!("[Step 4] ✓ 新 secret 验证成功");

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：重新生成 TOTP Secret");
}

/// ============================================================================
/// User Story: Backup Codes 统计
///
/// **场景描述**：
/// 验证 backup codes 的统计功能。
///
/// **测试步骤**：
/// 1. 创建测试用户
/// 2. 配置 Realm TOTP
/// 3. 登录
/// 4. 启用并验证 TOTP
/// 5. 检查 backup codes 统计
///
/// **验收标准**：
/// - total=10
/// - used=0
/// - remaining=10
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_backup_codes_stats
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_backup_codes_stats(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    // ============================================================================
    // Setup: 创建用户、配置 Realm、登录
    // ============================================================================
    println!("[Setup] 准备测试环境");
    let email = "totp_user5@cas.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");
    println!("[Setup] ✓ 准备完成");

    // ============================================================================
    // Step 1-2: 启用并验证 TOTP
    // ============================================================================
    println!("[Step 1] 启用 TOTP setup");
    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();

    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let secret = enable_body["secret"].as_str().unwrap();
    let temp_token = enable_body["tempToken"].as_str().unwrap();
    println!("[Step 1] ✓ TOTP setup 成功");

    println!("[Step 2] 验证 TOTP code");
    let totp_code = generate_totp_code(secret);
    let verify_payload = json!({
        "tempToken": temp_token,
        "code": totp_code
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();

    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    println!("[Step 2] ✓ TOTP 验证成功");

    // ============================================================================
    // Step 3: 检查 backup codes 统计
    // ============================================================================
    println!("[Step 3] 检查 backup codes 统计");
    let status_request = Request::builder()
        .method("GET")
        .uri("/api/user/totp/status")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::empty())
        .unwrap();

    let status_response = app.clone().oneshot(status_request).await.unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);

    let status_body_bytes = axum::body::to_bytes(status_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let status_body: serde_json::Value =
        serde_json::from_slice(&status_body_bytes).expect("Failed to parse JSON");

    assert_eq!(status_body["enabled"], true, "TOTP should be enabled");
    assert_eq!(
        status_body["backupCodes"]["total"], 10,
        "Should have 10 total backup codes"
    );
    assert_eq!(
        status_body["backupCodes"]["used"], 0,
        "Should have 0 used backup codes"
    );
    assert_eq!(
        status_body["backupCodes"]["remaining"], 10,
        "Should have 10 remaining backup codes"
    );
    println!("[Step 3] ✓ Backup codes 统计正确");
    println!(
        "  - total: {}, used: {}, remaining: {}",
        status_body["backupCodes"]["total"],
        status_body["backupCodes"]["used"],
        status_body["backupCodes"]["remaining"]
    );

    // Verify database directly
    let db_total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_totp_backup_codes
         WHERE user_totp_config_id = (SELECT id FROM user_totp_config WHERE user_id = (SELECT id FROM account WHERE email = $1))",
    )
    .bind(email)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(db_total, 10, "Database should have 10 backup codes");

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：Backup Codes 统计");
}

// ============================================================================
// Phase 1: US-TO-003 Login Scenarios (6 tests)
// ============================================================================

/// ============================================================================
/// User Story: TOTP Login - Success
///
/// **场景描述**：
/// 用户启用 TOTP 后，正常 TOTP 登录流程。
///
/// **测试步骤**：
/// 1. 创建测试用户并启用 TOTP
/// 2. 调用登录 API 获取 temp_token
/// 3. 生成有效 TOTP code
/// 4. 调用 verify-totp API 完成登录
/// 5. 验证返回 session_token
/// 6. 验证 TOTP last_used 时间更新
///
/// **验收标准**：
/// - 返回 200 状态码
/// - 签发的 Bearer token 可访问受保护端点
/// - 数据库中 last_updated 时间已更新
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_login_success
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_login_success(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    // ============================================================================
    // Setup: 创建用户、启用 TOTP
    // ============================================================================
    println!("[Setup] 创建用户并启用 TOTP");
    let email = "totp_login_user1@cas.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    // Login to enable TOTP
    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");

    // Enable TOTP
    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();
    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let secret = enable_body["secret"].as_str().unwrap();
    let temp_token = enable_body["tempToken"].as_str().unwrap();
    let totp_code = generate_totp_code(secret);

    let verify_payload = json!({
        "tempToken": temp_token,
        "code": totp_code
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();
    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    println!("[Setup] ✓ TOTP 已启用");

    // Get user_id for cleanup
    let user_id: String = sqlx::query_scalar("SELECT id::text FROM account WHERE email = $1")
        .bind(email)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap();

    // ============================================================================
    // Step 1: 调用登录 API 获取 temp_token
    // ============================================================================
    println!("[Step 1] 调用登录 API 获取 temp_token");
    let (temp_token, _) = create_temp_totp_session(ctx, email, password).await;
    println!("[Step 1] ✓ 获取 temp_token: {}", temp_token);

    // ============================================================================
    // Step 2: 生成有效 TOTP code
    // ============================================================================
    println!("[Step 2] 生成有效 TOTP code");
    let totp_code = generate_totp_code(secret);
    println!("[Step 2] ✓ TOTP code: {}", totp_code);

    // ============================================================================
    // Step 3: 调用 verify-totp API 完成登录
    // ============================================================================
    println!("[Step 3] 调用 verify-totp API 完成登录");
    let realm_id = ctx._realm_id.clone();
    let result = complete_totp_login(ctx, &realm_id, &temp_token, Some(&totp_code), None).await;
    assert!(result.is_ok(), "TOTP verification should succeed");
    let (session_token, _) = result.unwrap();
    println!("[Step 3] ✓ 登录成功, session_token: {}", session_token);

    // ============================================================================
    // Step 4: 验证签发的 Bearer token 可访问受保护端点
    // ============================================================================
    println!("[Step 4] 验证 session_token 可访问受保护端点");
    let profile_request = Request::builder()
        .method("GET")
        .uri("/api/user/profile")
        .header("authorization", format!("Bearer {}", session_token))
        .body(Body::empty())
        .unwrap();
    let profile_response = app.clone().oneshot(profile_request).await.unwrap();
    assert_eq!(profile_response.status(), StatusCode::OK);
    let profile_body: serde_json::Value = crate::tests::response_json(profile_response).await;
    assert_eq!(profile_body["id"], json!(user_id));
    println!("[Step 4] ✓ Bearer token 有效");

    // ============================================================================
    // Step 5: 验证 TOTP last_used 时间更新
    // ============================================================================
    println!("[Step 5] 验证 TOTP last_used 时间更新");
    let last_used_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT last_used_at FROM user_totp_config WHERE user_id = $1::uuid")
            .bind(&user_id)
            .fetch_optional(&ctx._app_state.pool)
            .await
            .unwrap();
    assert!(
        last_used_at.is_some(),
        "last_used_at should be updated after TOTP login"
    );
    println!("[Step 5] ✓ last_used_at 已更新: {:?}", last_used_at);

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：TOTP Login - Success");
}
/// ============================================================================
/// User Story: TOTP Login - Expired Code Failure
///
/// **场景描述**：
/// TOTP 验证码过期（失败场景）。
///
/// **测试步骤**：
/// 1. 创建用户并启用 TOTP
/// 2. 调用登录 API 获取 temp_token
/// 3. 生成 31 秒前的 TOTP code（使用 generate_expired_totp_code）
/// 4. 调用 verify-totp API
/// 5. 验证错误响应
///
/// **验收标准**：
/// - 返回 401 状态码
/// - 错误消息正确
/// - temp_token 已删除
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_login_expired_code_failure
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_login_expired_code_failure(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    // ============================================================================
    // Setup: 创建用户并启用 TOTP
    // ============================================================================
    println!("[Setup] 创建用户并启用 TOTP");
    let email = "totp_login_user3@cas.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    // Login and enable TOTP
    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");

    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();
    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let secret = enable_body["secret"].as_str().unwrap();
    let temp_token = enable_body["tempToken"].as_str().unwrap();
    let totp_code = generate_totp_code(secret);

    let verify_payload = json!({
        "tempToken": temp_token,
        "code": totp_code
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();
    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    println!("[Setup] ✓ TOTP 已启用");

    // ============================================================================
    // Step 1: 获取 temp_token
    // ============================================================================
    println!("[Step 1] 获取 temp_token");
    let (temp_token, _) = create_temp_totp_session(ctx, email, password).await;
    println!("[Step 1] ✓ temp_token: {}", temp_token);

    // ============================================================================
    // Step 2: 生成过期的 TOTP code（31 秒前）
    // ============================================================================
    println!("[Step 2] 生成过期的 TOTP code");
    let expired_code = generate_expired_totp_code(secret);
    println!("[Step 2] ✓ 过期 code: {}", expired_code);

    // ============================================================================
    // Step 3: 调用 verify-totp API
    // ============================================================================
    println!("[Step 3] 调用 verify-totp API");
    let realm_id = ctx._realm_id.clone();
    let result = complete_totp_login(ctx, &realm_id, &temp_token, Some(&expired_code), None).await;
    assert!(result.is_err(), "TOTP verification should fail");
    let status = result.unwrap_err();
    assert_eq!(status, StatusCode::UNAUTHORIZED, "Should return 401");
    println!("[Step 3] ✓ 返回 401 状态码");

    // ============================================================================
    // Step 4: 验证 temp_token 已删除
    // ============================================================================
    println!("[Step 4] 验证 temp_token 已删除");
    let temp_key = format!("totp:temp:{}", temp_token);
    let mut conn = ctx._app_state.redis_manager.get().await.unwrap();
    let temp_exists: i64 = redis::cmd("EXISTS")
        .arg(&temp_key)
        .query_async(&mut conn)
        .await
        .unwrap();
    assert_eq!(
        temp_exists, 0,
        "temp_token should be deleted after failed verification"
    );
    println!("[Step 4] ✓ temp_token 已删除");

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：TOTP Login - Expired Code Failure");
}

/// ============================================================================
/// User Story: TOTP Login with Backup Code
///
/// **场景描述**：
/// 使用备份恢复码登录。
///
/// **测试步骤**：
/// 1. 创建用户并启用 TOTP（记录 backup_codes）
/// 2. 调用登录 API 获取 temp_token
/// 3. 使用第一个 backup_code 调用 verify-totp（使用 backup_code 字段）
/// 4. 验证登录成功
/// 5. 查询数据库确认该 backup_code 标记为已使用
/// 6. 再次尝试使用相同 backup_code（应失败）
///
/// **验收标准**：
/// - 首次使用返回 200
/// - backup_code 标记为 used=true
/// - 重复使用返回 401
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_login_with_backup_code
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_login_with_backup_code(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    // ============================================================================
    // Setup: 创建用户并启用 TOTP，记录 backup_codes
    // ============================================================================
    println!("[Setup] 创建用户并启用 TOTP");
    let email = "totp_login_user4@cas.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    // Login and enable TOTP
    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");

    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();
    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let secret = enable_body["secret"].as_str().unwrap();
    let temp_token = enable_body["tempToken"].as_str().unwrap();
    let backup_codes: Vec<String> = enable_body["backupCodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let first_backup_code = backup_codes[0].clone();

    let totp_code = generate_totp_code(secret);
    let verify_payload = json!({
        "tempToken": temp_token,
        "code": totp_code
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();
    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    println!("[Setup] ✓ TOTP 已启用");
    println!("[Setup] Backup codes: {:?}", backup_codes);

    // ============================================================================
    // Step 1: 获取 temp_token
    // ============================================================================
    println!("[Step 1] 获取 temp_token");
    let (temp_token, _) = create_temp_totp_session(ctx, email, password).await;
    println!("[Step 1] ✓ temp_token: {}", temp_token);

    // ============================================================================
    // Step 2: 使用第一个 backup_code 调用 verify-totp
    // ============================================================================
    println!("[Step 2] 使用第一个 backup_code 调用 verify-totp");
    let realm_id = ctx._realm_id.clone();
    let result =
        complete_totp_login(ctx, &realm_id, &temp_token, None, Some(&first_backup_code)).await;
    assert!(result.is_ok(), "Backup code verification should succeed");
    let (session_token, _) = result.unwrap();
    println!("[Step 2] ✓ 登录成功, session_token: {}", session_token);

    // ============================================================================
    // Step 3: 查询数据库确认 backup_code 标记为已使用
    // ============================================================================
    println!("[Step 3] 查询数据库确认 backup_code 标记为已使用");
    let user_id: String = sqlx::query_scalar("SELECT id::text FROM account WHERE email = $1")
        .bind(email)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap();
    let used_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_totp_backup_codes
         WHERE user_totp_config_id = (SELECT id FROM user_totp_config WHERE user_id = $1::uuid)
         AND used = true",
    )
    .bind(&user_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(used_count, 1, "One backup code should be marked as used");
    println!("[Step 3] ✓ Backup code 标记为已使用");

    // ============================================================================
    // Step 4: 再次尝试使用相同 backup_code（应失败）
    // ============================================================================
    println!("[Step 4] 再次尝试使用相同 backup_code（应失败）");
    let (temp_token2, _) = create_temp_totp_session(ctx, email, password).await;
    let result2 =
        complete_totp_login(ctx, &realm_id, &temp_token2, None, Some(&first_backup_code)).await;
    assert!(result2.is_err(), "Reusing backup code should fail");
    let status2 = result2.unwrap_err();
    assert_eq!(status2, StatusCode::UNAUTHORIZED, "Should return 401");
    println!("[Step 4] ✓ 重复使用返回 401");

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：TOTP Login with Backup Code");
}

/// ============================================================================
/// User Story: TOTP Login - Backup Codes Exhausted Failure
///
/// **场景描述**：
/// 备份恢复码耗尽（失败场景）。
///
/// **测试步骤**：
/// 1. 创建用户并启用 TOTP
/// 2. 使用 SQL 将所有 10 个 backup_code 标记为已使用
/// 3. 调用登录 API 获取 temp_token
/// 4. 尝试使用一个无效的 backup_code
/// 5. 验证错误响应
///
/// **验收标准**：
/// - 返回 401 状态码
/// - 备份码已耗尽时拒绝登录
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_login_backup_codes_exhausted_failure
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_login_backup_codes_exhausted_failure(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    // ============================================================================
    // Setup: 创建用户并启用 TOTP
    // ============================================================================
    println!("[Setup] 创建用户并启用 TOTP");
    let email = "totp_login_user5@cas.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    // Login and enable TOTP
    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");

    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();
    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let secret = enable_body["secret"].as_str().unwrap();
    let temp_token = enable_body["tempToken"].as_str().unwrap();
    let totp_code = generate_totp_code(secret);

    let verify_payload = json!({
        "tempToken": temp_token,
        "code": totp_code
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();
    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    println!("[Setup] ✓ TOTP 已启用");

    // ============================================================================
    // Step 1: 将所有 10 个 backup_code 标记为已使用
    // ============================================================================
    println!("[Step 1] 将所有 10 个 backup_code 标记为已使用");
    let user_id: String = sqlx::query_scalar("SELECT id::text FROM account WHERE email = $1")
        .bind(email)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE user_totp_backup_codes SET used = true
         WHERE user_totp_config_id = (SELECT id FROM user_totp_config WHERE user_id = $1::uuid)",
    )
    .bind(&user_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();
    println!("[Step 1] ✓ 所有 backup_code 标记为已使用");

    // ============================================================================
    // Step 2: 获取 temp_token
    // ============================================================================
    println!("[Step 2] 获取 temp_token");
    let (temp_token, _) = create_temp_totp_session(ctx, email, password).await;
    println!("[Step 2] ✓ temp_token: {}", temp_token);

    // ============================================================================
    // Step 3: 尝试使用无效的 backup_code
    // ============================================================================
    println!("[Step 3] 尝试使用无效的 backup_code");
    let realm_id = ctx._realm_id.clone();
    let result = complete_totp_login(ctx, &realm_id, &temp_token, None, Some("000000")).await;
    assert!(result.is_err(), "Invalid backup code should fail");
    let status = result.unwrap_err();
    assert_eq!(status, StatusCode::UNAUTHORIZED, "Should return 401");
    println!("[Step 3] ✓ 返回 401 状态码");

    // Cleanup
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：TOTP Login - Backup Codes Exhausted Failure");
}
// ============================================================================
// Phase 2: US-TO-002 Enable Scenarios (5 tests)
// ============================================================================

/// ============================================================================
/// User Story: TOTP Enable - Invalid Code Failure
///
/// **场景描述**：
/// 验证码错误（失败场景）。
///
/// **测试步骤**：
/// 1. 创建测试用户
/// 2. 配置 Realm TOTP
/// 3. 登录
/// 4. 启动 TOTP setup
/// 5. 使用无效 code 验证
/// 6. 检查 TOTP 配置仍为未启用
///
/// **验收标准**：
/// - 返回 401 状态码
/// - TOTP 配置仍为 enabled=false
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_enable_invalid_code_failure
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_enable_invalid_code_failure(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    println!("[Setup] 创建测试用户");
    let email = "totp_enable_user1@cas.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");

    println!("[Step 1] 启动 TOTP setup");
    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();

    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let temp_token = enable_body["tempToken"].as_str().unwrap();
    println!("[Step 1] ✓ TOTP setup 启动成功");

    println!("[Step 2] 使用无效 code 验证");
    let verify_payload = json!({
        "tempToken": temp_token,
        "code": "000000"
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();

    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::UNAUTHORIZED);
    println!("[Step 2] ✓ 返回 401 状态码");

    println!("[Step 3] 检查 TOTP 配置仍为未启用");
    let user_id: String = sqlx::query_scalar("SELECT id::text FROM account WHERE email = $1")
        .bind(email)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap();
    let config_enabled: Option<bool> =
        sqlx::query_scalar("SELECT enabled FROM user_totp_config WHERE user_id = $1::uuid")
            .bind(&user_id)
            .fetch_optional(&ctx._app_state.pool)
            .await
            .unwrap();
    assert_eq!(config_enabled, Some(false), "TOTP should not be enabled");
    println!("[Step 3] ✓ TOTP 配置仍为 enabled=false");

    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：TOTP Enable - Invalid Code Failure");
}

/// ============================================================================
/// User Story: TOTP Enable - Backup Codes Displayed Once
///
/// **场景描述**：
/// 保存备份恢复码。
///
/// **测试步骤**：
/// 1. 创建测试用户
/// 2. 配置 Realm TOTP
/// 3. 登录
/// 4. 启动 TOTP setup
/// 5. 验证首次调用返回 10 个 backup_codes
/// 6. 验证后 backup_codes 不再显示
///
/// **验收标准**：
/// - 首次调用返回 10 个 backup_codes
/// - 验证后 backup_codes 不再显示
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_enable_backup_codes_displayed_once
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_enable_backup_codes_displayed_once(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    println!("[Setup] 创建测试用户");
    let email = "totp_enable_user3@cas.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");

    println!("[Step 1] 首次调用 TOTP setup");
    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();

    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let backup_codes1: Vec<String> = enable_body["backupCodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(backup_codes1.len(), 10, "Should have 10 backup codes");
    println!("[Step 1] ✓ 首次调用返回 10 个 backup_codes");

    println!("[Step 2] 查询 TOTP status（验证后）");
    let status_request = Request::builder()
        .method("GET")
        .uri("/api/user/totp/status")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::empty())
        .unwrap();

    let status_response = app.clone().oneshot(status_request).await.unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);

    let status_body_bytes = axum::body::to_bytes(status_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let status_body: serde_json::Value =
        serde_json::from_slice(&status_body_bytes).expect("Failed to parse JSON");

    assert_eq!(status_body["enabled"], false, "TOTP not yet enabled");
    // backup_codes in status is stats, not the actual codes
    assert_eq!(
        status_body["backupCodes"]["total"], 10,
        "Should have 10 backup codes total"
    );
    println!("[Step 2] ✓ Status API 只返回统计信息，不返回实际 codes");

    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：TOTP Enable - Backup Codes Displayed Once");
}
// ============================================================================
// Phase 4: US-TO-005 Regenerate Scenarios (2 tests)
// ============================================================================

/// ============================================================================
/// User Story: TOTP Regenerate - Invalid Password Failure
///
/// **场景描述**：
/// 密码验证失败（失败场景）。
///
/// **测试步骤**：
/// 1. 创建测试用户
/// 2. 配置 Realm TOTP
/// 3. 登录
/// 4. 启用并验证 TOTP
/// 5. 使用错误密码请求 reauth ticket（密码校验在 reauth 阶段进行）
/// 6. 验证 reauth verify 返回 401
/// 7. 检查原有 secret 保持有效
///
/// **验收标准**：
/// - 返回 401 状态码
/// - 原有 secret 保持有效
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_regenerate_invalid_password_failure
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_regenerate_invalid_password_failure(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    println!("[Setup] 创建测试用户并启用 TOTP");
    let email = "totp_regenerate_user1@cas.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");

    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();

    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let old_secret = enable_body["secret"].as_str().unwrap();
    let temp_token = enable_body["tempToken"].as_str().unwrap();
    let totp_code = generate_totp_code(old_secret);

    let verify_payload = json!({
        "tempToken": temp_token,
        "code": totp_code
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();

    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    println!("[Setup] ✓ TOTP 已启用");
    println!("[Setup] Old secret: {}", old_secret);

    println!("[Step 1] 使用错误密码请求 reauth ticket（密码校验在 reauth 阶段进行）");
    let reauth_response =
        attempt_reauth_verify(ctx, &login_token, "bind_authenticator", "wrongpassword").await;
    assert_eq!(reauth_response.status(), StatusCode::UNAUTHORIZED);
    println!("[Step 1] ✓ reauth verify 返回 401 状态码");

    println!("[Step 2] 检查原有 secret 保持有效");
    // Try to login with old secret to verify it's still valid
    let (temp_token2, _) = create_temp_totp_session(ctx, email, password).await;
    let totp_code2 = generate_totp_code(old_secret);
    let realm_id = ctx._realm_id.clone();
    let result = complete_totp_login(ctx, &realm_id, &temp_token2, Some(&totp_code2), None).await;
    assert!(result.is_ok(), "Old secret should still be valid");
    println!("[Step 2] ✓ 原有 secret 保持有效");

    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：TOTP Regenerate - Invalid Password Failure");
}

/// ============================================================================
/// User Story: TOTP Regenerate - Verification Required
///
/// **场景描述**：
/// 重新生成后需立即验证（回滚机制）。
///
/// **测试步骤**：
/// 1. 创建测试用户
/// 2. 配置 Realm TOTP
/// 3. 登录
/// 4. 启用并验证 TOTP
/// 5. 重新生成 TOTP secret
/// 6. 验证前旧 secret 有效
/// 7. 使用新 secret 验证
/// 8. 验证后仅新 secret 有效（回滚机制工作）
///
/// **验收标准**：
/// - 验证前旧 secret 有效
/// - 验证后仅新 secret 有效（回滚机制工作）
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_totp_regenerate_verification_required
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_totp_regenerate_verification_required(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    unsafe {
        std::env::set_var("TOTP_SECRET_KEY", "test_key_32_bytes_long_1234567890");
    }

    println!("[Setup] 创建测试用户并启用 TOTP");
    let email = "totp_regenerate_user2@cas.com";
    let password = "password123";
    create_test_user(ctx, email, password).await;
    setup_realm_totp_config(ctx, true, false).await;

    let login_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });
    let login_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/login", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(login_payload.to_string()))
        .unwrap();
    let login_response = app.clone().oneshot(login_request).await.unwrap();
    let (_response, login_token) = crate::tests::extract_bearer_token(login_response).await;
    let login_token = login_token.expect("Login should return accessToken");

    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let enable_payload = json!({ "reauth_token": reauth_token });
    let enable_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(enable_payload.to_string()))
        .unwrap();

    let enable_response = app.clone().oneshot(enable_request).await.unwrap();
    assert_eq!(enable_response.status(), StatusCode::OK);

    let enable_body_bytes = axum::body::to_bytes(enable_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let enable_body: serde_json::Value =
        serde_json::from_slice(&enable_body_bytes).expect("Failed to parse JSON");

    let old_secret = enable_body["secret"].as_str().unwrap();
    let temp_token = enable_body["tempToken"].as_str().unwrap();
    let totp_code = generate_totp_code(old_secret);

    let verify_payload = json!({
        "tempToken": temp_token,
        "code": totp_code
    });
    let verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(verify_payload.to_string()))
        .unwrap();

    let verify_response = app.clone().oneshot(verify_request).await.unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    println!("[Setup] ✓ TOTP 已启用");
    println!("[Setup] Old secret: {}", old_secret);

    println!("[Step 1] 验证前旧 secret 有效");
    let (temp_token2, _) = create_temp_totp_session(ctx, email, password).await;
    let totp_code2 = generate_totp_code(old_secret);
    let realm_id = ctx._realm_id.clone();
    let result = complete_totp_login(ctx, &realm_id, &temp_token2, Some(&totp_code2), None).await;
    assert!(
        result.is_ok(),
        "Old secret should be valid before regeneration"
    );
    println!("[Step 1] ✓ 旧 secret 有效");

    println!("[Step 2] 重新生成 TOTP secret");
    let reauth_token = obtain_reauth_token(ctx, &login_token, "bind_authenticator", password).await;
    let regenerate_payload = json!({ "reauth_token": reauth_token });
    let regenerate_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/regenerate")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(regenerate_payload.to_string()))
        .unwrap();

    let regenerate_response = app.clone().oneshot(regenerate_request).await.unwrap();
    assert_eq!(regenerate_response.status(), StatusCode::OK);

    let regenerate_body_bytes = axum::body::to_bytes(regenerate_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let regenerate_body: serde_json::Value =
        serde_json::from_slice(&regenerate_body_bytes).expect("Failed to parse JSON");

    let new_secret = regenerate_body["secret"].as_str().unwrap();
    let new_temp_token = regenerate_body["tempToken"].as_str().unwrap();
    assert_ne!(new_secret, old_secret, "New secret should be different");
    println!("[Step 2] ✓ 新 secret 生成成功: {}", new_secret);

    println!("[Step 3] 使用新 secret 验证");
    let new_totp_code = generate_totp_code(new_secret);
    let new_verify_payload = json!({
        "tempToken": new_temp_token,
        "code": new_totp_code
    });
    let new_verify_request = Request::builder()
        .method("POST")
        .uri("/api/user/totp/verify")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", login_token))
        .body(Body::from(new_verify_payload.to_string()))
        .unwrap();

    let new_verify_response = app.clone().oneshot(new_verify_request).await.unwrap();
    assert_eq!(new_verify_response.status(), StatusCode::OK);
    println!("[Step 3] ✓ 新 secret 验证成功");

    println!("[Step 4] 验证后仅新 secret 有效");
    let (temp_token3, _) = create_temp_totp_session(ctx, email, password).await;
    let totp_code3 = generate_totp_code(new_secret);
    let result3 = complete_totp_login(ctx, &realm_id, &temp_token3, Some(&totp_code3), None).await;
    assert!(
        result3.is_ok(),
        "New secret should be valid after verification"
    );
    println!("[Step 4] ✓ 新 secret 有效");

    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM realm_config WHERE realm_id = $1 AND config_type = 'totp'")
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：TOTP Regenerate - Verification Required");
}

#[test_context(TestContext)]
#[tokio::test]
async fn dream_check_totp_threshold_starts_full_lockout(ctx: &mut TestContext) {
    use herald_core::domain::user_totp::{UserTotpConfig, UserTotpRepository};
    use herald_core::infrastructure::user_totp::PostgresUserTotpRepository;
    use redis::AsyncCommands;
    let user = create_test_user(ctx, "dream-lockout@test.com", "Password123!").await;
    let user_id = uuid::Uuid::parse_str(&user).unwrap();
    let mut config = UserTotpConfig::new(
        user_id,
        ctx._realm_id.clone(),
        "unused-backup-code-path".to_string(),
        1,
    );
    config.enable();
    PostgresUserTotpRepository::new(ctx.app_state.db.clone())
        .create_config(config)
        .await
        .unwrap();
    let temp = uuid::Uuid::now_v7().to_string();
    let mut conn = ctx.app_state.redis_manager.get().await.unwrap();
    let _: () = conn.set_ex(format!("totp:temp:{temp}"), json!({
        "user_id": user, "realm_id": ctx._realm_id, "client_id": ctx._client_id,
        "client_app_id": ctx._client_app_id, "client_ip": "127.0.0.1", "flow": "custom_user_ui"
    }).to_string(), 300).await.unwrap();
    let key = format!("totp:fail_count:{user}");
    // A slow series of failures has almost exhausted the counting window.
    let _: () = conn.set_ex(&key, 4, 10).await.unwrap();
    let result = complete_totp_login(ctx, &ctx._realm_id, &temp, None, Some("000000")).await;
    assert_eq!(result.unwrap_err(), StatusCode::UNAUTHORIZED);
    let ttl: i64 = conn.ttl(&key).await.unwrap();
    assert!(
        ttl >= 890,
        "fifth failure must start a full 900-second lockout, got {ttl}"
    );
}
