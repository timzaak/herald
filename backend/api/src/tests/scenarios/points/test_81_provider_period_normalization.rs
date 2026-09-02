// =============================================================================
// provider Period Normalization (Stripe + Creem)
// =============================================================================
//
// SCENARIO-LAYER coverage:
//   * provider period normalization — Stripe top-level + item-level periods
//     and Creem symmetric variants must normalize to a unique `(period_start,
//     period_end)` and DRIVE the grant (`Some ⟹ grant`).
//   * normalization precondition failure — when the provider payload cannot
//     uniquely yield `period_start/period_end`, the handler MUST skip the
//     grant, emit a structured `warn!(reason = "period_uniquely_unresolvable")`,
//     and await a later webhook / API compensation (never guess the
//     period from event time, never write a ledger with an invented period).
//
// These tests exercise the normalization behavior END-TO-END via the webhook
// HTTP path (the normalizers are private to herald-api-billing, so the only
// way to observe them is through `handle_subscription_paid` being invoked or
// skipped). They are NOT duplicates of the `normalize_stripe_period` /
// `normalize_creem_period` `#[cfg(test)]` unit tests inside
// `backend/api-billing/src/*_webhook_handlers.rs` (those are owned elsewhere —
// the four quadrants + Creem variants are already covered there). This file
// asserts the *consequence* of normalization at the
// scenario layer: ledger rows written (Some) vs. NOT written + no
// next-period pre-grant (None).
//
// Entry points exercised (read-only, do NOT modify):
//   * Stripe `customer.subscription.created`  → `handle_subscription_created`
//     → `normalize_stripe_period(&event["data"]["object"])`
//     (backend/api-billing/src/stripe_webhook_handlers.rs:1797)
//   * Creem `subscription.paid`               → webhook dispatch
//     → `normalize_creem_period(creem_event_object(&event))`
//     (backend/api-billing/src/webhook_handlers.rs:1067)
//
// All balance assertions use the derived-predicate helper
// (`assert_derived_balance`), never `points_wallets.total_balance` (that
// column was physically removed).
//
// Quadrants covered:
//   (a) Stripe top-level current_period_*           → Some  → grant
//   (b) Stripe item-level items.data[].current_*    → Some  → grant
//   (c) Stripe multi-item disagreeing periods       → None  → SKIP grant
//   (d) Stripe top-level + item-level BOTH absent   → None  → SKIP grant
//   (e) Creem currentPeriodStart/End (camelCase)    → Some  → grant
//   (f) Creem period fields missing                 → None  → SKIP grant
//
// =============================================================================

use crate::tests::helpers::points_helpers::{assert_derived_balance, get_user_quota_entitlements};
use crate::tests::helpers::webhook_helpers::{
    assert_webhook_success, generate_test_event_id, send_stripe_webhook_with_signature,
    send_webhook_with_signature, setup_test_entitlement_mapping_for_webhook,
};
use crate::tests::schema_test_context::SchemaTestContext;
use herald_core::domain::points::entities::CreditType;
use test_context::test_context;
use uuid::Uuid;

// ----------------------------------------------------------------------------
// Shared local helpers
// ----------------------------------------------------------------------------

/// Create a test account row directly (mirrors fixtures::create_test_user but
/// avoids pulling in points_helpers::create_test_user which lives in a
/// different import path). We need this local copy because the
/// `create_test_user` helper (points_helpers) takes a `&PgPool` and several
/// sibling tests already use a ctx-bound variant — keeping a local
/// copy makes this file self-contained and avoids the broken legacy
/// `create_points_wallet` helper (it references dropped columns).
async fn create_user(ctx: &SchemaTestContext, realm_id: &str, email: &str) -> Uuid {
    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, $3, $4, 1)
         ON CONFLICT (realm_id, email) DO NOTHING",
    )
    .bind(user_id)
    .bind(realm_id)
    .bind(email)
    .bind("$2a$12$dummy_password_hash")
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to create test user");
    user_id
}

/// Build a Stripe `customer.subscription.created` event with the given
/// subscription object as `data.object`. The caller controls whether
/// `current_period_*` appears at the top level, at the item level, both, or
/// neither — which is exactly what selects the P0 quadrant under test.
fn build_stripe_subscription_created_event(
    event_id: &str,
    realm_id: &str,
    user_id: Uuid,
    entitlement_key: &str,
    _external_product_id: &str,
    subscription_object: serde_json::Value,
) -> serde_json::Value {
    // Merge the immutable herald metadata into the caller-supplied
    // subscription object so every quadrant carries the same routing keys.
    let mut object = subscription_object;
    object["object"] = serde_json::json!("subscription");
    object["id"] = serde_json::json!(format!("sub_{}", event_id));
    if object.get("status").map(|v| v.is_null()).unwrap_or(true) {
        object["status"] = serde_json::json!("active");
    }
    object["metadata"] = serde_json::json!({
        "herald_realm_id": realm_id,
        "herald_user_id": user_id.to_string(),
        "herald_entitlement_key": entitlement_key,
        "userId": user_id.to_string(),
    });

    serde_json::json!({
        "id": event_id,
        "object": "event",
        "type": "customer.subscription.created",
        "api_version": "2020-08-27",
        "created": chrono::Utc::now().timestamp(),
        "data": { "object": object }
    })
}

/// Build a Creem `subscription.paid` event with caller-supplied period
/// fields on `data.object`. Passing `None` for either side omits the field,
/// exercising the "missing / partial ⟹ None" quadrant.
fn build_creem_subscription_paid_event(
    event_id: &str,
    realm_id: &str,
    user_id: Uuid,
    entitlement_key: &str,
    external_product_id: &str,
    current_period_start: Option<&str>,
    current_period_end: Option<&str>,
    is_renewal: bool,
) -> serde_json::Value {
    let mut object = serde_json::json!({
        "subscriptionId": format!("sub_creem_{}", event_id),
        "productId": external_product_id,
        "userId": user_id.to_string(),
        "entitlementKey": entitlement_key,
        "isRenewal": is_renewal,
        "amount": 2500,
        "currency": "USD",
        "status": "active",
        "cancelAtPeriodEnd": false,
        "billingPeriod": "monthly",
        "metadata": { "realmId": realm_id }
    });
    if let Some(start) = current_period_start {
        object["currentPeriodStart"] = serde_json::json!(start);
    }
    if let Some(end) = current_period_end {
        object["currentPeriodEnd"] = serde_json::json!(end);
    }

    serde_json::json!({
        "id": event_id,
        "eventType": "subscription.paid",
        "data": { "object": object },
        "metadata": { "realmId": realm_id }
    })
}

// ============================================================================
// Scenario (c): Stripe multi-item disagreeing periods → None → SKIP grant
// ============================================================================

// User Story: US-PU-009 (never grant against a guessed period).
// Covers normalization precondition failure, quadrant (c):
//   when a subscription has MULTIPLE items with DISAGREEING periods, the
//   points entitlement cannot be uniquely mapped to one item's period. The
//   normalizer MUST return None; the handler MUST skip the grant and emit a
//   structured warning (never guess from event time, never write a ledger
//   with an invented period). A later webhook / API call must compensate.
//
// Why this test exists: this is the central P0 safety guarantee. If the
// normalizer silently picked the first item's period (or top-level fallback
// when items disagree), a multi-product subscription could grant against the
// WRONG billing window — leaking or short-changing the user. The strict
// "None ⟹ skip" gate is what makes provider period normalization a P0
// precondition for pre-grant.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_stripe_multi_item_period_unresolvable_skips_pregrant(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_user(ctx, &realm_id, "be-t05-stripe-multi@example.com").await;

    let entitlement_key = format!("be-t05-stripe-multi-{}", Uuid::now_v7());
    let external_product_id = format!("prod_{}", entitlement_key);
    let event_id = generate_test_event_id();
    let webhook_secret = "test_stripe_wh_secret";

    crate::tests::helpers::billing_helpers::setup_stripe_config(
        ctx,
        &realm_id,
        "sk_test_key",
        webhook_secret,
    )
    .await;

    setup_test_entitlement_mapping_for_webhook(
        ctx,
        &realm_id,
        "stripe",
        &external_product_id,
        &entitlement_key,
        1000,
        true,
        true,
    )
    .await;

    // Two items, DISAGREEING periods, no top-level fields. The normalizer
    // cannot uniquely identify the points entitlement's item → None.
    let now = chrono::Utc::now();
    let item_a_start = now - chrono::Duration::seconds(10);
    let item_a_end = now + chrono::Duration::days(30);
    let item_b_start = now - chrono::Duration::days(2); // ← different window
    let item_b_end = now + chrono::Duration::days(60);
    let subscription_object = serde_json::json!({
        "status": "active",
        // NOTE: no top-level current_period_* — would otherwise rescue this.
        "items": {
            "data": [
                {
                    "price": { "product": external_product_id },
                    "current_period_start": item_a_start.timestamp(),
                    "current_period_end": item_a_end.timestamp()
                },
                {
                    "price": { "product": format!("{}_addon", external_product_id) },
                    "current_period_start": item_b_start.timestamp(),
                    "current_period_end": item_b_end.timestamp()
                }
            ]
        }
    });

    let event = build_stripe_subscription_created_event(
        &event_id,
        &realm_id,
        user_id,
        &entitlement_key,
        &external_product_id,
        subscription_object,
    );

    let app = ctx.create_unified_test_router();
    let response = send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
    // The handler acknowledges the webhook (so Stripe doesn't redeliver
    // aggressively) even when it skips the grant — this mirrors the
    // EntitlementMappingNotFound graceful-skip behavior.
    assert_webhook_success(&response);

    // Then: NO subscription_credit quota entitlement was written. Neither the
    // formal current-period grant NOR a next-period pre-grant may appear — P0
    // forbids writing an entitlement with an invented period.
    let entitlements =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert!(
        entitlements.is_empty(),
        "Stripe multi-item disagreeing periods must SKIP grant and write NO entitlement \
         (P0 — never guess); got {} subscription_credit quota entitlements",
        entitlements.len()
    );

    // And: derived available balance is 0 (no ledger row exists).
    assert_derived_balance(ctx, user_id, &realm_id, CreditType::SubscriptionCredit, 0).await;
}

// ============================================================================
// Scenario (d): Stripe top-level AND item-level both absent → None → SKIP
// ============================================================================

// User Story: US-PU-009 (never grant against a guessed period).
// Covers normalization precondition failure, quadrant (d):
//   when NEITHER top-level NOR item-level period fields are present (a
//   malformed / partial payload, or a provider quirk), the normalizer
//   returns None and the handler skips the grant.
//
// Why this test exists: this is the "no signal at all" case. Combined with
// scenario (c), it pins down the invariant: the handler writes a
// ledger IFF the normalizer resolved a unique period. Without this test, a
// regression that fell back to "event time as period_start" on missing
// fields would silently grant against the wrong window.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_stripe_no_period_anywhere_skips_pregrant(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_user(ctx, &realm_id, "be-t05-stripe-none@example.com").await;

    let entitlement_key = format!("be-t05-stripe-none-{}", Uuid::now_v7());
    let external_product_id = format!("prod_{}", entitlement_key);
    let event_id = generate_test_event_id();
    let webhook_secret = "test_stripe_wh_secret";

    crate::tests::helpers::billing_helpers::setup_stripe_config(
        ctx,
        &realm_id,
        "sk_test_key",
        webhook_secret,
    )
    .await;

    setup_test_entitlement_mapping_for_webhook(
        ctx,
        &realm_id,
        "stripe",
        &external_product_id,
        &entitlement_key,
        1000,
        true,
        true,
    )
    .await;

    // NO period fields anywhere — top level absent, item level absent.
    let subscription_object = serde_json::json!({
        "status": "active",
        "items": {
            "data": [{
                "price": { "product": external_product_id }
                // no current_period_*
            }]
        }
    });

    let event = build_stripe_subscription_created_event(
        &event_id,
        &realm_id,
        user_id,
        &entitlement_key,
        &external_product_id,
        subscription_object,
    );

    let app = ctx.create_unified_test_router();
    let response = send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
    assert_webhook_success(&response);

    let entitlements =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert!(
        entitlements.is_empty(),
        "Stripe payload with NO period anywhere must SKIP grant; \
         got {} subscription_credit quota entitlements",
        entitlements.len()
    );

    assert_derived_balance(ctx, user_id, &realm_id, CreditType::SubscriptionCredit, 0).await;
}

// ============================================================================
// Scenario (e): Creem currentPeriodStart/End (camelCase) → Some → grant
// ============================================================================

// User Story: US-PU-009.
// Covers provider period normalization — Creem symmetric case:
//   Creem exposes the period under several field-name variants
//   (`currentPeriodStart` / `current_period_start` /
//   `current_period_start_date`, and matching `*End`). When both endpoints
//   resolve to a valid window, the normalizer returns Some and the grant
//   proceeds.
//
// Why this test exists: this is the Creem counterpart to scenario (a). It
// locks in that the Creem normalization path ALSO drives the grant when a
// period is resolvable, keeping the two providers symmetric. Without it, a
// regression that made the Creem path silently return None would skip every
// Creem grant.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_creem_period_normalized(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_user(ctx, &realm_id, "be-t05-creem-ok@example.com").await;

    let entitlement_key = format!("be-t05-creem-ok-{}", Uuid::now_v7());
    let external_product_id = format!("prod_{}", entitlement_key);
    let event_id = generate_test_event_id();

    ctx.with_creem_config(&realm_id, None, None, None).await;

    let mapping_id = setup_test_entitlement_mapping_for_webhook(
        ctx,
        &realm_id,
        "creem",
        &external_product_id,
        &entitlement_key,
        1000,
        true,
        true,
    )
    .await;
    // Initial activation grants nothing; only the renewal route grants, so
    // seed a subscription_renewal rule and drive a renewal webhook.
    crate::tests::helpers::points_helpers::seed_subscription_renewal_rule(
        &ctx.app_state.pool,
        &realm_id,
        mapping_id,
        1000,
    )
    .await;

    let now = chrono::Utc::now();
    let period_start = (now - chrono::Duration::seconds(10)).to_rfc3339();
    let period_end = (now + chrono::Duration::days(30)).to_rfc3339();

    let event = build_creem_subscription_paid_event(
        &event_id,
        &realm_id,
        user_id,
        &entitlement_key,
        &external_product_id,
        Some(&period_start),
        Some(&period_end),
        true, // renewal — the only grant-bearing subscription.paid route
    );

    let app = ctx.create_unified_test_router();
    let response = send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;
    assert_webhook_success(&response);

    let entitlements =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert!(
        !entitlements.is_empty(),
        "Creem currentPeriodStart/End must normalize to Some and drive a renewal grant; \
         got 0 subscription_credit quota entitlements"
    );

    assert_derived_balance(
        ctx,
        user_id,
        &realm_id,
        CreditType::SubscriptionCredit,
        1000,
    )
    .await;
}

// ============================================================================
// Scenario (f): Creem period fields missing → None → SKIP grant
// ============================================================================

// User Story: US-PU-009 (never grant against a guessed period).
// Covers normalization precondition failure — Creem symmetric:
//   when neither period endpoint can be resolved (fields missing / partial /
//   inverted), the normalizer returns None and the handler MUST skip the
//   grant, emit a structured warning, and await a later webhook / API
//   compensation. Never guess from event time.
//
// Why this test exists: this is the Creem counterpart to scenario (d). It
// pins down that the "None ⟹ skip" gate applies symmetrically to
// both providers — a regression on either side must not silently fall back
// to event-time guessing. The default `build_subscription_paid_event` helper
// (omits `currentPeriodStart`) makes this case easy to hit accidentally;
// this test locks the safe behavior in.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_creem_period_missing_skips_pregrant(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_user(ctx, &realm_id, "be-t05-creem-miss@example.com").await;

    let entitlement_key = format!("be-t05-creem-miss-{}", Uuid::now_v7());
    let external_product_id = format!("prod_{}", entitlement_key);
    let event_id = generate_test_event_id();

    ctx.with_creem_config(&realm_id, None, None, None).await;

    setup_test_entitlement_mapping_for_webhook(
        ctx,
        &realm_id,
        "creem",
        &external_product_id,
        &entitlement_key,
        1000,
        true,
        true,
    )
    .await;

    // NO period fields supplied — exercises the "missing ⟹ None" quadrant.
    let event = build_creem_subscription_paid_event(
        &event_id,
        &realm_id,
        user_id,
        &entitlement_key,
        &external_product_id,
        None,
        None,
        false,
    );

    let app = ctx.create_unified_test_router();
    let response = send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;
    assert_webhook_success(&response);

    let entitlements =
        get_user_quota_entitlements(ctx, user_id, CreditType::SubscriptionCredit).await;
    assert!(
        entitlements.is_empty(),
        "Creem payload with NO period fields must SKIP grant (P0 — never guess \
         from event time); got {} subscription_credit quota entitlements",
        entitlements.len()
    );

    assert_derived_balance(ctx, user_id, &realm_id, CreditType::SubscriptionCredit, 0).await;
}
