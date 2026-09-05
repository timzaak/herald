// =============================================================================
// Billing Test Helpers
// =============================================================================
//
// Shared helpers for billing-related API tests.
// Adapted for product_reduce: subscription uses entitlement_key instead of
// plan_id/tier/billing_period; Product/Plan helpers removed; entitlement
// mapping helpers added.
//
// =============================================================================

#![allow(dead_code)]

use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{body::Body, http::Request};
use herald_core::domain::billing::entities::SubscriptionStatus;
use hex;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tower::ServiceExt;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// ============================================================================
/// Billing Test Setup Helpers
/// ============================================================================
///
/// Setup admin session for billing tests
pub async fn setup_billing_admin_session(ctx: &mut TestContext, email: &str) -> String {
    let (admin_token, user_id) =
        crate::tests::helpers::create_admin_session_with_user(ctx, email, 1800).await;

    // Grant Realm Admin role
    crate::tests::helpers::grant_realm_admin_role(ctx, &user_id).await;

    admin_token
}

/// Setup admin session for billing tests and return both token and user_id
pub async fn setup_billing_admin_session_with_user(
    ctx: &mut TestContext,
    email: &str,
) -> (String, Uuid) {
    let (admin_token, user_id) =
        crate::tests::helpers::create_admin_session_with_user(ctx, email, 1800).await;

    // Grant Realm Admin role
    crate::tests::helpers::grant_realm_admin_role(ctx, &user_id).await;

    let user_uuid = Uuid::parse_str(&user_id).expect("Invalid user_id format");
    (admin_token, user_uuid)
}

/// ============================================================================
/// Entitlement Mapping Test Data Creation Helpers
/// =============================================================================
///
/// Create a test entitlement mapping via direct SQL insertion.
/// Returns the mapping ID.
pub async fn setup_test_entitlement_mapping(
    ctx: &mut TestContext,
    realm_id: &str,
    payment_provider: &str,
    external_product_id: &str,
    entitlement_key: &str,
) -> Uuid {
    let mapping_id = Uuid::now_v7();

    // Distribution-rules model: grant config lives in `points_distribution_rules`
    // (owner = entitlement_mapping), so the mapping row carries no grant columns.
    // Mirrors `seed_mapping`. A bare row defaults to enabled=false.
    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key, enabled)
         VALUES ($1, $2, $3, $4, $5, false)",
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

/// Seed a fixed-amount distribution rule owned by `mapping_id`, mirroring
/// `multi_wallet_grant_rule_scenarios::seed_rule` (direct SQL, so the parent
/// mapping's billing_type is not constrained by domain-level validation). Used
/// to preserve the grant semantics the old mapping-level columns encoded.
#[allow(clippy::too_many_arguments)]
async fn seed_mapping_owned_fixed_rule(
    ctx: &mut TestContext,
    realm_id: &str,
    mapping_id: Uuid,
    bucket_id: Uuid,
    trigger_sources: &[&str],
    points_amount: i64,
    validity_days: i64,
    enabled: bool,
) {
    let rule_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO points_distribution_rules
            (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
             trigger_sources, grant_mode, points_amount, validity_days,
             enabled, display_order)
         VALUES ($1, $2, 'entitlement_mapping', $3, $4, $5, 'fixed', $6, $7, $8, 0)",
    )
    .bind(rule_id)
    .bind(realm_id)
    .bind(mapping_id)
    .bind(bucket_id)
    .bind(trigger_sources)
    .bind(points_amount)
    .bind(validity_days)
    .bind(enabled)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to seed mapping-owned distribution rule");
}

/// Create a test entitlement mapping with full points policy via direct SQL insertion.
///
/// In the distribution-rules model the grant surfaces as a fixed
/// `subscription_initial` rule owned by this mapping (grant_on_subscribe=true),
/// targeting the realm's legacy test bucket. When `grant_on_subscribe` is false
/// no rule is seeded (no grant configured). Returns the mapping ID.
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
            (id, realm_id, payment_provider, external_product_id, entitlement_key, enabled)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(mapping_id)
    .bind(realm_id)
    .bind(payment_provider)
    .bind(external_product_id)
    .bind(entitlement_key)
    .bind(enabled)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create test entitlement mapping with points");

    if grant_on_subscribe {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        seed_mapping_owned_fixed_rule(
            ctx,
            realm_id,
            mapping_id,
            bucket_id,
            &["subscription_initial"],
            points_per_period,
            0,
            enabled,
        )
        .await;
    }

    mapping_id
}

/// Create a full entitlement mapping with all optional fields via direct SQL.
///
/// In the distribution-rules model, grant config is seeded as a fixed rule owned
/// by this mapping when both the points amount and a billing-type-determined
/// trigger are known:
///   - `one_time` -> `topup`
///   - `recurring` / `non_renewing` (with grant_on_subscribe) -> `subscription_initial`
///
/// Otherwise the mapping ships without a rule. `grant_period_type` / `max_periods`
/// have no mapping-owned fixed-rule equivalent (periodic grants are
/// realm-registration-only) and are intentionally dropped.
#[allow(clippy::too_many_arguments, unused_variables)]
pub async fn setup_test_entitlement_mapping_full(
    ctx: &mut TestContext,
    realm_id: &str,
    payment_provider: &str,
    external_product_id: &str,
    external_price_id: Option<&str>,
    entitlement_key: &str,
    billing_type: Option<&str>,
    billing_period: Option<&str>,
    points_per_period: Option<i64>,
    grant_period_type: Option<&str>,
    validity_days: Option<i64>,
    grant_on_subscribe: bool,
    max_periods: Option<i64>,
    enabled: bool,
    provider_product_info: Option<serde_json::Value>,
) -> Uuid {
    let mapping_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, external_price_id,
             entitlement_key, billing_type, billing_period, enabled, provider_product_info)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(mapping_id)
    .bind(realm_id)
    .bind(payment_provider)
    .bind(external_product_id)
    .bind(external_price_id)
    .bind(entitlement_key)
    .bind(billing_type)
    .bind(billing_period)
    .bind(enabled)
    .bind(provider_product_info)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create full test entitlement mapping");

    let trigger = match billing_type {
        Some("one_time") => Some("topup"),
        Some("recurring") | Some("non_renewing") if grant_on_subscribe => {
            Some("subscription_initial")
        }
        _ => None,
    };
    if let (Some(amount), Some(trig)) = (points_per_period, trigger) {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        seed_mapping_owned_fixed_rule(
            ctx,
            realm_id,
            mapping_id,
            bucket_id,
            &[trig],
            amount,
            validity_days.unwrap_or(0),
            enabled,
        )
        .await;
    }

    mapping_id
}

/// ============================================================================
/// Subscription Test Data Creation Helpers
/// =============================================================================
///
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

/// ============================================================================
/// Client App Creation Helper
/// =============================================================================
///
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

/// ============================================================================
/// Payment Flow Helpers
/// ============================================================================
///
/// Send a webhook event to the system (Creem)
///
/// Returns the HTTP response
pub async fn send_webhook_event(
    app: &axum::Router,
    realm_id: &str,
    payload: serde_json::Value,
    webhook_secret: &str,
) -> axum::response::Response {
    let payload_str = serde_json::to_string(&payload).unwrap();

    // Generate signature
    let mut mac = HmacSha256::new_from_slice(webhook_secret.as_bytes()).unwrap();
    mac.update(payload_str.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/third/pay/{}/creem/webhooks", realm_id))
                .header("creem-signature", signature)
                .header("Content-Type", "application/json")
                .body(Body::from(payload_str))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Verify subscription status in database
pub async fn verify_subscription_status(
    ctx: &TestContext,
    subscription_id: Uuid,
    expected_status: SubscriptionStatus,
) {
    let status_str: String = sqlx::query_scalar("SELECT status FROM subscription WHERE id = $1")
        .bind(subscription_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("Subscription not found");

    let actual_status: SubscriptionStatus = status_str
        .parse()
        .expect("Invalid subscription status in database");

    assert_eq!(
        actual_status, expected_status,
        "Expected status {:?}, got {:?}",
        expected_status, actual_status
    );
}

/// Verify payment event exists in database
pub async fn verify_payment_event_exists(ctx: &TestContext, creem_event_id: &str) -> bool {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM payment_event WHERE external_event_id = $1 AND payment_provider = 'creem'")
            .bind(creem_event_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();

    count > 0
}

/// Get subscription by client app ID
pub async fn get_subscription_by_client_app(
    ctx: &TestContext,
    client_app_id: Uuid,
) -> Option<Uuid> {
    sqlx::query_scalar("SELECT id FROM subscription WHERE client_app_id = $1")
        .bind(client_app_id)
        .fetch_optional(&ctx.app_state.pool)
        .await
        .unwrap()
}

/// ============================================================================
/// Subscription Status Transition Helpers
/// ============================================================================
///
/// Update subscription status directly via SQL
pub async fn update_subscription_status(
    ctx: &mut TestContext,
    subscription_id: Uuid,
    new_status: &str,
) {
    // When canceling, also set cancel_at to now
    if new_status == "canceled" {
        sqlx::query(
            "UPDATE subscription SET status = $1, cancel_at = NOW(), updated_at = NOW() WHERE id = $2"
        )
        .bind(new_status)
        .bind(subscription_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    } else {
        sqlx::query("UPDATE subscription SET status = $1, updated_at = NOW() WHERE id = $2")
            .bind(new_status)
            .bind(subscription_id)
            .execute(&ctx.app_state.pool)
            .await
            .unwrap();
    }
}

/// Update subscription period dates
pub async fn update_subscription_period(
    ctx: &mut TestContext,
    subscription_id: Uuid,
    period_start: chrono::DateTime<chrono::Utc>,
    period_end: chrono::DateTime<chrono::Utc>,
) {
    sqlx::query(
        "UPDATE subscription SET current_period_start = $1, current_period_end = $2, updated_at = NOW() WHERE id = $3"
    )
    .bind(period_start)
    .bind(period_end)
    .bind(subscription_id)
    .execute(&ctx.app_state.pool)
    .await
    .unwrap();
}

/// ============================================================================
/// Cleanup Helpers
/// ============================================================================
///
/// Clean up payment events for a specific subscription
pub async fn cleanup_payment_events(ctx: &mut TestContext, subscription_id: Uuid) {
    sqlx::query("DELETE FROM payment_event WHERE subscription_id = $1")
        .bind(subscription_id)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
}

/// Clean up payment events by Creem event ID
pub async fn cleanup_payment_event_by_creem_id(ctx: &mut TestContext, creem_event_id: &str) {
    sqlx::query(
        "DELETE FROM payment_event WHERE external_event_id = $1 AND payment_provider = 'creem'",
    )
    .bind(creem_event_id)
    .execute(&ctx.app_state.pool)
    .await
    .unwrap();
}

/// ============================================================================
/// Stripe Configuration Helpers
/// =============================================================================
///
/// Setup Stripe configuration for a test realm
pub async fn setup_stripe_config(
    ctx: &TestContext,
    realm_id: &str,
    api_key: &str,
    webhook_secret: &str,
) {
    // Insert Stripe API key
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled, created_at, updated_at)
         VALUES ($1, 'stripe', $2, $3, true, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key) DO UPDATE SET config_value = $3, enabled = true, updated_at = NOW()"
    )
    .bind(realm_id)
    .bind("api_key")
    .bind(api_key)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to insert Stripe API key");

    // Insert Stripe webhook secret
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled, created_at, updated_at)
         VALUES ($1, 'stripe', $2, $3, true, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key) DO UPDATE SET config_value = $3, enabled = true, updated_at = NOW()"
    )
    .bind(realm_id)
    .bind("webhook_secret")
    .bind(webhook_secret)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to insert Stripe webhook secret");

    // Insert Stripe timeout (default 30 seconds)
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled, created_at, updated_at)
         VALUES ($1, 'stripe', $2, $3, true, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key) DO UPDATE SET config_value = $3, enabled = true, updated_at = NOW()"
    )
    .bind(realm_id)
    .bind("timeout")
    .bind("30")
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to insert Stripe timeout");
}

/// Point a provider's `base_url` realm_config override at a test mock server.
pub async fn insert_realm_base_url(
    ctx: &TestContext,
    realm_id: &str,
    provider: &str,
    base_url: &str,
) {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled, created_at, updated_at)
         VALUES ($1, $2, 'base_url', $3, true, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key) DO UPDATE SET config_value = $3, enabled = true, updated_at = NOW()"
    )
    .bind(realm_id)
    .bind(provider)
    .bind(base_url)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to insert base_url realm config");
}

/// Verify payment event exists with Stripe event ID
pub async fn verify_stripe_payment_event_exists(ctx: &TestContext, stripe_event_id: &str) -> bool {
    let count: i64 =
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM payment_event WHERE external_event_id = $1 AND payment_provider = 'stripe'"
        )
        .bind(stripe_event_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();

    count > 0
}

/// Get subscription by Stripe subscription ID
pub async fn get_subscription_by_stripe_id(
    ctx: &TestContext,
    stripe_subscription_id: &str,
) -> Option<Uuid> {
    sqlx::query_scalar(
        "SELECT id FROM subscription WHERE external_subscription_id = $1 AND payment_provider = 'stripe'"
    )
    .bind(stripe_subscription_id)
    .fetch_optional(&ctx.app_state.pool)
    .await
    .unwrap()
}
