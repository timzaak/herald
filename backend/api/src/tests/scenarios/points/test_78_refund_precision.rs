// =============================================================================
// Test: Refund Precision (Integer Arithmetic)
// =============================================================================
//
// Tests that revoke_topup_proportional_atomic uses integer-only arithmetic.
// Formula: amount_to_revoke = (granted * refund + original / 2) / original,
// then capped at the ledger's remaining balance. (After the distribution-rules
// refactor the ratio is taken against the ORIGINAL grant amount, not the
// remaining balance, so multi-grant sources are revoked independently.)
// Guard: if amount_to_revoke <= 0, skip revocation entirely.
//
// User Story: docs/user-stories/points-billing-events.md
// Covers: US-PO-06 - Integer rounding precision for proportional refund revocation
//
// =============================================================================

use crate::tests::helpers::points_helpers::*;
use crate::tests::helpers::webhook_helpers::*;
use crate::tests::scenarios::points::fixtures::*;
use crate::tests::schema_test_context::SchemaTestContext;
use herald_core::domain::points::entities::{
    CreditLedgerStatus, CreditSourceType, CreditType, RevocationType,
};
use test_context::test_context;
use uuid::Uuid;

// ============================================================================
// Test 1: Non-round ratio produces correct integer result
// ============================================================================

// User Story: docs/user-stories/points-billing-events.md
// Covers: US-PO-06 - Integer arithmetic: (10000*3333 + 5000) / 10000 = 3333
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_refund_non_round_ratio(ctx: &mut SchemaTestContext) {
    // Given
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(&ctx.app_state.pool, &realm_id, "user_rp1@example.com").await;
    let event_id = generate_test_event_id();
    let refund_id = format!("refund_{}", Uuid::now_v7());
    let payment_id = format!("payment_{}", Uuid::now_v7());

    create_points_wallet(ctx, user_id, &realm_id).await;

    ctx.with_creem_config(&realm_id, None, None, None).await;

    // Seed the payment_attempt snapshot (Creem refund resolves the originating
    // attempt by provider_reference) AND a rule-attributed topup ledger
    // mirroring `fulfill_one_time_purchase`, so the refund's
    // `revoke_topup_source_proportional` finds the grant.
    let bucket_id = get_wallet_bucket_id(ctx, &realm_id, user_id).await;
    let (attempt_id, mapping_id, rule_id) =
        create_payment_attempt_snapshot(ctx, &realm_id, user_id, &payment_id, bucket_id, 10000)
            .await;
    let ledger_id = seed_attributed_topup_ledger(
        ctx, &realm_id, user_id, attempt_id, mapping_id, rule_id, bucket_id, 10000, None,
    )
    .await;

    // Consume 3000, remaining 7000
    consume_points_from_ledger(ctx, ledger_id, 3000).await;

    // When: Refund 3333 of original 10000
    // Integer formula (ratio vs original grant): (10000 * 3333 + 10000/2) / 10000
    //   = (33330000 + 5000) / 10000 = 3333, capped at remaining 7000 -> 3333
    let event = build_refund_created_event_with_user(
        event_id,
        refund_id.clone(),
        payment_id.clone(),
        3333,  // refund amount
        10000, // original amount
        &realm_id,
        user_id,
    );

    let app = ctx.create_unified_test_router();
    let response = send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;

    // Then
    assert_webhook_success(&response);

    // Verify ledger: remaining = 7000 - 3333 = 3667, revoked = 3333
    let ledger = get_ledger_by_id(ctx, ledger_id).await;
    assert_eq!(
        ledger.revoked_amount, 3333,
        "revoked_amount must be 3333 per integer formula (ratio vs original grant)"
    );
    assert_eq!(
        ledger.remaining_amount, 3667,
        "remaining must be 7000 - 3333 = 3667"
    );
    assert_eq!(ledger.used_amount, 3000, "used_amount unchanged");

    // Verify revocation record
    let revocations = get_revocation_records(ctx, user_id).await;
    assert_eq!(revocations.len(), 1, "exactly one revocation record");
    assert_eq!(revocations[0].revocation_type, RevocationType::RefundRevoke);
    assert_eq!(revocations[0].revoked_amount, 3333);
    assert_eq!(revocations[0].reference_id, Some(refund_id));
}

// ============================================================================
// Test 2: Tiny refund ratio rounds to zero and is skipped
// ============================================================================

// User Story: docs/user-stories/points-billing-events.md
// Covers: US-PO-06 - amount_to_revoke <= 0 guard skips revocation
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_refund_zero_amount_skipped(ctx: &mut SchemaTestContext) {
    // Given
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(&ctx.app_state.pool, &realm_id, "user_rp2@example.com").await;
    let event_id = generate_test_event_id();
    let refund_id = format!("refund_{}", Uuid::now_v7());
    let payment_id = format!("payment_{}", Uuid::now_v7());

    create_points_wallet(ctx, user_id, &realm_id).await;

    ctx.with_creem_config(&realm_id, None, None, None).await;

    // Grant 100 topup credits (small ledger), no consumption so remaining = 100
    let ledger_id = create_credit_ledger_entry_v2(
        ctx,
        user_id,
        &realm_id,
        CreditType::TopupCredit,
        CreditSourceType::Topup,
        payment_id.clone(),
        100,
        None,
    )
    .await;

    // Seed the payment_attempts snapshot the Creem refund webhook resolves the
    // routing bucket from.
    let bucket_id = get_wallet_bucket_id(ctx, &realm_id, user_id).await;
    create_payment_attempt_snapshot(ctx, &realm_id, user_id, &payment_id, bucket_id, 100000).await;

    // When: Refund 1 of original 100000
    // Integer formula: (100 * 1 + 100000/2) / 100000 = (100 + 50000) / 100000 = 50100 / 100000 = 0
    // amount_to_revoke = 0, which is <= 0, so revocation is skipped
    let event = build_refund_created_event_with_user(
        event_id,
        refund_id.clone(),
        payment_id.clone(),
        1,      // refund amount
        100000, // original amount (very large)
        &realm_id,
        user_id,
    );

    let app = ctx.create_unified_test_router();
    let response = send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;

    // Then
    assert_webhook_success(&response);

    // Verify no revocation record was created
    let revocations = get_revocation_records(ctx, user_id).await;
    assert_eq!(
        revocations.len(),
        0,
        "no revocation record when amount_to_revoke rounds to zero"
    );

    // Verify ledger unchanged: remaining still 100, revoked still 0
    let ledger = get_ledger_by_id(ctx, ledger_id).await;
    assert_eq!(ledger.remaining_amount, 100, "remaining unchanged");
    assert_eq!(ledger.revoked_amount, 0, "nothing revoked");
}

// ============================================================================
// Test 3: Full refund revokes all remaining and sets status to Revoked
// ============================================================================

// User Story: docs/user-stories/points-billing-events.md
// Covers: US-PO-06 - Full refund revokes all remaining credits
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_refund_full_amount_revokes_all_remaining(ctx: &mut SchemaTestContext) {
    // Given
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(&ctx.app_state.pool, &realm_id, "user_rp3@example.com").await;
    let event_id = generate_test_event_id();
    let refund_id = format!("refund_{}", Uuid::now_v7());
    let payment_id = format!("payment_{}", Uuid::now_v7());

    create_points_wallet(ctx, user_id, &realm_id).await;

    ctx.with_creem_config(&realm_id, None, None, None).await;

    // Seed the payment_attempt snapshot (the Creem refund webhook resolves the
    // originating attempt by provider_reference) AND a rule-attributed topup
    // ledger mirroring `fulfill_one_time_purchase` output, so the refund's
    // `revoke_topup_source_proportional(source_id = attempt.id)` finds and
    // revokes the grant. A raw unattributed ledger would be silently skipped.
    let bucket_id = get_wallet_bucket_id(ctx, &realm_id, user_id).await;
    let (attempt_id, mapping_id, rule_id) =
        create_payment_attempt_snapshot(ctx, &realm_id, user_id, &payment_id, bucket_id, 10000)
            .await;
    let ledger_id = seed_attributed_topup_ledger(
        ctx, &realm_id, user_id, attempt_id, mapping_id, rule_id, bucket_id, 10000, None,
    )
    .await;

    // Consume 3000, remaining 7000
    consume_points_from_ledger(ctx, ledger_id, 3000).await;

    // When: Full refund (refund_amount = original_amount = 10000)
    // Integer formula: (7000 * 10000 + 10000/2) / 10000 = (70000000 + 5000) / 10000 = 7000
    let event = build_refund_created_event_with_user(
        event_id,
        refund_id.clone(),
        payment_id.clone(),
        10000, // refund amount (full)
        10000, // original amount
        &realm_id,
        user_id,
    );

    let app = ctx.create_unified_test_router();
    let response = send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;

    // Then
    assert_webhook_success(&response);

    // Verify ledger: revoked = 7000, remaining = 0, status = Revoked
    let ledger = get_ledger_by_id(ctx, ledger_id).await;
    assert_eq!(
        ledger.revoked_amount, 7000,
        "all remaining revoked on full refund"
    );
    assert_eq!(
        ledger.remaining_amount, 0,
        "no remaining after full refund revocation"
    );
    assert_eq!(ledger.used_amount, 3000, "used_amount unchanged");
    assert_eq!(
        ledger.status,
        CreditLedgerStatus::Revoked,
        "ledger status must be Revoked after full remaining revocation"
    );

    // Verify revocation record
    let revocations = get_revocation_records(ctx, user_id).await;
    assert_eq!(revocations.len(), 1, "exactly one revocation record");
    assert_eq!(revocations[0].revocation_type, RevocationType::RefundRevoke);
    assert_eq!(revocations[0].revoked_amount, 7000);
    assert_eq!(revocations[0].reference_id, Some(refund_id));
}

// ============================================================================
// Test 4: Multi-refund half-up residue is swept by the final full refund
// ============================================================================

// User Story: docs/user-stories/points-billing-events.md
// Covers: US-PO-06 - Integer rounding precision for proportional refund revocation
//
// Half-up rounding loses a fraction on each PARTIAL refund when the ratio
// does not divide evenly; the cumulative-full-refund gate must claw back the
// accumulated residue so the total revoked equals the full grant exactly.
//
// Payment 1000 grants 9998 points. Refunds 334/333/333 of the 1000 payment:
// - refund 334: (9998*334 + 500) / 1000 = 3339 (true ratio 3339.33)
// - refund 333: (9998*333 + 500) / 1000 = 3329 (true ratio 3329.33;
//   cumulative 667 < 1000, partial)
// - refund 333: cumulative 334+333+333 = 1000 = original -> full-refund gate
//   opens, revoke target is the whole grant capped at remaining ->
//   sweeps the remaining 3330, total revoked exactly 9998.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_refund_rounding_residue_swept_by_final_full_refund(ctx: &mut SchemaTestContext) {
    // Given
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(&ctx.app_state.pool, &realm_id, "user_rp4@example.com").await;
    let payment_id = format!("payment_{}", Uuid::now_v7());

    create_points_wallet(ctx, user_id, &realm_id).await;

    ctx.with_creem_config(&realm_id, None, None, None).await;

    // Payment 1000 -> granted 9998 points (rule grants a non-round amount).
    let bucket_id = get_wallet_bucket_id(ctx, &realm_id, user_id).await;
    let (attempt_id, mapping_id, rule_id) =
        create_payment_attempt_snapshot(ctx, &realm_id, user_id, &payment_id, bucket_id, 1000)
            .await;
    let ledger_id = seed_attributed_topup_ledger(
        ctx, &realm_id, user_id, attempt_id, mapping_id, rule_id, bucket_id, 9998, None,
    )
    .await;

    let app = ctx.create_unified_test_router();

    // When/Then: refund 334 -> revoke 3339
    let event = build_refund_created_event_with_user(
        generate_test_event_id(),
        format!("refund_{}", Uuid::now_v7()),
        payment_id.clone(),
        334,
        1000,
        &realm_id,
        user_id,
    );
    let response = send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;
    assert_webhook_success(&response);

    let ledger = get_ledger_by_id(ctx, ledger_id).await;
    assert_eq!(
        ledger.revoked_amount, 3339,
        "first partial refund revokes half-up 3339, not the true 3339.33"
    );
    assert_eq!(ledger.remaining_amount, 6659);

    // And: refund 333 -> revoke 3329 (cumulative 667 < 1000, still partial)
    let event = build_refund_created_event_with_user(
        generate_test_event_id(),
        format!("refund_{}", Uuid::now_v7()),
        payment_id.clone(),
        333,
        1000,
        &realm_id,
        user_id,
    );
    let response = send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;
    assert_webhook_success(&response);

    let ledger = get_ledger_by_id(ctx, ledger_id).await;
    assert_eq!(
        ledger.revoked_amount, 6668,
        "second partial refund adds half-up 3329 (3339 + 3329)"
    );
    assert_eq!(ledger.remaining_amount, 3330);

    // And: final refund 333 brings cumulative to 1000 = original -> the
    // full-refund gate sweeps ALL remaining, absorbing the two lost fractions.
    let event = build_refund_created_event_with_user(
        generate_test_event_id(),
        format!("refund_{}", Uuid::now_v7()),
        payment_id.clone(),
        333,
        1000,
        &realm_id,
        user_id,
    );
    let response = send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;
    assert_webhook_success(&response);

    let ledger = get_ledger_by_id(ctx, ledger_id).await;
    assert_eq!(
        ledger.revoked_amount, 9998,
        "final full refund sweeps the rounding residue: total revoked equals the full grant"
    );
    assert_eq!(ledger.remaining_amount, 0);
    assert_eq!(
        ledger.status,
        CreditLedgerStatus::Revoked,
        "ledger status must be Revoked after the full-refund sweep"
    );

    // Three distinct refund ids -> exactly three revocation records.
    let revocations = get_revocation_records(ctx, user_id).await;
    assert_eq!(revocations.len(), 3, "one revocation record per refund");
    for record in &revocations {
        assert_eq!(record.revocation_type, RevocationType::RefundRevoke);
    }
}
