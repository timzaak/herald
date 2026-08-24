// =============================================================================
// User Register API Scenarios Tests
// =============================================================================
//
// 测试 /api/auth/{realmId}/register API
//
// **测试目标**：
// 1. 验证用户注册 API 返回正确的响应
// 2. 验证注册后用户状态为 0（等待邮箱验证）
// 3. 验证注册后未验证邮箱无法登录
// 4. 验证邮箱重复注册被正确拒绝
//
// **运行方式**：
// ```bash
// cargo nextest run --workspace user_register
// ```
//
// =============================================================================

use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

fn response_message(body: &serde_json::Value) -> &str {
    body.get("message")
        .or_else(|| body.get("error"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

/// ============================================================================
/// User Story: 用户注册成功
///
/// **场景描述**：
/// 新用户在 realm1 下注册账号，验证注册 API 返回正确的响应。
///
/// **测试步骤**：
/// 1. 启用 realm 的注册功能
/// 2. 使用有效的邮箱和密码注册
/// 3. 验证响应状态码为 200
/// 4. 验证返回的消息包含验证要求
/// 5. 验证用户已创建且状态为 0（等待验证）
/// 6. 验证邮箱验证码已生成
///
/// **验收标准**：
/// - 注册成功，返回 200 状态码
/// - 响应包含 verification_required: true
/// - 用户状态为 0（等待验证）
/// - 邮箱验证码已存储在数据库中
///
/// **测试账号**：
/// - Email: test-register@cas.com
/// - Password: password123
/// - Realm: realm1
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_user_register_success
/// ```
/// ============================================================================
/// User Story: 邮箱重复注册被拒绝
///
/// **场景描述**：
/// 尝试使用已注册的邮箱再次注册，系统应该拒绝请求。
///
/// **测试步骤**：
/// 1. 启用注册功能并注册新用户
/// 2. 尝试使用相同的邮箱再次注册
/// 3. 验证返回 409 Conflict
///
/// **验收标准**：
/// - 返回 409 Conflict
/// - 返回错误信息 "email already registered"
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_user_register_duplicate_email
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_user_register_duplicate_email(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // ============================================================================
    // Step 1: 启用注册功能并注册新用户
    // ============================================================================
    println!("[Step 1] 注册新用户: duplicate@cas.com");
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .unwrap();

    let email = "duplicate@cas.com";

    let payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": "password123",
        "turnstileToken": "dummy"
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    println!("[Step 1] ✓ 用户注册成功");

    // ============================================================================
    // Step 2: 尝试使用相同的邮箱再次注册
    // ============================================================================
    println!("[Step 2] 尝试重复注册");
    let duplicate_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let duplicate_response = app.clone().oneshot(duplicate_request).await.unwrap();

    // ============================================================================
    // Step 3: 验证返回 409 Conflict
    // ============================================================================
    println!("[Step 3] 验证返回 409 Conflict");
    assert_eq!(
        duplicate_response.status(),
        StatusCode::CONFLICT,
        "Should return 409 Conflict for duplicate email"
    );

    // 验证错误信息
    let body_bytes = axum::body::to_bytes(duplicate_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    assert!(
        response_message(&body)
            .to_lowercase()
            .contains("email already registered"),
        "Error message should mention email already registered"
    );

    println!("[Step 3] ✓ 返回 409 Conflict，错误信息正确");

    // 清理测试数据
    sqlx::query("DELETE FROM email_verification_code WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：邮箱重复注册被正确拒绝");
}

/// ============================================================================
/// User Story: 注册功能禁用时无法注册
///
/// **场景描述**：
/// 当 realm 的注册功能被禁用时，尝试注册应该失败。
///
/// **测试步骤**：
/// 1. 不启用注册功能（或显式禁用）
/// 2. 尝试注册新用户
/// 3. 验证返回 400 Bad Request
///
/// **验收标准**：
/// - 返回 400 Bad Request
/// - 返回错误信息 "Registration is not enabled for this realm"
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_user_register_disabled
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_user_register_disabled(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // ============================================================================
    // Step 1: 不启用注册功能
    // ============================================================================
    println!("[Step 1] 确认注册功能未启用");

    // ============================================================================
    // Step 2: 尝试注册新用户
    // ============================================================================
    println!("[Step 2] 尝试注册（注册功能未启用）");
    let payload = json!({
        "clientId": ctx._client_id,
        "email": "disabled@cas.com",
        "password": "password123",
        "turnstileToken": "dummy"
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();

    // ============================================================================
    // Step 3: 验证返回 400 Bad Request
    // ============================================================================
    println!("[Step 3] 验证返回 400 Bad Request");
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Should return 400 Bad Request when registration is disabled"
    );

    // 验证错误信息
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    assert!(
        response_message(&body)
            .to_lowercase()
            .contains("registration is not enabled"),
        "Error message should mention registration not enabled"
    );

    println!("[Step 3] ✓ 返回 400 Bad Request，错误信息正确");

    println!("\n✅ User Story 完成：注册功能禁用时无法注册");
}
/// ============================================================================
/// User Story: 不需要邮箱验证时注册创建激活用户
///
/// **场景描述**：
/// 当 realm 配置 `require_email_verification = false` 或未配置时，注册应该：
/// - 创建状态为 1（已激活）的用户
/// - 不生成邮箱验证码
/// - 返回 verification_required: false
///
/// **测试步骤**：
/// 1. 启用注册功能
/// 2. 禁用邮箱验证要求 (require_email_verification = false)
/// 3. 注册新用户
/// 4. 验证用户状态为 1（已激活）
/// 5. 验证邮箱验证码未生成
/// 6. 验证响应包含 verification_required: false
///
/// **验收标准**：
/// - 用户状态为 1（已激活）
/// - 邮箱验证码不存在于数据库
/// - verification_required 为 false
/// - 消息提示注册成功
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_user_register_without_verification_required
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_user_register_without_verification_required(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // ============================================================================
    // Step 1: 启用注册功能
    // ============================================================================
    println!("[Step 1] 启用注册功能");
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to enable registration");

    // ============================================================================
    // Step 2: 禁用邮箱验证要求
    // ============================================================================
    println!("[Step 2] 禁用邮箱验证要求 (require_email_verification = false)");
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'require_email_verification', 'false', true)",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to disable email verification requirement");
    println!("[Step 2] ✓ 邮箱验证要求已禁用");

    // ============================================================================
    // Step 3: 注册新用户
    // ============================================================================
    println!("[Step 3] 注册新用户: no-verify-required@cas.com");
    let email = "no-verify-required@cas.com";
    let password = "password123";

    let payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Registration should succeed"
    );
    println!("[Step 3] ✓ 注册请求完成");

    // ============================================================================
    // Step 4: 验证用户状态为 1（已激活）
    // ============================================================================
    println!("[Step 4] 验证用户状态");
    let (user_id, status): (String, i16) =
        sqlx::query_as("SELECT id::text, status FROM account WHERE realm_id = $1 AND email = $2")
            .bind(&ctx._realm_id)
            .bind(email)
            .fetch_one(&ctx._app_state.pool)
            .await
            .expect("Failed to fetch user");

    assert_eq!(status, 1, "User status should be 1 (active)");
    println!(
        "[Step 4] ✓ 用户状态正确: user_id={}, status=1 (active)",
        user_id
    );

    // ============================================================================
    // Step 5: 验证邮箱验证码未生成
    // ============================================================================
    println!("[Step 5] 验证邮箱验证码不存在");
    let code_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM email_verification_code WHERE email = $1 AND type = 'register'",
    )
    .bind(email)
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to count verification codes");

    assert_eq!(code_count, 0, "Should have no verification code");
    println!("[Step 5] ✓ 邮箱验证码未生成: count=0");

    // ============================================================================
    // Step 6: 验证响应包含 verification_required: false
    // ============================================================================
    println!("[Step 6] 验证响应 JSON");
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    assert_eq!(
        body["verificationRequired"], false,
        "verification_required should be false"
    );
    assert_eq!(
        body["message"].as_str().unwrap(),
        "Registration successful.",
        "Message should indicate successful registration"
    );
    println!(
        "[Step 6] ✓ 响应正确: verification_required=false, message={}",
        body["message"]
    );

    // 清理测试数据
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 完成：不需要邮箱验证时注册创建激活用户");
}
/// ============================================================================
/// User Story: 场景 1d - 重新发送验证邮件
///
/// **场景描述**：
/// 用户在邮箱验证页面点击"重新发送验证邮件"按钮，系统应该重新发送验证邮件到用户邮箱。
///
/// **测试步骤**：
/// 1. 启用注册功能和邮箱验证
/// 2. 注册新用户（状态为 0，等待验证）
/// 3. 记录初始验证码数量
/// 4. 调用重新发送验证邮件 API
/// 5. 验证新的验证码已生成
/// 6. 验证响应状态码正确
///
/// **验收标准**：
/// - 重新发送 API 返回 200 OK
/// - 验证码数量增加（或被更新）
/// - 响应消息提示验证邮件已发送
///
/// **API 端点**：
/// POST /api/auth/{realmId}/verify_email/trigger
///
/// **运行方式**：
/// ```bash
/// cargo nextest run test_scenario_resend_verification_email
/// ```
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_resend_verification_email(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // ============================================================================
    // Step 1: 启用注册功能和邮箱验证
    // ============================================================================
    println!("[Step 1] 启用注册功能和邮箱验证");
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'enabled', 'true', true)",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to enable registration");

    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
         VALUES ($1, 'registration', 'require_email_verification', 'true', true)",
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to enable email verification requirement");

    println!("[Step 1] ✓ 注册功能和邮箱验证已启用");

    // ============================================================================
    // Step 2: 注册新用户（状态为 0，等待验证）
    // ============================================================================
    println!("[Step 2] 注册新用户: resend-test@cas.com");
    let email = "resend-test@cas.com";
    let password = "password123";

    let payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Registration should succeed"
    );
    println!("[Step 2] ✓ 用户注册成功，状态为 0（等待验证）");

    // ============================================================================
    // Step 3: 记录当前最新验证码
    // ============================================================================
    println!("[Step 3] 记录当前最新验证码");
    let initial_latest_code: Option<String> =
        sqlx::query_scalar("SELECT verification_code FROM email_verification_code WHERE email = $1 ORDER BY id DESC LIMIT 1")
            .bind(email)
            .fetch_optional(&ctx._app_state.pool)
            .await
            .expect("Failed to read latest verification code");
    println!("[Step 3] ✓ 初始验证码: {:?}", initial_latest_code);

    // ============================================================================
    // Step 4: 调用重新发送验证邮件 API
    // ============================================================================
    println!("[Step 4] 调用重新发送验证邮件 API");
    let resend_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "turnstileToken": "dummy"
    });

    let resend_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/verify_email/trigger", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "3.3.3.3")
        .body(Body::from(resend_payload.to_string()))
        .unwrap();

    let resend_response = app
        .clone()
        .oneshot(resend_request)
        .await
        .expect("Failed to send resend request");
    println!("[Step 4] ✓ 重新发送请求完成");

    // ============================================================================
    // Step 5: 验证响应状态码正确
    // ============================================================================
    println!("[Step 5] 验证响应状态码");
    assert_eq!(
        resend_response.status(),
        StatusCode::OK,
        "Resend should return 200 OK"
    );

    let body_bytes = axum::body::to_bytes(resend_response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    assert_eq!(
        body["message"].as_str().unwrap(),
        "ok",
        "Message should be 'ok'"
    );
    println!("[Step 5] ✓ 响应状态码为 200，消息正确");

    // ============================================================================
    // Step 6: 验证新的验证码已生成
    // ============================================================================
    // Issuing a new code invalidates prior ones (newest-code-wins), so the
    // assertion is on the code VALUE changing, not on the row count growing.
    println!("[Step 6] 验证新的验证码已生成");
    let final_latest_code: Option<String> =
        sqlx::query_scalar("SELECT verification_code FROM email_verification_code WHERE email = $1 ORDER BY id DESC LIMIT 1")
            .bind(email)
            .fetch_optional(&ctx._app_state.pool)
            .await
            .expect("Failed to read latest verification code");

    assert!(
        final_latest_code.is_some() && final_latest_code != initial_latest_code,
        "Resend should replace the previous verification code with a new one"
    );
    println!("[Step 6] ✓ 新验证码已生成: {:?}", final_latest_code);

    // 清理测试数据
    sqlx::query("DELETE FROM email_verification_code WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM account WHERE email = $1")
        .bind(email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    println!("\n✅ User Story 场景 1d 完成：重新发送验证邮件");
}
