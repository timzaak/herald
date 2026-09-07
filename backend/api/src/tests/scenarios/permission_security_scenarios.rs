/// 场景测试：权限安全性验证
///
/// 测试目标：验证权限系统的安全功能
/// - 删除权限定义需要 `permissions.manage` 权限
/// - 删除角色定义需要 `roles.manage` 权限
/// - 内置权限不能被删除
/// - 内置角色不能被删除
///
/// 用户故事：US-RA-002, US-RA-010 (docs/user-stories/core/realm-admin.md)
#[cfg(test)]
mod tests {
    use crate::application::http::admin::permission_definitions::types::PermissionCreateRequest;
    use crate::tests::helpers::*;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use herald_core::domain::authorization::PermissionService;
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;

    use SchemaTestContext as PermissionSecurityTestContext;

    /// 场景测试：删除权限定义需要 manage 权限
    ///
    /// **Given**: 用户拥有 `permissions:view` 权限但没有 `permissions.manage` 权限
    /// **When**: 用户尝试删除权限定义
    /// **Then**: API 返回 403 Forbidden
    /// **And**: 错误消息包含 "Missing permissions.manage permission"
    #[test_context(PermissionSecurityTestContext)]
    #[tokio::test]
    async fn test_scenario_delete_permission_requires_manage_permission(
        ctx: &mut PermissionSecurityTestContext,
    ) {
        // ========================================================================
        // Given: 创建测试用户并授予 permissions:view 权限
        // ========================================================================
        let (admin_token, user_id_str) =
            create_admin_session_with_user(ctx, "test-view-perm@test.com", 1800).await;
        let _user_id = uuid::Uuid::parse_str(&user_id_str).expect("Invalid user_id UUID");

        // 临时授予 realm-admin 角色来创建测试角色
        grant_realm_admin_role(ctx, &user_id_str).await;

        // 创建自定义角色
        let role_name = "test-perm-view-role";
        let role_id = create_role(
            ctx,
            &ctx._realm_id,
            &admin_token,
            role_name,
            "Role with permissions:view only",
        )
        .await;

        // 撤销 realm-admin 角色
        sqlx::query("DELETE FROM user_roles WHERE user_id = $1::uuid")
            .bind(&user_id_str)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to revoke realm-admin role");

        // 清除缓存
        let _ = ctx
            ._app_state
            .permission_checker
            .invalidate_user_role_cache(&ctx._realm_id, &user_id_str)
            .await;

        // 为角色分配 permissions:view 权限（没有 permissions.manage）
        sqlx::query(
            "INSERT INTO role_policies (id, realm_id, role_id, resource, action, created_at)
             VALUES ($1, $2, $3, $4, $5, NOW())",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(&ctx._realm_id)
        .bind(role_id)
        .bind("permissions")
        .bind("view")
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to assign permission to role");

        // 为用户分配角色
        assign_role_to_user(
            ctx,
            &ctx._realm_id,
            &admin_token,
            uuid::Uuid::parse_str(&user_id_str).unwrap(),
            role_id,
        )
        .await;

        // 清除权限缓存
        let _ = ctx
            ._app_state
            .permission_checker
            .invalidate_user_role_cache(&ctx._realm_id, &user_id_str)
            .await;

        // 创建测试权限定义
        let app = ctx.create_unified_test_router();
        let create_req = Request::builder()
            .method("POST")
            .uri(format!("/api/permission/{}/define", ctx._realm_id))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::from(
                json!(PermissionCreateRequest {
                    name: "test.delete".to_string(),
                    description: Some("Test permission to delete".to_string()),
                })
                .to_string(),
            ))
            .unwrap();

        // 注意：这里需要临时授予 permissions.manage 权限来创建测试数据
        // 然后撤销该权限再测试删除
        let _ = sqlx::query(
            "INSERT INTO role_policies (id, realm_id, role_id, resource, action, created_at)
             VALUES ($1, $2, $3, $4, $5, NOW())",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(&ctx._realm_id)
        .bind(role_id)
        .bind("permissions")
        .bind("manage")
        .execute(&ctx._app_state.pool)
        .await;

        let create_response = app.clone().oneshot(create_req).await.unwrap();
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let created_permission: serde_json::Value =
            crate::tests::response_json(create_response).await;
        let permission_id = created_permission["id"].as_str().unwrap();

        // 撤销 permissions.manage 权限
        sqlx::query("DELETE FROM role_policies WHERE role_id = $1 AND resource = 'permissions' AND action = 'manage'")
            .bind(role_id)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to revoke permission");

        // 清除角色策略缓存
        let _ = ctx
            ._app_state
            .permission_checker
            .invalidate_role_policy_cache(&ctx._realm_id, &role_id.to_string())
            .await;

        // 清除用户权限缓存
        let _ = ctx
            ._app_state
            .permission_checker
            .invalidate_user_role_cache(&ctx._realm_id, &user_id_str)
            .await;

        // ========================================================================
        // When: 尝试删除权限定义
        // ========================================================================
        let app = ctx.create_unified_test_router();
        let delete_response = Request::builder()
            .method("DELETE")
            .uri(format!(
                "/api/permission/{}/define/{}",
                ctx._realm_id, permission_id
            ))
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();

        let delete_response = app.oneshot(delete_response).await.unwrap();

        // ========================================================================
        // Then: 验证返回 403 Forbidden
        // ========================================================================
        assert_eq!(
            delete_response.status(),
            StatusCode::FORBIDDEN,
            "Delete should be forbidden without permissions.manage"
        );

        let error_body: serde_json::Value = crate::tests::response_json(delete_response).await;
        let error_message = error_body["message"].as_str().unwrap();

        assert!(
            error_message.contains("permissions") && error_message.contains("manage"),
            "Error message should mention missing permissions.manage permission, got: {}",
            error_message
        );

        tracing::info!("✓ Delete permission correctly requires permissions.manage");
    }

    /// 场景测试：删除角色定义需要 manage 权限
    ///
    /// **Given**: 用户拥有 `roles:view` 权限但没有 `roles.manage` 权限
    /// **When**: 用户尝试删除角色定义
    /// **Then**: API 返回 403 Forbidden
    /// **And**: 错误消息包含 "Missing roles.manage permission"
    #[test_context(PermissionSecurityTestContext)]
    #[tokio::test]
    async fn test_scenario_delete_role_requires_manage_permission(
        ctx: &mut PermissionSecurityTestContext,
    ) {
        // ========================================================================
        // Given: 创建测试用户并授予 roles:view 权限
        // ========================================================================
        let (admin_token, user_id_str) =
            create_admin_session_with_user(ctx, "test-view-role@test.com", 1800).await;
        let _user_id = uuid::Uuid::parse_str(&user_id_str).expect("Invalid user_id UUID");

        // 临时授予 realm-admin 角色来创建测试角色
        grant_realm_admin_role(ctx, &user_id_str).await;

        // 创建自定义角色
        let role_name = "test-role-view-role";
        let role_id = create_role(
            ctx,
            &ctx._realm_id,
            &admin_token,
            role_name,
            "Role with roles:view only",
        )
        .await;

        // 撤销 realm-admin 角色
        sqlx::query("DELETE FROM user_roles WHERE user_id = $1::uuid")
            .bind(&user_id_str)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to revoke realm-admin role");

        // 清除缓存
        let _ = ctx
            ._app_state
            .permission_checker
            .invalidate_user_role_cache(&ctx._realm_id, &user_id_str)
            .await;

        // 为角色分配 roles:view 权限（没有 roles.manage）
        sqlx::query(
            "INSERT INTO role_policies (id, realm_id, role_id, resource, action, created_at)
             VALUES ($1, $2, $3, $4, $5, NOW())",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(&ctx._realm_id)
        .bind(role_id)
        .bind("roles")
        .bind("view")
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to assign permission to role");

        // 为用户分配角色
        assign_role_to_user(
            ctx,
            &ctx._realm_id,
            &admin_token,
            uuid::Uuid::parse_str(&user_id_str).unwrap(),
            role_id,
        )
        .await;

        // 清除权限缓存
        let _ = ctx
            ._app_state
            .permission_checker
            .invalidate_user_role_cache(&ctx._realm_id, &user_id_str)
            .await;

        // 创建测试角色定义
        let _app = ctx.create_unified_test_router();

        // 临时授予 roles.manage 权限来创建测试数据
        let _ = sqlx::query(
            "INSERT INTO role_policies (id, realm_id, role_id, resource, action, created_at)
             VALUES ($1, $2, $3, $4, $5, NOW())",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(&ctx._realm_id)
        .bind(role_id)
        .bind("roles")
        .bind("manage")
        .execute(&ctx._app_state.pool)
        .await;

        let test_role_id = create_role(
            ctx,
            &ctx._realm_id,
            &admin_token,
            "test-role-to-delete",
            "Test role to delete",
        )
        .await;

        // 撤销 roles.manage 权限
        sqlx::query("DELETE FROM role_policies WHERE role_id = $1 AND resource = 'roles' AND action = 'manage'")
            .bind(role_id)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to revoke permission");

        // 清除角色策略缓存
        let _ = ctx
            ._app_state
            .permission_checker
            .invalidate_role_policy_cache(&ctx._realm_id, &role_id.to_string())
            .await;

        // 清除用户权限缓存
        let _ = ctx
            ._app_state
            .permission_checker
            .invalidate_user_role_cache(&ctx._realm_id, &user_id_str)
            .await;

        // ========================================================================
        // When: 尝试删除角色定义
        // ========================================================================
        let app = ctx.create_unified_test_router();
        let delete_response = Request::builder()
            .method("DELETE")
            .uri(format!(
                "/api/roles/{}/define/{}",
                ctx._realm_id, test_role_id
            ))
            .header("authorization", format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();

        let delete_response = app.oneshot(delete_response).await.unwrap();

        // ========================================================================
        // Then: 验证返回 403 Forbidden
        // ========================================================================
        assert_eq!(
            delete_response.status(),
            StatusCode::FORBIDDEN,
            "Delete should be forbidden without roles.manage"
        );

        let error_body: serde_json::Value = crate::tests::response_json(delete_response).await;
        let error_message = error_body["message"].as_str().unwrap();

        assert!(
            error_message.contains("roles") && error_message.contains("manage"),
            "Error message should mention missing roles.manage permission, got: {}",
            error_message
        );

        tracing::info!("✓ Delete role correctly requires roles.manage");
    }

    /// 构造更新权限定义的 PUT 请求（供下方 in-use rename 场景复用）
    fn put_permission_request(
        token: &str,
        realm_id: &str,
        permission_id: &str,
        name: &str,
        description: &str,
    ) -> Request<Body> {
        Request::builder()
            .method("PUT")
            .uri(format!("/api/permission/{realm_id}/define/{permission_id}"))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(
                json!({ "name": name, "description": description }).to_string(),
            ))
            .unwrap()
    }

    /// 场景测试：使用中的权限定义不可修改 resource/action
    ///
    /// **Given**: 一个已分配给角色的自定义权限定义
    /// **When**: 管理员尝试修改其 resource/action（变更授权语义）
    /// **Then**: API 返回 409 Conflict（授权运行时读 role_policies 的
    /// resource/action 快照，放行会导致展示与运行时授权漂移）；
    /// 仅修改 description 不受影响，解除引用后 rename 恢复允许
    #[test_context(PermissionSecurityTestContext)]
    #[tokio::test]
    async fn test_scenario_update_in_use_permission_resource_action_conflict(
        ctx: &mut PermissionSecurityTestContext,
    ) {
        // Given: 管理员 + 自定义角色 + 自定义权限定义并分配给该角色
        let (admin_token, user_id_str) =
            create_admin_session_with_user(ctx, "test-update-perm@test.com", 1800).await;
        grant_realm_admin_role(ctx, &user_id_str).await;

        let role_id = create_role(
            ctx,
            &ctx._realm_id,
            &admin_token,
            "test-perm-update-role",
            "Role referencing an updatable permission",
        )
        .await;

        let app = ctx.create_unified_test_router();
        let create_req = Request::builder()
            .method("POST")
            .uri(format!("/api/permission/{}/define", ctx._realm_id))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::from(
                json!(PermissionCreateRequest {
                    name: "test.update".to_string(),
                    description: Some("Test permission to update".to_string()),
                })
                .to_string(),
            ))
            .unwrap();
        let create_response = app.clone().oneshot(create_req).await.unwrap();
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let created: serde_json::Value = crate::tests::response_json(create_response).await;
        let permission_id = created["id"].as_str().unwrap();

        sqlx::query(
            "INSERT INTO role_permissions (role_id, permission_id) VALUES ($1::uuid, $2::uuid)",
        )
        .bind(role_id)
        .bind(uuid::Uuid::parse_str(permission_id).unwrap())
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to link permission to role");

        // When: 尝试 rename resource/action
        let rename_response = app
            .clone()
            .oneshot(put_permission_request(
                &admin_token,
                &ctx._realm_id,
                permission_id,
                "test.renamed",
                "Renamed while in use",
            ))
            .await
            .unwrap();

        // Then: 409，防止授权语义漂移
        assert_eq!(
            rename_response.status(),
            StatusCode::CONFLICT,
            "Renaming an in-use permission's resource/action must conflict"
        );

        // And: 仅改 description 仍允许
        let desc_response = app
            .clone()
            .oneshot(put_permission_request(
                &admin_token,
                &ctx._realm_id,
                permission_id,
                "test.update",
                "Description-only change while in use",
            ))
            .await
            .unwrap();
        assert_eq!(desc_response.status(), StatusCode::OK);

        // And: 解除角色引用后 rename 恢复允许
        sqlx::query("DELETE FROM role_permissions WHERE role_id = $1 AND permission_id = $2")
            .bind(role_id)
            .bind(uuid::Uuid::parse_str(permission_id).unwrap())
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to unlink permission");

        let rename_after_response = app
            .clone()
            .oneshot(put_permission_request(
                &admin_token,
                &ctx._realm_id,
                permission_id,
                "test.renamed",
                "Renamed after unlink",
            ))
            .await
            .unwrap();
        assert_eq!(rename_after_response.status(), StatusCode::OK);

        tracing::info!("✓ In-use permission resource/action updates conflict until unassigned");
    }

    /// 场景测试：使用中的权限定义不可删除
    ///
    /// **Given**: 一个自定义权限定义，其 resource/action 仍被角色引用
    /// **When**: 管理员尝试删除该定义
    /// **Then**: API 返回 409 Conflict——role_policies 是运行时授权的
    /// resource/action 快照，删除定义会留下无主的运行时授权（展示与
    /// 运行时漂移）；直接策略镜像（无 role_permissions 关联）同样阻止
    /// 删除，解除引用后删除恢复允许
    #[test_context(PermissionSecurityTestContext)]
    #[tokio::test]
    async fn test_scenario_delete_in_use_permission_conflicts(
        ctx: &mut PermissionSecurityTestContext,
    ) {
        // Given: 管理员 + 自定义角色 + 自定义权限定义
        let (admin_token, user_id_str) =
            create_admin_session_with_user(ctx, "test-delete-perm@test.com", 1800).await;
        grant_realm_admin_role(ctx, &user_id_str).await;

        let role_id = create_role(
            ctx,
            &ctx._realm_id,
            &admin_token,
            "test-perm-delete-role",
            "Role referencing a deletable permission",
        )
        .await;

        let app = ctx.create_unified_test_router();
        let create_req = Request::builder()
            .method("POST")
            .uri(format!("/api/permission/{}/define", ctx._realm_id))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::from(
                json!(PermissionCreateRequest {
                    name: "test.delete-me".to_string(),
                    description: Some("Test permission to delete".to_string()),
                })
                .to_string(),
            ))
            .unwrap();
        let create_response = app.clone().oneshot(create_req).await.unwrap();
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let created: serde_json::Value = crate::tests::response_json(create_response).await;
        let permission_id = uuid::Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();

        // When: 分配给角色（写入 role_permissions + role_policies 镜像）后删除
        sqlx::query(
            "INSERT INTO role_permissions (role_id, permission_id) VALUES ($1::uuid, $2::uuid)",
        )
        .bind(role_id)
        .bind(permission_id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to link permission to role");

        let delete_uri = format!("/api/permission/{}/define/{}", ctx._realm_id, permission_id);
        let delete_req = Request::builder()
            .method("DELETE")
            .uri(&delete_uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();
        let delete_response = app.clone().oneshot(delete_req).await.unwrap();
        assert_eq!(
            delete_response.status(),
            StatusCode::CONFLICT,
            "Deleting an assigned permission must conflict"
        );

        // And: 仅剩直接策略镜像（无 role_permissions 关联）时同样 409
        sqlx::query("DELETE FROM role_permissions WHERE role_id = $1 AND permission_id = $2")
            .bind(role_id)
            .bind(permission_id)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to unlink permission");

        sqlx::query(
            "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
             VALUES ($1, $2, $3, 'test', 'delete-me')",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(role_id)
        .bind(&ctx._realm_id)
        .execute(&ctx._app_state.pool)
        .await
        .expect("Failed to mirror permission into role_policies");

        let mirror_only_req = Request::builder()
            .method("DELETE")
            .uri(&delete_uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();
        let mirror_only_response = app.clone().oneshot(mirror_only_req).await.unwrap();
        assert_eq!(
            mirror_only_response.status(),
            StatusCode::CONFLICT,
            "A role_policies runtime mirror must block deletion even without a role_permissions link"
        );

        // And: 解除镜像后删除恢复允许
        sqlx::query("DELETE FROM role_policies WHERE role_id = $1 AND resource = 'test' AND action = 'delete-me'")
            .bind(role_id)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to remove policy mirror");

        let unlink_req = Request::builder()
            .method("DELETE")
            .uri(&delete_uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::empty())
            .unwrap();
        let unlink_response = app.clone().oneshot(unlink_req).await.unwrap();
        assert_eq!(unlink_response.status(), StatusCode::NO_CONTENT);

        tracing::info!("✓ In-use permission deletion conflicts until fully unreferenced");
    }
}
