//
// Shared helpers for billing-related API tests.
// Adapted for product_reduce: subscription uses entitlement_key instead of
// plan_id/tier/billing_period; Product/Plan helpers removed.
//

#![allow(dead_code)]

use crate::schema_test_context::SchemaTestContext as TestContext;
use herald_core::domain::authentication::Identity;
use herald_core::domain::user::entities::{User, UserStatus};
use uuid::Uuid;

/// Billing Test Setup Helpers
///
/// Setup admin session for billing tests
pub async fn setup_billing_admin_session(ctx: &mut TestContext, email: &str) -> String {
    let (admin_token, user_id) =
        crate::helpers::create_admin_session_with_user(ctx, email, 1800).await;

    // Grant Realm Admin role
    crate::helpers::grant_realm_admin_role(ctx, &user_id).await;

    admin_token
}

/// Entitlement Mapping Test Data Creation Helpers
///
/// Create a test entitlement mapping via direct SQL insertion.
///
/// Returns the mapping ID.
pub async fn setup_test_entitlement_mapping(
    ctx: &mut TestContext,
    realm_id: &str,
    payment_provider: &str,
    external_product_id: &str,
    entitlement_key: &str,
) -> Uuid {
    let mapping_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key,
             grant_on_subscribe, enabled, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, false, false, NOW(), NOW())",
    )
    .bind(mapping_id)
    .bind(realm_id)
    .bind(payment_provider)
    .bind(external_product_id)
    .bind(entitlement_key)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create test entitlement mapping");

    mapping_id
}

/// Create a test entitlement mapping with full points policy via direct SQL insertion.
///
/// Returns the mapping ID.
#[allow(clippy::too_many_arguments)]
pub async fn setup_test_entitlement_mapping_with_points(
    ctx: &mut TestContext,
    realm_id: &str,
    payment_provider: &str,
    external_product_id: &str,
    entitlement_key: &str,
    points_per_period: i64,
    grant_on_subscribe: bool,
    enabled: bool,
) -> Uuid {
    let mapping_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key,
             points_per_period, grant_on_subscribe, enabled, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW(), NOW())",
    )
    .bind(mapping_id)
    .bind(realm_id)
    .bind(payment_provider)
    .bind(external_product_id)
    .bind(entitlement_key)
    .bind(points_per_period)
    .bind(grant_on_subscribe)
    .bind(enabled)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create test entitlement mapping with points");

    mapping_id
}

/// Ensure the realm has a legacy credit bucket and return its id.
///
/// `subscription.bucket_id` is NOT NULL (eager binding), so direct-SQL
/// subscription inserts must bind a real bucket. This find-or-creates the
/// realm's legacy test bucket (deterministic slug, idempotent). Mirrors
/// `points_helpers::ensure_test_bucket_for_realm` in the api crate, which
/// test-support cannot depend on.
pub(crate) async fn ensure_test_bucket_for_realm(pool: &sqlx::PgPool, realm_id: &str) -> Uuid {
    use sqlx::Row;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    realm_id.hash(&mut hasher);
    let slug = format!("legacy-{:016x}", hasher.finish());

    sqlx::query(
        r#"INSERT INTO credit_buckets
             (id, realm_id, bucket_key, name, display_order, enabled,
              created_at, updated_at)
           VALUES ($1, $2, $3, 'Legacy Test Bucket', 0, true, NOW(), NOW())
           ON CONFLICT (realm_id, bucket_key) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(realm_id)
    .bind(&slug)
    .execute(pool)
    .await
    .expect("Failed to ensure legacy credit bucket");

    let row = sqlx::query("SELECT id FROM credit_buckets WHERE realm_id = $1 AND bucket_key = $2")
        .bind(realm_id)
        .bind(&slug)
        .fetch_one(pool)
        .await
        .expect("Failed to fetch legacy credit bucket");
    row.get::<Uuid, _>("id")
}

/// Create a test entitlement mapping at **price granularity** via direct SQL.
///
/// Binds `external_price_id` and `enabled`. Grant configuration is NOT set
/// here: it lives on `points_distribution_rule` rows owned by the mapping.
///
/// `points_per_period`, `grant_on_subscribe` and `bucket_id` are accepted for
/// call-site stability but intentionally ignored (grant config moved to
/// distribution rules); do not pass values expecting them to take effect.
#[allow(clippy::too_many_arguments)]
pub async fn setup_test_price_mapping(
    ctx: &mut TestContext,
    realm_id: &str,
    payment_provider: &str,
    external_product_id: &str,
    external_price_id: Option<&str>,
    entitlement_key: &str,
    points_per_period: i64,
    grant_on_subscribe: bool,
    enabled: bool,
    bucket_id: Uuid,
) -> Uuid {
    // Vestigial grant-config inputs; columns removed by the distribution-rules
    // refactor. Bound to `_` to fail loud if accidentally re-referenced.
    let _ = (points_per_period, grant_on_subscribe, bucket_id);

    let mapping_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, external_price_id,
             entitlement_key, enabled, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())",
    )
    .bind(mapping_id)
    .bind(realm_id)
    .bind(payment_provider)
    .bind(external_product_id)
    .bind(external_price_id)
    .bind(entitlement_key)
    .bind(enabled)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create price-granularity entitlement mapping");

    mapping_id
}

/// Create a test subscription with entitlement_key via direct SQL insertion.
/// Uses the new schema (entitlement_key, external_price_id, provider_metadata).
/// `subscription.bucket_id` was removed by the distribution-rules refactor, so
/// no bucket is bound here (grant routing is configured via distribution rules).
/// Returns the subscription ID.
pub async fn create_test_subscription_with_entitlement(
    ctx: &mut TestContext,
    realm_id: &str,
    client_app_id: Uuid,
    entitlement_key: &str,
    external_price_id: &str,
) -> Uuid {
    let subscription_id = Uuid::now_v7();
    let external_subscription_id = format!("sub_test_{}", subscription_id);
    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(format!("subscription-owner-{}@test.com", user_id))
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create subscription owner");

    sqlx::query(
        "INSERT INTO subscription
            (id, realm_id, user_id, client_app_id, status, entitlement_key, external_price_id,
             external_subscription_id, external_product_id, payment_provider,
             current_period_start, current_period_end,
             cancel_at_period_end, created_at, updated_at, billing_type)
         VALUES ($1, $2, $3, $4, 'active', $5, $6,
                 $7, $8, 'creem', NOW(), NOW() + INTERVAL '30 days',
                 false, NOW(), NOW(), 'recurring')",
    )
    .bind(subscription_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(client_app_id)
    .bind(entitlement_key)
    .bind(external_price_id)
    .bind(&external_subscription_id)
    .bind(format!("prod_{}", subscription_id))
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create test subscription with entitlement");

    subscription_id
}

/// Ensure the realm has a dedicated credit bucket (key `registration-pool`) and
/// return its id.
///
/// Historically this flagged the bucket with `receives_registration_credits` so
/// the (now-removed) `RegistrationPoolResolver` could bind draft mappings'
/// `bucket_id` to it. That model is gone: `provider_entitlement_mappings.
/// bucket_id` is nullable for drafts, and registration grant routing is
/// configured via `RealmRegistration` distribution rules. The helper now just
/// materializes a stable realm bucket that callers reuse to bind price mappings
/// and subscriptions (whose `subscription.bucket_id` is NOT NULL). The name is
/// kept for call-site stability; it no longer implies a registration-pool flag.
pub async fn ensure_registration_pool_bucket(ctx: &mut TestContext, realm_id: &str) -> Uuid {
    sqlx::query(
        r#"INSERT INTO credit_buckets
             (id, realm_id, bucket_key, name, display_order, enabled,
              created_at, updated_at)
           VALUES ($1, $2, $3, 'Registration Pool', 0, true, NOW(), NOW())
           ON CONFLICT (realm_id, bucket_key) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(realm_id)
    .bind("registration-pool")
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to ensure registration-pool credit bucket");

    use sqlx::Row;
    let row = sqlx::query("SELECT id FROM credit_buckets WHERE realm_id = $1 AND bucket_key = $2")
        .bind(realm_id)
        .bind("registration-pool")
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("Failed to fetch registration-pool credit bucket");
    row.get::<Uuid, _>("id")
}

/// Build an `Identity::User` for the given user id + the context's realm.
///
/// Used by scenario tests that drive a domain service directly (bypassing the
/// HTTP layer) and therefore need a real `Identity` for the policy + realm
/// access checks. `status = Normal` and a placeholder email keep it valid; the
/// load-bearing field is `realm_id` (must equal the test realm so
/// `has_access_to_realm` passes).
pub fn build_admin_identity(ctx: &TestContext, user_id: Uuid) -> Identity {
    Identity::User(User {
        id: user_id,
        realm_id: ctx._realm_id.clone(),
        email: "sync-operator@test.local".to_string(),
        nickname: None,
        password_hash: None,
        provider_ids: vec![],
        status: UserStatus::Normal,
        phone: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    })
}

/// Create an `active` subscription bound to a SPECIFIC price mapping's
/// `(realm, provider, external_product_id, external_price_id)` triple.
///
/// Unlike `create_test_subscription_with_entitlement` (which synthesizes its
/// own `external_product_id` and uses `client_app_id`), this helper binds the
/// subscription to the exact product/price the batch active-subscription lock
/// (`batch_update_mappings` step 3 — counts by `(realm, provider, product,
/// external_price_id)`) counts against, and uses a NULL `client_app_id` to
/// avoid the `uq_subscription_client_app` unique constraint (so multiple
/// protected mappings can coexist in one realm).
///
/// `bucket_id` is VESTIGIAL: `subscription.bucket_id` was removed by the
/// distribution-rules refactor (a subscription no longer carries a bucket; grant
/// routing is configured via distribution rules). It is accepted for call-site
/// stability and intentionally ignored.
///
/// Returns the subscription ID.
#[allow(clippy::too_many_arguments)]
pub async fn create_active_subscription_for_price_mapping(
    ctx: &mut TestContext,
    realm_id: &str,
    payment_provider: &str,
    external_product_id: &str,
    external_price_id: Option<&str>,
    entitlement_key: &str,
    bucket_id: Uuid,
) -> Uuid {
    // Vestigial: subscription.bucket_id was removed; grant routing is on
    // distribution rules now.
    let _ = bucket_id;

    let subscription_id = Uuid::now_v7();
    let external_subscription_id = format!("sub_protected_{}", subscription_id);
    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(format!("protected-sub-owner-{}@test.com", user_id))
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create protected subscription owner");

    // NULL client_app_id avoids uq_subscription_client_app; the active-sub
    // count query filters by (realm, provider, product, price), NOT by app.
    sqlx::query(
        "INSERT INTO subscription \
            (id, realm_id, user_id, client_app_id, status, entitlement_key, external_price_id, \
             external_subscription_id, external_product_id, payment_provider, \
             current_period_start, current_period_end, cancel_at_period_end, \
             created_at, updated_at, billing_type) \
         VALUES ($1, $2, $3, NULL, 'active', $4, $5, $6, $7, $8, \
                 NOW(), NOW() + INTERVAL '30 days', false, NOW(), NOW(), 'recurring')",
    )
    .bind(subscription_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(entitlement_key)
    .bind(external_price_id)
    .bind(&external_subscription_id)
    .bind(external_product_id)
    .bind(payment_provider)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create active subscription bound to price mapping");

    subscription_id
}

/// Delete a subscription via SQL (for cleanup)
pub async fn delete_test_subscription(ctx: &mut TestContext, subscription_id: Uuid) {
    sqlx::query("DELETE FROM subscription WHERE id = $1")
        .bind(subscription_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
}

/// Delete subscriptions by client app ID (for cleanup)
pub async fn delete_subscriptions_by_client_app(ctx: &mut TestContext, client_app_id: Uuid) {
    sqlx::query("DELETE FROM subscription WHERE client_app_id = $1")
        .bind(client_app_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
}

/// Client App Creation Helper
///
/// JSON payload for creating a basic client app
pub fn client_app_create_json(client_id: &str, name: &str, redirect_uris: &[&str]) -> String {
    use serde_json::json;

    let payload = json!({
        "clientId": client_id,
        "name": name,
        "redirectUris": redirect_uris,
        "enabled": true
    });

    payload.to_string()
}
