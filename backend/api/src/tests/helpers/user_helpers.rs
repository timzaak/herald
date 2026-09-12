// =============================================================================
// 通用用户管理辅助函数
// =============================================================================

#![allow(dead_code)]

use crate::tests::schema_test_context::SchemaTestContext as TestContext;

/// ============================================================================
/// 用户创建和管理
/// ============================================================================
///
/// 创建测试用户（直接在数据库中创建，使用假密码哈希）
///
/// **返回**: user_id (Uuid)
///
pub async fn create_simple_test_user(ctx: &TestContext, email: &str) -> uuid::Uuid {
    let user_uuid = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status) VALUES ($1, $2, $3, $4, 1)",
    )
    .bind(user_uuid)
    .bind(&ctx._realm_id)
    .bind(email)
    .bind("$2a$12$dummy_password_hash") // 假密码哈希
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create test user");
    user_uuid
}

/// 统计 realm 内某邮箱的 account 行数（JIT/matching 场景断言账号是否
/// 被创建或复用时使用）。
pub async fn count_accounts_by_email(ctx: &TestContext, email: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM account WHERE realm_id = $1 AND email = $2")
        .bind(&ctx._realm_id)
        .bind(email)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap()
}
