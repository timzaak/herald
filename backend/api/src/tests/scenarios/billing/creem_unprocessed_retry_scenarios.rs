// =============================================================================
// Creem Unprocessed-Event Retry Scenario Tests
// =============================================================================
//
// Regression (audit run-1 confirmed finding
// herald.api-billing/webhook_handlers.rs/
// creem-webhook-unprocessed-event-acked-and-tombstoned):
//
// A Creem event whose first processing fails must remain retryable. The
// webhook duplicate check must be processed-aware (200 only for
// processed=true rows; a processed=false row is a prior failed attempt and
// must be retried by reusing the row), and the compensation/reprocess path
// must reuse unprocessed rows instead of skipping them — mirroring the
// Stripe sibling contract — so a failed refund/revoke can never be
// acknowledged and then tombstoned by the PaymentEventRetryJob without its
// side effects ever running (PRD 4.1: a missed revoke is a P0 fault).
//
// The Stripe arm is the positive control: the same fail-then-retry sequence
// must work through the Stripe handler, pinning the contract the Creem side
// was aligned to.
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::WebhookEventProcessorImpl;
    use crate::tests::helpers::async_payment_helpers::create_test_user;
    use crate::tests::helpers::billing_helpers::setup_stripe_config;
    use crate::tests::helpers::billing_seed_helpers::{
        create_stripe_one_time_mapping_with_role, create_stripe_succeeded_attempt,
    };
    use crate::tests::helpers::points_helpers::{
        create_payment_attempt_snapshot, create_points_wallet, get_ledger_by_id,
        get_wallet_bucket_id, seed_attributed_topup_ledger,
        seed_fulfilled_topup_ledger_for_attempt,
    };
    use crate::tests::helpers::webhook_helpers::{
        build_refund_created_event_with_user, build_stripe_charge_refunded_topup_event,
        generate_test_event_id, send_stripe_webhook_with_signature, send_webhook_with_signature,
    };
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::http::StatusCode;
    use herald_worker::PaymentEventRetryJob;
    use std::sync::Arc;
    use test_context::test_context;
    use uuid::Uuid;

    async fn payment_event_row(
        ctx: &SchemaTestContext,
        realm_id: &str,
        external_event_id: &str,
        provider: &str,
    ) -> (bool, Option<chrono::NaiveDateTime>) {
        sqlx::query_as::<_, (bool, Option<chrono::NaiveDateTime>)>(
            "SELECT processed, next_retry_at FROM payment_event \
             WHERE realm_id = $1 AND external_event_id = $2 AND payment_provider = $3",
        )
        .bind(realm_id)
        .bind(external_event_id)
        .bind(provider)
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("payment_event row should exist")
    }

    /// US：Creem 首投失败的事件必须保持可重试 —— 重投不被判重分支 200 吞掉，
    /// retry job 用真实 WebhookEventProcessorImpl 重跑后 revoke 真正执行。
    /// 旧代码：重投直接 200（processed 仍 false），retry job 把行 tombstone 成
    /// processed=true 而 revoke 从未执行 —— 钱已退、点数未收回。
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_creem_failed_event_redelivery_retries_and_retry_job_revokes(
        ctx: &mut SchemaTestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let user_id = create_test_user(ctx, &realm_id, "creem-retry@test.com").await;
        // Deliberately NO payment_attempt snapshot for this payment yet — the
        // refund handler fails deterministically (BadRequest: cannot resolve
        // bucket), standing in for any first-delivery failure.
        let payment_id = format!("payment_creem_retry_{}", Uuid::now_v7());

        create_points_wallet(ctx, user_id, &realm_id).await;
        ctx.with_creem_config(&realm_id, None, None, None).await;

        let app = ctx.create_unified_test_router();
        let event_id = generate_test_event_id();
        let event = build_refund_created_event_with_user(
            event_id.clone(),
            format!("refund_creem_retry_{}", Uuid::now_v7()),
            payment_id.clone(),
            300,
            1000,
            &realm_id,
            user_id,
        );

        // Step 1: first delivery fails 400, leaving an unprocessed row.
        let response =
            send_webhook_with_signature(&app, &realm_id, event.clone(), "test_webhook_secret")
                .await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "first delivery with no attempt snapshot must fail loud"
        );
        let (processed, next_retry_at) =
            payment_event_row(ctx, &realm_id, &event_id, "creem").await;
        assert!(
            !processed,
            "failed first delivery must leave processed=false"
        );
        assert!(next_retry_at.is_none(), "row is sweep-eligible immediately");

        // Step 2: redelivering the same event while the failure persists must
        // NOT be acknowledged as a duplicate — the handler re-runs and fails
        // again (old code returned 200 here and swallowed the event).
        let response =
            send_webhook_with_signature(&app, &realm_id, event.clone(), "test_webhook_secret")
                .await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "redelivery of an unprocessed event must retry the handler, not ack it"
        );
        let (processed, _) = payment_event_row(ctx, &realm_id, &event_id, "creem").await;
        assert!(
            !processed,
            "still-failing redelivery must keep processed=false"
        );

        // Step 3: repair the data state (create the attempt snapshot + ledger
        // the refund claws back against).
        let bucket_id = get_wallet_bucket_id(ctx, &realm_id, user_id).await;
        let (attempt_id, mapping_id, rule_id) =
            create_payment_attempt_snapshot(ctx, &realm_id, user_id, &payment_id, bucket_id, 1000)
                .await;
        let ledger_id = seed_attributed_topup_ledger(
            ctx, &realm_id, user_id, attempt_id, mapping_id, rule_id, bucket_id, 10000, None,
        )
        .await;

        // Step 4: run the PaymentEventRetryJob once with the REAL
        // WebhookEventProcessorImpl — the unprocessed row must be re-processed
        // for real and the revoke applied (old code: Ok(()) skip tombstoned
        // the row without any revoke).
        let job = PaymentEventRetryJob::new(
            ctx.app_state.pool.clone(),
            Arc::new(WebhookEventProcessorImpl::new((*ctx.app_state).clone())),
            50,
            60,
        );
        let stats = job.run().await.expect("retry job should succeed");
        assert!(
            stats.reprocessed >= 1,
            "retry job should re-process the unprocessed Creem row, got {stats:?}"
        );

        let (processed, _) = payment_event_row(ctx, &realm_id, &event_id, "creem").await;
        assert!(
            processed,
            "successfully retried event must be marked processed"
        );

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(
            ledger.revoked_amount, 3000,
            "the retried refund must revoke its 300/1000 share — never silently drop the clawback"
        );
        assert_eq!(ledger.remaining_amount, 7000);
    }

    /// 阳性对照：同样的“首投失败→修复数据→重投”序列在 Stripe 一侧成立，
    /// 固化 Creem 侧被对齐的兄弟契约。
    #[test_context(SchemaTestContext)]
    #[tokio::test]
    async fn test_stripe_failed_event_redelivery_retries_positive_control(
        ctx: &mut SchemaTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_creem_retry_ctl";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_creem_retry_ctl", webhook_secret).await;

        let user_id = create_test_user(ctx, &realm_id, "stripe-retry-ctl@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let mapping_id = create_stripe_one_time_mapping_with_role(
            ctx,
            &realm_id,
            "stripe-retry-ctl",
            Some(10000),
            &[],
        )
        .await;
        let charge_id = format!("ch_stripe_retry_ctl_{}", Uuid::now_v7());
        let attempt_id =
            create_stripe_succeeded_attempt(ctx, &realm_id, user_id, mapping_id, &charge_id, 1000)
                .await;
        let ledger_id = seed_fulfilled_topup_ledger_for_attempt(
            ctx, &realm_id, user_id, attempt_id, 10000, None,
        )
        .await;

        // First delivery: refund payload WITHOUT the refunds list → 400
        // (pinned behavior), leaving the row unprocessed.
        let event_id = generate_test_event_id();
        let mut bad_event = build_stripe_charge_refunded_topup_event(
            &event_id,
            &realm_id,
            user_id,
            &charge_id,
            1000,
            300,
            &format!("re_ctl_{}", Uuid::now_v7()),
            300,
        );
        bad_event["data"]["object"]
            .as_object_mut()
            .unwrap()
            .remove("refunds");
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, bad_event, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let (processed, _) = payment_event_row(ctx, &realm_id, &event_id, "stripe").await;
        assert!(
            !processed,
            "failed Stripe delivery must leave processed=false"
        );

        // Redelivery of the SAME event id with the refunds list present must
        // reuse the unprocessed row and complete the revoke.
        let good_event = build_stripe_charge_refunded_topup_event(
            &event_id,
            &realm_id,
            user_id,
            &charge_id,
            1000,
            300,
            &format!("re_ctl_{}", Uuid::now_v7()),
            300,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, good_event, webhook_secret).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "redelivery of an unprocessed Stripe event must retry and succeed"
        );
        let (processed, _) = payment_event_row(ctx, &realm_id, &event_id, "stripe").await;
        assert!(processed);

        let ledger = get_ledger_by_id(ctx, ledger_id).await;
        assert_eq!(ledger.revoked_amount, 3000);
        assert_eq!(ledger.remaining_amount, 7000);
    }
}
