// =============================================================================
// Test: Subscription Paid Webhook
// =============================================================================
//
// Tests for subscription.paid webhook events (initial subscription and renewals)
// under the window-quota model.
//
// User Story: docs/user-stories/points-billing-events.md
// Covers: US-PO-06 (Subscription grants and renewals)
//
// Under the quota model, subscription.paid creates ONE PointsQuotaEntitlement
// row per (subscription, period). There are no points_credit_ledger rows, no
// points_grant_schedules, no points_grant_records, and no chained next-period
// pre-grants.
//
// =============================================================================

use crate::tests::helpers::points_helpers::*;
use crate::tests::helpers::webhook_helpers::*;
use crate::tests::scenarios::points::fixtures::*;
use crate::tests::schema_test_context::SchemaTestContext;
use herald_core::domain::billing::BillingRepository;
use herald_core::domain::points::entities::{CreditType, QuotaEntitlementStatus, QuotaSourceType};
use test_context::test_context;
use uuid::Uuid;

/// Resolve the price-level EntitlementMapping for a key in these scenarios.
///
/// The price-level mapping refactor changed `handle_subscription_paid` to consume
/// the price-level mapping directly. These scenarios seed a single mapping per
/// entitlement_key, so resolving by key is identity-equivalent to the price-level
/// mapping the webhook path resolves.
async fn mapping_for_key(
    ctx: &SchemaTestContext,
    realm_id: &str,
    key: &str,
) -> herald_core::domain::billing::entities::EntitlementMapping {
    ctx.app_state
        .billing_repository
        .find_entitlement_mapping_by_key(realm_id, key)
        .await
        .unwrap_or_else(|_| panic!("mapping for key '{key}' should exist"))
        .unwrap_or_else(|| panic!("mapping for key '{key}' should be Some"))
}

/// Seed a `subscription_renewal` quota rule on `mapping_id` so a renewal grant
/// (`handle_subscription_paid(is_renewal=true)` → `CurrentOwnerRules`) fires.
/// The bare mapping seeders (`setup_test_plan_config_with_points`,
/// `setup_test_entitlement_mapping_for_webhook`) create no distribution rule;
/// the renewal trigger reads CURRENT rules at grant time, so this rule must
/// exist first. This is the renewal analog of production's recurring mapping
/// config — faithful input, real production grant code.
async fn seed_subscription_renewal_rule(
    ctx: &SchemaTestContext,
    realm_id: &str,
    mapping_id: Uuid,
    limit: i64,
) {
    let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, realm_id).await;
    let rule_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO points_distribution_rules
            (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
             trigger_sources, grant_mode, validity_days, quota_windows,
             enabled, display_order)
         VALUES ($1, $2, 'entitlement_mapping', $3, $4, $5, 'quota', 0, $6, true, 0)",
    )
    .bind(rule_id)
    .bind(realm_id)
    .bind(mapping_id)
    .bind(bucket_id)
    .bind(&["subscription_renewal"][..])
    .bind(serde_json::json!([{"windowSeconds": 2_592_000, "limit": limit, "key": "period"}]))
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to seed subscription_renewal quota rule");
}

/// Seed a `subscription_renewal` quota rule on the strategy mapping a
/// `subscription.paid` webhook resolves for `plan_id`. The webhook event built
/// by `build_subscription_paid_event` carries `productId=prod_test_monthly`, so
/// the price-aware resolver lands on the generic `prod_test_monthly` mapping
/// (entitlement_key = plan_id) created by `setup_test_plan_config_with_points`.
async fn seed_renewal_rule_for_plan_webhook(
    ctx: &SchemaTestContext,
    realm_id: &str,
    plan_id: Uuid,
    limit: i64,
) {
    let mapping_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM provider_entitlement_mappings
         WHERE realm_id = $1 AND external_product_id = 'prod_test_monthly'
           AND entitlement_key = $2",
    )
    .bind(realm_id)
    .bind(plan_id.to_string())
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("generic prod_test_monthly mapping must exist for the plan");
    seed_subscription_renewal_rule(ctx, realm_id, mapping_id, limit).await;
}

/// Subscription activation may mix fixed credit and rolling quota without partial fulfillment.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_multi_wallet_grant_rule_subscription_initial_fixed_and_quota(
    ctx: &mut SchemaTestContext,
) {
    super::multi_wallet_grant_rule_scenarios::assert_fixed_and_quota_event(ctx).await;
}

/// Renewal must select only rules that explicitly declare the renewal trigger.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_multi_wallet_grant_rule_renewal_selects_declared_trigger_only(
    ctx: &mut SchemaTestContext,
) {
    super::multi_wallet_grant_rule_scenarios::assert_two_account_fixed_event(
        ctx,
        herald_core::domain::points::DistributionTrigger::SubscriptionRenewal,
    )
    .await;
    super::multi_wallet_grant_rule_scenarios::assert_replay_is_stable(ctx).await;
}

/// Seed a `subscription` row and return its id. Used by subscription tests so
/// the `subscription_id` is known ahead of the service call.
async fn seed_subscription_row(
    ctx: &mut SchemaTestContext,
    user_id: Uuid,
    realm_id: &str,
    entitlement_key: &str,
) -> Uuid {
    // `subscription.bucket_id` was removed by the distribution-rules refactor
    // (grant routing is configured via distribution rules). `billing_type` is
    // NOT NULL (0011_pay_model).
    let subscription_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO subscription
            (id, realm_id, user_id, status, entitlement_key,
             external_subscription_id, external_product_id, payment_provider,
             current_period_start, current_period_end, cancel_at_period_end,
             created_at, updated_at, billing_type)
         VALUES ($1, $2, $3, 'active', $4, $5, $6, 'creem',
                 NOW(), NOW() + INTERVAL '30 days', false, NOW(), NOW(), 'recurring')",
    )
    .bind(subscription_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(entitlement_key)
    .bind(format!("sub_be_t04_{}", subscription_id))
    .bind(format!("prod_be_t04_{}", entitlement_key))
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to seed subscription row");
    subscription_id
}

// ============================================================================
// Test 1: Subscription Paid Grant
// ============================================================================

// User Story: docs/user-stories/points-billing-events.md
// Covers: US-PO-06 场景 - subscription.paid grants subscription_credit
//
// After the distribution-rules refactor, `handle_subscription_paid(is_renewal=false)`
// grants NO points by design — initial fulfillment is owned by the captured
// PaymentAttempt flow (BE-D04). The only subscription.paid points path is the
// renewal route (`CurrentOwnerRules` over a `subscription_renewal` rule). This
// test therefore exercises the renewal grant.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_subscription_paid_initial_grant(ctx: &mut SchemaTestContext) {
    // Given
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(&ctx.app_state.pool, &realm_id, "user1@example.com").await;
    let plan_id = Uuid::now_v7();
    let event_id = generate_test_event_id();

    // Configure Creem webhook for this realm
    ctx.with_creem_config(&realm_id, None, None, None).await;

    // Setup plan config for the test
    setup_test_plan_config(ctx, &realm_id, plan_id).await;
    // Initial activation grants nothing; only the renewal route grants points,
    // so seed a subscription_renewal rule and drive a renewal webhook.
    seed_renewal_rule_for_plan_webhook(ctx, &realm_id, plan_id, 1000).await;

    // Create points account for user
    create_points_wallet(ctx, user_id, &realm_id).await;

    // Build subscription.paid event (renewal is the grant-bearing route).
    let event = build_subscription_paid_event(
        event_id, user_id, plan_id, true, // renewal
        &realm_id,
    );

    // When
    let app = ctx.create_unified_test_router();
    let response = send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;

    // Then
    assert_webhook_success(&response);

    // Verify subscription_credit quota entitlement was created
    let entitlements =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert_eq!(
        entitlements.len(),
        1,
        "Should create one subscription quota entitlement"
    );

    let entitlement = &entitlements[0];
    assert_eq!(entitlement.credit_type, CreditType::SubscriptionCredit);
    assert_eq!(
        entitlement.source_type,
        QuotaSourceType::SubscriptionRenewal
    );
    assert_eq!(
        entitlement.quota_windows.len(),
        1,
        "Should have one quota window"
    );
    assert_eq!(
        entitlement.quota_windows[0].limit,
        1000, // Limit from the seeded subscription_renewal rule
        "Window limit should equal the seeded rule limit"
    );
    assert_eq!(entitlement.status, QuotaEntitlementStatus::Active);
    assert!(
        entitlement.effective_until.is_some(),
        "Subscription entitlement should have effective_until"
    );
}

// ============================================================================
// Subscription activation / renewal window-quota idempotency
// ============================================================================
//
// These tests exercise the period-aware `handle_subscription_paid` path
// directly with a pre-seeded `subscription` row. Direct service invocation
// lets the test bind a known `subscription_id` and assert on the resulting
// `points_quota_entitlements` rows deterministically.

/// User Story: US-PU-009 (use current-period points without distribution delay).
/// Covers (P0 — 订阅当前周期配额):
///   - Subscription activation grants the CURRENT period only.
///   - Derived available balance equals one period's worth.
///   - NO next-period pre-grant entitlement is written.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_subscription_activation_grants_current_period_only(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(
        &ctx.app_state.pool,
        &realm_id,
        "be_t04_activate@example.com",
    )
    .await;

    let entitlement_key = format!("be-t04-act-{}", Uuid::now_v7());
    let points_per_period: i64 = 1000;

    setup_test_entitlement_mapping_for_webhook(
        ctx,
        &realm_id,
        "creem",
        &format!("prod_be_t04_{}", entitlement_key),
        &entitlement_key,
        points_per_period,
        true,
        true,
    )
    .await;

    let subscription_id = seed_subscription_row(ctx, user_id, &realm_id, &entitlement_key).await;
    let now = chrono::Utc::now();
    let current_period_start = now - chrono::Duration::seconds(10);
    let current_period_end = now + chrono::Duration::days(30);

    // --- When: subscription renewal fires handle_subscription_paid -----------
    // Initial activation grants no points (BE-D04 owns initial fulfillment);
    // the renewal route is the grant-bearing path, so seed a subscription_renewal
    // rule on this mapping and drive a renewal.
    let mapping = mapping_for_key(ctx, &realm_id, &entitlement_key).await;
    seed_subscription_renewal_rule(ctx, &realm_id, mapping.id, points_per_period).await;
    let result = ctx
        .app_state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            &realm_id,
            &mapping,
            true, // renewal — the grant-bearing subscription.paid route
            current_period_start,
            current_period_end,
            format!("evt_be_t04_act_{}", Uuid::now_v7()),
        )
        .await;
    assert!(result.is_ok(), "renewal grant failed: {:?}", result);

    // --- Then: exactly one active entitlement for the current period ---------
    let entitlements =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert_eq!(
        entitlements.len(),
        1,
        "renewal should create exactly one current-period quota entitlement, got {}",
        entitlements.len()
    );

    let entitlement = &entitlements[0];
    assert_eq!(entitlement.credit_type, CreditType::SubscriptionCredit);
    assert_eq!(
        entitlement.source_type,
        QuotaSourceType::SubscriptionRenewal
    );
    assert_eq!(entitlement.status, QuotaEntitlementStatus::Active);
    assert_eq!(
        entitlement.quota_windows.first().map(|w| w.limit),
        Some(points_per_period)
    );
    assert!(
        entitlement.effective_until.is_some(),
        "current-period entitlement should have effective_until"
    );

    assert_derived_balance(
        ctx,
        user_id,
        &realm_id,
        CreditType::SubscriptionCredit,
        points_per_period,
    )
    .await;

    assert_eq!(
        count_subscription_quota_entitlements(ctx, user_id).await,
        1,
        "no next-period pre-grant should exist"
    );
}

/// User Story: US-PU-009 (renewal must not double-grant the same period).
/// Covers (P0 — 续费周期幂等): calling `handle_subscription_paid` twice with
/// the same `(subscription_id, period_start)` produces exactly one quota
/// entitlement row.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_subscription_renewal_period_idempotency(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(
        &ctx.app_state.pool,
        &realm_id,
        "be_t04_period_idem@example.com",
    )
    .await;

    let entitlement_key = format!("be-t04-pi-{}", Uuid::now_v7());
    let points_per_period: i64 = 500;

    setup_test_entitlement_mapping_for_webhook(
        ctx,
        &realm_id,
        "creem",
        &format!("prod_be_t04_{}", entitlement_key),
        &entitlement_key,
        points_per_period,
        true,
        true,
    )
    .await;

    let subscription_id = seed_subscription_row(ctx, user_id, &realm_id, &entitlement_key).await;
    let now = chrono::Utc::now();
    let period_start = now - chrono::Duration::seconds(10);
    let period_end = now + chrono::Duration::days(30);

    let mapping = mapping_for_key(ctx, &realm_id, &entitlement_key).await;
    seed_subscription_renewal_rule(ctx, &realm_id, mapping.id, points_per_period).await;
    let event_id = format!("evt_be_t04_pi_{}", Uuid::now_v7());

    // --- When: first renewal for this period --------------------------------
    let result1 = ctx
        .app_state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            &realm_id,
            &mapping,
            true, // renewal
            period_start,
            period_end,
            event_id.clone(),
        )
        .await;
    assert!(
        result1.is_ok(),
        "first renewal should succeed: {:?}",
        result1
    );

    // --- Then: one entitlement exists ---------------------------------------
    let count1 = count_subscription_quota_entitlements(ctx, user_id).await;
    assert_eq!(
        count1, 1,
        "first renewal should create exactly one quota entitlement"
    );

    // --- When: same period is processed again -------------------------------
    let result2 = ctx
        .app_state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            &realm_id,
            &mapping,
            true,
            period_start,
            period_end,
            event_id,
        )
        .await;
    assert!(
        result2.is_ok(),
        "duplicate renewal should be idempotent: {:?}",
        result2
    );

    // --- Then: still exactly one entitlement, still active ------------------
    let count2 = count_subscription_quota_entitlements(ctx, user_id).await;
    assert_eq!(
        count2, 1,
        "duplicate renewal must not create additional quota entitlement rows"
    );

    let entitlements =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    let entitlement = &entitlements[0];
    assert_eq!(entitlement.status, QuotaEntitlementStatus::Active);
    assert_eq!(
        entitlement.source_type,
        QuotaSourceType::SubscriptionRenewal
    );
}

/// User Story: US-PU-009 (duplicate provider webhook delivery must not
/// double-grant).
/// Covers (P0 — provider event-level idempotency preserved):
/// when the SAME `event_id` is delivered twice, the webhook layer deduplicates
/// the delivery and does NOT re-enter `handle_subscription_paid`.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_subscription_renewal_event_idempotency(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(
        &ctx.app_state.pool,
        &realm_id,
        "be_t04_event_idem@example.com",
    )
    .await;
    let plan_id = Uuid::now_v7();
    let event_id = generate_test_event_id();

    ctx.with_creem_config(&realm_id, None, None, None).await;
    setup_test_plan_config(ctx, &realm_id, plan_id).await;
    create_points_wallet(ctx, user_id, &realm_id).await;
    seed_renewal_rule_for_plan_webhook(ctx, &realm_id, plan_id, 1000).await;

    // Build a subscription.paid renewal webhook with explicit period bounds.
    let now = chrono::Utc::now();
    let period_start_str = now.to_rfc3339();
    let period_end_str = (now + chrono::Duration::days(30)).to_rfc3339();
    let base = build_subscription_paid_event(
        event_id.clone(),
        user_id,
        plan_id,
        true, // renewal
        &realm_id,
    );
    let mut event = base.clone();
    event["data"]["object"]["currentPeriodStart"] = serde_json::Value::String(period_start_str);
    event["data"]["object"]["currentPeriodEnd"] = serde_json::Value::String(period_end_str);

    let app = ctx.create_unified_test_router();

    // --- When: first webhook delivery ---------------------------------------
    let response1 =
        send_webhook_with_signature(&app, &realm_id, event.clone(), "test_webhook_secret").await;
    assert_webhook_success(&response1);

    let count_after_first = count_subscription_quota_entitlements(ctx, user_id).await;
    assert_eq!(
        count_after_first, 1,
        "first delivery should create exactly one quota entitlement"
    );

    // --- When: second webhook delivery (SAME event_id) ----------------------
    let response2 =
        send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;
    assert_webhook_success(&response2);

    // --- Then: the duplicate delivery must NOT add any additional row -------
    let count_after_second = count_subscription_quota_entitlements(ctx, user_id).await;
    assert_eq!(
        count_after_first, count_after_second,
        "duplicate webhook event_id must not create additional entitlement rows"
    );

    // Verify the persisted idempotency key. After the distribution-rules
    // refactor the entitlement row carries `distribution:{event_id}:{rule_id}`
    // (the internal event + rule UUIDs), not the legacy sub:period form.
    let entitlements =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert_eq!(entitlements.len(), 1);
    let entitlement = &entitlements[0];
    let expected_key = format!(
        "distribution:{}:{}",
        entitlement
            .distribution_event_id
            .expect("rule-attributed entitlement must carry distribution_event_id"),
        entitlement
            .distribution_rule_id
            .expect("rule-attributed entitlement must carry distribution_rule_id"),
    );
    assert_eq!(
        entitlement.idempotency_key, expected_key,
        "entitlement idempotency key must be distribution:{{event_id}}:{{rule_id}}"
    );
}
