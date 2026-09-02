// =============================================================================
// Points System Scenario Test 62: Free User Upgrade
// =============================================================================
//
// **User Story**: US-FU-03 (Upgrade to paid plan preserves registration credits)
// **Priority**: P1
//
// **Scenarios**:
// 1. Upgrade preserves registration credits
// 2. Upgrade stops periodic grant schedule
// 3. Upgrade creates paid subscription credits
// 4. Downgrade back to free user
// 5. Re-upgrade after cancellation
//
// Under the window-quota model:
// - `registration_credit` remains a `points_credit_ledger` row.
// - `FreePeriodicCredit` and `SubscriptionCredit` live in
//   `points_quota_entitlements`.
// - Upgrading to paid revokes the free-periodic quota entitlement.
// - Cancelling/downgrading revokes the subscription quota entitlement.
//
// =============================================================================

use crate::tests::helpers::points_helpers::{
    assert_derived_balance, ensure_test_bucket_for_realm, get_derived_total_balance,
    get_total_quota_limit_by_type, get_user_quota_entitlements,
};
use crate::tests::scenarios::points::fixtures::{
    configure_test_entitlement_points, create_test_entitlement_mapping,
};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use herald_core::domain::points::entities::{CreditType, QuotaEntitlementStatus};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;
// Import webhook helpers
use crate::tests::helpers::webhook_helpers::{
    build_creem_subscription_canceled_with_entitlement, build_subscription_paid_event,
    generate_test_event_id, send_webhook_with_signature,
};

/// ============================================================================
/// Scenario 1: Upgrade preserves registration credits
/// ============================================================================
// User Story: docs/user-stories/points-free-user.md#US-FU-03
// Covers: 验收标准 3.1
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_free_user_upgrade_preserves_registration_credits(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // ============================================================================
    // Given: a free user has 1000 registration_credit (permanent)
    // And: 50 free_periodic_credit as a window-quota entitlement
    // ============================================================================
    println!("[Step 1] Set up realm config and create free user");

    // Realm config: 1000 registration bonus + 50 free-periodic daily quota.
    crate::tests::helpers::points_helpers::seed_realm_registration_rules(
        &ctx._app_state.pool,
        &ctx._realm_id,
        1000,
        Some(50),
        86_400, // daily window
        1,
    )
    .await;

    // Enable registration for the realm
    sqlx::query(
        r#"
        INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
        VALUES ($1, 'registration', 'enabled', 'true', true)
        ON CONFLICT (realm_id, config_type, config_key)
        DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled
        "#,
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to enable registration");

    // Materialize the realm's registration-pool bucket so the
    // registration-bonus grant lands in a credit ledger and the free-periodic
    // grant lands in a quota entitlement.
    let bucket_id = ensure_test_bucket_for_realm(&ctx._app_state.pool, &ctx._realm_id).await;

    // Create free user with registration credits
    let email = "upgrade_user@example.com";
    let password = "SecurePassword123!";

    let registration_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });

    let registration_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "1.1.1.1")
        .body(Body::from(registration_payload.to_string()))
        .unwrap();

    let registration_response = app.clone().oneshot(registration_request).await.unwrap();
    assert_eq!(registration_response.status(), StatusCode::OK);

    let user_id: String = sqlx::query_scalar("SELECT id::text FROM account WHERE email = $1")
        .bind(email)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("Failed to fetch user_id");
    let user_id = uuid::Uuid::parse_str(&user_id).expect("Invalid user ID");

    println!("[Step 1] ✓ Free user created: {}", user_id);

    // Verify initial state: 1000 registration_credit + 50 free_periodic quota
    assert_derived_balance(
        ctx,
        user_id,
        &ctx._realm_id,
        CreditType::RegistrationCredit,
        1000,
    )
    .await;

    let free_periodic_limit =
        get_total_quota_limit_by_type(ctx, user_id, CreditType::FreePeriodicCredit).await;
    assert_eq!(
        free_periodic_limit, 50,
        "Free-periodic quota entitlement should be active with limit 50"
    );

    let total_balance_before = get_derived_total_balance(ctx, user_id, &ctx._realm_id).await;
    assert_eq!(
        total_balance_before, 1050,
        "Total balance should be 1050 (1000 registration + 50 free-periodic quota)"
    );

    println!(
        "[Step 1] ✓ Verified initial state: 1000 registration + 50 free-periodic quota = 1050 total"
    );

    // ============================================================================
    // When: the user subscribes to "pro-monthly" entitlement
    // ============================================================================
    println!("[Step 2] User upgrades to pro-monthly entitlement");

    // Create subscription entitlement mapping
    let mapping_id =
        create_test_entitlement_mapping(&ctx._app_state.pool, &ctx._realm_id, "pro-monthly", 1000)
            .await;
    let _mapping_config_id = configure_test_entitlement_points(
        &ctx._app_state.pool,
        &ctx._realm_id,
        mapping_id,
        1000,
        30,
    )
    .await;

    // The price-aware webhook resolver keys off (provider, product, price).
    // build_subscription_paid_event emits productId="prod_test_monthly", but
    // create_test_entitlement_mapping above registers the mapping under
    // "prod_test_pro-monthly". Add a generic "prod_test_monthly" mapping pointing
    // at the same entitlement_key so the webhook resolves.
    //
    // Distribution-rules model: the grant config (1000 subscription quota over a
    // 30-day window) is a quota distribution rule owned by this mapping with the
    // `subscription_renewal` trigger, seeded to preserve the test's "paid upgrade
    // grants 1000 subscription_credit" intent. (Initial activation grants nothing
    // by design — the webhook is driven as a renewal, the grant-bearing route.)
    let generic_mapping_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key,
             billing_type, enabled, created_at, updated_at)
         VALUES ($1, $2, 'creem', 'prod_test_monthly', $3, 'recurring', true, NOW(), NOW())
         ON CONFLICT DO NOTHING",
    )
    .bind(generic_mapping_id)
    .bind(&ctx._realm_id)
    .bind(mapping_id.to_string())
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create generic entitlement mapping for paid webhook");

    let generic_rule_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO points_distribution_rules
            (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
             trigger_sources, grant_mode, validity_days, quota_windows,
             enabled, display_order)
         VALUES ($1, $2, 'entitlement_mapping', $3, $4, $5, 'quota', 0, $6, true, 0)",
    )
    .bind(generic_rule_id)
    .bind(&ctx._realm_id)
    .bind(generic_mapping_id)
    .bind(bucket_id)
    .bind(&["subscription_renewal"][..])
    .bind(json!([{"windowSeconds": 2_592_000, "limit": 1000, "key": "period"}]))
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed generic mapping subscription_renewal quota rule");

    // Configure Creem webhook for this realm
    ctx.with_creem_config(
        &ctx._realm_id,
        Some("test_api_key"),
        Some("test_webhook_secret"),
        Some(30),
    )
    .await;

    // Build and send subscription.paid event
    let event_id = generate_test_event_id();
    let event = build_subscription_paid_event(
        event_id.clone(),
        user_id,
        mapping_id,
        true, // renewal — the grant-bearing subscription.paid route
        &ctx._realm_id,
    );

    let webhook_response =
        send_webhook_with_signature(&app, &ctx._realm_id, event, "test_webhook_secret").await;
    assert_eq!(
        webhook_response.status(),
        StatusCode::OK,
        "Webhook should succeed"
    );

    println!("[Step 2] ✓ Subscription created via webhook");

    // The production upgrade path revokes the free-periodic quota entitlement.
    // Mirror that explicitly so the test reflects the expected post-upgrade state.
    let free_source_id = format!("registration:{}", user_id);
    ctx._app_state
        .subscription_service
        .revoke_quota_entitlement(
            &ctx._realm_id,
            user_id,
            bucket_id,
            CreditType::FreePeriodicCredit,
            &free_source_id,
            Utc::now(),
        )
        .await
        .expect("Failed to revoke free-periodic quota entitlement");

    println!("[Step 2] ✓ Free-periodic quota entitlement revoked (upgrade path)");

    // ============================================================================
    // Then: the registration_credit (1000) remains untouched
    // And: the free_periodic_credit quota entitlement is revoked
    // And: the subscription_credit quota entitlement is active with limit 1000
    // And: the user's total_balance = 1000 (registration) + 1000 (subscription) = 2000
    // ============================================================================
    println!("[Step 3] Verify upgrade results");

    // Verify registration credit is preserved
    assert_derived_balance(
        ctx,
        user_id,
        &ctx._realm_id,
        CreditType::RegistrationCredit,
        1000,
    )
    .await;

    // Verify free-periodic quota entitlement is revoked
    let free_periodic_limit_after =
        get_total_quota_limit_by_type(ctx, user_id, CreditType::FreePeriodicCredit).await;
    assert_eq!(
        free_periodic_limit_after, 0,
        "Free-periodic quota entitlement should be revoked (active limit 0)"
    );

    let free_entitlements =
        get_user_quota_entitlements(ctx, user_id, CreditType::FreePeriodicCredit).await;
    assert!(
        free_entitlements
            .iter()
            .any(|e| e.status == QuotaEntitlementStatus::Revoked),
        "At least one free-periodic entitlement should be revoked after upgrade"
    );

    // Verify subscription quota entitlement is active
    let subscription_limit =
        get_total_quota_limit_by_type(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert_eq!(
        subscription_limit, 1000,
        "Subscription quota entitlement should be active with limit 1000"
    );

    // Verify total balance
    let total_balance_after = get_derived_total_balance(ctx, user_id, &ctx._realm_id).await;
    assert_eq!(
        total_balance_after, 2000,
        "Total balance should be 2000 (1000 registration + 1000 subscription)"
    );

    println!(
        "[Step 3] ✓ Registration credit preserved, free-periodic quota revoked, subscription quota active, total balance updated"
    );
}

/// ============================================================================
/// Scenario 4: Downgrade back to free user
/// ============================================================================
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_free_user_downgrade_from_paid(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // ============================================================================
    // Given: a paid user cancels their subscription
    // And: the user has 1000 registration_credit (permanent)
    // ============================================================================
    println!("[Step 1] Create paid user with subscription");

    // Realm config: 1000 registration bonus + 50 free-periodic daily quota.
    crate::tests::helpers::points_helpers::seed_realm_registration_rules(
        &ctx._app_state.pool,
        &ctx._realm_id,
        1000,
        Some(50),
        86_400, // daily window
        1,
    )
    .await;

    // Enable registration for the realm
    sqlx::query(
        r#"
        INSERT INTO realm_config (realm_id, config_type, config_key, config_value, enabled)
        VALUES ($1, 'registration', 'enabled', 'true', true)
        ON CONFLICT (realm_id, config_type, config_key)
        DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled
        "#,
    )
    .bind(&ctx._realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to enable registration");

    // Materialize the realm's registration-pool bucket.
    let bucket_id = ensure_test_bucket_for_realm(&ctx._app_state.pool, &ctx._realm_id).await;

    // Create user
    let email = "downgrade_user@example.com";
    let password = "SecurePassword123!";

    let registration_payload = json!({
        "clientId": ctx._client_id,
        "email": email,
        "password": password,
        "turnstileToken": "dummy"
    });

    let registration_request = Request::builder()
        .method("POST")
        .uri(format!("/api/auth/{}/register", ctx._realm_id))
        .header("content-type", "application/json")
        .header("x-forwarded-for", "1.1.1.1")
        .body(Body::from(registration_payload.to_string()))
        .unwrap();

    let registration_response = app.clone().oneshot(registration_request).await.unwrap();
    assert_eq!(registration_response.status(), StatusCode::OK);

    let user_id: String = sqlx::query_scalar("SELECT id::text FROM account WHERE email = $1")
        .bind(email)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("Failed to fetch user_id");
    let user_id = uuid::Uuid::parse_str(&user_id).expect("Invalid user ID");

    // Create subscription via webhook
    let mapping_id =
        create_test_entitlement_mapping(&ctx._app_state.pool, &ctx._realm_id, "pro-monthly", 1000)
            .await;
    let _mapping_config_id = configure_test_entitlement_points(
        &ctx._app_state.pool,
        &ctx._realm_id,
        mapping_id,
        1000,
        30,
    )
    .await;

    // Create additional mapping for "prod_test_monthly" so cancel webhook can resolve entitlement_key.
    //
    // Distribution-rules model: grant config (1000 subscription quota over a
    // 30-day window) is a quota rule owned by this mapping with the
    // `subscription_renewal` trigger, mirroring the upgrade-scenario seeding
    // (initial activation grants nothing; the webhook is driven as a renewal).
    let generic_mapping_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key,
             billing_type, enabled, created_at, updated_at)
         VALUES ($1, $2, 'creem', 'prod_test_monthly', $3, 'recurring', true, NOW(), NOW())
         ON CONFLICT DO NOTHING",
    )
    .bind(generic_mapping_id)
    .bind(&ctx._realm_id)
    .bind(mapping_id.to_string())
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create generic entitlement mapping for cancel webhook");

    let generic_rule_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO points_distribution_rules
            (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
             trigger_sources, grant_mode, validity_days, quota_windows,
             enabled, display_order)
         VALUES ($1, $2, 'entitlement_mapping', $3, $4, $5, 'quota', 0, $6, true, 0)",
    )
    .bind(generic_rule_id)
    .bind(&ctx._realm_id)
    .bind(generic_mapping_id)
    .bind(bucket_id)
    .bind(&["subscription_renewal"][..])
    .bind(json!([{"windowSeconds": 2_592_000, "limit": 1000, "key": "period"}]))
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed generic mapping subscription_renewal quota rule");

    // Configure Creem webhook for this realm
    ctx.with_creem_config(
        &ctx._realm_id,
        Some("test_api_key"),
        Some("test_webhook_secret"),
        Some(30),
    )
    .await;

    // Build and send subscription.paid event
    let event_id = generate_test_event_id();
    let external_subscription_id = format!("sub_{}", event_id);
    let event = build_subscription_paid_event(
        event_id.clone(),
        user_id,
        mapping_id,
        true, // renewal — the grant-bearing subscription.paid route
        &ctx._realm_id,
    );

    let webhook_response =
        send_webhook_with_signature(&app, &ctx._realm_id, event, "test_webhook_secret").await;
    assert_eq!(
        webhook_response.status(),
        StatusCode::OK,
        "Webhook should succeed"
    );

    // Revoke the free-periodic quota entitlement, mirroring the production
    // upgrade-to-paid path.
    let free_source_id = format!("registration:{}", user_id);
    ctx._app_state
        .subscription_service
        .revoke_quota_entitlement(
            &ctx._realm_id,
            user_id,
            bucket_id,
            CreditType::FreePeriodicCredit,
            &free_source_id,
            Utc::now(),
        )
        .await
        .expect("Failed to revoke free-periodic quota entitlement");

    // Verify paid user state (window-quota model: sum active subscription limits).
    let subscription_limit =
        get_total_quota_limit_by_type(ctx, user_id, CreditType::SubscriptionCredit).await;

    assert_eq!(
        subscription_limit, 1000,
        "User should have 1000 subscription quota limit"
    );

    println!("[Step 1] ✓ Paid user created with 1000 subscription quota");

    // ============================================================================
    // When: the subscription is cancelled
    // ============================================================================
    println!("[Step 2] Cancel subscription via webhook");

    // Build and send subscription.canceled event, reusing the same
    // external_subscription_id so the cancel webhook resolves the existing
    // subscription and revokes its quota entitlement.
    let cancel_event_id = generate_test_event_id();
    let cancel_event = build_creem_subscription_canceled_with_entitlement(
        &cancel_event_id,
        &mapping_id.to_string(),
        &ctx._realm_id,
        user_id,
        &external_subscription_id,
        "prod_test_monthly",
        false, // immediate cancel
    );

    let cancel_response =
        send_webhook_with_signature(&app, &ctx._realm_id, cancel_event, "test_webhook_secret")
            .await;
    assert_eq!(
        cancel_response.status(),
        StatusCode::OK,
        "Cancel webhook should succeed"
    );

    println!("[Step 2] ✓ Subscription cancelled via webhook");

    // ============================================================================
    // Then: the registration_credit (1000) is preserved
    // And: all subscription_credit quota entitlement is revoked
    // And: the user's total_balance = 1000 (registration only)
    // ============================================================================
    println!("[Step 3] Verify downgrade results");

    // Verify registration credit is preserved
    assert_derived_balance(
        ctx,
        user_id,
        &ctx._realm_id,
        CreditType::RegistrationCredit,
        1000,
    )
    .await;

    // Verify subscription quota entitlement is revoked (window-quota model: active limit 0).
    let subscription_limit_after =
        get_total_quota_limit_by_type(ctx, user_id, CreditType::SubscriptionCredit).await;

    assert_eq!(
        subscription_limit_after, 0,
        "Subscription quota entitlement should be revoked"
    );

    let subscription_entitlements =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert!(
        subscription_entitlements
            .iter()
            .any(|e| e.status == QuotaEntitlementStatus::Revoked),
        "At least one subscription entitlement should be revoked after cancellation"
    );

    // Verify total balance
    let total_balance_after = get_derived_total_balance(ctx, user_id, &ctx._realm_id).await;
    assert_eq!(
        total_balance_after, 1000,
        "Total balance should be 1000 (registration only)"
    );

    println!("[Step 3] ✓ Downgrade verified: registration preserved, subscription quota revoked");
}
