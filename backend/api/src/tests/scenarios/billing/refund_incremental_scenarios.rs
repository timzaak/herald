// =============================================================================
// Refund Incremental Revocation Scenario Tests
// =============================================================================
//
// Multi-partial-refund semantics for one-time (topup) purchases:
// - points revocation is PER-REFUND (incremental), idempotent per provider
//   refund id, capped at the remaining balance, and the cumulative
//   full-refund gate sweeps all remaining points.
// - the permanent payment-source role is revoked only when the cumulative
//   refund reaches 100% of the original payment; any partial refund keeps
//   the role, and dispute events never revoke it.
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::async_payment_helpers::{create_test_user, get_topup_balance};
    use crate::tests::helpers::billing_helpers::setup_stripe_config;
    use crate::tests::helpers::billing_seed_helpers::{
        count_payment_roles_by_source_id, create_stripe_one_time_mapping_with_role,
        create_stripe_succeeded_attempt, seed_manual_role_grant, seed_payment_role_grant,
    };
    use crate::tests::helpers::points_helpers::{
        consume_points_from_ledger, create_credit_ledger_entry_v2, create_payment_attempt_snapshot,
        create_points_wallet, get_ledger_by_id, get_revocation_records, get_wallet_bucket_id,
        seed_attributed_topup_ledger, seed_fulfilled_topup_ledger_for_attempt,
    };
    use crate::tests::helpers::webhook_helpers::{
        assert_webhook_success, build_refund_created_event_with_user,
        build_stripe_charge_refunded_dashboard_event, build_stripe_charge_refunded_topup_event,
        generate_test_event_id, send_stripe_webhook_with_signature, send_webhook_with_signature,
    };
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::http::StatusCode;
    use herald_core::domain::points::entities::{CreditLedgerStatus, CreditSourceType, CreditType};
    use serde_json::json;
    use test_context::test_context;
    use uuid::Uuid;

    // =========================================================================
    // Local helpers
    // =========================================================================

    /// Count `user_roles` rows with `source='manual'` for a role.
    async fn count_manual_roles_for_role(
        ctx: &SchemaTestContext,
        user_id: Uuid,
        role_id: Uuid,
    ) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles
             WHERE user_id = $1 AND source = 'manual' AND role_id = $2",
        )
        .bind(user_id)
        .bind(role_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    /// Build a Stripe `charge.dispute.created` event whose charge carries no
    /// subscription-mappable metadata — the one-time-purchase situation. The
    /// handler must log + ignore it (no role revocation, no points change).
    fn build_stripe_dispute_created_unmapped(
        event_id: &str,
        realm_id: &str,
        user_id: Uuid,
        charge_id: &str,
    ) -> serde_json::Value {
        json!({
            "id": event_id,
            "object": "event",
            "type": "charge.dispute.created",
            "api_version": "2020-08-27",
            "created": chrono::Utc::now().timestamp(),
            "data": {
                "object": {
                    "id": format!("dp_{}", Uuid::now_v7()),
                    "object": "dispute",
                    "charge": charge_id,
                    "payment_intent": format!("pi_{}", Uuid::now_v7()),
                    "amount": 1000,
                    "reason": "customer_requested",
                    "status": "needs_response",
                    "metadata": {
                        "herald_realm_id": realm_id,
                        "herald_user_id": user_id.to_string(),
                    }
                }
            }
        })
    }

    /// Build a Creem `dispute.created` event referencing a subscription that
    /// does not exist locally. The handler must answer 400 (dispute handling
    /// is subscription-only today).
    fn build_creem_dispute_unresolvable(event_id: &str) -> serde_json::Value {
        let unknown_subscription = format!("sub_unknown_{}", Uuid::now_v7());
        json!({
            "id": event_id,
            "eventType": "dispute.created",
            "data": {
                "object": {
                    "id": format!("dispute_{}", Uuid::now_v7()),
                    "amount": 1000,
                    "currency": "USD",
                    "subscription": {
                        "id": unknown_subscription,
                        "product": "prod_unknown",
                    },
                    "transaction": {
                        "subscription": unknown_subscription,
                    }
                }
            }
        })
    }

    /// Build a `charge.refunded` Stripe topup event WITHOUT the `refunds`
    /// list. The topup branch must fail loud (400) rather than silently
    /// revoking from the cumulative `amount_refunded`.
    fn build_stripe_charge_refunded_topup_without_refunds(
        event_id: &str,
        realm_id: &str,
        user_id: Uuid,
        charge_id: &str,
        amount: i64,
        amount_refunded: i64,
    ) -> serde_json::Value {
        let mut event = build_stripe_charge_refunded_topup_event(
            event_id,
            realm_id,
            user_id,
            charge_id,
            amount,
            amount_refunded,
            &format!("re_{}", Uuid::now_v7()),
            amount_refunded,
        );
        event["data"]["object"]
            .as_object_mut()
            .unwrap()
            .remove("refunds");
        event
    }

    // =========================================================================
    // Per-refund incremental points revocation
    // =========================================================================

    /// Stripe anchor: a payment of 1000 grants 10000 points; refunds of 300
    /// + 200 must revoke 3000 + 2000 (per-refund increments), NOT 3000 + 5000
    /// (cumulative misread). The old bug fed the cumulative `amount_refunded`
    /// into the per-refund ratio, double-revoking already-refunded amounts on
    /// every later refund.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp001_stripe_multi_refund_revokes_per_refund_increment(
        ctx: &mut SchemaTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_ri_s1_stripe";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_ri_s1", webhook_secret).await;

        let user_id = create_test_user(ctx, &realm_id, "ri-s1-stripe@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id = create_stripe_one_time_mapping_with_role(
            ctx,
            &realm_id,
            "ri-s1-stripe",
            Some(10000),
            &[],
        )
        .await;
        let charge_id = format!("ch_ri_s1_{}", Uuid::now_v7());
        let attempt_id =
            create_stripe_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &charge_id, 1000)
                .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;

        // First refund: 300 of 1000 (amount_refunded = 300 cumulative).
        let event = build_stripe_charge_refunded_topup_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            300,
            &format!("re_ri_s1a_{}", Uuid::now_v7()),
            300,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(ledger.revoked_amount, 3000, "300/1000 refund revokes 3000");
        assert_eq!(ledger.remaining_amount, 7000);
        assert_eq!(
            get_topup_balance(ctx, user_id, &realm_id).await,
            7000,
            "wallet topup balance after first refund"
        );

        // Second refund: 200 of 1000, amount_refunded is now the CUMULATIVE
        // 500. Only the 200 increment may be revoked this time.
        let event = build_stripe_charge_refunded_topup_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            500,
            &format!("re_ri_s1b_{}", Uuid::now_v7()),
            200,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.revoked_amount, 5000,
            "second refund adds only its own 2000 (3000 + 2000), not the cumulative 5000"
        );
        assert_eq!(ledger.remaining_amount, 5000);
        assert_eq!(
            get_topup_balance(ctx, user_id, &realm_id).await,
            5000,
            "anchor: wallet holds exactly half the grant after refunding half the payment"
        );
    }

    /// P1-1/P1-2 regression: a Stripe Dashboard-initiated refund carries no
    /// `refundType` metadata, and the stored provider_reference of a
    /// fulfilled one-time attempt is the PaymentIntent id (pi_*) — the charge
    /// id (ch_*) never matches it. The handler must resolve the attempt via
    /// the charge's `payment_intent`, route into the topup clawback by the
    /// resolved one-time purchase, and revoke proportionally — instead of
    /// defaulting into the subscription branch's hard 400 while Stripe
    /// retries the delivery forever.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp001_stripe_dashboard_refund_resolves_attempt_via_payment_intent(
        ctx: &mut SchemaTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_ri_dash_stripe";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_ri_dash", webhook_secret).await;

        let user_id = create_test_user(ctx, &realm_id, "ri-dash-stripe@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id = create_stripe_one_time_mapping_with_role(
            ctx,
            &realm_id,
            "ri-dash-stripe",
            Some(10000),
            &[],
        )
        .await;
        // The stored reference of a FULFILLED hosted/PaymentIntent one-time
        // attempt is the PaymentIntent id, not the charge id.
        let payment_intent_id = format!("pi_ri_dash_{}", Uuid::now_v7());
        let charge_id = format!("ch_ri_dash_{}", Uuid::now_v7());
        let attempt_id = create_stripe_succeeded_attempt(
            ctx,
            &realm_id,
            user_id,
            mapping_id,
            &payment_intent_id,
            1000,
        )
        .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;

        // Dashboard-style refund: no refundType marker, charge names its PI.
        let event = build_stripe_charge_refunded_dashboard_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            Some(&payment_intent_id),
            1000,
            400,
            &format!("re_ri_dash_{}", Uuid::now_v7()),
            400,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.revoked_amount, 4000,
            "400/1000 dashboard refund revokes 4000 via the payment_intent-resolved attempt"
        );
        assert_eq!(ledger.remaining_amount, 6000);
        assert_eq!(get_topup_balance(ctx, user_id, &realm_id).await, 6000);
    }

    /// Creem anchor: the same 300 + 200 flow via Creem. Creem payloads
    /// carry only the per-refund amount (no cumulative field), so the
    /// cumulative total is derived from the recorded refund rows; the
    /// per-refund revoke semantics are the invariant pinned here.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp001_creem_multi_refund_revokes_per_refund_increment(
        ctx: &mut SchemaTestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user(ctx, &realm_id, "ri-s1-creem@test.com").await;
        let payment_id = format!("payment_ri_s1c_{}", Uuid::now_v7());

        create_points_wallet(ctx, user_id, &realm_id).await;
        ctx.with_creem_config(&realm_id, None, None, None).await;

        let bucket_id = get_wallet_bucket_id(ctx, &realm_id, user_id).await;
        let (attempt_id, mapping_id, rule_id) =
            create_payment_attempt_snapshot(ctx, &realm_id, user_id, &payment_id, bucket_id, 1000)
                .await;
        let ledger_id = seed_attributed_topup_ledger(
            ctx, &realm_id, user_id, attempt_id, mapping_id, rule_id, bucket_id, 10000, None,
        )
        .await;

        let app = ctx.create_unified_test_router();

        let event = build_refund_created_event_with_user(
            generate_test_event_id(),
            format!("refund_ri_s1a_{}", Uuid::now_v7()),
            payment_id.clone(),
            300,
            1000,
            &realm_id,
            user_id,
        );
        let response =
            send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(ledger.revoked_amount, 3000, "300/1000 refund revokes 3000");
        assert_eq!(ledger.remaining_amount, 7000);

        let event = build_refund_created_event_with_user(
            generate_test_event_id(),
            format!("refund_ri_s1b_{}", Uuid::now_v7()),
            payment_id.clone(),
            200,
            1000,
            &realm_id,
            user_id,
        );
        let response =
            send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.revoked_amount, 5000,
            "second refund adds only its own 2000 (3000 + 2000)"
        );
        assert_eq!(ledger.remaining_amount, 5000);
        assert_eq!(
            get_topup_balance(ctx, user_id, &realm_id).await,
            5000,
            "anchor: wallet holds exactly half the grant after refunding half the payment"
        );
    }

    /// With 6000 of the 10000-point grant consumed (4000 remaining), a
    /// 500/1000 refund must revoke only the remaining 4000: the revoke is
    /// capped at the remaining balance, never goes negative, and ledgers
    /// from other sources are untouched.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp001_refund_capped_at_remaining_and_other_sources_untouched(
        ctx: &mut SchemaTestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user(ctx, &realm_id, "ri-s2@test.com").await;
        let payment_id = format!("payment_ri_s2_{}", Uuid::now_v7());

        create_points_wallet(ctx, user_id, &realm_id).await;
        ctx.with_creem_config(&realm_id, None, None, None).await;

        let bucket_id = get_wallet_bucket_id(ctx, &realm_id, user_id).await;
        let (attempt_id, mapping_id, rule_id) =
            create_payment_attempt_snapshot(ctx, &realm_id, user_id, &payment_id, bucket_id, 1000)
                .await;
        let ledger_id = seed_attributed_topup_ledger(
            ctx, &realm_id, user_id, attempt_id, mapping_id, rule_id, bucket_id, 10000, None,
        )
        .await;

        // A registration-bonus ledger from another source must survive the
        // refund revocation untouched.
        let other_ledger_id = create_credit_ledger_entry_v2(
            ctx,
            user_id,
            &realm_id,
            CreditType::TopupCredit,
            CreditSourceType::Registration,
            "ri-s2-registration-bonus".to_string(),
            1000,
            None,
        )
        .await;

        consume_points_from_ledger(ctx, ledger_id, 6000).await;

        let app = ctx.create_unified_test_router();
        let event = build_refund_created_event_with_user(
            generate_test_event_id(),
            format!("refund_ri_s2_{}", Uuid::now_v7()),
            payment_id.clone(),
            500,
            1000,
            &realm_id,
            user_id,
        );
        let response =
            send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;
        assert_webhook_success(&response);

        // Proportional target is 5000 but only 4000 remain: revoke the
        // remainder, never below zero, and never touch consumed points.
        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.revoked_amount, 4000,
            "revocation is capped at the remaining 4000"
        );
        assert_eq!(ledger.remaining_amount, 0, "no negative balance");
        assert_eq!(
            ledger.used_amount, 6000,
            "consumed points are not clawed back"
        );

        let other = get_ledger_by_id(ctx, other_ledger_id).await;
        assert_eq!(
            other.remaining_amount, 1000,
            "registration-bonus ledger from another source is untouched"
        );
        assert_eq!(other.revoked_amount, 0);
        assert_eq!(
            get_topup_balance(ctx, user_id, &realm_id).await,
            1000,
            "wallet keeps exactly the other-source ledger"
        );
    }

    /// Replays must not double-revoke:
    /// (a) the same event id re-pushed is dropped at the payment_event layer;
    /// (b) the same refund id under a NEW event id passes event dedup and is
    /// dropped at the payment_refunds layer. No extra revocation record, and
    /// the retained role is never spuriously re-triggered.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp001_duplicate_event_and_refund_id_never_double_revoke(
        ctx: &mut SchemaTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_ri_s3";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_ri_s3", webhook_secret).await;

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "ri-s3-admin@test.com",
        )
        .await;
        let role_id = crate::tests::helpers::rbac_helpers::create_role(
            ctx,
            &realm_id,
            &token,
            "ri-s3-role",
            "refund idempotency role",
        )
        .await;

        let user_id = create_test_user(ctx, &realm_id, "ri-s3@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id =
            create_stripe_one_time_mapping_with_role(ctx, &realm_id, "ri-s3", Some(10000), &[])
                .await;
        let charge_id = format!("ch_ri_s3_{}", Uuid::now_v7());
        let attempt_id =
            create_stripe_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &charge_id, 1000)
                .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;
        seed_payment_role_grant(ctx, &realm_id, user_id, role_id, attempt_id).await;

        let refund_id_r1 = format!("re_ri_s3a_{}", Uuid::now_v7());
        let refund_id_r2 = format!("re_ri_s3b_{}", Uuid::now_v7());

        // Refund 1 (R1, 300) and refund 2 (R2, 200) — the anchor flow.
        let event1_id = generate_test_event_id();
        let event = build_stripe_charge_refunded_topup_event(
            &event1_id,
            &realm_id,
            user_id,
            &charge_id,
            1000,
            300,
            &refund_id_r1,
            300,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let event = build_stripe_charge_refunded_topup_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            500,
            &refund_id_r2,
            200,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(ledger.revoked_amount, 5000, "anchor state: 3000 + 2000");

        // (a) Re-push the exact same event id: dropped at payment_event dedup.
        let event = build_stripe_charge_refunded_topup_event(
            &event1_id,
            &realm_id,
            user_id,
            &charge_id,
            1000,
            300,
            &refund_id_r1,
            300,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        // (b) Same refund id R1 under a NEW event id: passes event dedup, is
        // dropped by the payment_refunds unique-refund-id insert.
        let event = build_stripe_charge_refunded_topup_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            500,
            &refund_id_r1,
            300,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.revoked_amount, 5000,
            "neither replay may revoke anything more"
        );
        assert_eq!(ledger.remaining_amount, 5000);

        let revocations = get_revocation_records(ctx, user_id).await;
        assert_eq!(
            revocations.len(),
            2,
            "exactly one revocation record per distinct refund id"
        );

        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            1,
            "partial refunds never revoke the role, and replays never re-trigger it"
        );
    }

    /// Refund 30% first, then push the
    /// cumulative to 100% with a second refund: the full-refund gate opens
    /// (cumulative >= original) and sweeps ALL remaining points to zero.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp001_cumulative_full_refund_sweeps_all_remaining(
        ctx: &mut SchemaTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_ri_s4";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_ri_s4", webhook_secret).await;

        let user_id = create_test_user(ctx, &realm_id, "ri-s4@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id =
            create_stripe_one_time_mapping_with_role(ctx, &realm_id, "ri-s4", Some(10000), &[])
                .await;
        let charge_id = format!("ch_ri_s4_{}", Uuid::now_v7());
        let attempt_id =
            create_stripe_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &charge_id, 1000)
                .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;

        // 30% first: revoke 3000, remaining 7000.
        let event = build_stripe_charge_refunded_topup_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            300,
            &format!("re_ri_s4a_{}", Uuid::now_v7()),
            300,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(ledger.revoked_amount, 3000);

        // Push cumulative to 100%: refund 700 (amount_refunded = 1000).
        let event = build_stripe_charge_refunded_topup_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            1000,
            &format!("re_ri_s4b_{}", Uuid::now_v7()),
            700,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.revoked_amount, 10000,
            "full-refund gate sweeps the whole grant"
        );
        assert_eq!(ledger.remaining_amount, 0);
        assert_eq!(ledger.status, CreditLedgerStatus::Revoked);
        assert_eq!(
            get_topup_balance(ctx, user_id, &realm_id).await,
            0,
            "wallet balance is zero after the cumulative full refund"
        );
    }

    // =========================================================================
    // Cumulative-full-refund gate for role revocation
    // =========================================================================

    /// A partial refund (300 of 1000) revokes
    /// its proportional 3000 points but KEEPS the permanent payment-source
    /// role; a manual grant of the same role is equally untouched.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp002_partial_refund_keeps_permanent_role(ctx: &mut SchemaTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_ri_rp1";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_ri_rp1", webhook_secret).await;

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "ri-rp1-admin@test.com",
        )
        .await;
        let role_id = crate::tests::helpers::rbac_helpers::create_role(
            ctx,
            &realm_id,
            &token,
            "ri-rp1-role",
            "partial refund retention role",
        )
        .await;

        let user_id = create_test_user(ctx, &realm_id, "ri-rp1@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id = create_stripe_one_time_mapping_with_role(
            ctx,
            &realm_id,
            "ri-rp1",
            Some(10000),
            &[role_id],
        )
        .await;
        let charge_id = format!("ch_ri_rp1_{}", Uuid::now_v7());
        let attempt_id =
            create_stripe_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &charge_id, 1000)
                .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;
        seed_payment_role_grant(ctx, &realm_id, user_id, role_id, attempt_id).await;
        seed_manual_role_grant(ctx, &realm_id, user_id, role_id).await;

        let event = build_stripe_charge_refunded_topup_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            300,
            &format!("re_ri_rp1_{}", Uuid::now_v7()),
            300,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(ledger.revoked_amount, 3000, "points are still revoked");
        assert_eq!(ledger.remaining_amount, 7000);

        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            1,
            "a partial refund must NOT revoke the permanent payment-source role"
        );
        assert_eq!(
            count_manual_roles_for_role(ctx, user_id, role_id).await,
            1,
            "manual grant of the same role survives"
        );
    }

    /// After 60% refunded the role is kept;
    /// pushing the cumulative to 100% sweeps the remaining points AND revokes
    /// the payment-source role, while the manual grant survives.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp002_cumulative_full_refund_revokes_payment_role(
        ctx: &mut SchemaTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_ri_rp2";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_ri_rp2", webhook_secret).await;

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "ri-rp2-admin@test.com",
        )
        .await;
        let role_id = crate::tests::helpers::rbac_helpers::create_role(
            ctx,
            &realm_id,
            &token,
            "ri-rp2-role",
            "cumulative full refund role",
        )
        .await;

        let user_id = create_test_user(ctx, &realm_id, "ri-rp2@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id = create_stripe_one_time_mapping_with_role(
            ctx,
            &realm_id,
            "ri-rp2",
            Some(10000),
            &[role_id],
        )
        .await;
        let charge_id = format!("ch_ri_rp2_{}", Uuid::now_v7());
        let attempt_id =
            create_stripe_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &charge_id, 1000)
                .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;
        seed_payment_role_grant(ctx, &realm_id, user_id, role_id, attempt_id).await;
        seed_manual_role_grant(ctx, &realm_id, user_id, role_id).await;

        // 60% first: points revoked proportionally, role retained.
        let event = build_stripe_charge_refunded_topup_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            600,
            &format!("re_ri_rp2a_{}", Uuid::now_v7()),
            600,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(ledger.revoked_amount, 6000);
        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            1,
            "60% cumulative keeps the role"
        );

        // Push to 100%: remaining points swept + payment role revoked.
        let event = build_stripe_charge_refunded_topup_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            1000,
            &format!("re_ri_rp2b_{}", Uuid::now_v7()),
            400,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(ledger.revoked_amount, 10000, "remaining points swept");
        assert_eq!(ledger.remaining_amount, 0);
        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            0,
            "cumulative 100% refund revokes the payment-source role"
        );
        assert_eq!(
            count_manual_roles_for_role(ctx, user_id, role_id).await,
            1,
            "manual grant of the same role survives the revocation"
        );
    }

    /// At 60% then 90% cumulative: incremental
    /// points revocation continues (6000 + 3000) but the role is still kept
    /// because the cumulative never reaches 100%.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp002_ninety_percent_cumulative_keeps_role(ctx: &mut SchemaTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_ri_rp3";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_ri_rp3", webhook_secret).await;

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "ri-rp3-admin@test.com",
        )
        .await;
        let role_id = crate::tests::helpers::rbac_helpers::create_role(
            ctx,
            &realm_id,
            &token,
            "ri-rp3-role",
            "ninety percent retention role",
        )
        .await;

        let user_id = create_test_user(ctx, &realm_id, "ri-rp3@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id = create_stripe_one_time_mapping_with_role(
            ctx,
            &realm_id,
            "ri-rp3",
            Some(10000),
            &[role_id],
        )
        .await;
        let charge_id = format!("ch_ri_rp3_{}", Uuid::now_v7());
        let attempt_id =
            create_stripe_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &charge_id, 1000)
                .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;
        seed_payment_role_grant(ctx, &realm_id, user_id, role_id, attempt_id).await;

        for (refund_amount, amount_refunded) in [(600, 600), (300, 900)] {
            let event = build_stripe_charge_refunded_topup_event(
                &generate_test_event_id(),
                &realm_id,
                user_id,
                &charge_id,
                1000,
                amount_refunded,
                &format!("re_ri_rp3_{}_{}", refund_amount, Uuid::now_v7()),
                refund_amount,
            );
            let response =
                send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
            assert_webhook_success(&response);
        }

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.revoked_amount, 9000,
            "incremental revocation continues across refunds (6000 + 3000)"
        );
        assert_eq!(ledger.remaining_amount, 1000);
        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            1,
            "90% cumulative still keeps the role: only 100% revokes"
        );
    }

    /// Self-heal: the payment-role revocation
    /// after a cumulative full refund is best-effort (logged, not propagated).
    /// When it fails transiently, or the process dies between the refund
    /// commit and the event being marked processed, the re-pushed refund must
    /// revoke the roles again: the duplicate branch reports the recorded
    /// row's real gate state (fully_refunded=true) and the idempotent role
    /// revoke re-runs, while the per-refund points revocation stays deduped —
    /// no second sweep, no extra revocation record.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp002_full_refund_replay_self_heals_role_revocation(
        ctx: &mut SchemaTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_ri_rp4";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_ri_rp4", webhook_secret).await;

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "ri-rp4-admin@test.com",
        )
        .await;
        let role_id = crate::tests::helpers::rbac_helpers::create_role(
            ctx,
            &realm_id,
            &token,
            "ri-rp4-role",
            "full refund self-heal role",
        )
        .await;

        let user_id = create_test_user(ctx, &realm_id, "ri-rp4@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id = create_stripe_one_time_mapping_with_role(
            ctx,
            &realm_id,
            "ri-rp4",
            Some(10000),
            &[role_id],
        )
        .await;
        let charge_id = format!("ch_ri_rp4_{}", Uuid::now_v7());
        let attempt_id =
            create_stripe_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &charge_id, 1000)
                .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;
        seed_payment_role_grant(ctx, &realm_id, user_id, role_id, attempt_id).await;

        // Full refund 1000/1000 in a single event: gate opens, points swept.
        let refund_id = format!("re_ri_rp4_{}", Uuid::now_v7());
        let event = build_stripe_charge_refunded_topup_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            1000,
            &refund_id,
            1000,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(ledger.revoked_amount, 10000, "full refund sweeps the grant");
        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            0,
            "first delivery revoked the payment-source role"
        );

        // Simulate the missed revocation: the best-effort role revoke after
        // the refund transaction committed failed transiently, leaving the
        // grant in place (re-seeded here as the state that failure leaves).
        seed_payment_role_grant(ctx, &realm_id, user_id, role_id, attempt_id).await;
        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            1,
            "precondition: the role is back as if the first revoke never landed"
        );

        // Re-push the SAME refund id under a NEW event id — Stripe's
        // re-delivery shape for a retried/displaced event.
        let event = build_stripe_charge_refunded_topup_event(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            1000,
            &refund_id,
            1000,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_webhook_success(&response);

        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            0,
            "duplicate re-delivery of a full refund re-runs the idempotent role revoke (self-heal)"
        );
        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.revoked_amount, 10000,
            "the replay must not revoke points a second time"
        );
        assert_eq!(ledger.remaining_amount, 0);
        let revocations = get_revocation_records(ctx, user_id).await;
        assert_eq!(
            revocations.len(),
            1,
            "exactly one points revocation record for the single refund id"
        );
    }

    /// Stripe dispute: a dispute on a one-time
    /// purchase maps to no local subscription; the dispute handler ignores
    /// it. Pinning current behavior: no role revocation, no points change.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp002_stripe_dispute_on_one_time_purchase_ignored(
        ctx: &mut SchemaTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_ri_dp_s";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_ri_dp_s", webhook_secret).await;

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "ri-dp-s-admin@test.com",
        )
        .await;
        let role_id = crate::tests::helpers::rbac_helpers::create_role(
            ctx,
            &realm_id,
            &token,
            "ri-dp-s-role",
            "stripe dispute retention role",
        )
        .await;

        let user_id = create_test_user(ctx, &realm_id, "ri-dp-s@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id = create_stripe_one_time_mapping_with_role(
            ctx,
            &realm_id,
            "ri-dp-s",
            Some(10000),
            &[role_id],
        )
        .await;
        let charge_id = format!("ch_ri_dp_s_{}", Uuid::now_v7());
        let attempt_id =
            create_stripe_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &charge_id, 1000)
                .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;
        seed_payment_role_grant(ctx, &realm_id, user_id, role_id, attempt_id).await;

        let event = build_stripe_dispute_created_unmapped(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "unmappable dispute is acknowledged (ignored), not failed"
        );

        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            1,
            "dispute events never revoke the one-time purchase role (current behavior)"
        );
        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.remaining_amount, 10000,
            "points retention not invalidated"
        );
        assert_eq!(ledger.revoked_amount, 0);
    }

    /// Creem dispute: a Creem dispute referencing
    /// an unresolvable subscription answers 400; the one-time role and points
    /// retention are untouched. Pins current behavior.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_us_rp002_creem_dispute_without_subscription_rejected(
        ctx: &mut SchemaTestContext,
    ) {
        let realm_id = ctx._realm_id.clone();

        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "ri-dp-c-admin@test.com",
        )
        .await;
        let role_id = crate::tests::helpers::rbac_helpers::create_role(
            ctx,
            &realm_id,
            &token,
            "ri-dp-c-role",
            "creem dispute retention role",
        )
        .await;

        let user_id = create_test_user(ctx, &realm_id, "ri-dp-c@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id = create_stripe_one_time_mapping_with_role(
            ctx,
            &realm_id,
            "ri-dp-c",
            Some(10000),
            &[role_id],
        )
        .await;
        let charge_id = format!("ch_ri_dp_c_{}", Uuid::now_v7());
        let attempt_id =
            create_stripe_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &charge_id, 1000)
                .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;
        seed_payment_role_grant(ctx, &realm_id, user_id, role_id, attempt_id).await;

        ctx.with_creem_config(&realm_id, None, None, None).await;
        let app = ctx.create_unified_test_router();

        let event = build_creem_dispute_unresolvable(&generate_test_event_id());
        let response =
            send_webhook_with_signature(&app, &realm_id, event, "test_webhook_secret").await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "dispute handling is subscription-only: unresolvable subscription fails loud"
        );

        assert_eq!(
            count_payment_roles_by_source_id(ctx, user_id, &attempt_id.to_string()).await,
            1,
            "dispute events never revoke the one-time purchase role (current behavior)"
        );
        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.remaining_amount, 10000,
            "points retention not invalidated"
        );
        assert_eq!(ledger.revoked_amount, 0);
    }

    // =========================================================================
    // Fail-loud contract
    // =========================================================================

    /// Fail-loud: a Stripe topup `charge.refunded` payload without
    /// the `refunds` list must be rejected with 400 instead of silently
    /// revoking from the cumulative `amount_refunded` (the original bug
    /// vector). Stripe retries + the compensation sweep re-deliver the full
    /// object.
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_stripe_topup_refund_without_refunds_list_rejected(ctx: &mut SchemaTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_ri_fail";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_ri_fail", webhook_secret).await;

        let user_id = create_test_user(ctx, &realm_id, "ri-fail@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id =
            create_stripe_one_time_mapping_with_role(ctx, &realm_id, "ri-fail", Some(10000), &[])
                .await;
        let charge_id = format!("ch_ri_fail_{}", Uuid::now_v7());
        let attempt_id =
            create_stripe_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &charge_id, 1000)
                .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;

        let event = build_stripe_charge_refunded_topup_without_refunds(
            &generate_test_event_id(),
            &realm_id,
            user_id,
            &charge_id,
            1000,
            500,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, event, webhook_secret).await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "topup refund payload without refunds data must fail loud, not revoke from the cumulative"
        );

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.revoked_amount, 0,
            "nothing revoked on the rejected payload"
        );
        assert_eq!(ledger.remaining_amount, 10000);
    }

    /// 回归（审计 run-1：creem-refund-subscription-realm-unchecked）：订阅退款
    /// 臂按 (external_subscription_id, provider) 无域解析目标订阅，未校验
    /// sub.realm_id == 路径 realm —— handle_subscription_cancel 以传入的
    /// realm_id 为键执行撤销，他域订阅会得到零行撤销却被当作成功、事件在
    /// 接收域被标记 processed：钱已退、所属域的配额/角色永不追回（PRD 4.1
    /// 称之为 P0）。修复：镜像 sync_subscription 的 Forbidden 守卫。
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_creem_subscription_refund_foreign_realm_subscription_rejected(
        ctx: &mut SchemaTestContext,
    ) {
        let realm_a = ctx._realm_id.clone();
        let user_id = create_test_user(ctx, &realm_a, "ri-sfr-a@test.com").await;
        ctx.with_creem_config(&realm_a, None, None, None).await;

        // Realm B owns the subscription; the refund is delivered to realm A.
        let realm_b = format!("ri-sfr-b-{}", Uuid::now_v7().simple());
        sqlx::query("INSERT INTO realm (id, name) VALUES ($1, 'ri-sfr-realm-b')")
            .bind(&realm_b)
            .execute(&ctx._app_state.pool)
            .await
            .unwrap();
        let owner_b = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO account (id, realm_id, email, password, status)
             VALUES ($1, $2, $3, '$2a$12$dummy_password_hash', 1)",
        )
        .bind(owner_b)
        .bind(&realm_b)
        .bind(format!("ri-sfr-owner-{}@test.com", Uuid::now_v7().simple()))
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();
        let external_sub = format!("sub_ri_sfr_{}", Uuid::now_v7().simple());
        sqlx::query(
            "INSERT INTO subscription
                (id, realm_id, user_id, external_subscription_id, external_product_id,
                 payment_provider, status, entitlement_key, synced_at,
                 current_period_start, current_period_end, cancel_at_period_end,
                 cancel_at, created_at, updated_at, billing_type)
             VALUES ($1, $2, $3, $4, 'prod.ri.sfr',
                     'creem', 'active', 'pro', NOW(),
                     NOW(), NOW() + INTERVAL '5 days', false,
                     NULL, NOW(), NOW(), 'recurring')",
        )
        .bind(Uuid::now_v7())
        .bind(&realm_b)
        .bind(owner_b)
        .bind(&external_sub)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

        // The refund pre-branch resolves the originating attempt by
        // payment_id unconditionally (fail-loud 400 without a snapshot), so
        // seed an attempt in realm A owned by the payload user.
        let payment_id = format!("payment_ri_sfr_{}", Uuid::now_v7().simple());
        sqlx::query(
            "INSERT INTO payment_attempts
                (id, realm_id, user_id, payment_provider, target_type, target_id,
                 amount, currency, status, provider_reference, provider_status,
                 expires_at, created_at, updated_at)
             VALUES ($1, $2, $3, 'creem', 'entitlement_mapping', $4,
                     1000, 'usd', 'Succeeded', $5, 'succeeded',
                     NOW() + INTERVAL '1 hour', NOW(), NOW())",
        )
        .bind(Uuid::now_v7())
        .bind(&realm_a)
        .bind(user_id)
        .bind(Uuid::now_v7())
        .bind(&payment_id)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

        let app = ctx.create_unified_test_router();
        let event = json!({
            "id": generate_test_event_id(),
            "eventType": "refund.created",
            "data": {
                "object": {
                    "id": format!("refund_ri_sfr_{}", Uuid::now_v7().simple()),
                    "paymentId": payment_id,
                    "subscriptionId": external_sub,
                    "amount": 500,
                    "originalAmount": 1000,
                    "currency": "USD",
                    "reason": "customer_request",
                    "metadata": {
                        "userId": user_id.to_string(),
                        "refundType": "subscription"
                    }
                }
            },
            "metadata": { "realmId": realm_a }
        });
        let response =
            send_webhook_with_signature(&app, &realm_a, event, "test_webhook_secret").await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "a refund signed for realm A must not cancel realm B's subscription"
        );

        // Realm B's subscription must be untouched (old code: silently canceled).
        let status: String = sqlx::query_scalar(
            "SELECT status FROM subscription WHERE external_subscription_id = $1 AND payment_provider = 'creem'",
        )
        .bind(&external_sub)
        .fetch_one(&ctx._app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            status, "active",
            "the owning realm's subscription must stay active"
        );
    }
}
