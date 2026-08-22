// =============================================================================
// PostgresBillingRepository Unit Tests
// =============================================================================
//
// Unit tests for PostgreSQL billing repository operations.
// Adapted for product_reduce schema: subscription uses entitlement_key
// instead of plan_id/tier/billing_period; Product/Plan CRUD removed.
//
// =============================================================================

use super::*;
use chrono::Utc;
use futures::FutureExt;
use herald_domain::billing::{
    BillingRepository, Subscription, SubscriptionStatus, test_helpers::*,
};
use herald_domain::common::entities::app_errors::CoreError;
use herald_test_db::{SharedTestDatabaseHandle, create_isolated_schema_database};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use std::future::Future;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

// =============================================================================
// Test Database Setup
// =============================================================================

struct BillingTestDb {
    db: DatabaseConnection,
    pool: sqlx::PgPool,
    schema: SharedTestDatabaseHandle,
}

impl BillingTestDb {
    async fn teardown(self) {
        let BillingTestDb { db, pool, schema } = self;
        drop(db);
        drop(pool);
        schema.teardown().await;
    }
}

async fn setup_test_db() -> BillingTestDb {
    let (schema, pool, db) = create_isolated_schema_database(3).await;

    create_test_realms(&db).await;

    BillingTestDb { db, pool, schema }
}

async fn run_with_repo<F, Fut>(test_fn: F)
where
    F: FnOnce(PostgresBillingRepository) -> Fut,
    Fut: Future<Output = ()>,
{
    let test_db = setup_test_db().await;
    let repo = PostgresBillingRepository::new(test_db.db.clone());
    let result = AssertUnwindSafe(test_fn(repo)).catch_unwind().await;
    test_db.teardown().await;

    if let Err(panic_payload) = result {
        resume_unwind(panic_payload);
    }
}

/// Create test realm records
async fn create_test_realms(db: &DatabaseConnection) {
    let backend = db.get_database_backend();

    let test_realm_ids = test_realm_ids();

    for realm_id in test_realm_ids {
        let _ = db
            .execute(Statement::from_string(
                backend,
                format!(
                    "INSERT INTO realm (id, name) VALUES ('{}', 'Test Realm {}') ON CONFLICT (id) DO NOTHING",
                    realm_id, realm_id
                ),
            ))
            .await;
    }
}

/// Returns all test realm IDs used across tests
fn test_realm_ids() -> Vec<&'static str> {
    vec![
        "test_create_sub",
        "test_optional_fields",
        "test_find_by_realm",
        "test_find_by_creem",
        "test_update_status",
        "test_update_entitlement",
        "test_update_cancel",
        "test_different_realms_1",
        "test_different_realms_2",
        "test_idempotent",
        "test_realm_1",
        "test_realm_2",
        "test_realm_3",
        "test_create_event",
        "test_event_with_sub",
        "test_find_event",
        "test_mark_processed",
        "test_cancel_period",
        "test_idempotency",
        "test_find_realm",
        "test_find_creem",
        "test_nonexistent",
        "test_status_0",
        "test_status_1",
        "test_status_2",
        "test_status_3",
        "test_status_4",
        "test_status_5",
        "test_status_6",
    ]
}

// =============================================================================
// Subscription Tests
// =============================================================================

macro_rules! billing_repo_test {
    ($name:ident, |$repo:ident| $body:block) => {
        #[tokio::test]
        async fn $name() {
            run_with_repo(|$repo| async move $body).await;
        }
    };
}

billing_repo_test!(test_repository_create_subscription, |repo| {
    let subscription = test_subscription("test_create_sub");

    let result = repo.create_subscription(subscription.clone()).await;

    assert!(result.is_ok(), "Failed to create subscription");
    let created = result.unwrap();
    assert_eq!(created.realm_id, "test_create_sub");
    assert_subscription_status(&created, SubscriptionStatus::Active);
    assert_eq!(created.entitlement_key, "starter-plan");
});

billing_repo_test!(
    test_repository_create_subscription_with_optional_fields,
    |repo| {
        let subscription = SubscriptionBuilder::new()
            .with_realm_id("test_optional_fields")
            .with_external_subscription_id("sub_test123")
            .with_external_product_id("prod_professional_yearly")
            .with_status(SubscriptionStatus::Trialing)
            .with_entitlement_key("pro-plan")
            .with_period_end(Utc::now() + chrono::Duration::days(14))
            .with_cancel_at_period_end(true)
            .build();

        let result = repo.create_subscription(subscription.clone()).await;

        assert!(result.is_ok(), "Failed to create subscription");
        let created = result.unwrap();
        assert_eq!(created.realm_id, "test_optional_fields");
        assert_subscription_status(&created, SubscriptionStatus::Trialing);
        assert_eq!(created.entitlement_key, "pro-plan");
        assert_eq!(created.external_subscription_id, "sub_test123");
        assert!(created.cancel_at_period_end);
    }
);

billing_repo_test!(test_repository_find_by_realm_id_exists, |repo| {
    let subscription = test_subscription("test_find_realm");
    let expected_id = subscription.id;
    repo.create_subscription(subscription).await.unwrap();

    let result = repo.find_by_realm_id("test_find_realm").await;

    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    let sub = found.unwrap();
    assert_eq!(sub.realm_id, "test_find_realm");
    assert_eq!(sub.id, expected_id);
    assert_subscription_status(&sub, SubscriptionStatus::Active);
});

billing_repo_test!(test_repository_find_by_realm_id_not_found, |repo| {
    let result = repo.find_by_realm_id("test_nonexistent_realm").await;

    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_none());
});

billing_repo_test!(
    test_repository_find_by_external_subscription_id_exists,
    |repo| {
        let subscription = SubscriptionBuilder::new()
            .with_realm_id("test_find_creem")
            .with_external_subscription_id("creem_sub_test123")
            .build();
        repo.create_subscription(subscription).await.unwrap();

        let result = repo
            .find_by_external_subscription_id("creem_sub_test123", "creem")
            .await;

        assert!(result.is_ok());
        let found = result.unwrap();
        assert!(found.is_some());
        let sub = found.unwrap();
        assert_eq!(sub.realm_id, "test_find_creem");
        assert_eq!(sub.external_subscription_id, "creem_sub_test123");
    }
);

billing_repo_test!(
    test_repository_find_by_external_subscription_id_not_found,
    |repo| {
        let result = repo
            .find_by_external_subscription_id("nonexistent_creem_sub", "creem")
            .await;

        assert!(result.is_ok());
        let found = result.unwrap();
        assert!(found.is_none());
    }
);

billing_repo_test!(test_repository_update_subscription_status, |repo| {
    let subscription = test_subscription("test_update_status");
    let mut created = repo.create_subscription(subscription).await.unwrap();

    created.status = SubscriptionStatus::Canceled;
    created.updated_at = Utc::now();

    let result = repo.update_subscription(created.clone()).await;

    assert!(result.is_ok());
    let updated = result.unwrap();
    assert_subscription_status(&updated, SubscriptionStatus::Canceled);
    assert_eq!(updated.realm_id, "test_update_status");
});

billing_repo_test!(
    test_repository_update_subscription_entitlement_key,
    |repo| {
        let subscription = test_subscription("test_update_entitlement");
        let mut created = repo.create_subscription(subscription).await.unwrap();

        created.entitlement_key = "enterprise-plan".to_string();
        created.updated_at = Utc::now();

        let result = repo.update_subscription(created.clone()).await;

        assert!(result.is_ok());
        let updated = result.unwrap();
        assert_eq!(updated.entitlement_key, "enterprise-plan");
    }
);

billing_repo_test!(
    test_repository_update_subscription_cancel_at_period_end,
    |repo| {
        let subscription = test_subscription("test_cancel_period");
        let mut created = repo.create_subscription(subscription).await.unwrap();

        created.cancel_at_period_end = true;
        created.updated_at = Utc::now();

        let result = repo.update_subscription(created.clone()).await;

        assert!(result.is_ok());
        let updated = result.unwrap();
        assert!(updated.cancel_at_period_end);
    }
);

billing_repo_test!(test_repository_update_nonexistent_subscription, |repo| {
    let subscription = test_subscription("test_nonexistent");
    let fake_id = Uuid::now_v7();

    let nonexistent_sub = Subscription {
        id: fake_id,
        ..subscription
    };

    let result = repo.update_subscription(nonexistent_sub).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        CoreError::SubscriptionNotFound(id) => {
            assert_eq!(id, fake_id.to_string());
        }
        _ => panic!("Expected SubscriptionNotFound error"),
    }
});

billing_repo_test!(
    test_repository_multiple_subscriptions_different_realms,
    |repo| {
        let sub1 = repo
            .create_subscription(test_subscription("test_realm_1"))
            .await
            .unwrap();
        let sub2 = repo
            .create_subscription(test_subscription("test_realm_2"))
            .await
            .unwrap();
        let sub3 = repo
            .create_subscription(test_subscription("test_realm_3"))
            .await
            .unwrap();

        let found1 = repo
            .find_by_realm_id("test_realm_1")
            .await
            .unwrap()
            .unwrap();
        let found2 = repo
            .find_by_realm_id("test_realm_2")
            .await
            .unwrap()
            .unwrap();
        let found3 = repo
            .find_by_realm_id("test_realm_3")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(found1.id, sub1.id);
        assert_eq!(found2.id, sub2.id);
        assert_eq!(found3.id, sub3.id);

        // Verify they are different
        assert_ne!(found1.id, found2.id);
        assert_ne!(found2.id, found3.id);
        assert_ne!(found1.id, found3.id);
    }
);

billing_repo_test!(test_repository_subscription_all_statuses, |repo| {
    let statuses = [
        SubscriptionStatus::Active,
        SubscriptionStatus::Canceled,
        SubscriptionStatus::Expired,
        SubscriptionStatus::Pending,
        SubscriptionStatus::Trialing,
        SubscriptionStatus::Paused,
    ];

    for (i, status) in statuses.iter().enumerate() {
        let realm_id = format!("test_status_{}", i);
        let sub = SubscriptionBuilder::new()
            .with_realm_id(realm_id)
            .with_status(status.clone())
            .build();

        let created = repo.create_subscription(sub).await.unwrap();
        assert_subscription_status(&created, status.clone());
    }
});

// =============================================================================
// Payment Event Tests
// =============================================================================

billing_repo_test!(test_repository_create_payment_event, |repo| {
    let event = test_payment_event("test_create_event", "evt_create123");

    let result = repo.create_payment_event(event.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.realm_id, "test_create_event");
    assert_eq!(created.external_event_id, "evt_create123");
    assert_eq!(created.payment_provider, "creem");
    assert_eq!(created.event_type, "subscription.paid");
    assert!(!created.processed);
});

billing_repo_test!(
    test_repository_create_payment_event_with_subscription,
    |repo| {
        let subscription = test_subscription("test_event_with_sub");
        let created_sub = repo.create_subscription(subscription).await.unwrap();

        let event = PaymentEventBuilder::new()
            .with_realm_id("test_event_with_sub")
            .with_external_event_id("evt_with_sub123")
            .with_payment_provider("creem")
            .with_subscription_id(created_sub.id)
            .build();

        let result = repo.create_payment_event(event).await;

        assert!(result.is_ok());
        let created = result.unwrap();
        assert_eq!(created.subscription_id, Some(created_sub.id));
    }
);

billing_repo_test!(
    test_repository_find_payment_event_by_creem_id_exists,
    |repo| {
        let event = test_payment_event("test_find_event", "evt_find123");
        repo.create_payment_event(event).await.unwrap();

        let result = repo
            .find_payment_event_by_external_id("test_find_event", "evt_find123", "creem")
            .await;

        assert!(result.is_ok());
        let found = result.unwrap();
        assert!(found.is_some());
        let evt = found.unwrap();
        assert_eq!(evt.realm_id, "test_find_event");
        assert_eq!(evt.external_event_id, "evt_find123");
        assert_eq!(evt.payment_provider, "creem");
    }
);

billing_repo_test!(
    test_repository_find_payment_event_by_creem_id_not_found,
    |repo| {
        let result = repo
            .find_payment_event_by_external_id("test_find_event", "nonexistent_event_id", "creem")
            .await;

        assert!(result.is_ok());
        let found = result.unwrap();
        assert!(found.is_none());
    }
);

billing_repo_test!(test_repository_mark_payment_event_processed, |repo| {
    let event = test_payment_event("test_mark_processed", "evt_mark123");
    let created = repo.create_payment_event(event).await.unwrap();

    assert!(!created.processed);

    let result = repo.mark_payment_event_processed(created.id).await;

    assert!(result.is_ok());

    // Verify the event is marked as processed
    let found = repo
        .find_payment_event_by_external_id("test_mark_processed", "evt_mark123", "creem")
        .await
        .unwrap();
    assert!(found.is_some());
    assert!(found.unwrap().processed);
});

billing_repo_test!(test_repository_mark_processed_nonexistent_event, |repo| {
    let fake_id = Uuid::now_v7();

    let result = repo.mark_payment_event_processed(fake_id).await;

    assert!(result.is_err());
});

billing_repo_test!(test_repository_payment_event_idempotency, |repo| {
    let event1 = test_payment_event("test_idempotency", "evt_dup123");
    repo.create_payment_event(event1).await.unwrap();

    // Try to create another event with same creem_event_id
    let event2 = test_payment_event("test_idempotency", "evt_dup123");

    let result = repo.create_payment_event(event2).await;

    // Should fail due to UNIQUE constraint
    assert!(result.is_err());
});
