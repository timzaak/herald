/// 场景测试：Client App 基本设置功能
///
/// 测试 Client App 的跳转地址白名单、Session 配置、启用/禁用等功能
#[cfg(test)]
mod tests {
    use crate::tests::helpers::oauth_pkce_helpers::{
        MCP_LOOPBACK_REDIRECT_TEMPLATES, ensure_mcp_client_seeded,
    };
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
            .uri("/api/client".to_string())
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
            .uri("/api/client".to_string())
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
            .uri("/api/client".to_string())
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
            .uri("/api/client".to_string())
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
            .uri(format!("/api/client/{}", client_app_id))
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
            .uri("/api/client".to_string())
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
            .uri(format!("/api/client/{}", client_app_id))
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
            .uri(format!("/api/client/{}", client_app_id))
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
            .uri("/api/client".to_string())
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
            .uri(format!("/api/client/{}", client_app_id))
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
            .uri("/api/client".to_string())
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
            .uri(format!("/api/client/{}", builtin_id))
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

    // =========================================================================
    // V7: built-in MCP client app protection (mcp-server US-MCP-001 场景 3 /
    // US-MCP-011 候选故事——管理台对内置 MCP 客户端只能查看与停用)
    // =========================================================================
    //
    // User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
    // （禁用 Client App 后 agent 连接被拒的管理面来源）；内置行的 seed 形状、
    // 系统标记、保留 ID、删除/更新保护、enabled 开关与权限零副作用。

    /// 测试：通过真实创建 Realm 链路（POST /api/realms → create_realm），
    /// 新 Realm 恰好 seed 一行 herald-mcp，且形状是 PKCE 公共客户端契约
    /// （enabled、无 secret、非 first-party、generation 0、三条 loopback 模板）。
    ///
    /// WHY：该行是整个 MCP 授权面的锚点——redirect 白名单、无 secret 的
    /// 公共客户端形状都来自它；seed 漂移（比如带上 secret 或改 redirect）
    /// 会在 authorize/token 处静默改变安全边界。
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_new_realm_seeds_exactly_one_builtin_mcp_client(ctx: &mut ClientAppTestContext) {
        let app = ctx.create_unified_test_router();
        let (super_admin_token, super_admin_user_id) =
            create_admin_session_with_user(ctx, "mcp-seed-superadmin@test.com", 1800).await;
        grant_realm_admin_role(ctx, &super_admin_user_id).await;

        let new_realm_id = format!(
            "mcprealm{}",
            chrono::Utc::now().timestamp_millis() % 1000000000
        );
        let create_request = Request::builder()
            .method("POST")
            .uri("/api/realms".to_string())
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", super_admin_token))
            .body(Body::from(
                json!({
                    "id": new_realm_id,
                    "name": "MCP Seed Realm",
                    "adminUser": { "email": "seed-admin@mcprealm.com", "password": "Password123" }
                })
                .to_string(),
            ))
            .unwrap();
        let create_response = app.oneshot(create_request).await.unwrap();
        assert_eq!(
            create_response.status(),
            201,
            "realm creation must succeed so the real seed path runs"
        );

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM client_app WHERE realm_id = $1 AND client_id = 'herald-mcp'",
        )
        .bind(&new_realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            count, 1,
            "a fresh realm must seed exactly one herald-mcp row"
        );

        let row = sqlx::query(
            "SELECT enabled, client_secret, is_first_party, mcp_token_generation, redirect_uris
             FROM client_app WHERE realm_id = $1 AND client_id = 'herald-mcp'",
        )
        .bind(&new_realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert!(
            row.get::<bool, _>("enabled"),
            "the MCP client must be seeded enabled"
        );
        assert!(
            row.get::<Option<String>, _>("client_secret").is_none(),
            "a PKCE-only public client must be seeded without a secret"
        );
        assert!(
            !row.get::<bool, _>("is_first_party"),
            "the MCP client is not a first-party UI client"
        );
        assert_eq!(
            row.get::<i64, _>("mcp_token_generation"),
            0,
            "a fresh seed has generation 0"
        );
        let redirect_uris: serde_json::Value = row.get("redirect_uris");
        assert_eq!(
            redirect_uris
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<std::collections::BTreeSet<_>>(),
            MCP_LOOPBACK_REDIRECT_TEMPLATES
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            "the seed must register exactly the three loopback /callback templates"
        );
    }

    /// 测试：列表与详情的 isSystemBuiltin 只读标记——herald-mcp 为 true，
    /// 普通新建应用为 false。
    ///
    /// WHY：这是资源保护标记而非用户权限；前端据此隐藏删除/编辑入口。
    /// 若标记缺失或对普通应用误报，管理台要么放行危险操作、要么把所有
    /// 应用锁死。
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_client_app_list_and_detail_mark_system_builtin(ctx: &mut ClientAppTestContext) {
        let app = ctx.create_unified_test_router();
        let admin_token = setup_admin_session(ctx, "mcp-builtin-marker@test.com").await;
        let mcp_app_id = ensure_mcp_client_seeded(ctx).await;

        // 列表：herald-mcp 标记为系统内置。
        let list_request = Request::builder()
            .method("GET")
            .uri("/api/client?page=0&pageSize=100".to_string())
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();
        let list_response = app.clone().oneshot(list_request).await.unwrap();
        assert_eq!(list_response.status(), 200);
        let list_json: serde_json::Value = response_json(list_response).await;
        let mcp_item = list_json["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["clientId"].as_str() == Some("herald-mcp"))
            .expect("the built-in MCP client must appear in the realm's list")
            .clone();
        assert_eq!(
            mcp_item["isSystemBuiltin"], true,
            "herald-mcp must be marked isSystemBuiltin"
        );
        assert_eq!(
            mcp_item["isFirstParty"], false,
            "the MCP client is not a first-party client"
        );

        // 新建普通应用：标记必须为 false。
        let create_request = Request::builder()
            .method("POST")
            .uri("/api/client".to_string())
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "clientId": "mcp-builtin-normal-app",
                    "name": "MCP Builtin Normal App",
                    "redirectUris": ["https://normal-mcp-app.com/callback"]
                })
                .to_string(),
            ))
            .unwrap();
        let create_response = app.clone().oneshot(create_request).await.unwrap();
        assert_eq!(create_response.status(), 201);
        let created: serde_json::Value = response_json(create_response).await;
        assert_eq!(
            created["isSystemBuiltin"], false,
            "an ordinary client app must not be marked system builtin"
        );
        let normal_app_id = created["id"].as_str().unwrap().to_string();

        // 详情：两个应用分别断言。
        let get_mcp = Request::builder()
            .method("GET")
            .uri(format!("/api/client/{}", mcp_app_id))
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();
        let get_response = app.clone().oneshot(get_mcp).await.unwrap();
        assert_eq!(get_response.status(), 200);
        let detail: serde_json::Value = response_json(get_response).await;
        assert_eq!(detail["isSystemBuiltin"], true);
        assert_eq!(detail["clientId"].as_str(), Some("herald-mcp"));
        assert!(
            detail["clientSecret"].is_null(),
            "the MCP client has no secret to show"
        );

        let get_normal = Request::builder()
            .method("GET")
            .uri(format!("/api/client/{}", normal_app_id))
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();
        let get_response = app.oneshot(get_normal).await.unwrap();
        assert_eq!(get_response.status(), 200);
        let detail: serde_json::Value = response_json(get_response).await;
        assert_eq!(detail["isSystemBuiltin"], false);
    }

    /// 测试：create 撞保留 client_id "herald-mcp" → 403，且不产生第二行、
    /// 不改写既有 seed 行的 redirect 白名单。
    ///
    /// WHY：公共 create 入口若允许占用保留 ID，一个普通（可配任意
    /// redirect、可轮换 secret 的）应用就会顶替 MCP 公共客户端身份。
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_create_client_app_cannot_shadow_builtin_mcp_client_id(
        ctx: &mut ClientAppTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let admin_token = setup_admin_session(ctx, "mcp-reserved-id@test.com").await;
        ensure_mcp_client_seeded(ctx).await;

        let request = Request::builder()
            .method("POST")
            .uri("/api/client".to_string())
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "clientId": "herald-mcp",
                    "name": "Shadow App",
                    "redirectUris": ["https://shadow.example/callback"]
                })
                .to_string(),
            ))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            403,
            "occupying the reserved MCP client id must be forbidden"
        );
        let body: serde_json::Value = response_json(response).await;
        assert!(
            body["message"]
                .as_str()
                .is_some_and(|m| m.contains("Reserved")),
            "rejection must say the client id is reserved, got {body}"
        );

        // 仍然只有 seed 那一行，且 redirect 白名单未被改写。
        let row = sqlx::query(
            "SELECT COUNT(*)::text, MIN(redirect_uris::text) FROM client_app
             WHERE realm_id = $1 AND client_id = 'herald-mcp'",
        )
        .bind(&ctx._realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            row.get::<String, _>("count"),
            "1",
            "the reserved id must not create a second row"
        );
        let redirect_uris: Option<String> = row.get("min");
        assert!(
            redirect_uris.is_some_and(|uris| uris.contains("127.0.0.1/callback")),
            "the seeded loopback whitelist must survive the shadowing attempt"
        );
    }

    /// 测试：内置 MCP 客户端不可删除（400 固定文案），普通应用删除不受影响。
    ///
    /// WHY：该行是 realm 的 agent 接入锚点且无自动重建路径；"停用"才是
    /// 受支持的操作。文案是管理台识别该拒绝原因的契约。
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_delete_builtin_mcp_client_rejected_but_normal_app_deletable(
        ctx: &mut ClientAppTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let admin_token = setup_admin_session(ctx, "mcp-delete-protection@test.com").await;
        let mcp_app_id = ensure_mcp_client_seeded(ctx).await;

        let delete_request = Request::builder()
            .method("DELETE")
            .uri(format!("/api/client/{}", mcp_app_id))
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();
        let delete_response = app.clone().oneshot(delete_request).await.unwrap();
        assert_eq!(
            delete_response.status(),
            400,
            "deleting the built-in MCP client must be rejected"
        );
        let body: serde_json::Value = response_json(delete_response).await;
        assert_eq!(
            body["message"].as_str(),
            Some("Cannot delete the built-in MCP client app"),
            "the delete rejection message is the admin-UI contract"
        );

        // 行必须仍然存在。
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM client_app WHERE id = $1 AND client_id = 'herald-mcp'",
        )
        .bind(mcp_app_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(count, 1, "the built-in MCP client must survive the delete");

        // 对照：普通应用仍可删除，保护不外溢。
        let create_request = Request::builder()
            .method("POST")
            .uri("/api/client".to_string())
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({
                    "clientId": "mcp-deletable-normal-app",
                    "name": "MCP Deletable Normal App",
                    "redirectUris": ["https://deletable-mcp-app.com/callback"]
                })
                .to_string(),
            ))
            .unwrap();
        let create_response = app.clone().oneshot(create_request).await.unwrap();
        assert_eq!(create_response.status(), 201);
        let created: serde_json::Value = response_json(create_response).await;
        let normal_app_id = created["id"].as_str().unwrap();

        let delete_normal = Request::builder()
            .method("DELETE")
            .uri(format!("/api/client/{}", normal_app_id))
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();
        let delete_response = app.oneshot(delete_normal).await.unwrap();
        assert_eq!(
            delete_response.status(),
            204,
            "an ordinary app stays deletable"
        );
    }

    /// 测试：内置 MCP 客户端只接受 enabled 开关——改 name/redirectUris 是
    /// 400 固定文案；disable 落库且递增 mcp_token_generation；再 enable 与
    /// 同值幂等均 200，且 redirect 白名单始终未被改动。
    ///
    /// WHY：redirect 白名单与无 secret 形状是 MCP 授权契约本身，不是租户
    /// 配置；generation 递增（DEC-006）保证 disable 期间签发的凭证在
    /// Redis 撤销失败时也会在下一次 DB 复核被拒绝。
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_update_builtin_mcp_client_only_allows_enabled_toggle(
        ctx: &mut ClientAppTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let admin_token = setup_admin_session(ctx, "mcp-update-protection@test.com").await;
        let mcp_app_id = ensure_mcp_client_seeded(ctx).await;

        let update = |body: serde_json::Value| {
            let app = app.clone();
            let token = admin_token.clone();
            let id = mcp_app_id.to_string();
            async move {
                let request = Request::builder()
                    .method("PUT")
                    .uri(format!("/api/client/{}", id))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", token))
                    .body(Body::from(body.to_string()))
                    .unwrap();
                app.oneshot(request).await.unwrap()
            }
        };

        // 非 enabled 字段一律 400 + 固定文案。
        for body in [
            json!({ "name": "Renamed MCP" }),
            json!({ "redirectUris": ["https://evil.example/callback"] }),
            json!({ "regenerateSecret": true }),
        ] {
            let response = update(body).await;
            assert_eq!(
                response.status(),
                400,
                "non-enabled fields must be rejected on the built-in MCP client"
            );
            let error: serde_json::Value = response_json(response).await;
            assert_eq!(
                error["message"].as_str(),
                Some("Built-in MCP client only supports enabled updates"),
                "the protection message is the admin-UI contract"
            );
        }

        // disable → 200，落库 enabled=false 且 generation 递增。
        let response = update(json!({ "enabled": false })).await;
        assert_eq!(
            response.status(),
            200,
            "the enabled toggle must be accepted"
        );
        let row = sqlx::query(
            "SELECT enabled, mcp_token_generation, redirect_uris FROM client_app WHERE id = $1",
        )
        .bind(mcp_app_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert!(!row.get::<bool, _>("enabled"), "disable must persist");
        assert!(
            row.get::<i64, _>("mcp_token_generation") >= 1,
            "disable must bump the MCP token generation (DEC-006 anti-revival)"
        );
        let redirect_uris: serde_json::Value = row.get("redirect_uris");
        assert_eq!(
            redirect_uris
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<std::collections::BTreeSet<_>>(),
            MCP_LOOPBACK_REDIRECT_TEMPLATES
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            "the loopback whitelist must be untouched by the toggle"
        );
        let generation_after_disable: i64 =
            sqlx::query_scalar("SELECT mcp_token_generation FROM client_app WHERE id = $1")
                .bind(mcp_app_id)
                .fetch_one(&ctx.app_state.pool)
                .await
                .unwrap();

        // re-enable → 200；同值幂等（重复 enable）也 200，且 generation 不再变化。
        let response = update(json!({ "enabled": true })).await;
        assert_eq!(response.status(), 200, "re-enable must be accepted");
        let enabled: bool = sqlx::query_scalar("SELECT enabled FROM client_app WHERE id = $1")
            .bind(mcp_app_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
        assert!(enabled, "re-enable must persist");

        let response = update(json!({ "enabled": true })).await;
        assert_eq!(
            response.status(),
            200,
            "a same-value enabled update is idempotent"
        );
        let generation_final: i64 =
            sqlx::query_scalar("SELECT mcp_token_generation FROM client_app WHERE id = $1")
                .bind(mcp_app_id)
                .fetch_one(&ctx.app_state.pool)
                .await
                .unwrap();
        assert_eq!(
            generation_final, generation_after_disable,
            "re-enable (and same-value repeats) must never bump or reset the generation"
        );
    }

    /// 测试：无 clients.manage 权限的管理台用户 disable 内置 MCP 客户端 →
    /// 403，且数据库零副作用（enabled 不变、generation 不变）。
    ///
    /// WHY：内置客户端是资源保护，不是绕过权限的后门；权限拒绝必须发生
    /// 在任何持久化之前。
    #[test_context(ClientAppTestContext)]
    #[tokio::test]
    async fn test_disable_builtin_mcp_client_requires_clients_manage_permission(
        ctx: &mut ClientAppTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let mcp_app_id = ensure_mcp_client_seeded(ctx).await;

        // 有用户身份但未授予任何角色（无 clients.manage）。
        let (unprivileged_token, _user_id) =
            create_admin_session_with_user(ctx, "mcp-no-manage-permission@test.com", 1800).await;

        let request = Request::builder()
            .method("PUT")
            .uri(format!("/api/client/{}", mcp_app_id))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", unprivileged_token))
            .body(Body::from(json!({ "enabled": false }).to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            403,
            "a user without clients.manage must not disable the MCP client"
        );

        let row = sqlx::query("SELECT enabled, mcp_token_generation FROM client_app WHERE id = $1")
            .bind(mcp_app_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
        assert!(
            row.get::<bool, _>("enabled"),
            "the rejected disable must leave the client enabled (zero side effects)"
        );
        assert_eq!(
            row.get::<i64, _>("mcp_token_generation"),
            0,
            "the rejected disable must not bump the generation"
        );
    }
}
