// Subscription Test Helpers
//
// Helpers for subscription CRUD with the new schema (entitlement_key,
// external_price_id, provider_metadata, synced_at).
//

#![allow(dead_code)]

use crate::tests::schema_test_context::SchemaTestContext;
use uuid::Uuid;

/// Create a test subscription bound to an existing user via direct SQL insertion.
///
/// Unlike `create_test_subscription_with_entitlement_key` below, no owner
/// account is created; the subscription is bound to the given `user_id`
/// (used by self-service endpoints that enforce subscription ownership).
///
/// Returns the subscription ID.
#[allow(clippy::too_many_arguments)]
pub async fn create_test_subscription_for_user(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    client_app_id: Uuid,
    user_id: Uuid,
    entitlement_key: &str,
    external_price_id: &str,
    payment_provider: &str,
    status: &str,
) -> Uuid {
    let subscription_id = Uuid::now_v7();
    let external_subscription_id = format!("sub_test_{}", subscription_id);
    let external_product_id = format!("prod_{}", entitlement_key);

    sqlx::query(
        "INSERT INTO subscription
            (id, realm_id, user_id, client_app_id, status, entitlement_key, external_price_id,
             external_subscription_id, external_product_id, payment_provider,
             current_period_start, current_period_end,
             cancel_at_period_end, created_at, updated_at, billing_type)
         VALUES ($1, $2, $3, $4, $5, $6, $7,
                 $8, $9, $10, NOW(), NOW() + INTERVAL '30 days',
                 false, NOW(), NOW(), 'recurring')",
    )
    .bind(subscription_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(client_app_id)
    .bind(status)
    .bind(entitlement_key)
    .bind(external_price_id)
    .bind(&external_subscription_id)
    .bind(&external_product_id)
    .bind(payment_provider)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create test subscription for user");

    subscription_id
}

/// Create a test subscription with all new fields via direct SQL insertion.
///
/// Returns the subscription ID.
#[allow(clippy::too_many_arguments)]
pub async fn create_test_subscription_with_entitlement_key(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    client_app_id: Uuid,
    entitlement_key: &str,
    external_price_id: &str,
    payment_provider: &str,
    status: &str,
) -> Uuid {
    let subscription_id = Uuid::now_v7();
    let external_subscription_id = format!("sub_test_{}", subscription_id);
    let external_product_id = format!("prod_{}", entitlement_key);
    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(format!("subscription-helper-owner-{}@test.com", user_id))
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create subscription owner");

    // subscription.bucket_id was removed by the distribution-rules refactor;
    // grant routing is configured via distribution rules.
    sqlx::query(
        "INSERT INTO subscription
            (id, realm_id, user_id, client_app_id, status, entitlement_key, external_price_id,
             external_subscription_id, external_product_id, payment_provider,
             current_period_start, current_period_end,
             cancel_at_period_end, created_at, updated_at, billing_type)
         VALUES ($1, $2, $3, $4, $5, $6, $7,
                 $8, $9, $10, NOW(), NOW() + INTERVAL '30 days',
                 false, NOW(), NOW(), 'recurring')",
    )
    .bind(subscription_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(client_app_id)
    .bind(status)
    .bind(entitlement_key)
    .bind(external_price_id)
    .bind(&external_subscription_id)
    .bind(&external_product_id)
    .bind(payment_provider)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create test subscription with entitlement_key");

    subscription_id
}

/// Create a test subscription with provider_metadata and synced_at via direct SQL.
///
/// Returns the subscription ID.
#[allow(clippy::too_many_arguments)]
pub async fn create_test_subscription_full(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    client_app_id: Uuid,
    entitlement_key: &str,
    external_price_id: &str,
    payment_provider: &str,
    status: &str,
    provider_metadata: Option<serde_json::Value>,
) -> Uuid {
    let subscription_id = Uuid::now_v7();
    let external_subscription_id = format!("sub_test_{}", subscription_id);
    let external_product_id = format!("prod_{}", entitlement_key);
    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(format!("subscription-full-owner-{}@test.com", user_id))
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create subscription owner");

    sqlx::query(
        "INSERT INTO subscription
            (id, realm_id, user_id, client_app_id, status, entitlement_key, external_price_id,
             external_subscription_id, external_product_id, payment_provider,
             current_period_start, current_period_end,
             provider_metadata, synced_at,
             cancel_at_period_end, created_at, updated_at, billing_type)
         VALUES ($1, $2, $3, $4, $5, $6, $7,
                 $8, $9, $10, NOW(), NOW() + INTERVAL '30 days',
                 $11, NOW(),
                 false, NOW(), NOW(), 'recurring')",
    )
    .bind(subscription_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(client_app_id)
    .bind(status)
    .bind(entitlement_key)
    .bind(external_price_id)
    .bind(&external_subscription_id)
    .bind(&external_product_id)
    .bind(payment_provider)
    .bind(provider_metadata)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create full test subscription");

    subscription_id
}

/// Verify that a subscription has the expected entitlement_key.
pub async fn verify_subscription_entitlement_key(
    ctx: &SchemaTestContext,
    subscription_id: Uuid,
    expected_entitlement_key: &str,
) {
    let actual: String =
        sqlx::query_scalar("SELECT entitlement_key FROM subscription WHERE id = $1")
            .bind(subscription_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("Subscription not found");

    assert_eq!(
        actual, expected_entitlement_key,
        "Subscription entitlement_key mismatch"
    );
}

/// Count subscriptions by entitlement_key in a realm.
pub async fn count_subscriptions_by_entitlement_key(
    ctx: &SchemaTestContext,
    realm_id: &str,
    entitlement_key: &str,
) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM subscription WHERE realm_id = $1 AND entitlement_key = $2",
    )
    .bind(realm_id)
    .bind(entitlement_key)
    .fetch_one(&ctx.app_state.pool)
    .await
    .unwrap()
}
