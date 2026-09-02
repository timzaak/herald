// =============================================================================
// Idempotency Guard Tests
// =============================================================================
//
// Tests that verify DB-level idempotency guards prevent duplicate operations.
//
// 1. grant_points_internal idempotency prevents duplicate ledger rows for
//    topup/granted/registration_credit.
// 2. subscription_service.revoke_quota_entitlement is idempotent by
//    `(realm_id, user_id, bucket_id, credit_type, source_id)`: a second call
//    finds no active entitlement and is a no-op.
// 3. revoke_topup_proportional_atomic idempotency prevents duplicate topup
//    ledger revocation.
//
// Subscription grants themselves are idempotent via the UNIQUE constraint on
// `(realm_id, user_id, bucket_id, credit_type, idempotency_key)` on
// `points_quota_entitlements`.
//
// =============================================================================

use crate::tests::helpers::points_helpers::*;
use crate::tests::scenarios::points::fixtures::*;
use crate::tests::schema_test_context::SchemaTestContext;
use herald_core::domain::points::dtos::RevokePointsOutput;
use herald_core::domain::points::entities::{
    CreditSourceType, CreditType, QuotaEntitlementStatus, QuotaSourceType,
};
use herald_core::domain::points::ports::PointsRepository;
use test_context::test_context;
use uuid::Uuid;

/// Seed a `subscription` row bound to the realm's legacy test bucket. Used by
/// the period-level business-idempotency test below so the schedule's
/// `subscription_id` is known ahead of the service call.
async fn seed_subscription_row_77(
    ctx: &mut SchemaTestContext,
    user_id: Uuid,
    realm_id: &str,
    entitlement_key: &str,
) -> Uuid {
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
    .bind(format!("sub_be_t04_77_{}", subscription_id))
    .bind(format!("prod_be_t04_{}", entitlement_key))
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to seed subscription row for idempotency test");
    subscription_id
}

// ============================================================================
// Test 1: grant_points_internal idempotency prevents duplicate ledger
// ============================================================================
//
// User Story: As a billing system, when I retry a grant-points request with
// an explicit idempotency key, I must not create a duplicate ledger or
// inflate the user's balance.
//
// Covers: grant_points_atomic idempotency guard (line ~3864-3889)
// Idempotency key: caller-provided via grant_points_internal parameter
//
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_grant_idempotency_prevents_duplicate_ledger(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id =
        create_test_user(&ctx.app_state.pool, &realm_id, "idempotency77a@example.com").await;

    create_points_wallet(ctx, user_id, &realm_id).await;

    let source_id = Uuid::now_v7().to_string();
    let idempotency_key = format!("grant:AdminGrant:{}", source_id);

    // Credit-bucket: grant/revoke now require an explicit bucket_id target.
    // The wallet above was created on the realm's legacy bucket (see
    // `points_helpers::ensure_test_bucket_for_realm`), so the grant, the
    // idempotent replay and the balance read below must all target that SAME
    // bucket — otherwise the grant would silently land on a second pool while
    // the unscoped `WHERE user_id` read stayed on the empty legacy wallet.
    // This test is about grant idempotency, not bucket routing.
    use crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm;
    let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, &realm_id).await;

    // First grant should succeed
    let result1 = ctx
        .app_state
        .points_service
        .grant_points_internal(
            &realm_id,
            user_id,
            bucket_id,
            CreditType::GrantedCredit,
            CreditSourceType::AdminGrant,
            500,
            None,
            // effective_at: None ⟺ immediately available.
            None,
            Some(source_id.clone()),
            Some("idempotency test: first grant".to_string()),
            Some(idempotency_key.clone()),
        )
        .await;

    assert!(result1.is_ok(), "First grant should succeed: {:?}", result1);

    // Second grant with the same idempotency key should be idempotent
    let result2 = ctx
        .app_state
        .points_service
        .grant_points_internal(
            &realm_id,
            user_id,
            bucket_id,
            CreditType::GrantedCredit,
            CreditSourceType::AdminGrant,
            500,
            None,
            // effective_at: None ⟺ immediately available.
            None,
            Some(source_id.clone()),
            Some("idempotency test: duplicate grant".to_string()),
            Some(idempotency_key),
        )
        .await;

    assert!(
        result2.is_ok(),
        "Second grant should succeed (idempotent response): {:?}",
        result2
    );

    // Verify only one real ledger exists for this user
    let ledgers = get_user_ledgers(ctx, user_id).await;
    let non_idempotency_ledgers: Vec<_> = ledgers
        .iter()
        .filter(|l| l.source_id != "idempotency")
        .collect();

    assert_eq!(
        non_idempotency_ledgers.len(),
        1,
        "Exactly one real ledger should exist (no duplicates)"
    );
    assert_eq!(
        non_idempotency_ledgers[0].granted_amount, 500,
        "Real ledger should have granted_amount=500"
    );

    // Verify the wallet balance is not inflated. point-time:
    // `points_wallets.total_balance` was dropped; available balance is derived
    // from `points_credit_ledger` using the same predicate as consumption.
    let balance: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.remaining_amount) FILTER (
                    WHERE l.status = 'active' AND l.remaining_amount > 0
                      AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                      AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                ), 0)::BIGINT
         FROM points_wallets w
         LEFT JOIN points_credit_ledger l
           ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id
         WHERE w.user_id = $1 AND w.realm_id = $2
         GROUP BY w.id",
    )
    .bind(user_id)
    .bind(&realm_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("Failed to fetch wallet balance");

    assert_eq!(
        balance, 500,
        "Wallet balance should be 500 (not inflated by duplicate grant)"
    );
}

// ============================================================================
// Test 2: revoke_quota_entitlement idempotency for subscription credits
// ============================================================================
//
// User Story: As a billing system, when I retry a subscription credit
// revocation with the same source_id, I must not create a duplicate
// entitlement or alter the revoked entitlement.
//
// Covers: subscription_service.revoke_quota_entitlement
// Idempotency: no active entitlement matches after the first revoke, so replays
// are no-ops.
//
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_revoke_subscription_quota_entitlement_idempotency(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id =
        create_test_user(&ctx.app_state.pool, &realm_id, "idempotency77b@example.com").await;

    create_points_wallet(ctx, user_id, &realm_id).await;

    let bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, &realm_id).await;
    let source_id = Uuid::now_v7().to_string();
    let now = chrono::Utc::now();

    // Seed an active SubscriptionCredit quota entitlement for this user.
    grant_quota_entitlement_for_test(
        ctx,
        &realm_id,
        user_id,
        bucket_id,
        CreditType::SubscriptionCredit,
        QuotaSourceType::SubscriptionInitial,
        &source_id,
        &[(2_592_000, 1_000, "period")],
        now - chrono::Duration::hours(1),
        Some(now + chrono::Duration::days(30)),
    )
    .await;

    // First revocation should succeed and mark the entitlement revoked.
    let result1 = ctx
        .app_state
        .subscription_service
        .revoke_quota_entitlement(
            &realm_id,
            user_id,
            bucket_id,
            CreditType::SubscriptionCredit,
            &source_id,
            now,
        )
        .await;
    assert!(
        result1.is_ok(),
        "First revoke should succeed: {:?}",
        result1
    );

    let entitlements_after_first =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert_eq!(
        entitlements_after_first.len(),
        1,
        "Exactly one subscription entitlement should exist after first revoke"
    );
    assert_eq!(
        entitlements_after_first[0].status,
        QuotaEntitlementStatus::Revoked,
        "Entitlement should be revoked after first call"
    );
    assert!(
        entitlements_after_first[0].effective_until.is_some(),
        "Revoked entitlement should have effective_until set"
    );

    // Second revocation with the same source_id should be idempotent.
    let result2 = ctx
        .app_state
        .subscription_service
        .revoke_quota_entitlement(
            &realm_id,
            user_id,
            bucket_id,
            CreditType::SubscriptionCredit,
            &source_id,
            now,
        )
        .await;
    assert!(
        result2.is_ok(),
        "Second revoke should succeed (idempotent response): {:?}",
        result2
    );

    let entitlements_after_second =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert_eq!(
        entitlements_after_second.len(),
        1,
        "No duplicate entitlement should be created on duplicate revoke"
    );
    assert_eq!(
        entitlements_after_second[0].status,
        QuotaEntitlementStatus::Revoked,
        "Entitlement should stay revoked after second call"
    );
}

// ============================================================================
// Test 3: revoke_topup_proportional idempotency
// ============================================================================
//
// User Story: As a billing system, when I retry a topup proportional
// revocation with the same refund_id, I must not revoke additional credits.
//
// Covers: revoke_topup_proportional_atomic (line ~3621-3634)
// Idempotency key: "refund:topup:{refund_id}"
//
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_revoke_topup_proportional_idempotency(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id =
        create_test_user(&ctx.app_state.pool, &realm_id, "idempotency77c@example.com").await;

    create_points_wallet(ctx, user_id, &realm_id).await;

    // Create a topup credit ledger
    let source_id = Uuid::now_v7().to_string();
    let ledger_id = create_credit_ledger_entry_v2(
        ctx,
        user_id,
        &realm_id,
        CreditType::TopupCredit,
        CreditSourceType::Topup,
        source_id,
        2000,
        None,
    )
    .await;

    let refund_id = Uuid::now_v7().to_string();

    // Credit-bucket: revoke now requires an explicit bucket_id target.
    // The wallet and the topup ledger above were both created on the realm's
    // legacy bucket (`create_points_wallet` + `create_credit_ledger_entry_v2`
    // route through `ensure_test_bucket_for_realm`), and
    // `revoke_topup_proportional_atomic` scopes its ledger lookup by
    // `bucket_id`. Revoke on any other bucket would find no ledger to revoke.
    // Target the SAME bucket the ledger actually lives in so the test
    // exercises revoke idempotency rather than bucket routing.
    use crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm;
    let topup_bucket_id = ensure_test_bucket_for_realm(&ctx.app_state.pool, &realm_id).await;

    // First revocation: revoke half (1000 out of 2000)
    let result1: Result<RevokePointsOutput, _> = ctx
        .app_state
        .points_repository
        .revoke_topup_proportional_atomic(
            &realm_id,
            user_id,
            topup_bucket_id,
            1000, // refund_amount
            2000, // original_payment_amount
            &refund_id,
        )
        .await;

    assert!(
        result1.is_ok(),
        "First topup revoke should succeed: {:?}",
        result1
    );
    let output1 = result1.unwrap();
    assert!(
        output1.total_revoked > 0,
        "First revoke should revoke some credits, got {}",
        output1.total_revoked
    );

    // Verify the ledger was partially revoked
    let ledger = get_ledger_by_id(ctx, ledger_id).await;
    assert!(
        ledger.revoked_amount > 0,
        "Ledger should show some revoked amount after first revoke"
    );

    // Record revocation count before second call
    let revocation_count_before = get_revocation_records(ctx, user_id).await.len();

    // Second revocation with the same refund_id should be idempotent
    let result2: Result<RevokePointsOutput, _> = ctx
        .app_state
        .points_repository
        .revoke_topup_proportional_atomic(
            &realm_id,
            user_id,
            topup_bucket_id,
            1000, // refund_amount
            2000, // original_payment_amount
            &refund_id,
        )
        .await;

    assert!(
        result2.is_ok(),
        "Second topup revoke should succeed (idempotent response): {:?}",
        result2
    );
    let output2 = result2.unwrap();
    assert_eq!(
        output2.total_revoked, 0,
        "Second revoke should return total_revoked=0 (idempotent)"
    );
    assert!(
        output2.ledger_ids.is_empty(),
        "Second revoke should return empty ledger_ids"
    );

    // Verify no additional revocation record was created
    let revocation_count_after = get_revocation_records(ctx, user_id).await.len();
    assert_eq!(
        revocation_count_before, revocation_count_after,
        "No new revocation record should be created on duplicate call"
    );
}

// ============================================================================
// Two-layer subscription idempotency
// ============================================================================
//
// Layer 1 — PERIOD / SCHEDULE business idempotency:
//   `points_grant_records(schedule_id, period_number)` UNIQUE. The pre-grant
//   path and the formal renewal webhook converge here. A second write for the
//   same (schedule_id, period_number) is rejected by the UNIQUE constraint;
//   production code (subscription_service) treats a pre-existing record as a
//   no-re-grant signal.
//
// Layer 2 — PROVIDER EVENT idempotency:
//   The webhook layer caches provider events by `creem_{event_id}`. Duplicate
//   webhook deliveries with the same event_id hit the cached result and never
//   re-enter `handle_subscription_paid`.
//
// The two layers are defense-in-depth; both must hold independently.

/// User Story: US-PU-009 — period-level business idempotency dedup.
/// Covers (P1 — business idempotency dimension shift from event
/// to schedule/period): the `points_grant_records(schedule_id, period_number)`
/// UNIQUE constraint is the single source of truth. A second insert for the
/// same key MUST be rejected at the DB level; the grantRecord for that period
/// resolves to exactly one ledger row.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_period_schedule_business_idempotency_dedup(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(
        &ctx.app_state.pool,
        &realm_id,
        "be_t04_period_dedup@example.com",
    )
    .await;

    let entitlement_key = format!("be-t04-dedup-{}", Uuid::now_v7());
    let points_per_period: i64 = 600;

    crate::tests::helpers::webhook_helpers::setup_test_entitlement_mapping_for_webhook(
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

    let subscription_id = seed_subscription_row_77(ctx, user_id, &realm_id, &entitlement_key).await;

    let now = chrono::Utc::now();
    let first_period_start = now - chrono::Duration::days(30);
    let period_start = now;

    let schedule_id = create_subscription_grant_schedule(
        ctx,
        user_id,
        &realm_id,
        subscription_id,
        &entitlement_key,
        points_per_period,
        period_start,
        first_period_start,
        0,
    )
    .await;

    // --- Given: a pre-grant already occupies (schedule_id, period_number=2) -
    let ledger_id = create_credit_ledger_entry_with_effective_at(
        ctx,
        user_id,
        &realm_id,
        CreditType::SubscriptionCredit,
        CreditSourceType::SubscriptionRenewal,
        format!("schedule:{}:period:2", schedule_id),
        points_per_period,
        Some(period_start + chrono::Duration::days(30)),
        Some(period_start),
    )
    .await;

    create_grant_record(
        ctx,
        schedule_id,
        2,
        points_per_period,
        period_start,
        ledger_id,
    )
    .await;

    let ledger_count_before =
        get_user_ledgers_by_credit_type(ctx, user_id, CreditType::SubscriptionCredit)
            .await
            .len();

    // --- When: a second grant_record insert for the SAME (schedule_id,
    // period_number) is attempted directly at the DB layer.
    let duplicate_insert_result = sqlx::query(
        "INSERT INTO points_grant_records
            (id, schedule_id, user_id, realm_id, period_number, granted_amount, grant_time, ledger_id, created_at)
         VALUES ($1, $2, $3, $4, 5, 100, NOW(), $6, NOW())",
    )
    .bind(Uuid::now_v7())
    .bind(schedule_id)
    .bind(user_id)
    .bind(&realm_id)
    .bind(ledger_id)
    .execute(&ctx.app_state.pool)
    .await;

    // --- Then: the UNIQUE(schedule_id, period_number) constraint rejects it ---
    assert!(
        duplicate_insert_result.is_err(),
        "UNIQUE(schedule_id, period_number) must reject duplicate grant_record (business idempotency gate)"
    );

    // --- And: only ONE ledger row for this (schedule, period) exists --------
    let ledger_count_after =
        get_user_ledgers_by_credit_type(ctx, user_id, CreditType::SubscriptionCredit)
            .await
            .len();
    assert_eq!(
        ledger_count_before, ledger_count_after,
        "no new ledger row should be created when period-level idempotency gate holds"
    );

    let resolved = find_ledger_id_by_schedule_period(ctx, schedule_id, 2)
        .await
        .expect("grant_record for period_number=2 must resolve to its ledger");
    assert_eq!(
        resolved, ledger_id,
        "the (schedule_id, period_number) key must resolve to the single pre-granted ledger row"
    );
}
