use super::*;
use futures::FutureExt;
use herald_domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventFilters, AuditEventRepository, AuditResult,
    AuditTargetType, NewAuditEvent,
};
use herald_test_db::{SharedTestDatabaseHandle, create_isolated_schema_database};
use std::future::Future;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

struct AuditTestDb {
    db: sea_orm::DatabaseConnection,
    pool: sqlx::PgPool,
    schema: SharedTestDatabaseHandle,
}

impl AuditTestDb {
    async fn teardown(self) {
        let AuditTestDb { db, pool, schema } = self;
        drop(db);
        drop(pool);
        schema.teardown().await;
    }
}

async fn setup_test_db() -> AuditTestDb {
    let (schema, pool, db) = create_isolated_schema_database(3).await;
    AuditTestDb { db, pool, schema }
}

async fn run_with_repo<F, Fut>(test_fn: F)
where
    F: FnOnce(PostgresAuditEventRepository) -> Fut,
    Fut: Future<Output = ()>,
{
    let test_db = setup_test_db().await;
    let repo = PostgresAuditEventRepository::new(test_db.db.clone());
    let result = AssertUnwindSafe(test_fn(repo)).catch_unwind().await;
    test_db.teardown().await;

    if let Err(panic_payload) = result {
        resume_unwind(panic_payload);
    }
}

fn make_event_with_overrides(
    realm_id: &str,
    category: AuditCategory,
    action: AuditAction,
    actor_id: &str,
    result: AuditResult,
) -> NewAuditEvent {
    NewAuditEvent {
        realm_id: realm_id.to_string(),
        category,
        action,
        actor_id: actor_id.to_string(),
        actor_type: Some(ActorType::Admin),
        actor_name: Some(format!("actor_{}", actor_id)),
        target_type: AuditTargetType::User,
        target_id: format!("target_{}", Uuid::now_v7().to_string().get(..8).unwrap()),
        target_name: Some("test_target".to_string()),
        result,
        details: None,
        ip_address: None,
        user_agent: None,
        trace_id: Some(format!(
            "trace_{}",
            Uuid::now_v7().to_string().get(..8).unwrap()
        )),
    }
}

macro_rules! audit_repo_test {
    ($name:ident, |$repo:ident| $body:block) => {
        #[tokio::test]
        async fn $name() {
            run_with_repo(|$repo| async move $body).await;
        }
    };
}

audit_repo_test!(test_list_paginated_action_filter, |repo| {
    let realm_id = "audit_action_filter_realm";

    repo.create(make_event_with_overrides(
        realm_id,
        AuditCategory::Rbac,
        AuditAction::RoleCreate,
        "actor_1",
        AuditResult::Success,
    ))
    .await
    .unwrap();

    repo.create(make_event_with_overrides(
        realm_id,
        AuditCategory::Rbac,
        AuditAction::RoleDelete,
        "actor_2",
        AuditResult::Success,
    ))
    .await
    .unwrap();

    repo.create(make_event_with_overrides(
        realm_id,
        AuditCategory::Rbac,
        AuditAction::PermissionCreate,
        "actor_3",
        AuditResult::Success,
    ))
    .await
    .unwrap();

    let filters = AuditEventFilters {
        action: Some(AuditAction::RoleCreate),
        ..Default::default()
    };

    let result = repo
        .list_paginated(realm_id, filters)
        .await
        .expect("list_paginated should succeed");

    assert_eq!(result.total, 1, "should find exactly 1 RoleCreate event");
    assert_eq!(result.items[0].action, AuditAction::RoleCreate);
});

audit_repo_test!(test_list_paginated_combined_filters, |repo| {
    let realm_id = "audit_combined_filter_realm";

    // Insert events with varying properties
    repo.create(make_event_with_overrides(
        realm_id,
        AuditCategory::Auth,
        AuditAction::AuthLogin,
        "actor_combined_1",
        AuditResult::Success,
    ))
    .await
    .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    repo.create(make_event_with_overrides(
        realm_id,
        AuditCategory::Auth,
        AuditAction::AuthLoginFailed,
        "actor_combined_1",
        AuditResult::Failure,
    ))
    .await
    .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    repo.create(make_event_with_overrides(
        realm_id,
        AuditCategory::UserManagement,
        AuditAction::UserCreate,
        "actor_combined_2",
        AuditResult::Success,
    ))
    .await
    .unwrap();

    // Filter: Auth category + actor_combined_1
    let filters = AuditEventFilters {
        category: Some(AuditCategory::Auth),
        actor_id: Some("actor_combined_1".to_string()),
        ..Default::default()
    };

    let result = repo
        .list_paginated(realm_id, filters)
        .await
        .expect("list_paginated should succeed");

    assert_eq!(
        result.total, 2,
        "should find 2 Auth events by actor_combined_1"
    );

    // Filter: Auth category + Success result
    let filters_success = AuditEventFilters {
        category: Some(AuditCategory::Auth),
        action: Some(AuditAction::AuthLogin),
        ..Default::default()
    };

    let result_success = repo
        .list_paginated(realm_id, filters_success)
        .await
        .expect("list_paginated should succeed");

    assert_eq!(
        result_success.total, 1,
        "should find 1 successful AuthLogin event"
    );
    assert_eq!(result_success.items[0].result, AuditResult::Success);
});
