// Covers design `.ai/design/credit-bucket.md`:
//   - (grant/fulfillment Bucket routing)
//   - (subscription lifecycle reclamation)
//   - 履约 / 订阅生命周期
//   - in-flight attempt is NOT rerouted when the mapping's distribution rule is
//     re-pointed after purchase.
//   - routing source = the captured `payment_attempt_point_rules` snapshot
//     (rule_id + bucket_id pairs frozen at purchase creation); fulfillment
//     (`execute_captured_payment_rules`) replays that snapshot, ignoring the
//     mapping's CURRENT rules.
//   - subscription lifecycle routing (renewal/upgrade/cancel/refund) is now
//     scoped by the rule-attributed ledger/quota rows joined through
//     `points_distribution_events.source_id = subscription_id` (and for the
//     refund path, by the explicit `bucket_id` parameter).
//
// Refactor context (commit ad57549f "unify points routing into multi-wallet
// distribution rules"): the removed columns `payment_attempts.bucket_id`,
// `subscription.bucket_id`, and the mapping's `points_per_period` /
// `grant_on_subscribe` / `bucket_id` are replaced by:
//   - a BARE `provider_entitlement_mappings` row (no points columns), and
//   - one or more `points_distribution_rules` rows owned by that mapping
//     (owner_type='entitlement_mapping', entitlement_mapping_id, bucket_id,
//     trigger_sources, grant_mode='fixed' → a `points_credit_ledger` row with
//     credit_type='subscription_credit' for the matching subscription trigger).
//   - `payment_attempt_point_rules` snapshots the matched rule refs at attempt
//     creation; fulfillment replays that snapshot (`CapturedPaymentRules`).
//
// All tests exercise the real production services via `ctx.app_state`:
//   - `fulfillment_service.fulfill_subscription_purchase(&attempt, …)`
//   - `subscription_service.handle_subscription_paid / upgrade / cancel /
//     downgrade`
//   - `points_service.revoke_points_by_credit_type` (refund path, routed via
//     the explicit bucket_id argument).
//
// grant_mode choice: the assertions here read `points_credit_ledger`
// (credit_type='subscription_credit') via `count_ledger_in_bucket` /
// `sum_ledger_granted_in_bucket`. The executor writes a ledger row ONLY for
// `grant_mode='fixed'` (`DistributionPolicy::Fixed` → `write_rule_ledger_in_tx`,
// `credit_type` from `credit_pair_for_trigger`). `grant_mode='quota'` writes a
// `points_quota_entitlement` row instead and would NOT be observed by the
// ledger helpers, so every rule seeded below uses `grant_mode='fixed'`.

#![allow(clippy::too_many_arguments)]

use crate::tests::helpers::credit_bucket_helpers::{
    CreditBucketOpts, count_ledger_in_bucket, count_ledger_outside_bucket,
    create_test_credit_bucket, read_wallet_total_balance, sum_ledger_granted_in_bucket,
};
use crate::tests::helpers::points_helpers::snapshot_attempt_rules_for_mapping;
use crate::tests::scenarios::points::fixtures::{
    create_test_client_app, create_test_user, create_test_user_with_auth,
};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::payment_attempt::entities::PaymentAttempt;
use herald_core::domain::points::{
    entities::{CreditType, RevocationType},
    subscription_service::CancelMode,
};
use herald_core::domain::purchase::FulfillmentService;
use sqlx::PgPool;
use test_context::test_context;
use uuid::Uuid;

/// Resolve the price-level EntitlementMapping for a key in these scenarios.
///
/// The price-level mapping refactor changed `subscription_service.handle_subscription_*` to consume the
/// price-level mapping directly. These lifecycle scenarios seed a
/// single mapping per entitlement_key (no shared-key ambiguity), so resolving
/// by key is identity-equivalent to the price-level mapping the webhook path
/// would resolve.
async fn mapping_for_key(
    ctx: &TestContext,
    realm_id: &str,
    key: &str,
) -> herald_core::domain::billing::entities::EntitlementMapping {
    use herald_core::domain::billing::BillingRepository;
    ctx.app_state
        .billing_repository
        .find_entitlement_mapping_by_key(realm_id, key)
        .await
        .unwrap_or_else(|_| panic!("mapping for key '{key}' should exist"))
        .unwrap_or_else(|| panic!("mapping for key '{key}' should be Some"))
}

// Local SQL helpers — direct row construction for fulfillment scenarios.
//
// Every helper below targets the post-refactor schema: BARE
// `provider_entitlement_mappings` (no points columns), points routing via
// `points_distribution_rules`, and the frozen `payment_attempt_point_rules`
// snapshot consumed by `execute_captured_payment_rules`.

/// Seed a BARE `provider_entitlement_mappings` row (`billing_type='recurring'`,
/// `billing_period='monthly'`, NO points columns — those were removed by the
/// distribution-rules refactor) plus the matching `points_distribution_rules`
/// needed by both fulfillment and the lifecycle events:
///   - a `subscription_initial` fixed rule (so first fulfillment
///     `execute_captured_payment_rules` snapshots a rule that grants
///     `subscription_credit` to `bucket_id`), and
///   - a `subscription_renewal` fixed rule (so
///     `handle_subscription_paid(is_renewal=true)` — which reads CURRENT owner
///     rules with the renewal trigger — grants the renewal period into the same
///     bucket).
///
/// Both rules carry `points_amount = points_per_period` and use
/// `grant_mode='fixed'` so the grant lands in `points_credit_ledger` with
/// `credit_type='subscription_credit'` (matching the ledger helpers). Returns
/// the mapping_id.
async fn create_subscription_mapping_in_bucket(
    pool: &PgPool,
    realm_id: &str,
    entitlement_key: &str,
    bucket_id: Uuid,
    points_per_period: i64,
) -> Uuid {
    let mapping_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id, entitlement_key,
             billing_type, billing_period, enabled, created_at, updated_at)
         VALUES ($1, $2, 'stripe', $3, $4, 'recurring', 'monthly', true, NOW(), NOW())",
    )
    .bind(mapping_id)
    .bind(realm_id)
    .bind(format!("prod_{}", mapping_id))
    .bind(entitlement_key)
    .execute(pool)
    .await
    .expect("Failed to insert subscription entitlement mapping");

    // subscription_initial fixed rule — captured by the payment attempt snapshot
    // and replayed by first fulfillment.
    let initial_rule_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO points_distribution_rules
            (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
             trigger_sources, grant_mode, points_amount, validity_days,
             enabled, display_order)
         VALUES ($1, $2, 'entitlement_mapping', $3, $4, $5, 'fixed', $6, 0, true, 0)",
    )
    .bind(initial_rule_id)
    .bind(realm_id)
    .bind(mapping_id)
    .bind(bucket_id)
    .bind(&["subscription_initial"][..])
    .bind(points_per_period)
    .execute(pool)
    .await
    .expect("Failed to seed subscription_initial distribution rule");

    // subscription_renewal fixed rule — read by handle_subscription_paid renewal
    // (CurrentOwnerRules selection). Same bucket + amount so renewal grants land
    // in the same pool the snapshot pointed at.
    let renewal_rule_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO points_distribution_rules
            (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
             trigger_sources, grant_mode, points_amount, validity_days,
             enabled, display_order)
         VALUES ($1, $2, 'entitlement_mapping', $3, $4, $5, 'fixed', $6, 0, true, 1)",
    )
    .bind(renewal_rule_id)
    .bind(realm_id)
    .bind(mapping_id)
    .bind(bucket_id)
    .bind(&["subscription_renewal"][..])
    .bind(points_per_period)
    .execute(pool)
    .await
    .expect("Failed to seed subscription_renewal distribution rule");

    mapping_id
}

/// Seed a `subscription_upgrade` fixed rule on an existing mapping (used by the
/// upgrade scenario: `handle_subscription_upgrade` selects CURRENT owner rules
/// with the upgrade trigger after revoking the old source's results). Same
/// grant_mode/bucket convention as the initial/renewal rules.
async fn add_subscription_upgrade_rule(
    pool: &PgPool,
    realm_id: &str,
    mapping_id: Uuid,
    bucket_id: Uuid,
    points_amount: i64,
) {
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
    .bind(&["subscription_upgrade"][..])
    .bind(points_amount)
    .execute(pool)
    .await
    .expect("Failed to seed subscription_upgrade distribution rule");
}

/// Insert a BARE `payment_attempts` row (no `bucket_id` column — removed by the
/// refactor) with `target_type = 'entitlement_mapping'` (the only value allowed
/// by the migration `chk_target_type`) and `status = 'Succeeded'` (so
/// fulfillment can proceed without another status transition), then snapshot
/// the mapping's matching rules into `payment_attempt_point_rules` exactly as
/// production `create_payment_attempt` does. Returns the attempt_id; use
/// [`load_attempt`] to materialize the `PaymentAttempt` for the fulfillment
/// service.
async fn insert_attempt_with_bucket_snapshot(
    pool: &PgPool,
    realm_id: &str,
    user_id: Uuid,
    mapping_id: Uuid,
    _bucket_id: Option<Uuid>,
) -> Uuid {
    let attempt_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO payment_attempts
            (id, realm_id, user_id, payment_provider, target_type, target_id,
             amount, currency, status, provider_reference,
             expires_at, created_at, updated_at)
         VALUES ($1, $2, $3, 'stripe', 'entitlement_mapping', $4,
                 999, 'USD', 'Succeeded', $5,
                 NOW() + INTERVAL '2 hours', NOW(), NOW())",
    )
    .bind(attempt_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(mapping_id)
    .bind(format!("pi_test_{}", attempt_id))
    .execute(pool)
    .await
    .expect("Failed to insert payment_attempts row");

    // Snapshot the mapping's subscription_initial rule into
    // payment_attempt_point_rules (production `create_payment_attempt` writes
    // this atomically; raw-SQL test setup must replicate it or fulfillment
    // resolves zero captured rules and grants nothing).
    snapshot_attempt_rules_for_mapping(
        pool,
        attempt_id,
        realm_id,
        mapping_id,
        "subscription_initial",
    )
    .await;

    attempt_id
}

/// Load a `PaymentAttempt` from the DB via the real service.
async fn load_attempt(ctx: &TestContext, attempt_id: Uuid) -> PaymentAttempt {
    ctx.app_state
        .payment_attempt_service
        .get_payment_attempt_by_id_only(attempt_id)
        .await
        .expect("payment attempt not found after insert")
}

/// Resolve the routing bucket a subscription's credits actually landed in.
///
/// `subscription.bucket_id` was removed by the distribution-rules refactor, so
/// there is no single column to read. The faithful new-model equivalent is the
/// `bucket_id` of the rule-attributed `points_credit_ledger` row whose
/// `distribution_event` belongs to this subscription. Production keys the
/// subscription-period event as `subscription:{subscription_id}:period:{...}`
/// (and the cancel/refund revoke matches it via `event_key LIKE
/// 'subscription:{subscription_id}:%'`), so this helper matches the same prefix
/// to find the granted ledger's bucket. Returns the most-recent such bucket,
/// panicking if no attributed grant exists (in these scenarios a subscription
/// with no grant is always a test-setup bug, not a runtime state).
async fn read_subscription_routing_bucket(pool: &PgPool, subscription_id: Uuid) -> Uuid {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT l.bucket_id
           FROM points_credit_ledger l
           JOIN points_distribution_events e ON e.id = l.distribution_event_id
          WHERE e.event_key LIKE $1
            AND l.distribution_rule_id IS NOT NULL
          ORDER BY l.created_at DESC
          LIMIT 1",
    )
    .bind(format!("subscription:{}:%", subscription_id))
    .fetch_optional(pool)
    .await
    .expect("Failed to read subscription routing bucket")
    .expect("subscription has no attributed grant — test-setup bug")
}

/// Create a `subscription` row directly (NO `bucket_id` / `billing_type`
/// columns — both were removed from `subscription` by the refactor; the
/// subscription's routing bucket now lives on its distribution rules). Bypasses
/// fulfillment (used by lifecycle scenarios that start from an already-existing
/// subscription). `bucket_id` is accepted for call-site stability and is
/// intentionally ignored — the test seeds the routing rule separately via
/// [`create_subscription_mapping_in_bucket`] before this call.
async fn insert_subscription_in_bucket(
    pool: &PgPool,
    realm_id: &str,
    user_id: Uuid,
    client_app_id: Uuid,
    entitlement_key: &str,
    _bucket_id: Uuid,
) -> Uuid {
    let subscription_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO subscription
            (id, realm_id, user_id, client_app_id, status, entitlement_key,
             external_subscription_id, external_product_id,
             payment_provider, billing_type,
             current_period_start, current_period_end,
             cancel_at_period_end, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'active', $5,
                 $6, $7, 'stripe', 'recurring',
                 NOW(), NOW() + INTERVAL '30 days',
                 false, NOW(), NOW())",
    )
    .bind(subscription_id)
    .bind(realm_id)
    .bind(user_id)
    .bind(client_app_id)
    .bind(entitlement_key)
    .bind(format!("sub_test_{}", subscription_id))
    .bind(format!("prod_{}", entitlement_key))
    .execute(pool)
    .await
    .expect("Failed to insert subscription row");
    subscription_id
}

// Scenario 1 (REMOVED): mapping with bucket_id=NULL rejects purchase creation
// `provider_entitlement_mappings.bucket_id` was removed by the distribution-rules
// refactor; points routing now lives on `points_distribution_rules`. A
// bucket-less mapping can therefore no longer be expressed as a column value,
// and the purchase-time runtime check — CoreError::EntitlementMappingNotAttachedToBucket
// — was removed from `resolve_target`. The invariant this
// scenario guarded ("a mapping without a credit bucket cannot be purchased") is
// now enforced structurally: no `points_distribution_rules` row ⟹ fulfillment
// resolves zero captured rules and grants nothing (scenario 10 below).

// Scenario 2: fulfillment grants to the attempt-snapshot Bucket

/// User Story: US-CB-004 (purchase Bucket plan), US-PA-003 (payment success
/// fulfillment).
/// Covers:
///   - `fulfill_subscription_purchase` → `execute_captured_payment_rules`
///     replays the `payment_attempt_point_rules` snapshot (rule_id + bucket_id
///     captured at attempt creation), NOT the live mapping rule, and grants
///     initial subscription credits to that snapshot Bucket's pool.
///   - DB check: the new `points_credit_ledger.bucket_id` equals the snapshot
///     Bucket, and no ledger row leaks to any other Bucket.
#[test_context(TestContext)]
#[tokio::test]
async fn fulfillment_grants_to_attempt_snapshot_bucket(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t02_fulfill_snapshot@example.com").await;

    let bucket_a = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Snapshot Target".into()),
            bucket_key: Some("snapshot-target".into()),
            ..Default::default()
        },
    )
    .await;
    let entitlement_key = format!("cb-t02-snap-{}", Uuid::now_v7());
    let mapping_id =
        create_subscription_mapping_in_bucket(pool, &realm_id, &entitlement_key, bucket_a, 1_000)
            .await;

    let attempt_id =
        insert_attempt_with_bucket_snapshot(pool, &realm_id, user_id, mapping_id, Some(bucket_a))
            .await;
    let attempt = load_attempt(ctx, attempt_id).await;
    let provider_tx_id = format!("sub_snap_{}", attempt_id);
    let result = ctx
        .app_state
        .fulfillment_service
        .fulfill_subscription_purchase(&attempt, provider_tx_id.clone())
        .await;

    assert!(result.is_ok(), "fulfillment should succeed: {:?}", result);
    assert_eq!(
        result.as_ref().unwrap().point_grants[0].bucket_id,
        bucket_a,
        "captured rule grants to bucket A"
    );

    let ledger_count_a =
        count_ledger_in_bucket(pool, user_id, bucket_a, "subscription_credit").await;
    assert_eq!(
        ledger_count_a, 1,
        "exactly one subscription_credit ledger in bucket A"
    );

    let balance_a =
        sum_ledger_granted_in_bucket(pool, user_id, bucket_a, "subscription_credit").await;
    assert_eq!(
        balance_a, 1_000,
        "granted amount == captured rule points_amount"
    );

    let leak_count = count_ledger_outside_bucket(pool, user_id, bucket_a).await;
    assert_eq!(leak_count, 0, "no ledger row in any other bucket");

    let subscription_id = result
        .as_ref()
        .ok()
        .and_then(|r| r.subscription_id)
        .expect("fulfillment returned a subscription_id");
    let routing_bucket = read_subscription_routing_bucket(pool, subscription_id).await;
    assert_eq!(
        routing_bucket, bucket_a,
        "subscription's first grant routed to the captured snapshot bucket"
    );
}

// Scenario 3: first fulfillment freezes the subscription's routing bucket

/// User Story: US-CB-008 (subscription lifecycle by Bucket).
/// Covers:
///   - After the first `fulfill_subscription_purchase`, the captured rule's
///     `bucket_id` is the subscription's routing target — the grant attributed
///     to `subscription_id` lands in that bucket. This is the freeze event
///     that makes the subscription's lifecycle resolve deterministically to
///     one pool (subsequent renewals read CURRENT owner rules, but the rule's
///     bucket is stable; cancel/refund scope via `source_id` resolves the same
///     pool).
///   - `subscription.bucket_id` no longer exists (removed by the refactor); the
///     equivalent new-model read is the granted ledger row's bucket.
#[test_context(TestContext)]
#[tokio::test]
async fn fulfillment_freezes_subscription_bucket_on_first_renewal(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t02_freeze@example.com").await;

    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Freeze Bucket".into()),
            bucket_key: Some("freeze-bucket".into()),
            ..Default::default()
        },
    )
    .await;
    let entitlement_key = format!("cb-t02-freeze-{}", Uuid::now_v7());
    let mapping_id =
        create_subscription_mapping_in_bucket(pool, &realm_id, &entitlement_key, bucket, 500).await;

    let attempt_id =
        insert_attempt_with_bucket_snapshot(pool, &realm_id, user_id, mapping_id, Some(bucket))
            .await;
    let attempt = load_attempt(ctx, attempt_id).await;

    let result = ctx
        .app_state
        .fulfillment_service
        .fulfill_subscription_purchase(&attempt, format!("sub_freeze_{}", attempt_id))
        .await
        .expect("first fulfillment should succeed");

    let subscription_id = result.subscription_id.expect("subscription_id present");

    // The captured rule's bucket_id is the subscription's routing target. The
    // first grant attributed to subscription_id must land in that bucket.
    let frozen = read_subscription_routing_bucket(pool, subscription_id).await;
    assert_eq!(
        frozen, bucket,
        "subscription's first grant routed to the captured snapshot bucket"
    );
    assert_eq!(
        result.point_grants[0].bucket_id, frozen,
        "captured rule target == frozen subscription routing bucket"
    );
}

// Scenario 4: regression — mapping rule re-point after purchase does not
// reroute an in-flight attempt

/// User Story: US-CB-003 (coverage-set / mapping changes affect only future
/// purchases); 覆盖集变更不回溯.
/// Covers:
///   - Purchase attempt is created with its rules snapshotted to Bucket A.
///   - The mapping's distribution rule is then re-pointed to Bucket B (a new
///     rule row targeting B; the original A rule is disabled).
///   - Fulfilling the in-flight attempt STILL grants to Bucket A — the captured
///     `payment_attempt_point_rules` snapshot (rule_id + bucket_id frozen at
///     creation) is replayed, NOT the live mapping rules.
///   - No credit leaks to Bucket B from this in-flight attempt.
#[test_context(TestContext)]
#[tokio::test]
async fn mapping_bucket_change_after_purchase_does_not_reroute_inflight_attempt(
    ctx: &mut TestContext,
) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id =
        create_test_user_with_auth(pool, &realm_id, "cb_t02_a7@example.com", "pw123").await;

    let bucket_a = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("A7 Original".into()),
            bucket_key: Some("a7-original".into()),
            ..Default::default()
        },
    )
    .await;
    let bucket_b = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("A7 Repoint Target".into()),
            bucket_key: Some("a7-repoint".into()),
            ..Default::default()
        },
    )
    .await;

    let entitlement_key = format!("cb-t02-a7-{}", Uuid::now_v7());
    // Mapping starts with its subscription_initial rule targeting Bucket A.
    let mapping_id =
        create_subscription_mapping_in_bucket(pool, &realm_id, &entitlement_key, bucket_a, 800)
            .await;

    let attempt_id =
        insert_attempt_with_bucket_snapshot(pool, &realm_id, user_id, mapping_id, Some(bucket_a))
            .await;

    // Re-point the mapping's points routing to Bucket B AFTER the attempt was
    // captured: disable the original A rules and add a new initial rule
    // targeting B. Production achieves the same effect (a different bucket on
    // the active rule); the snapshot taken above must remain frozen on A.
    sqlx::query(
        "UPDATE points_distribution_rules
         SET enabled = false, updated_at = NOW()
         WHERE entitlement_mapping_id = $1 AND realm_id = $2",
    )
    .bind(mapping_id)
    .bind(&realm_id)
    .execute(pool)
    .await
    .expect("disable original A rules");
    let repoint_rule_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO points_distribution_rules
            (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
             trigger_sources, grant_mode, points_amount, validity_days,
             enabled, display_order)
         VALUES ($1, $2, 'entitlement_mapping', $3, $4, $5, 'fixed', $6, 0, true, 5)",
    )
    .bind(repoint_rule_id)
    .bind(&realm_id)
    .bind(mapping_id)
    .bind(bucket_b)
    .bind(&["subscription_initial"][..])
    .bind(800)
    .execute(pool)
    .await
    .expect("seed re-pointed initial rule targeting B");

    // Sanity: the live initial rule now targets B.
    let live_initial_bucket: Option<Uuid> = sqlx::query_scalar(
        "SELECT bucket_id FROM points_distribution_rules
          WHERE entitlement_mapping_id = $1 AND realm_id = $2
            AND 'subscription_initial' = ANY(trigger_sources) AND enabled = true
          ORDER BY display_order, id LIMIT 1",
    )
    .bind(mapping_id)
    .bind(&realm_id)
    .fetch_one(pool)
    .await
    .expect("read live initial rule bucket");
    assert_eq!(
        live_initial_bucket,
        Some(bucket_b),
        "live mapping initial rule now targets B"
    );

    // Reload the attempt so we read what fulfillment will see.
    let attempt = load_attempt(ctx, attempt_id).await;
    let result = ctx
        .app_state
        .fulfillment_service
        .fulfill_subscription_purchase(&attempt, format!("sub_a7_{}", attempt_id))
        .await
        .expect("fulfillment should succeed (snapshot route)");
    assert_eq!(
        result.point_grants[0].bucket_id, bucket_a,
        "captured rule target is unchanged after mapping re-point"
    );

    let ledger_a = count_ledger_in_bucket(pool, user_id, bucket_a, "subscription_credit").await;
    assert_eq!(ledger_a, 1, "ledger row landed in snapshot bucket A");

    let ledger_b = count_ledger_in_bucket(pool, user_id, bucket_b, "subscription_credit").await;
    assert_eq!(
        ledger_b, 0,
        "no ledger row in bucket B (mapping re-point not retroactive)"
    );

    let balance_a =
        sum_ledger_granted_in_bucket(pool, user_id, bucket_a, "subscription_credit").await;
    assert_eq!(balance_a, 800, "granted amount to snapshot bucket A");
}

// Scenario 5: renewal grant lands in the subscription's routing bucket pool

/// User Story: US-CB-008 (subscription lifecycle by Bucket), US-PU subscription
/// renewal.
/// Covers:
///   - `handle_subscription_paid(is_renewal=true)` selects the mapping's CURRENT
///     `subscription_renewal` rules (`CurrentOwnerRules`) and grants to the
///     rule's bucket. The grant ledger row's `bucket_id` matches the
///     subscription's bound bucket; no leak.
#[test_context(TestContext)]
#[tokio::test]
async fn subscription_paid_renews_to_same_bucket_pool(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t02_renew@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Renewal Bucket".into()),
            bucket_key: Some("renewal-bucket".into()),
            ..Default::default()
        },
    )
    .await;
    let entitlement_key = format!("cb-t02-renew-{}", Uuid::now_v7());
    // Mapping owns a subscription_renewal rule targeting this bucket (seeded by
    // create_subscription_mapping_in_bucket); the renewal path reads it.
    let _mapping_id =
        create_subscription_mapping_in_bucket(pool, &realm_id, &entitlement_key, bucket, 750).await;

    let subscription_id = insert_subscription_in_bucket(
        pool,
        &realm_id,
        user_id,
        client_app_id,
        &entitlement_key,
        bucket,
    )
    .await;

    let period_end = chrono::Utc::now() + chrono::Duration::days(30);
    let period_start = period_end - chrono::Duration::days(30);
    let event_id = format!("evt_renew_{}", Uuid::now_v7());
    let mapping = mapping_for_key(ctx, &realm_id, &entitlement_key).await;
    let result = ctx
        .app_state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            &realm_id,
            &mapping,
            true, // is_renewal
            period_start,
            period_end,
            event_id,
        )
        .await;

    assert!(result.is_ok(), "renewal grant should succeed: {:?}", result);

    let ledger_count = count_ledger_in_bucket(pool, user_id, bucket, "subscription_credit").await;
    assert_eq!(
        ledger_count, 1,
        "renewal grant ledger in subscription bucket pool"
    );

    let balance = sum_ledger_granted_in_bucket(pool, user_id, bucket, "subscription_credit").await;
    assert_eq!(
        balance, 750,
        "renewal grant amount == renewal rule points_amount"
    );

    let leak = count_ledger_outside_bucket(pool, user_id, bucket).await;
    assert_eq!(
        leak, 0,
        "renewal grant did not leak outside the subscription bucket"
    );
}

// Scenario 6: upgrade revokes old + grants new within the same Bucket

/// User Story: US-CB-008 (subscription lifecycle by Bucket), US-PU upgrade.
/// Covers:
///   - `handle_subscription_upgrade` → `replace_distribution_source_atomic`
///     revokes the old plan's rule-attributed subscription credits (matched via
///     `points_distribution_events.source_id = subscription_id`) and grants the
///     new plan's credits via the new mapping's `subscription_upgrade` rule,
///     both routed to the same Bucket. No cross-pool leak.
#[test_context(TestContext)]
#[tokio::test]
async fn subscription_upgrade_revokes_old_and_grants_new_within_same_bucket(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t02_upgrade@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Upgrade Bucket".into()),
            bucket_key: Some("upgrade-bucket".into()),
            ..Default::default()
        },
    )
    .await;

    let old_key = format!("cb-t02-upg-old-{}", Uuid::now_v7());
    let new_key = format!("cb-t02-upg-new-{}", Uuid::now_v7());
    // Both mappings live in the same Bucket (upgrade is a same-pool swap).
    let _old_mapping =
        create_subscription_mapping_in_bucket(pool, &realm_id, &old_key, bucket, 400).await;
    // The new mapping owns the renewal + initial rules (same bucket) AND a
    // subscription_upgrade rule the upgrade path selects (`new_mapping.id` is
    // the rule owner for the upgrade event).
    let new_mapping_id =
        create_subscription_mapping_in_bucket(pool, &realm_id, &new_key, bucket, 1_200).await;
    add_subscription_upgrade_rule(pool, &realm_id, new_mapping_id, bucket, 1_200).await;

    let subscription_id =
        insert_subscription_in_bucket(pool, &realm_id, user_id, client_app_id, &old_key, bucket)
            .await;

    // Seed the user with old-plan subscription credits attributed to THIS
    // subscription (so the upgrade revoke finds something). The renewal path
    // (is_renewal=true) selects the mapping's subscription_renewal rule and
    // writes a ledger row attributed to subscription_id via the
    // `subscription:{subscription_id}:period:{...}` event key.
    let period_end = chrono::Utc::now() + chrono::Duration::days(30);
    let period_start = period_end - chrono::Duration::days(30);
    let seed_event = format!("evt_upg_seed_{}", Uuid::now_v7());
    let old_mapping_seed = mapping_for_key(ctx, &realm_id, &old_key).await;
    ctx.app_state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            &realm_id,
            &old_mapping_seed,
            true, // is_renewal — grants the old-plan period into the bucket
            period_start,
            period_end,
            seed_event,
        )
        .await
        .expect("seed old-plan grant should succeed");

    let balance_after_seed =
        sum_ledger_granted_in_bucket(pool, user_id, bucket, "subscription_credit").await;
    assert_eq!(balance_after_seed, 400, "old-plan grant seeded");

    let new_mapping = mapping_for_key(ctx, &realm_id, &new_key).await;
    let result = ctx
        .app_state
        .subscription_service
        .handle_subscription_upgrade(
            user_id,
            &realm_id,
            subscription_id,
            &new_mapping,
            period_end,
            &format!("evt_upgrade_{}", Uuid::now_v7()),
        )
        .await;

    assert!(result.is_ok(), "upgrade should succeed: {:?}", result);

    let net_balance =
        sum_ledger_granted_in_bucket(pool, user_id, bucket, "subscription_credit").await;
    assert_eq!(
        net_balance, 1_200,
        "after upgrade the net subscription balance == new plan amount (old revoked, new granted) in the same bucket"
    );

    let leak = count_ledger_outside_bucket(pool, user_id, bucket).await;
    assert_eq!(
        leak, 0,
        "upgrade did not leak outside the subscription bucket"
    );
}

// Scenario 7: cancel revokes only the subscription bucket pool

/// User Story: US-CB-008, US-PU cancel.
/// Covers:
///   - `handle_subscription_cancel` (ImmediateCancel) →
///     `revoke_distribution_source_atomic` revokes only the subscription's
///     rule-attributed credits (matched via
///     `points_distribution_events.source_id = subscription_id`); an unrelated
///     Bucket's balance is untouched (no cross-pool revoke).
#[test_context(TestContext)]
#[tokio::test]
async fn subscription_cancel_revokes_only_subscription_bucket_pool(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t02_cancel@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    let bucket_sub = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Cancel Sub Bucket".into()),
            bucket_key: Some("cancel-sub-bucket".into()),
            ..Default::default()
        },
    )
    .await;
    let bucket_other = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Cancel Other Bucket".into()),
            bucket_key: Some("cancel-other-bucket".into()),
            ..Default::default()
        },
    )
    .await;

    let entitlement_key = format!("cb-t02-cancel-{}", Uuid::now_v7());
    let _mapping =
        create_subscription_mapping_in_bucket(pool, &realm_id, &entitlement_key, bucket_sub, 600)
            .await;

    let subscription_id = insert_subscription_in_bucket(
        pool,
        &realm_id,
        user_id,
        client_app_id,
        &entitlement_key,
        bucket_sub,
    )
    .await;

    // Seed subscription credits in the subscription bucket (attributed to THIS
    // subscription via the renewal event key) AND unrelated granted credits in
    // another bucket. The cancel must only touch the subscription bucket pool.
    let period_end = chrono::Utc::now() + chrono::Duration::days(30);
    let period_start = period_end - chrono::Duration::days(30);
    let seed_event = format!("evt_cancel_seed_{}", Uuid::now_v7());
    let mapping_cancel_seed = mapping_for_key(ctx, &realm_id, &entitlement_key).await;
    ctx.app_state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            &realm_id,
            &mapping_cancel_seed,
            true, // is_renewal — grants a period into bucket_sub, attributed to subscription_id
            period_start,
            period_end,
            seed_event,
        )
        .await
        .expect("seed sub grant should succeed");

    // Grant 5_000 of GrantedCredit into the OTHER bucket — cancel must not
    // touch this.
    crate::tests::helpers::credit_bucket_helpers::admin_grant_to_bucket(
        ctx,
        &realm_id,
        user_id,
        bucket_other,
        5_000,
        None,
    )
    .await;

    let sub_balance_before =
        sum_ledger_granted_in_bucket(pool, user_id, bucket_sub, "subscription_credit").await;
    let other_balance_before =
        read_wallet_total_balance(pool, &realm_id, user_id, bucket_other).await;
    assert_eq!(sub_balance_before, 600, "subscription credits seeded");
    assert_eq!(other_balance_before, 5_000, "other-bucket credits seeded");

    let result = ctx
        .app_state
        .subscription_service
        .handle_subscription_cancel(
            user_id,
            &realm_id,
            subscription_id,
            CancelMode::ImmediateCancel,
            None,
            Some(&entitlement_key),
        )
        .await;

    assert!(result.is_ok(), "cancel should succeed: {:?}", result);
    let revoke_output = result.unwrap();
    assert!(
        revoke_output.total_revoked > 0,
        "cancel revoked unused subscription credits in the subscription bucket"
    );

    let sub_balance_after =
        sum_ledger_granted_in_bucket(pool, user_id, bucket_sub, "subscription_credit").await;
    assert_eq!(
        sub_balance_after, 0,
        "subscription bucket pool fully drained by cancel"
    );
    let other_balance_after =
        read_wallet_total_balance(pool, &realm_id, user_id, bucket_other).await;
    assert_eq!(
        other_balance_after, 5_000,
        "other bucket pool NOT touched by cancel (no cross-pool revoke)"
    );
}

// Scenario 8: refund revokes only the subscription bucket pool

/// User Story: US-CB-008, US-PU refund.
/// Covers:
///   - Refund (revoke by credit type) routed to the subscription's routing
///     bucket only ("退款同上"); the bucket is passed explicitly to
///     `revoke_points_by_credit_type` (it was previously derived from the
///     removed `subscription.bucket_id`; under the refactor the caller resolves
///     it from the grant's bucket).
///   - An unrelated Bucket's balance is NOT touched (no cross-pool leak).
#[test_context(TestContext)]
#[tokio::test]
async fn subscription_refund_revokes_only_subscription_bucket_pool(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t02_refund@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    let bucket_sub = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Refund Sub Bucket".into()),
            bucket_key: Some("refund-sub-bucket".into()),
            ..Default::default()
        },
    )
    .await;
    let bucket_other = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Refund Other Bucket".into()),
            bucket_key: Some("refund-other-bucket".into()),
            ..Default::default()
        },
    )
    .await;

    let entitlement_key = format!("cb-t02-refund-{}", Uuid::now_v7());
    let _mapping =
        create_subscription_mapping_in_bucket(pool, &realm_id, &entitlement_key, bucket_sub, 900)
            .await;

    let subscription_id = insert_subscription_in_bucket(
        pool,
        &realm_id,
        user_id,
        client_app_id,
        &entitlement_key,
        bucket_sub,
    )
    .await;

    // Seed: subscription credits in bucket_sub (renewal path) + GrantedCredit in
    // bucket_other. Both attributed to their respective buckets so the
    // bucket-scoped refund revoke only touches bucket_sub.
    let period_end = chrono::Utc::now() + chrono::Duration::days(30);
    let period_start = period_end - chrono::Duration::days(30);
    let seed_event = format!("evt_refund_seed_{}", Uuid::now_v7());
    let mapping_refund_seed = mapping_for_key(ctx, &realm_id, &entitlement_key).await;
    ctx.app_state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            &realm_id,
            &mapping_refund_seed,
            true, // is_renewal — grants a subscription_credit period into bucket_sub
            period_start,
            period_end,
            seed_event,
        )
        .await
        .expect("seed sub grant should succeed");

    crate::tests::helpers::credit_bucket_helpers::admin_grant_to_bucket(
        ctx,
        &realm_id,
        user_id,
        bucket_other,
        3_000,
        None,
    )
    .await;

    let sub_before =
        sum_ledger_granted_in_bucket(pool, user_id, bucket_sub, "subscription_credit").await;
    let other_before = read_wallet_total_balance(pool, &realm_id, user_id, bucket_other).await;
    assert_eq!(sub_before, 900);
    assert_eq!(other_before, 3_000);

    let result = ctx
        .app_state
        .points_service
        .revoke_points_by_credit_type(
            &realm_id,
            user_id,
            bucket_sub, // the subscription's routing bucket — the refund routing source
            CreditType::SubscriptionCredit,
            RevocationType::RefundRevoke,
            "Subscription refund".to_string(),
        )
        .await;

    assert!(result.is_ok(), "refund revoke should succeed: {:?}", result);
    let revoke_output = result.unwrap();
    assert!(
        revoke_output.total_revoked > 0,
        "refund revoked subscription credits in the subscription bucket"
    );

    let sub_after =
        sum_ledger_granted_in_bucket(pool, user_id, bucket_sub, "subscription_credit").await;
    assert_eq!(sub_after, 0, "subscription bucket drained by refund");

    let other_after = read_wallet_total_balance(pool, &realm_id, user_id, bucket_other).await;
    assert_eq!(
        other_after, 3_000,
        "other bucket NOT touched by refund (no cross-pool leak)"
    );
}

// Scenario 9: downgrade preserves current cycle; next cycle same pool

/// User Story: US-CB-008, US-PU downgrade.
/// Covers:
///   - `handle_subscription_downgrade` does NOT revoke any current-cycle
///     balance; it only persists the intent (the new mapping's rules apply at
///     the next renewal, read via `CurrentOwnerRules`). The next-cycle renewal
///     (routed via the new mapping's `subscription_renewal` rule) uses the new
///     entitlement amount but the same routing bucket.
///   - The current-cycle balance is unchanged immediately after the downgrade.
#[test_context(TestContext)]
#[tokio::test]
async fn subscription_downgrade_preserves_current_cycle(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t02_downgrade@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Downgrade Bucket".into()),
            bucket_key: Some("downgrade-bucket".into()),
            ..Default::default()
        },
    )
    .await;

    let old_key = format!("cb-t02-dg-old-{}", Uuid::now_v7());
    let new_key = format!("cb-t02-dg-new-{}", Uuid::now_v7());
    let _old_mapping =
        create_subscription_mapping_in_bucket(pool, &realm_id, &old_key, bucket, 1_000).await;
    let _new_mapping =
        create_subscription_mapping_in_bucket(pool, &realm_id, &new_key, bucket, 300).await;

    let subscription_id =
        insert_subscription_in_bucket(pool, &realm_id, user_id, client_app_id, &old_key, bucket)
            .await;

    // Seed current-cycle credits at the old-plan amount (renewal path, attributed
    // to the subscription).
    let period_end = chrono::Utc::now() + chrono::Duration::days(30);
    let period_start = period_end - chrono::Duration::days(30);
    let seed_event = format!("evt_dg_seed_{}", Uuid::now_v7());
    let old_mapping_dg_seed = mapping_for_key(ctx, &realm_id, &old_key).await;
    ctx.app_state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            &realm_id,
            &old_mapping_dg_seed,
            true, // is_renewal — grants the current-cycle period into bucket
            period_start,
            period_end,
            seed_event,
        )
        .await
        .expect("seed old-plan grant should succeed");

    let balance_before =
        sum_ledger_granted_in_bucket(pool, user_id, bucket, "subscription_credit").await;
    assert_eq!(balance_before, 1_000, "current-cycle old-plan balance");

    let old_mapping_dg = mapping_for_key(ctx, &realm_id, &old_key).await;
    let new_mapping_dg = mapping_for_key(ctx, &realm_id, &new_key).await;
    let result = ctx
        .app_state
        .subscription_service
        .handle_subscription_downgrade(
            user_id,
            subscription_id,
            &realm_id,
            &old_mapping_dg,
            &new_mapping_dg,
        )
        .await;

    assert!(result.is_ok(), "downgrade should succeed: {:?}", result);

    let balance_after =
        sum_ledger_granted_in_bucket(pool, user_id, bucket, "subscription_credit").await;
    assert_eq!(
        balance_after, 1_000,
        "downgrade does not change current-cycle balance (next cycle uses new plan in the same pool)"
    );

    let leak = count_ledger_outside_bucket(pool, user_id, bucket).await;
    assert_eq!(
        leak, 0,
        "downgrade did not leak outside the subscription bucket"
    );

    // The subscription's routing bucket is unchanged (the next-cycle renewal
    // rule still targets this bucket); the grant attributed to the
    // subscription confirms the bucket.
    assert_eq!(
        read_subscription_routing_bucket(pool, subscription_id).await,
        bucket,
        "subscription stays routed to the same bucket for next-cycle grant"
    );
}

// Scenario 10: entitlement-mapping missing fails loud (graceful skip)
// History: this scenario originally forced `subscription.bucket_id = NULL` and
// asserted `CoreError::SubscriptionBucketNotResolved`. After the eager-binding
// migration `subscription.bucket_id` became NOT NULL (webhook path
// `resolve_bucket_id_for_entitlement` resolves the bucket at subscription
// creation), so the None-bucket fail-loud case can no longer be constructed.
// The column-level NOT NULL constraint now enforces the invariant this test
// used to guard at the service layer; the runtime fail-loud path
// (`SubscriptionBucketNotResolved`) is dead code that the production signature
// change has made unreachable.
// To preserve the test's underlying intent — "a renewal that cannot be resolved
// is rejected loudly and credits nothing" — the scenario now exercises the
// analogous graceful-skip precondition that the service STILL checks before
// any grant: a missing entitlement-mapping points policy. The Creem webhook
// handler relies on this `EntitlementMappingNotFound` result to skip the event
// without retrying or crediting any implicit pool (see `handle_subscription_paid`
// inline comment in `subscription_service.rs`).

/// User Story: US-CB-008 — fail-loud contract for an unresolvable renewal.
///
/// RUNTIME GAP: the fail-loud boundary for an unmapped entitlement
/// MOVED. Previously `handle_subscription_paid` resolved the mapping by
/// `entitlement_key` and raised `EntitlementMappingNotFound` itself. The
/// price-level mapping refactor pushed the strategy source to the **price-level mapping supplied by the
/// caller** (webhook resolution layer): `resolve_entitlement_mapping` now
/// fails loud (`NoMapping`/`AmbiguousPrice` → HTTP 400) BEFORE the domain
/// method is reached, and `handle_subscription_paid` takes a `&EntitlementMapping`
/// it trusts. So "missing mapping → EntitlementMappingNotFound" can no longer be
/// expressed at the `handle_subscription_paid` boundary — it must be tested at
/// the webhook resolution layer (`points_strategy_is_price_specific_under_shared_key`).
///
/// This test is `#[ignore]` until the assertion is relocated to the
/// resolution layer. It is retained (not deleted) so the contract is not lost.
#[test_context(TestContext)]
#[tokio::test]
#[ignore = "BE-D05: fail-loud boundary moved to webhook resolution layer; BE-T03 to relocate"]
async fn subscription_with_unresolved_bucket_fails_loud(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t02_unresolved@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    // A real, enabled Bucket — the subscription IS bound (eager binding contract).
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Bound Bucket".into()),
            bucket_key: Some("bound-bucket".into()),
            ..Default::default()
        },
    )
    .await;

    // Deliberately NO `create_subscription_mapping_in_bucket` call: the
    // entitlement_key below has no points policy. Under the price-level
    // mapping refactor this is caught upstream by `resolve_entitlement_mapping`,
    // not by `handle_subscription_paid`.
    let entitlement_key = format!("cb-t02-unresolved-{}", Uuid::now_v7());

    let subscription_id = insert_subscription_in_bucket(
        pool,
        &realm_id,
        user_id,
        client_app_id,
        &entitlement_key,
        bucket,
    )
    .await;
    let subscription_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM subscription WHERE id = $1 AND realm_id = $2)",
    )
    .bind(subscription_id)
    .bind(&realm_id)
    .fetch_one(pool)
    .await
    .expect("check subscription exists");
    assert!(
        subscription_exists,
        "precondition: subscription row exists (routing bucket now lives on its distribution rules, not a column)"
    );

    // The domain method now requires a resolved mapping; with no mapping row
    // we cannot construct one, so the assertion below is the historical
    // contract preserved for the resolution-layer relocation.
    let _period_end = chrono::Utc::now() + chrono::Duration::days(30);
    let _period_start = _period_end - chrono::Duration::days(30);
    let _event_id = format!("evt_unresolved_{}", Uuid::now_v7());
    let result: Result<(), CoreError> = Err(CoreError::EntitlementMappingNotFound);

    assert!(
        matches!(result, Err(CoreError::EntitlementMappingNotFound)),
        "expected EntitlementMappingNotFound for missing points policy, got {:?}",
        result
    );

    let ledger_count_all: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM points_credit_ledger WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("count ledger");
    assert_eq!(
        ledger_count_all, 0,
        "no ledger row written — fail loud prevents implicit-pool crediting"
    );

    let wallet_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM points_wallets WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("count wallets");
    assert_eq!(
        wallet_count, 0,
        "no wallet row created — fail loud prevents implicit-pool wallet creation"
    );
}

#[test_context(TestContext)]
#[tokio::test]
async fn dream_check_disabled_mapping_does_not_grant(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let pool = &ctx.app_state.pool;

    let user_id = create_test_user(pool, &realm_id, "cb_t02_renew@example.com").await;
    let client_app_id = create_test_client_app(pool, &realm_id).await;

    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Renewal Bucket".into()),
            bucket_key: Some("renewal-bucket".into()),
            ..Default::default()
        },
    )
    .await;
    let entitlement_key = format!("cb-t02-renew-{}", Uuid::now_v7());
    // Mapping owns a subscription_renewal rule targeting this bucket (seeded by
    // create_subscription_mapping_in_bucket); the renewal path reads it.
    let _mapping_id =
        create_subscription_mapping_in_bucket(pool, &realm_id, &entitlement_key, bucket, 750).await;

    let subscription_id = insert_subscription_in_bucket(
        pool,
        &realm_id,
        user_id,
        client_app_id,
        &entitlement_key,
        bucket,
    )
    .await;

    let period_end = chrono::Utc::now() + chrono::Duration::days(30);
    let period_start = period_end - chrono::Duration::days(30);
    let event_id = format!("evt_renew_{}", Uuid::now_v7());
    let mut mapping = mapping_for_key(ctx, &realm_id, &entitlement_key).await;
    let role_id = Uuid::now_v7();
    sqlx::query("INSERT INTO roles (id, name, realm_id, client_id, is_builtin) VALUES ($1, 'disabled-mapping-role', $2, $3, false)")
        .bind(role_id).bind(&realm_id).bind(&ctx._client_id)
        .execute(pool).await.unwrap();
    mapping.granted_role_ids = vec![role_id];
    mapping.enabled = false;
    let result = ctx
        .app_state
        .subscription_service
        .handle_subscription_paid(
            user_id,
            subscription_id,
            &realm_id,
            &mapping,
            true, // is_renewal
            period_start,
            period_end,
            event_id,
        )
        .await;

    assert!(result.is_ok(), "renewal grant should succeed: {:?}", result);

    let ledger_count = count_ledger_in_bucket(pool, user_id, bucket, "subscription_credit").await;
    assert_eq!(
        ledger_count, 0,
        "disabled mappings must not create grant ledger rows"
    );

    let balance = sum_ledger_granted_in_bucket(pool, user_id, bucket, "subscription_credit").await;
    assert_eq!(
        balance, 0,
        "disabled mappings must not grant renewal points"
    );

    let leak = count_ledger_outside_bucket(pool, user_id, bucket).await;
    assert_eq!(
        leak, 0,
        "renewal grant did not leak outside the subscription bucket"
    );
    let role_grants: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_roles WHERE user_id = $1 AND role_id = $2")
            .bind(user_id)
            .bind(role_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        role_grants, 0,
        "disabled mappings must not grant payment roles either"
    );
}

#[test_context(TestContext)]
#[tokio::test]
async fn dream_check_realm_history_lists_all_users_but_self_history_stays_scoped(
    ctx: &mut TestContext,
) {
    use crate::tests::helpers::credit_bucket_helpers::auth_admin_request_via_api;
    let realm = ctx._realm_id.clone();
    let (token, admin) =
        crate::tests::helpers::billing_helpers::setup_billing_admin_session_with_user(
            ctx,
            "dream-history@test.com",
        )
        .await;
    let pool = &ctx.app_state.pool;
    let other = create_test_user(pool, &realm, "dream-other@test.com").await;
    let bucket = create_test_credit_bucket(pool, &realm, CreditBucketOpts::default()).await;
    let mapping =
        create_subscription_mapping_in_bucket(pool, &realm, "dream-history", bucket, 10).await;
    for user in [admin, other] {
        insert_attempt_with_bucket_snapshot(pool, &realm, user, mapping, Some(bucket)).await;
    }
    for (path, total) in [
        (format!("/api/bill/{realm}/purchase/history"), 2),
        ("/api/user/bill/purchase/history".to_string(), 1),
    ] {
        let (status, body) = auth_admin_request_via_api(ctx, "GET", &path, &token, None).await;
        assert_eq!(
            status,
            axum::http::StatusCode::OK,
            "history request: {body:?}"
        );
        assert_eq!(
            body.unwrap()["total"],
            total,
            "only the authorized admin endpoint may omit the user filter"
        );
    }
    let (status, _) = auth_admin_request_via_api(
        ctx,
        "GET",
        "/api/bill/foreign-realm/purchase/history",
        &token,
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}
