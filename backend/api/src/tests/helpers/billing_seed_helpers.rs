// Shared seeding + counting helpers for Stripe one-time (topup) billing
// scenarios (mappings, Succeeded attempts, payment/manual role grants).

use crate::tests::helpers::points_helpers::{
    ensure_test_bucket_for_realm, snapshot_attempt_rules_for_mapping,
};
use crate::tests::schema_test_context::SchemaTestContext;
use uuid::Uuid;

/// Create a Stripe one-time entitlement mapping that grants `role_ids`
/// (optionally empty) plus an optional `topup` fixed points rule.
pub async fn create_stripe_one_time_mapping_with_role(
    ctx: &SchemaTestContext,
    realm_id: &str,
    entitlement_key: &str,
    points: Option<i64>,
    role_ids: &[Uuid],
) -> Uuid {
    let mapping_id = Uuid::now_v7();
    let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, realm_id).await;
    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key,
             billing_type, enabled, granted_role_ids, created_at, updated_at)
         VALUES ($1, $2, 'stripe', $3, $4, 'one_time', true, $5, NOW(), NOW())",
    )
    .bind(mapping_id)
    .bind(realm_id)
    .bind(format!("prod_ri_{entitlement_key}"))
    .bind(entitlement_key)
    .bind(role_ids)
    .execute(&ctx.app_state.pool)
    .await
    .expect("seed Stripe one-time mapping with granted_role_ids");

    if let Some(points_amount) = points {
        let rule_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO points_distribution_rules
                (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
                 trigger_sources, grant_mode, points_amount, validity_days,
                 enabled, display_order)
             VALUES ($1, $2, 'entitlement_mapping', $3, $4, $5, 'fixed', $6, 0, true, 0)",
        )
        .bind(rule_id)
        .bind(realm_id)
        .bind(mapping_id)
        .bind(bucket_id)
        .bind(&["topup"][..])
        .bind(points_amount)
        .execute(&ctx.app_state.pool)
        .await
        .expect("seed mapping-owned topup distribution rule");
    }
    mapping_id
}

/// Create a Stripe `payment_attempts` snapshot in the Succeeded state with
/// `provider_reference = charge_id`, so `handle_charge_refunded` resolves
/// the attempt + routing bucket.
pub async fn create_stripe_succeeded_attempt(
    ctx: &SchemaTestContext,
    realm_id: &str,
    user_id: Uuid,
    mapping_id: Uuid,
    charge_id: &str,
    amount: i64,
) -> Uuid {
    let attempt_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO payment_attempts
            (id, realm_id, user_id, payment_provider, target_type, target_id,
             amount, currency, status, provider_reference,
             provider_status, expires_at, created_at, updated_at)
         VALUES ($1, $2, $3, 'stripe', 'entitlement_mapping', $4,
                 $5, 'usd', 'Succeeded', $6,
                 'succeeded', NOW() + INTERVAL '1 hour', NOW(), NOW())",
    )
    .bind(attempt_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(mapping_id)
    .bind(amount)
    .bind(charge_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("seed Stripe succeeded payment_attempt snapshot");
    snapshot_attempt_rules_for_mapping(
        &ctx.app_state.pool,
        attempt_id,
        realm_id,
        mapping_id,
        "topup",
    )
    .await;
    attempt_id
}

/// Count `user_roles` rows with `source='payment'` for a source_id.
pub async fn count_payment_roles_by_source_id(
    ctx: &SchemaTestContext,
    user_id: Uuid,
    source_id: &str,
) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM user_roles
         WHERE user_id = $1 AND source = 'payment' AND source_id = $2",
    )
    .bind(user_id)
    .bind(source_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .unwrap()
}

/// Seed a permanent payment-source role grant keyed to the attempt, as the
/// one-time fulfillment path would write it.
pub async fn seed_payment_role_grant(
    ctx: &SchemaTestContext,
    realm_id: &str,
    user_id: Uuid,
    role_id: Uuid,
    attempt_id: Uuid,
) {
    sqlx::query(
        "INSERT INTO user_roles
            (id, user_id, role_id, realm_id, client_id, principal_type, principal_id,
             source, source_id, expires_at)
         VALUES ($1, $2, $3, $4, $5, 'user', $2::text, 'payment', $6, NULL)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(role_id)
    .bind(realm_id)
    .bind(&ctx._client_id)
    .bind(attempt_id.to_string())
    .execute(&ctx.app_state.pool)
    .await
    .expect("seed payment role grant");
}

/// Seed a manual role grant that must survive every payment-side
/// revocation (the revoke filters `source='payment'`).
pub async fn seed_manual_role_grant(
    ctx: &SchemaTestContext,
    realm_id: &str,
    user_id: Uuid,
    role_id: Uuid,
) {
    sqlx::query(
        "INSERT INTO user_roles
            (id, user_id, role_id, realm_id, client_id, principal_type, principal_id,
             source, source_id, expires_at)
         VALUES ($1, $2, $3, $4, $5, 'user', $2::text, 'manual', NULL, NULL)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(role_id)
    .bind(realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("seed manual role grant");
}
