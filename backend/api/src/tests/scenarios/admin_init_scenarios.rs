/// 场景测试：主管理员初始化
///
/// 测试空数据库启动时主管理员的创建流程
#[cfg(test)]
mod tests {
    use crate::tests::schema_test_context::SchemaTestContext;
    use herald_core::admin::user::{BUILTIN_ROLE_REALM_ADMIN, init_admin_user};
    use sqlx::Row;
    use test_context::test_context;
    use uuid::Uuid;

    use SchemaTestContext as AdminInitTestContext;

    /// 测试：空数据库自动创建主管理员
    #[test_context(AdminInitTestContext)]
    #[tokio::test]
    async fn test_empty_database_creates_admin_user(ctx: &mut AdminInitTestContext) {
        // SchemaTestContext 已经调用了 init_admin_user
        // 所以这里验证管理员用户已创建

        // 1. 验证主管理员账户已创建
        let count: i64 = sqlx::query("SELECT count(*) as count FROM account")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("count");
        assert_eq!(count, 1, "Should have exactly 1 admin user");

        // 2. 验证邮箱正确
        let email: String = sqlx::query("SELECT email FROM account LIMIT 1")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("email");
        assert_eq!(email, "admin@cas.com");

        // 3. 验证 profile 已创建
        let nickname: String = sqlx::query("SELECT nickname FROM profile LIMIT 1")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("nickname");
        assert_eq!(nickname, "Admin");

        // 4. 验证 super-admin 角色已创建
        let role_id: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM roles WHERE name = $1 AND realm_id = $2")
                .bind(BUILTIN_ROLE_REALM_ADMIN)
                .bind(&ctx._realm_id)
                .fetch_optional(&ctx.app_state.pool)
                .await
                .unwrap();

        assert!(
            role_id.is_some(),
            "super-admin role should exist after initialization"
        );

        // 5. 验证角色关联到正确的 client_app.client_id
        // 注意：roles.client_id 存储的是 client_app.client_id (字符串标识符)
        let client_app_client_id: String = sqlx::query("SELECT client_id FROM client_app LIMIT 1")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("client_id");

        let role_client_id: String = sqlx::query("SELECT client_id FROM roles WHERE name = $1")
            .bind(BUILTIN_ROLE_REALM_ADMIN)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("client_id");

        assert_eq!(
            role_client_id, client_app_client_id,
            "Role client_id should match client_app.client_id (string identifier)"
        );

        // A system-created account has not personally accepted any agreement.
        // Its first login must pass through the consent gate instead of relying
        // on consent inserted on the user's behalf.
        let consent_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_agreement_consent WHERE user_id = (SELECT id FROM account LIMIT 1)",
        )
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            consent_count, 0,
            "system-created admin must not have consent recorded on its behalf"
        );
    }
    /// 测试：重复运行不重复创建主管理员
    #[test_context(AdminInitTestContext)]
    #[tokio::test]
    async fn test_rerunning_init_does_not_duplicate_admin(ctx: &mut AdminInitTestContext) {
        // SchemaTestContext 已经调用了一次 init_admin_user

        // 获取初始状态
        let initial_count: i64 = sqlx::query("SELECT count(*) as count FROM account")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("count");

        let initial_role_count: i64 = sqlx::query("SELECT count(*) as count FROM roles")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("count");

        let initial_policy_count: i64 = sqlx::query("SELECT count(*) as count FROM role_policies")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("count");
        let initial_grouping_count: i64 = sqlx::query("SELECT count(*) as count FROM user_roles")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("count");

        // 再次运行 init_admin_user
        init_admin_user(&ctx.app_state.pool, "test")
            .await
            .expect("Second init should succeed");

        // 验证没有重复创建
        let final_count: i64 = sqlx::query("SELECT count(*) as count FROM account")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("count");

        assert_eq!(
            initial_count, final_count,
            "Should not create duplicate admin users"
        );

        // 验证角色没有重复
        let final_role_count: i64 = sqlx::query("SELECT count(*) as count FROM roles")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("count");

        assert_eq!(
            initial_role_count, final_role_count,
            "Should not create duplicate roles"
        );

        // 验证策略没有重复
        let final_policy_count: i64 = sqlx::query("SELECT count(*) as count FROM role_policies")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("count");
        let final_grouping_count: i64 = sqlx::query("SELECT count(*) as count FROM user_roles")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("count");

        assert_eq!(
            initial_policy_count, final_policy_count,
            "Should not create duplicate policies"
        );

        assert_eq!(
            initial_grouping_count, final_grouping_count,
            "Should not create duplicate grouping policies"
        );
    }

    /// 测试：init_admin_user 在已有用户时不执行
    #[test_context(AdminInitTestContext)]
    #[tokio::test]
    async fn test_init_with_existing_users_does_nothing(ctx: &mut AdminInitTestContext) {
        // SchemaTestContext 已经创建了管理员用户

        // 创建另一个普通用户
        let realm_id: String = sqlx::query("SELECT id FROM realm LIMIT 1")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("id");

        let hashed_password = bcrypt::hash("testpassword", 10).unwrap();

        sqlx::query(
            "INSERT INTO account (realm_id, email, password, status) VALUES ($1, $2, $3, 1)",
        )
        .bind(&realm_id)
        .bind("test@cas.com")
        .bind(&hashed_password)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();

        // 获取当前状态
        let user_count_before: i64 = sqlx::query("SELECT count(*) as count FROM account")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("count");

        // 再次运行 init_admin_user
        init_admin_user(&ctx.app_state.pool, "test")
            .await
            .expect("Init should succeed when users already exist");

        // 验证用户数量没有变化
        let user_count_after: i64 = sqlx::query("SELECT count(*) as count FROM account")
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap()
            .get("count");

        assert_eq!(
            user_count_before, user_count_after,
            "Should not create new admin users when users already exist"
        );
    }
}
