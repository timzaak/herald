//! 场景测试：内置保护功能验证
//!
//! 测试目标：验证内置角色和权限的保护机制
//! - 不能修改内置角色的名称
//! - 可以修改内置角色的描述
//! - 不能从内置角色移除内置权限
//! - 可以删除自定义角色
//! - API 返回 is_builtin 字段
//!
//! 来源：`.ai/design/fix-permission.md` Section 5.5.2
//! 用户故事：US-RA-010 (`docs/user-stories/builtin_protection.md`)

#[cfg(test)]
mod tests {
    use crate::application::http::role_definitions::types::RoleUpdateRequest;
    use crate::tests::helpers::*;

    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;

    use SchemaTestContext as BuiltinProtectionTestContext;

    /// 场景测试：不能修改内置角色的名称
    ///
    /// **Given**: 管理员用户拥有所有权限
    /// **And**: 存在内置角色 `realm-admin` (is_builtin = true)
    /// **When**: 用户尝试修改该角色的名称
    /// **Then**: API 返回 403 Forbidden
    /// **And**: 错误消息包含 "Cannot change built-in role name"
    #[test_context(BuiltinProtectionTestContext)]
    #[tokio::test]
    async fn test_scenario_cannot_modify_builtin_role_name(ctx: &mut BuiltinProtectionTestContext) {
        // ========================================================================
        // Given: 创建管理员用户并获取内置角色
        // ========================================================================
        let (admin_token, _user_id) =
            create_admin_session_with_user(ctx, "test-admin-protect@test.com", 1800).await;
        grant_realm_admin_role(ctx, &_user_id).await;

        // 查询内置角色 realm-admin
        let realm_admin_role: Option<(String, String)> = sqlx::query_as(
            "SELECT id::text, name FROM roles WHERE realm_id = $1 AND name = 'realm-admin' AND is_builtin = true",
        )
        .bind(&ctx._realm_id)
        .fetch_optional(&ctx._app_state.pool)
        .await
        .expect("Failed to query realm-admin role");

        let (role_id, _role_name) = realm_admin_role.expect("realm-admin role should exist");

        // ========================================================================
        // When: 尝试修改内置角色的名称
        // ========================================================================
        let app = ctx.create_unified_test_router();
        let update_req = Request::builder()
            .method("PUT")
            .uri(format!("/api/roles/define/{}", role_id))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::from(
                json!(RoleUpdateRequest {
                    name: "hacked-admin".to_string(),
                    description: Some("Hacked admin role".to_string()),
                })
                .to_string(),
            ))
            .unwrap();

        let update_response = app.clone().oneshot(update_req).await.unwrap();

        // ========================================================================
        // Then: 验证返回 403 Forbidden
        // ========================================================================
        assert_eq!(
            update_response.status(),
            StatusCode::FORBIDDEN,
            "Update should be forbidden for builtin role name change"
        );

        let error_body: serde_json::Value = crate::tests::response_json(update_response).await;
        let error_message = error_body
            .get("message")
            .and_then(|v| v.as_str())
            .expect("Error response should contain 'message' field");

        assert!(
            error_message.to_lowercase().contains("built-in")
                && error_message.to_lowercase().contains("name"),
            "Error message should mention built-in role name protection, got: {}",
            error_message
        );

        tracing::info!("✓ Builtin role name correctly protected");
    }

    /// 场景测试：可以修改内置角色的描述
    ///
    /// **Given**: 管理员用户拥有所有权限
    /// **And**: 存在内置角色 `realm-admin` (is_builtin = true)
    /// **When**: 用户只修改该角色的描述
    /// **Then**: API 返回 200 OK
    /// **And**: 描述成功更新
    #[test_context(BuiltinProtectionTestContext)]
    #[tokio::test]
    async fn test_scenario_can_modify_builtin_role_description(
        ctx: &mut BuiltinProtectionTestContext,
    ) {
        // ========================================================================
        // Given: 创建管理员用户并获取内置角色
        // ========================================================================
        let (admin_token, _user_id) =
            create_admin_session_with_user(ctx, "test-admin-desc@test.com", 1800).await;
        grant_realm_admin_role(ctx, &_user_id).await;

        // 查询内置角色 realm-admin
        let realm_admin_role: Option<(String, String)> = sqlx::query_as(
            "SELECT id::text, description FROM roles WHERE realm_id = $1 AND name = 'realm-admin' AND is_builtin = true",
        )
        .bind(&ctx._realm_id)
        .fetch_optional(&ctx._app_state.pool)
        .await
        .expect("Failed to query realm-admin role");

        let (role_id, original_description) =
            realm_admin_role.expect("realm-admin role should exist");

        // ========================================================================
        // When: 只修改描述（不修改名称）
        // ========================================================================
        let app = ctx.create_unified_test_router();
        let new_description = "Updated description for realm-admin role";
        // 查询当前角色信息
        let current_role: Option<(String, String)> =
            sqlx::query_as("SELECT name, description FROM roles WHERE id::text = $1")
                .bind(&role_id)
                .fetch_optional(&ctx._app_state.pool)
                .await
                .expect("Failed to query role");

        let (current_name, _) = current_role.expect("Role should exist");

        let update_req = Request::builder()
            .method("PUT")
            .uri(format!("/api/roles/define/{}", role_id))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::from(
                json!(RoleUpdateRequest {
                    name: current_name, // 保持原有名称
                    description: Some(new_description.to_string()),
                })
                .to_string(),
            ))
            .unwrap();

        let update_response = app.clone().oneshot(update_req).await.unwrap();

        // ========================================================================
        // Then: 验证返回 200 OK
        // ========================================================================
        assert_eq!(
            update_response.status(),
            StatusCode::OK,
            "Description update should succeed for builtin role"
        );

        // 验证描述已更新
        let updated_role: Option<(String,)> =
            sqlx::query_as("SELECT description FROM roles WHERE id::text = $1")
                .bind(&role_id)
                .fetch_optional(&ctx._app_state.pool)
                .await
                .expect("Failed to query updated role");

        let (updated_description,) = updated_role.expect("Role should still exist");
        assert_eq!(updated_description, new_description);

        // 恢复原始描述
        sqlx::query("UPDATE roles SET description = $1 WHERE id::text = $2")
            .bind(&original_description)
            .bind(&role_id)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to restore description");

        tracing::info!("✓ Builtin role description can be modified");
    }

    /// 场景测试：不能从内置角色移除内置权限
    ///
    /// **Given**: 管理员用户拥有所有权限
    /// **And**: 存在内置角色 `realm-admin`，包含内置权限 `users.manage`
    /// **When**: 用户尝试从该角色移除 `users.manage` 权限
    /// **Then**: API 返回 403 Forbidden
    /// **And**: 错误消息包含 "Cannot remove built-in permission from built-in role"
    #[test_context(BuiltinProtectionTestContext)]
    #[tokio::test]
    async fn test_scenario_cannot_remove_builtin_permission_from_builtin_role(
        ctx: &mut BuiltinProtectionTestContext,
    ) {
        // ========================================================================
        // Given: 创建管理员用户
        // ========================================================================
        let (admin_token, _user_id) =
            create_admin_session_with_user(ctx, "test-admin-remove@test.com", 1800).await;
        grant_realm_admin_role(ctx, &_user_id).await;

        // 查询内置角色 realm-admin 和内置权限 users.manage
        let realm_admin_role: Option<String> = sqlx::query_scalar(
            "SELECT id::text FROM roles WHERE realm_id = $1 AND name = 'realm-admin' AND is_builtin = true",
        )
        .bind(&ctx._realm_id)
        .fetch_optional(&ctx._app_state.pool)
        .await
        .expect("Failed to query realm-admin role");

        let role_id = realm_admin_role.expect("realm-admin role should exist");
        let permission_id: String = sqlx::query_scalar(
            "SELECT id::text FROM permissions WHERE realm_id = $1 AND name = 'users.manage' AND is_builtin = true",
        )
        .bind(&ctx._realm_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("users.manage permission should exist");

        // ========================================================================
        // When: 尝试移除内置权限
        // ========================================================================
        let app = ctx.create_unified_test_router();
        let remove_req = Request::builder()
            .method("DELETE")
            .uri(format!(
                "/api/roles/define/{}/permissions/{}",
                role_id, permission_id
            ))
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();

        let remove_response = app.clone().oneshot(remove_req).await.unwrap();

        // ========================================================================
        // Then: 验证返回 403 Forbidden
        // ========================================================================
        assert_eq!(
            remove_response.status(),
            StatusCode::FORBIDDEN,
            "Remove should be forbidden for builtin permission from builtin role"
        );

        let error_body: serde_json::Value = crate::tests::response_json(remove_response).await;
        let error_message = error_body
            .get("message")
            .and_then(|v| v.as_str())
            .expect("Error response should contain 'message' field");

        assert!(
            error_message.to_lowercase().contains("built-in"),
            "Error message should mention built-in protection, got: {}",
            error_message
        );

        tracing::info!("✓ Builtin permission correctly protected in builtin role");
    }

    /// 场景测试：可以删除自定义角色
    ///
    /// **Given**: 管理员用户拥有所有权限
    /// **And**: 存在自定义角色 `content-admin` (is_builtin = false)
    /// **When**: 用户尝试删除该自定义角色
    /// **Then**: API 返回 204 No Content
    /// **And**: 角色成功删除
    #[test_context(BuiltinProtectionTestContext)]
    #[tokio::test]
    async fn test_scenario_can_delete_custom_role(ctx: &mut BuiltinProtectionTestContext) {
        // ========================================================================
        // Given: 创建管理员用户和自定义角色
        // ========================================================================
        let (admin_token, _user_id) =
            create_admin_session_with_user(ctx, "test-admin-custom@test.com", 1800).await;
        grant_realm_admin_role(ctx, &_user_id).await;

        // 创建自定义角色
        let custom_role_id = create_role(
            ctx,
            &ctx._realm_id,
            &admin_token,
            "content-admin",
            "Custom content admin role",
        )
        .await;

        // 验证角色已创建
        let role_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM roles WHERE id = $1 AND is_builtin = false)",
        )
        .bind(custom_role_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("Failed to check role existence");

        assert!(role_exists, "Custom role should exist");

        // ========================================================================
        // When: 删除自定义角色
        // ========================================================================
        let app = ctx.create_unified_test_router();
        let delete_response = Request::builder()
            .method("DELETE")
            .uri(format!("/api/roles/define/{}", custom_role_id))
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();

        let delete_response = app.oneshot(delete_response).await.unwrap();

        // ========================================================================
        // Then: 验证返回 204 No Content
        // ========================================================================
        assert_eq!(
            delete_response.status(),
            StatusCode::NO_CONTENT,
            "Delete should succeed for custom role"
        );

        // 验证角色已删除
        let role_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM roles WHERE id = $1)")
                .bind(custom_role_id)
                .fetch_one(&ctx._app_state.pool)
                .await
                .expect("Failed to check role existence");

        assert!(!role_exists, "Custom role should be deleted");

        tracing::info!("✓ Custom role can be deleted");
    }

    /// 场景测试：API 返回 is_builtin 字段
    ///
    /// **Given**: 管理员用户拥有所有权限
    /// **When**: 用户查询角色列表或权限列表
    /// **Then**: API 响应包含 `is_builtin` 字段
    /// **And**: 内置项目的 `is_builtin = true`
    /// **And**: 自定义项目的 `is_builtin = false`
    #[test_context(BuiltinProtectionTestContext)]
    #[tokio::test]
    async fn test_scenario_builtin_field_returned_in_api(ctx: &mut BuiltinProtectionTestContext) {
        // ========================================================================
        // Given: 创建管理员用户
        // ========================================================================
        let (admin_token, _user_id) =
            create_admin_session_with_user(ctx, "test-admin-flag@test.com", 1800).await;
        grant_realm_admin_role(ctx, &_user_id).await;

        // 创建自定义角色
        let custom_role_id = create_role(
            ctx,
            &ctx._realm_id,
            &admin_token,
            "custom-test-role",
            "Custom test role",
        )
        .await;

        // ========================================================================
        // When: 查询角色列表
        // ========================================================================
        let app = ctx.create_unified_test_router();
        let list_req = Request::builder()
            .method("GET")
            .uri("/api/roles/define".to_string())
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();

        let list_response = app.clone().oneshot(list_req).await.unwrap();

        // ========================================================================
        // Then: 验证 API 返回 is_builtin 字段
        // ========================================================================
        assert_eq!(
            list_response.status(),
            StatusCode::OK,
            "List roles should succeed"
        );

        let roles: serde_json::Value = crate::tests::response_json(list_response).await;
        let roles_array = roles.as_array().expect("Response should be an array");

        // 查找内置角色 realm-admin 和自定义角色
        let mut found_builtin = false;
        let mut found_custom = false;

        for role in roles_array {
            if let Some(name) = role["name"].as_str() {
                if name == "realm-admin" {
                    found_builtin = true;
                    assert_eq!(
                        role["isBuiltin"], true,
                        "realm-admin should have is_builtin = true"
                    );
                } else if name == "custom-test-role" {
                    found_custom = true;
                    assert_eq!(
                        role["isBuiltin"], false,
                        "custom-test-role should have is_builtin = false"
                    );
                }
            }
        }

        assert!(found_builtin, "Should find builtin role realm-admin");
        assert!(found_custom, "Should find custom role custom-test-role");

        // 清理测试数据
        sqlx::query("DELETE FROM roles WHERE id = $1")
            .bind(custom_role_id)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to cleanup");

        tracing::info!("✓ is_builtin field correctly returned in API");
    }
}
