// =============================================================================
// Test: Subscription Downgrade Webhook
// =============================================================================
//
// Tests for subscription.update webhook events (downgrades) under the
// window-quota model.
//
// User Story: docs/user-stories/points-billing-events.md
// Covers: US-BI-010 (Subscription downgrade takes effect next period)
//
// Under the quota model, handle_subscription_downgrade does NOT revoke the
// active entitlement. The user keeps their current window quota until
// effective_until; the next renewal webhook grants a fresh entitlement from the
// new mapping's quota_windows.
//
// =============================================================================

use crate::tests::helpers::points_helpers::*;
use crate::tests::helpers::webhook_helpers::*;
use crate::tests::scenarios::points::fixtures::*;
use crate::tests::schema_test_context::SchemaTestContext;
use chrono::{Duration, Utc};
use herald_core::domain::points::entities::{CreditType, QuotaEntitlementStatus, QuotaSourceType};
use test_context::test_context;
use uuid::Uuid;

// ============================================================================
// Test 1: Downgrade Takes Effect Next Period (No Immediate Revoke)
// ============================================================================

// User Story: docs/user-stories/points-billing-events.md
// Covers: US-BI-010 场景 1 - 降级下周期生效，不回收当前积分
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_subscription_downgrade_no_immediate_revoke(ctx: &mut SchemaTestContext) {
    // Given
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(&ctx.app_state.pool, &realm_id, "user1@example.com").await;
    let premium_plan_id = Uuid::now_v7();
    let basic_plan_id = Uuid::now_v7();
    let subscription_id = Uuid::now_v7();
    let event_id = generate_test_event_id();
    let period_end = Utc::now() + Duration::days(30);

    // Configure Creem webhook for this realm
    ctx.with_creem_config(&realm_id, None, None, None).await;

    // Setup plan configs for the test
    setup_test_plan_config_with_points(ctx, &realm_id, premium_plan_id, 10000).await;
    setup_test_plan_config_with_points(ctx, &realm_id, basic_plan_id, 5000).await;

    create_points_wallet(ctx, user_id, &realm_id).await;

    // User currently has Premium Plan (10000 points) as a window-quota entitlement
    let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, &realm_id).await;
    grant_quota_entitlement_for_test(
        ctx,
        &realm_id,
        user_id,
        bucket_id,
        CreditType::SubscriptionCredit,
        QuotaSourceType::SubscriptionInitial,
        &subscription_id.to_string(),
        &[(2_592_000, 10000, "period")],
        Utc::now(),
        Some(period_end),
    )
    .await;

    // When: User downgrades to Basic Plan (5000 points)
    // Use the new plan's own external_product_id so the price-aware webhook
    // resolver lands on the basic mapping instead of colliding on the shared
    // prod_test_monthly product.
    let mut event = build_subscription_updated_event_with_product(
        event_id,
        user_id,
        premium_plan_id,
        basic_plan_id,
        &realm_id,
        &format!("prod_test_{}", basic_plan_id),
    );
    event["data"]["object"]["subscriptionId"] = serde_json::json!(subscription_id.to_string());

    let app = ctx.create_unified_test_router();
    let response = send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;

    // Then
    assert_webhook_success(&response);

    // Existing entitlement should remain untouched
    let entitlements =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert_eq!(
        entitlements.len(),
        1,
        "Should not create new entitlement for downgrade"
    );

    let entitlement = &entitlements[0];
    assert_eq!(
        entitlement.status,
        QuotaEntitlementStatus::Active,
        "Status should remain active"
    );
    assert_eq!(
        entitlement.quota_windows.len(),
        1,
        "Should still have one quota window"
    );
    assert_eq!(
        entitlement.quota_windows[0].limit, 10000,
        "Window limit should remain unchanged"
    );
    assert_eq!(
        entitlement.source_type,
        QuotaSourceType::SubscriptionInitial,
        "Source type should remain subscription_initial"
    );
    assert_eq!(
        entitlement.source_id,
        subscription_id.to_string(),
        "Source id should remain unchanged"
    );

    assert_derived_balance(
        ctx,
        user_id,
        &realm_id,
        CreditType::SubscriptionCredit,
        10000,
    )
    .await;
}
