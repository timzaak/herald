/// 场景测试：Client App 基本设置功能
///
/// 测试 Client App 的跳转地址白名单、Session 配置、启用/禁用等功能
#[cfg(test)]
mod tests {
    use crate::tests::helpers::*;
    use crate::tests::response_json;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{body::Body, http::Request};
    use herald_core::domain::client_api_keys::ADMIN_API_CLIENT_ID;
    use serde_json::json;
    use sqlx::Row;
    use test_context::test_context;
    use tower::ServiceExt;

    use SchemaTestContext as ClientAppTestContext;

    /// Helper function to setup admin session for client app tests
    async fn setup_admin_session(ctx: &mut ClientAppTestContext, email: &str) -> String {
        let (admin_token, user_id) = create_admin_session_with_user(ctx, email, 1800).await;

        // 授予 Realm Admin 角色
        grant_realm_admin_role(ctx, &user_id).await;

        admin_token
    }

    /// 测试：创建 Client App 时自动生成 client_secret
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_create_client_app_generates_secret(ctx: &mut ClientAppTestContext) {
        let app = ctx.create_unified_test_router();

        // Setup authentication
        let admin_token = setup_admin_session(ctx, "test-generate-secret@test.com").await;

        // 创建一个 Client App
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/client/{}", ctx._realm_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "clientId": "test-app",
                    "name": "Test Application",
                    "description": "A test application",
                    "redirectUris": ["https://example.com/callback"],
                    "enabled": true,
                    "browserRefreshAbsoluteTtlSeconds": 86400
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 201);

        let json: serde_json::Value = response_json(response).await;
        assert!(
            json["clientSecret"].as_str().is_some(),
            "clientSecret should be returned on create"
        );

        // 验证 client_secret 已生成
        let row = sqlx::query("SELECT client_secret FROM client_app WHERE client_id = $1")
            .bind("test-app")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();

        let secret: Option<String> = row.get("client_secret");
        assert!(secret.is_some(), "client_secret should be auto-generated");
        assert!(
            !secret.unwrap().is_empty(),
            "client_secret should not be empty"
        );
    }

    /// 测试：创建 Client App 时 redirect_uris 至少需要一个
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_create_client_app_requires_redirect_uri(ctx: &mut ClientAppTestContext) {
        let app = ctx.create_unified_test_router();

        // Setup authentication
        let admin_token = setup_admin_session(ctx, "test-redirect-uri@test.com").await;

        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/client/{}", ctx._realm_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "clientId": "test-app",
                    "name": "Test Application",
                    "redirectUris": []
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // 应该返回 400 Bad Request
        assert_eq!(response.status(), 400);

        let json: serde_json::Value = response_json(response).await;
        assert_eq!(json["status"], 400);
        assert_eq!(json["code"], "bad_request");
        assert!(json["message"].as_str().is_some());
    }

    /// 测试：启用 Device Code Grant 时允许不配置 redirect_uri
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_create_device_code_client_app_allows_empty_redirect_uri(
        ctx: &mut ClientAppTestContext,
    ) {
        let app = ctx.create_unified_test_router();

        let admin_token =
            setup_admin_session(ctx, "test-device-code-empty-redirect@test.com").await;

        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/client/{}", ctx._realm_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "clientId": "test-device-code-app",
                    "name": "Test Device Code Application",
                    "redirectUris": [],
                    "deviceCodeGrantEnabled": true
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 201);

        let json: serde_json::Value = response_json(response).await;
        assert_eq!(json["deviceCodeGrantEnabled"], true);
        assert_eq!(json["redirectUris"].as_array().unwrap().len(), 0);
    }

    /// 测试：更新 Client App 的 redirect_uris
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_update_client_app_redirect_uris(ctx: &mut ClientAppTestContext) {
        let app = ctx.create_unified_test_router();

        // Setup authentication
        let admin_token = setup_admin_session(ctx, "test-update-redirect@test.com").await;

        // 先创建一个 Client App
        let create_request = Request::builder()
            .method("POST")
            .uri(format!("/api/client/{}", ctx._realm_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "clientId": "test-app",
                    "name": "Test Application",
                    "redirectUris": ["https://example.com/callback"]
                })
                .to_string(),
            ))
            .unwrap();

        let create_response = app.clone().oneshot(create_request).await.unwrap();
        assert_eq!(create_response.status(), 201);

        // 获取 client app ID
        let json: serde_json::Value = response_json(create_response).await;
        let client_app_id = json["id"].as_str().unwrap();

        // 更新 redirect_uris
        let update_request = Request::builder()
            .method("PUT")
            .uri(format!("/api/client/{}/{}", ctx._realm_id, client_app_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "redirectUris": ["https://example.com/callback", "https://app.example.com/auth"]
                })
                .to_string(),
            ))
            .unwrap();

        let update_response = app.clone().oneshot(update_request).await.unwrap();
        assert_eq!(update_response.status(), 200);

        let update_json: serde_json::Value = response_json(update_response).await;
        assert!(
            update_json["clientSecret"].is_null(),
            "clientSecret should be hidden on normal update"
        );

        // 验证更新成功
        let row = sqlx::query("SELECT redirect_uris FROM client_app WHERE id::text = $1")
            .bind(client_app_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();

        let redirect_uris: serde_json::Value = row.get("redirect_uris");
        assert_eq!(
            redirect_uris.as_array().unwrap().len(),
            2,
            "Should have 2 redirect URIs"
        );
    }

    /// 测试：禁用和启用 Client App
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_enable_disable_client_app(ctx: &mut ClientAppTestContext) {
        let app = ctx.create_unified_test_router();

        // Setup authentication
        let admin_token = setup_admin_session(ctx, "test-enable-disable@test.com").await;

        // 创建一个启用的 Client App
        let create_request = Request::builder()
            .method("POST")
            .uri(format!("/api/client/{}", ctx._realm_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "clientId": "test-app",
                    "name": "Test Application",
                    "redirectUris": ["https://example.com/callback"],
                    "enabled": true
                })
                .to_string(),
            ))
            .unwrap();

        let create_response = app.clone().oneshot(create_request).await.unwrap();
        assert_eq!(create_response.status(), 201);

        // 获取 client app ID
        let json: serde_json::Value = response_json(create_response).await;
        let client_app_id = json["id"].as_str().unwrap();

        // 禁用 Client App
        let disable_request = Request::builder()
            .method("PUT")
            .uri(format!("/api/client/{}/{}", ctx._realm_id, client_app_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "enabled": false
                })
                .to_string(),
            ))
            .unwrap();

        let disable_response = app.clone().oneshot(disable_request).await.unwrap();
        assert_eq!(disable_response.status(), 200);

        // 验证已禁用
        let row = sqlx::query("SELECT enabled FROM client_app WHERE id::text = $1")
            .bind(client_app_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();

        let enabled: bool = row.get("enabled");
        assert!(!enabled, "Client app should be disabled");

        // 重新启用
        let enable_request = Request::builder()
            .method("PUT")
            .uri(format!("/api/client/{}/{}", ctx._realm_id, client_app_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "enabled": true
                })
                .to_string(),
            ))
            .unwrap();

        let enable_response = app.clone().oneshot(enable_request).await.unwrap();
        assert_eq!(enable_response.status(), 200);

        // 验证已启用
        let row = sqlx::query("SELECT enabled FROM client_app WHERE id::text = $1")
            .bind(client_app_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();

        let enabled: bool = row.get("enabled");
        assert!(enabled, "Client app should be enabled");
    }

    /// 测试：重新生成 client_secret
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_regenerate_client_secret(ctx: &mut ClientAppTestContext) {
        let app = ctx.create_unified_test_router();

        // Setup authentication
        let admin_token = setup_admin_session(ctx, "test-regenerate-secret@test.com").await;

        // 创建 Client App
        let create_request = Request::builder()
            .method("POST")
            .uri(format!("/api/client/{}", ctx._realm_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "clientId": "test-app",
                    "name": "Test Application",
                    "redirectUris": ["https://example.com/callback"]
                })
                .to_string(),
            ))
            .unwrap();

        let create_response = app.clone().oneshot(create_request).await.unwrap();
        assert_eq!(create_response.status(), 201);

        // 获取 client app ID 和原始 secret
        let json: serde_json::Value = response_json(create_response).await;
        let client_app_id = json["id"].as_str().unwrap();
        let original_secret = json["clientSecret"].as_str().unwrap();

        // 重新生成 secret
        let update_request = Request::builder()
            .method("PUT")
            .uri(format!("/api/client/{}/{}", ctx._realm_id, client_app_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "regenerateSecret": true
                })
                .to_string(),
            ))
            .unwrap();

        let update_response = app.clone().oneshot(update_request).await.unwrap();
        assert_eq!(update_response.status(), 200);

        // 验证 secret 已改变
        let json: serde_json::Value = response_json(update_response).await;
        let new_secret = json["clientSecret"].as_str().unwrap();

        assert_ne!(
            original_secret, new_secret,
            "Client secret should be regenerated"
        );
    }
    /// 测试：浏览器 refresh token 绝对 TTL 验证
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_browser_refresh_absolute_ttl_validation(ctx: &mut ClientAppTestContext) {
        let app = ctx.create_unified_test_router();

        // Setup authentication
        let admin_token = setup_admin_session(ctx, "test-ttl-validation@test.com").await;

        // 尝试创建低于 1 天最小值的 Client App
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/client/{}", ctx._realm_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "clientId": "test-app",
                    "name": "Test Application",
                    "redirectUris": ["https://example.com/callback"],
                    "browserRefreshAbsoluteTtlSeconds": 30
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // 应该返回 400 Bad Request
        assert_eq!(response.status(), 400);
    }

    /// 测试：内置 API Key Client App（admin-api-client）不允许被删除
    ///
    /// 该 App 仅在 Realm 创建时播种、无自动重建路径；删除后该 Realm 将
    /// 永久无法创建默认绑定 API Key（client-app PRD §4.1 / api-key-roles PRD §5）。
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_cannot_delete_builtin_api_key_client_app(ctx: &mut ClientAppTestContext) {
        let app = ctx.create_unified_test_router();

        let admin_token = setup_admin_session(ctx, "test-delete-api-client@test.com").await;

        // 幂等确保内置 API Key Client App 存在（Realm 初始化通常已播种）
        let builtin_id = seed_realm_api_key_client(ctx).await;

        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/api/client/{}/{}", ctx._realm_id, builtin_id))
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            400,
            "deleting the builtin API Key client app must be rejected"
        );

        // 行必须仍然存在——默认 API Key 创建路径依赖它
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM client_app WHERE id = $1 AND client_id = $2")
                .bind(builtin_id)
                .bind(ADMIN_API_CLIENT_ID)
                .fetch_one(&ctx.app_state.pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "the builtin API Key client app must survive");
    }
}
