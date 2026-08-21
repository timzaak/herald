/// 场景测试：委派管理员（次管理员）提权路径封堵
///
/// 测试目标：持有受限权限（roles.manage / policies.manage）的次管理员
/// 不能通过任何 RBAC 管理端点获得自己未持有的权限：
/// - POST /api/permission/roles/{roleId}/policies（add_policy_to_role）
///   只能附加自己持有的权限
/// - PUT /api/roles/{realmId}/define/{roleId}/permissions
///   （assign_permission_to_role）只能附加自己持有的权限
/// - POST /api/permission/{realmId}/permissions（RoleWrap 角色分配）
///   不能授予自己未持有其全部权限的 builtin 角色
///
/// 正向对照：realm-admin（持有全部权限）的相同操作必须成功。
#[cfg(test)]
mod tests {
    use crate::tests::helpers::*;
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use herald_core::domain::authorization::permission_service::PermissionService;
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;

    use SchemaTestContext as DelegatedAdminTestContext;

    /// 创建一个只持有 `grants` 中列出的权限的次管理员会话。
    ///
    /// 实现：借 realm-admin 之力创建一个自定义角色并按 grants 写入
    /// role_policies，然后撤销 realm-admin、把自定义角色赋予该用户。
    async fn create_sub_admin_session(
        ctx: &mut DelegatedAdminTestContext,
        email: &str,
        grants: &[(&str, &str)],
    ) -> (String, String) {
        let (token, user_id) = create_admin_session_with_user(ctx, email, 1800).await;
        grant_realm_admin_role(ctx, &user_id).await;

        let role_id = create_role(
            ctx,
            &ctx._realm_id,
            &token,
            &format!("sub-admin-{}", email),
            "Delegated sub-admin with limited grants",
        )
        .await;

        for (resource, action) in grants {
            sqlx::query(
                "INSERT INTO role_policies (id, realm_id, role_id, resource, action, created_at)
                 VALUES ($1, $2, $3, $4, $5, NOW())",
            )
            .bind(uuid::Uuid::now_v7())
            .bind(&ctx._realm_id)
            .bind(role_id)
            .bind(resource)
            .bind(action)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to insert sub-admin policy");
        }

        // 撤销 realm-admin，只保留受限自定义角色
        sqlx::query("DELETE FROM user_roles WHERE user_id = $1::uuid AND role_id <> $2")
            .bind(&user_id)
            .bind(role_id)
            .execute(&ctx._app_state.pool)
            .await
            .expect("Failed to revoke realm-admin role");

        assign_role_to_user(
            ctx,
            &ctx._realm_id,
            &token,
            user_id.parse().unwrap(),
            role_id,
        )
        .await;

        let _ = ctx
            ._app_state
            .permission_checker
            .invalidate_user_role_cache(&ctx._realm_id, &user_id)
            .await;

        (token, user_id)
    }

    /// **Given**: 次管理员只持有 policies.manage（不持有 users.manage）
    /// **When**: 向一个自定义角色附加 ("users","manage") 策略
    /// **Then**: HTTP 403 Forbidden，且 role_policies 无新行
    /// **And**: realm-admin 执行相同操作返回 201（守卫不阻断合法操作）
    #[test_context(DelegatedAdminTestContext)]
    #[tokio::test]
    async fn test_scenario_add_policy_to_role_requires_holding_the_permission(
        ctx: &mut DelegatedAdminTestContext,
    ) {
        let (sub_token, _) =
            create_sub_admin_session(ctx, "sub-policy@test.com", &[("policies", "manage")]).await;
        let (admin_token, admin_user_id) =
            create_admin_session_with_user(ctx, "esc-admin-1@test.com", 1800).await;
        grant_realm_admin_role(ctx, &admin_user_id).await;

        let app = ctx.create_unified_test_router();

        // 受害角色：由主管理员创建的普通自定义角色
        let victim_role = create_role(
            ctx,
            &ctx._realm_id,
            &admin_token,
            "escalation-victim-policy",
            "Role the sub-admin tries to widen",
        )
        .await;

        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/permission/roles/{}/policies", victim_role))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", sub_token))
            .body(Body::from(
                json!({"resource": "users", "action": "manage"}).to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "a policies.manage holder must not attach a permission they do not hold"
        );

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM role_policies WHERE role_id = $1 AND resource = 'users' AND action = 'manage'",
        )
        .bind(victim_role)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap();
        assert_eq!(count, 0, "no policy row may be written for a denied grant");

        // 正向对照：realm-admin（持有 users.manage）附加成功
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/permission/roles/{}/policies", victim_role))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({"resource": "users", "action": "manage"}).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "a caller holding the permission must still be able to attach it"
        );
    }

    /// **Given**: 次管理员只持有 roles.manage（不持有 users.manage）
    /// **When**: 通过角色定义端点把 users.manage 权限定义挂到自定义角色
    /// **Then**: HTTP 403 Forbidden
    /// **And**: realm-admin 执行相同操作成功（守卫不阻断合法操作）
    #[test_context(DelegatedAdminTestContext)]
    #[tokio::test]
    async fn test_scenario_assign_permission_to_role_requires_holding_the_permission(
        ctx: &mut DelegatedAdminTestContext,
    ) {
        let (sub_token, _) =
            create_sub_admin_session(ctx, "sub-roles@test.com", &[("roles", "manage")]).await;
        let (admin_token, admin_user_id) =
            create_admin_session_with_user(ctx, "esc-admin-2@test.com", 1800).await;
        grant_realm_admin_role(ctx, &admin_user_id).await;

        let app = ctx.create_unified_test_router();

        let victim_role = create_role(
            ctx,
            &ctx._realm_id,
            &admin_token,
            "escalation-victim-perm",
            "Role the sub-admin tries to widen",
        )
        .await;

        // 本 realm 的 users.manage 权限定义行
        let users_manage_perm: uuid::Uuid = sqlx::query_scalar(
            "SELECT id FROM permissions WHERE realm_id = $1 AND resource = 'users' AND action = 'manage'",
        )
        .bind(&ctx._realm_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("realm should seed a users.manage permission definition");

        let req = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/roles/{}/define/{}/permissions",
                ctx._realm_id, victim_role
            ))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", sub_token))
            .body(Body::from(
                json!({"permissionId": users_manage_perm}).to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "a roles.manage holder must not attach a permission they do not hold"
        );

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM role_permissions WHERE role_id = $1")
                .bind(victim_role)
                .fetch_one(&ctx._app_state.pool)
                .await
                .unwrap();
        assert_eq!(
            count, 0,
            "no role_permissions row may be written for a denied grant"
        );

        // 正向对照：realm-admin 挂载成功
        let req = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/roles/{}/define/{}/permissions",
                ctx._realm_id, victim_role
            ))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
            .body(Body::from(
                json!({"permissionId": users_manage_perm}).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "a caller holding the permission must still be able to attach it"
        );
    }

    /// **Given**: 次管理员只持有 policies.manage（不持有 realm-admin 角色的任何权限）
    /// **When**: 通过 RoleWrap（POST /api/permission/{realmId}/permissions）
    /// 把 builtin realm-admin 角色分配给自己
    /// **Then**: HTTP 403 Forbidden，且 user_roles 无新行
    #[test_context(DelegatedAdminTestContext)]
    #[tokio::test]
    async fn test_scenario_create_permission_role_wrap_blocks_builtin_self_escalation(
        ctx: &mut DelegatedAdminTestContext,
    ) {
        let (sub_token, sub_user_id) =
            create_sub_admin_session(ctx, "sub-wrap@test.com", &[("policies", "manage")]).await;

        let app = ctx.create_unified_test_router();

        let realm_admin_role: uuid::Uuid = sqlx::query_scalar(
            "SELECT id FROM roles WHERE realm_id = $1 AND name = 'realm-admin' AND is_builtin = true",
        )
        .bind(&ctx._realm_id)
        .fetch_optional(&ctx._app_state.pool)
        .await
        .expect("Failed to query realm-admin role")
        .expect("realm should have a builtin realm-admin role");

        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/permission/{}/permissions", ctx._realm_id))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", sub_token))
            .body(Body::from(
                json!({
                    "clientId": ctx._client_id,
                    "permission": {"p_type": "g", "userId": sub_user_id, "role": realm_admin_role}
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "a policies.manage holder must not self-assign the builtin realm-admin role"
        );

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_roles WHERE user_id = $1 AND role_id = $2",
        )
        .bind(sub_user_id.parse::<uuid::Uuid>().unwrap())
        .bind(realm_admin_role)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            count, 0,
            "no user_roles row may be written for a denied grant"
        );
    }
}
