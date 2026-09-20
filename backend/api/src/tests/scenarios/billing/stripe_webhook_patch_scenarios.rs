// =============================================================================
// Stripe Webhook Patch Scenario Tests
// =============================================================================
//
// Tests for Stripe webhook handlers added in the billing patch:
// - Checkout lifecycle: expired, async_payment_succeeded/failed, completed(unpaid)
// - Disputes: created, closed (won/lost)
// - Subscription lifecycle: paused, resumed, updated (scheduled_cancel/reactivate), deleted
// - Idempotency: checkout.session.expired duplicate event_id
//
// Covers: checkout.session.expired/async_*, dispute.created/closed,
//         subscription paused/resumed/updated/deleted
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::billing_helpers::{
        setup_stripe_config, setup_test_entitlement_mapping_full,
    };
    use crate::tests::helpers::webhook_helpers::{
        generate_test_event_id, send_stripe_webhook_with_signature,
    };
    use crate::tests::schema_test_context::SchemaTestContext;
    use axum::http::StatusCode;
    use serde_json::json;
    use test_context::test_context;
    use uuid::Uuid;

    use SchemaTestContext as WebhookPatchTestContext;

    // =========================================================================
    // Shared Helpers
    // =========================================================================

    /// Create a test user in the test realm and return their UUID.
    async fn create_test_user(ctx: &SchemaTestContext, realm_id: &str, email: &str) -> Uuid {
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

    /// Create a points wallet for a user with zero balances.
    async fn create_points_wallet(ctx: &SchemaTestContext, user_id: Uuid, realm_id: &str) {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;

        sqlx::query(
            "INSERT INTO points_wallets (id, user_id, realm_id, bucket_id, total_topup_granted, total_subscription_granted, total_recharged, total_consumed, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, 0, 0, 0, 0, 'active', NOW(), NOW())
             ON CONFLICT (realm_id, user_id, bucket_id) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(realm_id)
        .bind(bucket_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create points wallet");
    }

    #[allow(dead_code)]
    /// Create a points wallet for a user with an initial subscription_balance.
    async fn create_points_wallet_with_sub_balance(
        ctx: &SchemaTestContext,
        user_id: Uuid,
        realm_id: &str,
        subscription_balance: i64,
    ) {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;

        sqlx::query(
            "INSERT INTO points_wallets (id, user_id, realm_id, bucket_id, total_topup_granted, total_subscription_granted, total_recharged, total_consumed, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, 0, $5, $5, 0, 'active', NOW(), NOW())
             ON CONFLICT (realm_id, user_id, bucket_id) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(realm_id)
        .bind(bucket_id)
        .bind(subscription_balance)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create points wallet with sub balance");

        // points_wallets no longer holds Stored per-type balances; the
        // requested subscription_balance is seeded as a ledger row so the
        // derived balance reflects it (mirrors create_points_wallet_with_balance).
        if subscription_balance > 0 {
            sqlx::query(
                "INSERT INTO points_credit_ledger (id, user_id, realm_id, bucket_id, credit_type, source_type, source_id, granted_amount, used_amount, revoked_amount, status, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, 'subscription_credit', 'system_grant', $5, $6, 0, 0, 'active', NOW(), NOW())",
            )
            .bind(Uuid::now_v7())
            .bind(user_id)
            .bind(realm_id)
            .bind(bucket_id)
            .bind(format!("seed-sub-{}", user_id))
            .bind(subscription_balance)
            .execute(&ctx.app_state.pool)
            .await
            .expect("Failed to seed subscription ledger for create_points_wallet_with_sub_balance");
        }
    }

    /// Create a quota-entitlement-backed subscription credit entry attributed
    /// to a completed subscription-period distribution event + rule, mirroring
    /// production `handle_subscription_paid` output. The attributed event/rule
    /// are required for a subsequent cancel/refund/expiry/dispute webhook's
    /// `revoke_distribution_source_in_tx` to find and revoke the entitlement
    /// (it JOINs `q.distribution_event_id = e.id` and requires
    /// `q.distribution_rule_id IS NOT NULL`).
    async fn create_subscription_credit_with_ledger(
        ctx: &SchemaTestContext,
        user_id: Uuid,
        realm_id: &str,
        amount: i64,
        source_id: &str,
    ) {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        // `source_id` arrives as `"{entitlement_key}:{subscription_uuid}"; the
        // subscription UUID is the revoke key production matches on.
        let subscription_id = source_id
            .rsplit(':')
            .next()
            .filter(|part| Uuid::parse_str(part).is_ok())
            .map(|part| Uuid::parse_str(part).expect("subscription id suffix is a valid UUID"))
            .expect("source_id must carry a subscription UUID suffix");
        let effective_from = chrono::Utc::now() - chrono::Duration::seconds(1);
        let effective_until = Some(chrono::Utc::now() + chrono::Duration::days(30));

        crate::tests::helpers::points_helpers::seed_attributed_subscription_quota(
            ctx,
            realm_id,
            user_id,
            subscription_id,
            bucket_id,
            herald_core::domain::points::entities::CreditType::SubscriptionCredit,
            herald_core::domain::points::entities::QuotaSourceType::SubscriptionInitial,
            &[(2_592_000, amount, "period")],
            effective_from,
            effective_until,
        )
        .await;

        sqlx::query(
            "UPDATE points_wallets
             SET total_subscription_granted = total_subscription_granted + $1,
                 updated_at = NOW()
             WHERE user_id = $2 AND realm_id = $3",
        )
        .bind(amount)
        .bind(user_id)
        .bind(realm_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to update wallet balance");
    }

    /// Create a one-time entitlement mapping with points.
    /// Returns the mapping ID.
    async fn create_one_time_mapping(
        ctx: &mut SchemaTestContext,
        realm_id: &str,
        provider: &str,
        entitlement_key: &str,
        points_per_period: i64,
    ) -> Uuid {
        setup_test_entitlement_mapping_full(
            ctx,
            realm_id,
            provider,
            &format!("prod_{}_{}", provider, entitlement_key),
            None,
            entitlement_key,
            Some("one_time"),
            None,
            Some(points_per_period),
            None,
            None,
            false,
            None,
            true,
            None,
        )
        .await
    }

    /// Create a recurring entitlement mapping with points.
    /// Returns the mapping ID.
    async fn create_recurring_mapping(
        ctx: &mut SchemaTestContext,
        realm_id: &str,
        provider: &str,
        entitlement_key: &str,
        points_per_period: i64,
    ) -> Uuid {
        setup_test_entitlement_mapping_full(
            ctx,
            realm_id,
            provider,
            &format!("prod_{}_{}", provider, entitlement_key),
            None,
            entitlement_key,
            Some("recurring"),
            Some("monthly"),
            Some(points_per_period),
            None,
            None,
            true,
            None,
            true,
            None,
        )
        .await
    }

    /// Pre-create a subscription via SQL with the given fields.
    /// Returns the subscription ID (internal UUID).
    async fn pre_create_subscription(
        ctx: &SchemaTestContext,
        realm_id: &str,
        user_id: Uuid,
        client_app_id: Uuid,
        external_subscription_id: &str,
        external_product_id: &str,
        payment_provider: &str,
        status: &str,
        entitlement_key: &str,
    ) -> Uuid {
        let subscription_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO subscription
                (id, realm_id, user_id, external_subscription_id, external_product_id,
                 payment_provider, status, entitlement_key, external_price_id,
                 provider_metadata, synced_at, current_period_start, current_period_end,
                 cancel_at_period_end, client_app_id, cancel_at, created_at, updated_at, billing_type)
             VALUES ($1, $2, $3, $4, $5,
                     $6, $7, $8, NULL,
                     NULL, NOW(), NOW(), NOW() + INTERVAL '30 days',
                     false, $9, NULL, NOW(), NOW(), 'recurring')",
        )
        .bind(subscription_id)
        .bind(realm_id)
        .bind(user_id)
        .bind(external_subscription_id)
        .bind(external_product_id)
        .bind(payment_provider)
        .bind(status)
        .bind(entitlement_key)
        .bind(client_app_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to pre-create subscription");
        subscription_id
    }

    /// Get subscription status by internal ID.
    async fn get_subscription_status(ctx: &SchemaTestContext, subscription_id: Uuid) -> String {
        sqlx::query_scalar::<_, String>("SELECT status FROM subscription WHERE id = $1")
            .bind(subscription_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("Subscription not found")
    }

    /// Get cancel_at_period_end for a subscription.
    async fn get_cancel_at_period_end(ctx: &SchemaTestContext, subscription_id: Uuid) -> bool {
        sqlx::query_scalar::<_, bool>(
            "SELECT COALESCE(cancel_at_period_end, false) FROM subscription WHERE id = $1",
        )
        .bind(subscription_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap_or(false)
    }

    /// Get topup_balance for a user's wallet.
    async fn get_topup_balance(ctx: &SchemaTestContext, user_id: Uuid, realm_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(l.remaining_amount) FILTER (
                        WHERE l.status = 'active' AND l.remaining_amount > 0
                          AND l.credit_type IN ('topup_credit','registration_credit','free_periodic_credit')
                          AND (l.effective_at IS NULL OR l.effective_at <= NOW())
                          AND (l.expires_at  IS NULL OR l.expires_at  >  NOW())
                    ), 0)::BIGINT
             FROM points_wallets w
             LEFT JOIN points_credit_ledger l
               ON l.realm_id = w.realm_id AND l.user_id = w.user_id AND l.bucket_id = w.bucket_id
             WHERE w.user_id = $1 AND w.realm_id = $2",
        )
        .bind(user_id)
        .bind(realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap_or(0)
    }

    /// Get subscription window availability for a user's wallet.
    async fn get_subscription_balance(
        ctx: &SchemaTestContext,
        user_id: Uuid,
        realm_id: &str,
    ) -> i64 {
        let bucket_id = crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            realm_id,
        )
        .await;
        crate::tests::helpers::points_helpers::compute_window_available(
            ctx,
            realm_id,
            user_id,
            bucket_id,
            herald_core::domain::points::entities::CreditType::SubscriptionCredit,
        )
        .await
    }

    /// Create a pending payment attempt targeting the given mapping.
    /// Returns the attempt ID.
    async fn create_pending_payment_attempt(
        ctx: &SchemaTestContext,
        realm_id: &str,
        user_id: Uuid,
        mapping_id: Uuid,
        payment_provider: &str,
    ) -> Uuid {
        let attempt_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO payment_attempts
                (id, realm_id, user_id, payment_provider, target_type, target_id,
                 amount, currency, status, expires_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, 'entitlement_mapping', $5,
                 1000, 'usd', 'Pending', NOW() + INTERVAL '1 hour', NOW(), NOW())",
        )
        .bind(attempt_id)
        .bind(realm_id)
        .bind(user_id)
        .bind(payment_provider)
        .bind(mapping_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create pending payment attempt");

        // This bypasses production `create_payment_attempt`, which writes the
        // `payment_attempt_point_rules` snapshot fulfillment reads to route the
        // grant. Every caller pairs this with a one_time mapping (`topup`
        // trigger), so capture that mapping's enabled `topup` rule here —
        // otherwise the subsequent webhook fulfillment grants 0 points.
        crate::tests::helpers::points_helpers::snapshot_attempt_rules_for_mapping(
            &ctx.app_state.pool,
            attempt_id,
            realm_id,
            mapping_id,
            "topup",
        )
        .await;

        attempt_id
    }

    /// Get payment attempt status by ID.
    async fn get_payment_attempt_status(
        ctx: &SchemaTestContext,
        attempt_id: Uuid,
    ) -> Option<String> {
        sqlx::query_scalar::<_, String>("SELECT status FROM payment_attempts WHERE id = $1")
            .bind(attempt_id)
            .fetch_optional(&ctx.app_state.pool)
            .await
            .unwrap_or(None)
    }

    /// Count subscription_history entries for a given subscription_id and event_type.
    async fn count_history_events(
        ctx: &SchemaTestContext,
        subscription_id: Uuid,
        event_type: &str,
    ) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM subscription_history WHERE subscription_id = $1 AND event_type = $2",
        )
        .bind(subscription_id)
        .bind(event_type)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap_or(0)
    }

    /// Count payment_event rows for a given external_event_id.
    async fn count_payment_events(ctx: &SchemaTestContext, external_event_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM payment_event WHERE external_event_id = $1",
        )
        .bind(external_event_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap_or(0)
    }

    // =========================================================================
    // Stripe Payload Builders
    // =========================================================================

    fn build_stripe_checkout_expired(
        event_id: &str,
        realm_id: &str,
        attempt_id: Option<Uuid>,
    ) -> serde_json::Value {
        let mut metadata = json!({
            "herald_realm_id": realm_id,
        });
        if let Some(aid) = attempt_id {
            metadata["attemptId"] = json!(aid.to_string());
        }
        json!({
            "id": event_id,
            "object": "event",
            "type": "checkout.session.expired",
            "api_version": "2020-08-27",
            "created": chrono::Utc::now().timestamp(),
            "data": {
                "object": {
                    "id": format!("cs_test_{}", Uuid::now_v7()),
                    "object": "checkout.session",
                    "metadata": metadata,
                    "expires_at": chrono::Utc::now().timestamp(),
                }
            }
        })
    }

    fn build_stripe_async_payment_succeeded(
        event_id: &str,
        realm_id: &str,
        attempt_id: Uuid,
    ) -> serde_json::Value {
        json!({
            "id": event_id,
            "object": "event",
            "type": "checkout.session.async_payment_succeeded",
            "api_version": "2020-08-27",
            "created": chrono::Utc::now().timestamp(),
            "data": {
                "object": {
                    "id": format!("cs_test_{}", Uuid::now_v7()),
                    "object": "checkout.session",
                    "mode": "payment",
                    "payment_status": "paid",
                    "metadata": {
                        "attemptId": attempt_id.to_string(),
                        "herald_realm_id": realm_id,
                    },
                    "created": chrono::Utc::now().timestamp(),
                }
            }
        })
    }

    fn build_stripe_async_payment_failed(
        event_id: &str,
        realm_id: &str,
        attempt_id: Uuid,
    ) -> serde_json::Value {
        json!({
            "id": event_id,
            "object": "event",
            "type": "checkout.session.async_payment_failed",
            "api_version": "2020-08-27",
            "created": chrono::Utc::now().timestamp(),
            "data": {
                "object": {
                    "id": format!("cs_test_{}", Uuid::now_v7()),
                    "object": "checkout.session",
                    "payment_status": "unpaid",
                    "metadata": {
                        "attemptId": attempt_id.to_string(),
                        "herald_realm_id": realm_id,
                    },
                    "created": chrono::Utc::now().timestamp(),
                }
            }
        })
    }

    fn build_stripe_checkout_completed_unpaid(
        event_id: &str,
        realm_id: &str,
        user_id: Uuid,
        client_app_id: Uuid,
        attempt_id: Uuid,
        entitlement_key: &str,
    ) -> serde_json::Value {
        json!({
            "id": event_id,
            "object": "event",
            "type": "checkout.session.completed",
            "api_version": "2020-08-27",
            "created": chrono::Utc::now().timestamp(),
            "data": {
                "object": {
                    "id": format!("cs_test_{}", Uuid::now_v7()),
                    "object": "checkout.session",
                    "status": "complete",
                    "payment_status": "unpaid",
                    "mode": "payment",
                    "metadata": {
                        "attemptId": attempt_id.to_string(),
                        "herald_realm_id": realm_id,
                        "herald_user_id": user_id.to_string(),
                        "herald_client_app_id": client_app_id.to_string(),
                        "herald_entitlement_key": entitlement_key,
                        "clientAppId": client_app_id.to_string(),
                    },
                    "display_items": [{
                        "price": {
                            "product": format!("prod_stripe_{}", entitlement_key),
                        }
                    }]
                }
            }
        })
    }

    fn build_stripe_dispute_created(
        event_id: &str,
        subscription_internal_id: Uuid,
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
                    "charge": format!("ch_{}", Uuid::now_v7()),
                    "payment_intent": format!("pi_{}", Uuid::now_v7()),
                    "amount": 1000,
                    "reason": "customer_requested",
                    "status": "needs_response",
                    "metadata": {
                        "herald_subscription_id": subscription_internal_id.to_string(),
                    }
                }
            }
        })
    }

    fn build_stripe_dispute_closed(
        event_id: &str,
        subscription_internal_id: Uuid,
        dispute_status: &str,
    ) -> serde_json::Value {
        json!({
            "id": event_id,
            "object": "event",
            "type": "charge.dispute.closed",
            "api_version": "2020-08-27",
            "created": chrono::Utc::now().timestamp(),
            "data": {
                "object": {
                    "id": format!("dp_{}", Uuid::now_v7()),
                    "object": "dispute",
                    "charge": format!("ch_{}", Uuid::now_v7()),
                    "payment_intent": format!("pi_{}", Uuid::now_v7()),
                    "amount": 1000,
                    "reason": "customer_requested",
                    "status": dispute_status,
                    "metadata": {
                        "herald_subscription_id": subscription_internal_id.to_string(),
                    }
                }
            }
        })
    }

    fn build_stripe_subscription_paused_resumed(
        event_id: &str,
        stripe_sub_id: &str,
        realm_id: &str,
        user_id: Uuid,
        entitlement_key: &str,
        stripe_status: &str,
        event_type: &str,
    ) -> serde_json::Value {
        let external_product_id = format!("prod_stripe_{}", entitlement_key);
        json!({
            "id": event_id,
            "object": "event",
            "type": event_type,
            "api_version": "2020-08-27",
            "created": chrono::Utc::now().timestamp(),
            "data": {
                "object": {
                    "id": stripe_sub_id,
                    "object": "subscription",
                    "status": stripe_status,
                    "current_period_start": chrono::Utc::now().timestamp(),
                    "current_period_end": (chrono::Utc::now() + chrono::Duration::days(30)).timestamp(),
                    "items": {
                        "data": [{
                            "price": {
                                "product": external_product_id,
                                "metadata": {
                                    "herald_entitlement_key": entitlement_key,
                                }
                            }
                        }]
                    },
                    "metadata": {
                        "herald_realm_id": realm_id,
                        "herald_user_id": user_id.to_string(),
                        "userId": user_id.to_string(),
                    }
                },
                "previous_attributes": {
                    "items": {
                        "data": [{
                            "price": {
                                "product": external_product_id,
                                "metadata": {
                                    "herald_entitlement_key": entitlement_key,
                                }
                            }
                        }]
                    }
                }
            }
        })
    }

    fn build_stripe_subscription_updated(
        event_id: &str,
        stripe_sub_id: &str,
        realm_id: &str,
        user_id: Uuid,
        entitlement_key: &str,
        cancel_at_period_end: bool,
    ) -> serde_json::Value {
        let external_product_id = format!("prod_stripe_{}", entitlement_key);
        json!({
            "id": event_id,
            "object": "event",
            "type": "customer.subscription.updated",
            "api_version": "2020-08-27",
            "created": chrono::Utc::now().timestamp(),
            "data": {
                "object": {
                    "id": stripe_sub_id,
                    "object": "subscription",
                    "status": "active",
                    "cancel_at_period_end": cancel_at_period_end,
                    "current_period_start": chrono::Utc::now().timestamp(),
                    "current_period_end": (chrono::Utc::now() + chrono::Duration::days(30)).timestamp(),
                    "items": {
                        "data": [{
                            "price": {
                                "product": external_product_id,
                                "metadata": {
                                    "herald_entitlement_key": entitlement_key,
                                }
                            }
                        }]
                    },
                    "metadata": {
                        "herald_realm_id": realm_id,
                        "herald_user_id": user_id.to_string(),
                        "userId": user_id.to_string(),
                    }
                },
                "previous_attributes": {
                    "items": {
                        "data": [{
                            "price": {
                                "product": external_product_id,
                                "metadata": {
                                    "herald_entitlement_key": entitlement_key,
                                }
                            }
                        }]
                    }
                }
            }
        })
    }

    fn build_stripe_subscription_deleted(
        event_id: &str,
        stripe_sub_id: &str,
        realm_id: &str,
        user_id: Uuid,
        entitlement_key: &str,
    ) -> serde_json::Value {
        let external_product_id = format!("prod_stripe_{}", entitlement_key);
        json!({
            "id": event_id,
            "object": "event",
            "type": "customer.subscription.deleted",
            "api_version": "2020-08-27",
            "created": chrono::Utc::now().timestamp(),
            "data": {
                "object": {
                    "id": stripe_sub_id,
                    "object": "subscription",
                    "status": "canceled",
                    "cancel_at_period_end": false,
                    "current_period_start": chrono::Utc::now().timestamp(),
                    "current_period_end": (chrono::Utc::now() + chrono::Duration::days(30)).timestamp(),
                    "items": {
                        "data": [{
                            "price": {
                                "product": external_product_id,
                                "metadata": {
                                    "herald_entitlement_key": entitlement_key,
                                }
                            }
                        }]
                    },
                    "metadata": {
                        "herald_realm_id": realm_id,
                        "herald_user_id": user_id.to_string(),
                        "userId": user_id.to_string(),
                    }
                }
            }
        })
    }

    // =========================================================================
    // Test 1: Stripe checkout.session.expired marks attempt failed
    // =========================================================================

    /// User Story: US-PA-003
    /// Covers: checkout.session.expired + attemptId -> fail payment attempt
    ///
    /// Given: A one-time mapping with 500 points, a user + wallet, and a pending payment attempt
    /// When: checkout.session.expired arrives with the attempt's ID in metadata
    /// Then: Attempt status becomes "Failed" and no points are granted.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_checkout_expired_marks_attempt_failed(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_expired";
        let realm_id = ctx._realm_id.clone();
        let entitlement_key = "patch-expired";

        setup_stripe_config(ctx, &realm_id, "sk_test_expired", webhook_secret).await;
        let mapping_id =
            create_one_time_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-expired@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;
        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, "stripe").await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_checkout_expired(&event_id, &realm_id, Some(attempt_id));
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let attempt_status = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist");
        assert_eq!(attempt_status, "Failed", "Payment attempt should be Failed");

        let topup = get_topup_balance(ctx, user_id, &realm_id).await;
        assert_eq!(topup, 0, "No points should be granted on expired checkout");
    }

    // =========================================================================
    // Test 2: Stripe checkout.session.expired without attemptId -> OK, no crash
    // =========================================================================

    /// User Story: US-PA-003
    /// Covers: checkout.session.expired without attemptId -> graceful no-op
    ///
    /// Given: Stripe config is set up
    /// When: checkout.session.expired arrives without attemptId in metadata
    /// Then: Response is 200 OK, no crash.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_checkout_expired_without_attempt_id_ok(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_no_att";
        let realm_id = ctx._realm_id.clone();

        setup_stripe_config(ctx, &realm_id, "sk_test_no_att", webhook_secret).await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_checkout_expired(&event_id, &realm_id, None);
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Expected 200 OK without attemptId"
        );
    }

    // =========================================================================
    // Test 3: Stripe async_payment_succeeded fulfills attempt
    // =========================================================================

    /// User Story: US-PA-003, US-PU-006
    /// Covers: checkout.session.async_payment_succeeded + mode=payment -> fulfill
    ///
    /// Given: A one-time mapping with 500 points, a user + wallet, and a pending payment attempt
    /// When: checkout.session.async_payment_succeeded arrives with the attempt's ID
    /// Then: Attempt status becomes "Succeeded" and topup_balance = 500.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_async_payment_succeeded_fulfills_attempt(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_async_ok";
        let realm_id = ctx._realm_id.clone();
        let entitlement_key = "patch-async-ok";

        setup_stripe_config(ctx, &realm_id, "sk_test_async_ok", webhook_secret).await;
        let mapping_id =
            create_one_time_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-async-ok@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;
        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, "stripe").await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_async_payment_succeeded(&event_id, &realm_id, attempt_id);
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let attempt_status = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist");
        assert_eq!(
            attempt_status, "Succeeded",
            "Payment attempt should be Succeeded"
        );

        let topup = get_topup_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            topup, 500,
            "topup_balance should be 500 after async payment succeeded"
        );
    }

    // =========================================================================
    // Test 4: Stripe async_payment_failed marks attempt failed
    // =========================================================================

    /// User Story: US-PA-003
    /// Covers: checkout.session.async_payment_failed + attemptId -> fail
    ///
    /// Given: A user with a pending payment attempt
    /// When: checkout.session.async_payment_failed arrives with the attempt's ID
    /// Then: Attempt status becomes "Failed".
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_async_payment_failed_marks_attempt_failed(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_async_fail";
        let realm_id = ctx._realm_id.clone();
        let entitlement_key = "patch-async-fail";

        setup_stripe_config(ctx, &realm_id, "sk_test_async_fail", webhook_secret).await;
        let mapping_id =
            create_one_time_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-async-fail@test.com").await;
        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, "stripe").await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_async_payment_failed(&event_id, &realm_id, attempt_id);
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let attempt_status = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist");
        assert_eq!(attempt_status, "Failed", "Payment attempt should be Failed");
    }

    // =========================================================================
    // Test 5: Stripe checkout.session.completed (unpaid) defers fulfillment
    // =========================================================================

    /// User Story: US-PA-003
    /// Covers: checkout.session.completed with payment_status=unpaid -> defer
    ///
    /// Given: A one-time mapping with 500 points, a user + wallet, and a pending payment attempt
    /// When: checkout.session.completed arrives with payment_status="unpaid"
    /// Then: Attempt status remains "Pending" and no points are granted.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_checkout_completed_unpaid_defers_fulfillment(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_unpaid";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-unpaid";

        setup_stripe_config(ctx, &realm_id, "sk_test_unpaid", webhook_secret).await;
        let mapping_id =
            create_one_time_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-unpaid@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;
        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, "stripe").await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_checkout_completed_unpaid(
            &event_id,
            &realm_id,
            user_id,
            client_app_id,
            attempt_id,
            entitlement_key,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let attempt_status = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist");
        assert_eq!(
            attempt_status, "Pending",
            "Payment attempt should remain Pending when payment_status=unpaid"
        );

        let topup = get_topup_balance(ctx, user_id, &realm_id).await;
        assert_eq!(topup, 0, "No points should be granted for unpaid checkout");
    }

    // =========================================================================
    // Test 6: Stripe dispute.created sets dispute status
    // =========================================================================

    /// User Story: US-BILL-DISPUTE
    /// Covers: charge.dispute.created + herald_subscription_id -> status=dispute
    ///
    /// Given: A recurring mapping, a user, and a pre-existing subscription in 'active' status
    /// When: charge.dispute.created arrives with herald_subscription_id pointing to the subscription
    /// Then: Subscription status becomes "dispute" and history has "disputed" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_dispute_created_sets_dispute_status(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_dispute";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-dispute";

        setup_stripe_config(ctx, &realm_id, "sk_test_dispute", webhook_secret).await;
        create_recurring_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-dispute@test.com").await;

        let ext_sub_id = format!("sub_stripe_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_stripe_{}", entitlement_key),
            "stripe",
            "active",
            entitlement_key,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_dispute_created(&event_id, sub_id);
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(status, "dispute", "Subscription status should be 'dispute'");

        let history_count = count_history_events(ctx, sub_id, "disputed").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'disputed' history event"
        );
    }

    // =========================================================================
    // Test 7: Stripe dispute.closed (won) reactivates
    // =========================================================================

    /// User Story: US-BILL-DISPUTE
    /// Covers: charge.dispute.closed with status=won -> reactivate subscription
    ///
    /// Given: A recurring mapping, a user, and a pre-existing subscription in 'dispute' status
    /// When: charge.dispute.closed arrives with status="won"
    /// Then: Subscription status becomes "active" and history has "reactivated" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_dispute_closed_won_reactivates(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_won";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-won";

        setup_stripe_config(ctx, &realm_id, "sk_test_won", webhook_secret).await;
        create_recurring_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-won@test.com").await;

        let ext_sub_id = format!("sub_stripe_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_stripe_{}", entitlement_key),
            "stripe",
            "dispute",
            entitlement_key,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_dispute_closed(&event_id, sub_id, "won");
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "active",
            "Subscription should be reactivated to 'active'"
        );

        let history_count = count_history_events(ctx, sub_id, "reactivated").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'reactivated' history event"
        );
    }

    // =========================================================================
    // Test 8: Stripe dispute.closed (lost) cancels
    // =========================================================================

    /// User Story: US-BILL-DISPUTE
    /// Covers: charge.dispute.closed with status=lost -> cancel + revoke credits
    ///
    /// Given: A recurring mapping, a user + wallet with subscription_balance > 0,
    ///        and a pre-existing subscription in 'dispute' status
    /// When: charge.dispute.closed arrives with status="lost"
    /// Then: Subscription status becomes "canceled", subscription_balance = 0 (credits revoked),
    ///       and history has "canceled" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_dispute_closed_lost_cancels(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_lost";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-lost";

        setup_stripe_config(ctx, &realm_id, "sk_test_lost", webhook_secret).await;
        create_recurring_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-lost@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let ext_sub_id = format!("sub_stripe_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_stripe_{}", entitlement_key),
            "stripe",
            "dispute",
            entitlement_key,
        )
        .await;

        create_subscription_credit_with_ledger(
            ctx,
            user_id,
            &realm_id,
            500,
            &format!("{}:{}", entitlement_key, sub_id),
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_dispute_closed(&event_id, sub_id, "lost");
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "canceled",
            "Subscription should be 'canceled' after lost dispute"
        );

        let sub_balance = get_subscription_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            sub_balance, 0,
            "subscription_balance should be 0 after dispute lost (credits revoked)"
        );

        let history_count = count_history_events(ctx, sub_id, "canceled").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'canceled' history event"
        );
    }

    // =========================================================================
    // Test 9: Stripe subscription.paused syncs status
    // =========================================================================

    /// User Story: US-BILL-PAUSE
    /// Covers: customer.subscription.paused -> status=paused
    ///
    /// Given: A recurring mapping, a user, and a pre-existing subscription in 'active' status
    /// When: customer.subscription.paused arrives with status="paused" and matching entitlement_key
    /// Then: Subscription status becomes "paused" and history has "paused" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_subscription_paused_syncs_status(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_paused";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-paused";

        setup_stripe_config(ctx, &realm_id, "sk_test_paused", webhook_secret).await;
        create_recurring_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-paused@test.com").await;

        let stripe_sub_id = format!("sub_stripe_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &stripe_sub_id,
            &format!("prod_stripe_{}", entitlement_key),
            "stripe",
            "active",
            entitlement_key,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_subscription_paused_resumed(
            &event_id,
            &stripe_sub_id,
            &realm_id,
            user_id,
            entitlement_key,
            "paused",
            "customer.subscription.paused",
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(status, "paused", "Subscription status should be 'paused'");

        let history_count = count_history_events(ctx, sub_id, "paused").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'paused' history event"
        );
    }

    // =========================================================================
    // Test 10: Stripe subscription.resumed syncs status
    // =========================================================================

    /// User Story: US-BILL-PAUSE
    /// Covers: customer.subscription.resumed -> status=active (reactivated)
    ///
    /// Given: A recurring mapping, a user, and a pre-existing subscription in 'paused' status
    /// When: customer.subscription.resumed arrives with status="active" and matching entitlement_key
    /// Then: Subscription status becomes "active" and history has "reactivated" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_subscription_resumed_syncs_status(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_resumed";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-resumed";

        setup_stripe_config(ctx, &realm_id, "sk_test_resumed", webhook_secret).await;
        create_recurring_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-resumed@test.com").await;

        let stripe_sub_id = format!("sub_stripe_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &stripe_sub_id,
            &format!("prod_stripe_{}", entitlement_key),
            "stripe",
            "paused",
            entitlement_key,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_subscription_paused_resumed(
            &event_id,
            &stripe_sub_id,
            &realm_id,
            user_id,
            entitlement_key,
            "active",
            "customer.subscription.resumed",
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "active",
            "Subscription status should be 'active' after resume"
        );

        let history_count = count_history_events(ctx, sub_id, "reactivated").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'reactivated' history event"
        );
    }

    // =========================================================================
    // Test B1: Stripe customer.subscription.updated -> scheduled_cancel
    // =========================================================================

    /// User Story: US-BILL-SCHED-CANCEL
    /// Covers: customer.subscription.updated + cancel_at_period_end=true -> scheduled_cancel
    ///
    /// Given: A recurring mapping, a user + wallet with subscription credits, and an active subscription
    /// When: customer.subscription.updated arrives with cancel_at_period_end=true
    /// Then: Subscription status becomes "scheduled_cancel", cancel_at_period_end=true,
    ///       subscription credits are NOT revoked (still has access until period end),
    ///       and history has "scheduled_cancel" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_subscription_updated_scheduled_cancel(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_sched_cancel";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-sched-cancel";

        setup_stripe_config(ctx, &realm_id, "sk_test_sched_cancel", webhook_secret).await;
        create_recurring_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-sched-cancel@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let stripe_sub_id = format!("sub_stripe_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &stripe_sub_id,
            &format!("prod_stripe_{}", entitlement_key),
            "stripe",
            "active",
            entitlement_key,
        )
        .await;

        create_subscription_credit_with_ledger(
            ctx,
            user_id,
            &realm_id,
            500,
            &format!("{}:{}", entitlement_key, sub_id),
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_subscription_updated(
            &event_id,
            &stripe_sub_id,
            &realm_id,
            user_id,
            entitlement_key,
            true,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "scheduled_cancel",
            "Subscription status should be 'scheduled_cancel'"
        );

        let cancel_at_end = get_cancel_at_period_end(ctx, sub_id).await;
        assert!(
            cancel_at_end,
            "cancel_at_period_end should be true for scheduled_cancel"
        );

        let sub_balance = get_subscription_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            sub_balance, 500,
            "subscription_balance should still be 500 (credits NOT revoked during scheduled_cancel)"
        );

        let history_count = count_history_events(ctx, sub_id, "scheduled_cancel").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'scheduled_cancel' history event"
        );
    }

    // =========================================================================
    // Test B2: Stripe customer.subscription.updated -> reactivate from scheduled_cancel
    // =========================================================================

    /// User Story: US-BILL-REACTIVATE
    /// Covers: customer.subscription.updated + cancel_at_period_end=false -> reactivate
    ///
    /// Given: A recurring mapping, a user + wallet, and a subscription in "scheduled_cancel" status
    /// When: customer.subscription.updated arrives with cancel_at_period_end=false
    /// Then: Subscription status becomes "active", cancel_at_period_end=false,
    ///       and history has "reactivated" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_subscription_updated_reactivate_from_scheduled_cancel(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_reactivate";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-reactivate";

        setup_stripe_config(ctx, &realm_id, "sk_test_reactivate", webhook_secret).await;
        create_recurring_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-reactivate@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let stripe_sub_id = format!("sub_stripe_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &stripe_sub_id,
            &format!("prod_stripe_{}", entitlement_key),
            "stripe",
            "scheduled_cancel",
            entitlement_key,
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_subscription_updated(
            &event_id,
            &stripe_sub_id,
            &realm_id,
            user_id,
            entitlement_key,
            false,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "active",
            "Subscription status should be 'active' after reactivation"
        );

        let cancel_at_end = get_cancel_at_period_end(ctx, sub_id).await;
        assert!(
            !cancel_at_end,
            "cancel_at_period_end should be false after reactivation"
        );

        let history_count = count_history_events(ctx, sub_id, "reactivated").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'reactivated' history event"
        );
    }

    // =========================================================================
    // Test B3: Stripe customer.subscription.deleted -> canceled + revoke credits
    // =========================================================================

    /// User Story: US-BILL-CANCEL
    /// Covers: customer.subscription.deleted -> status=canceled, credits revoked
    ///
    /// Given: A recurring mapping, a user + wallet with subscription credits, and an active subscription
    /// When: customer.subscription.deleted arrives
    /// Then: Subscription status becomes "canceled", subscription_balance drops to 0 (credits revoked),
    ///       and history has "canceled" event.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_subscription_deleted_cancels(ctx: &mut WebhookPatchTestContext) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_sub_deleted";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-sub-deleted";

        setup_stripe_config(ctx, &realm_id, "sk_test_sub_deleted", webhook_secret).await;
        create_recurring_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-sub-deleted@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let stripe_sub_id = format!("sub_stripe_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &stripe_sub_id,
            &format!("prod_stripe_{}", entitlement_key),
            "stripe",
            "active",
            entitlement_key,
        )
        .await;

        create_subscription_credit_with_ledger(
            ctx,
            user_id,
            &realm_id,
            500,
            &format!("{}:{}", entitlement_key, sub_id),
        )
        .await;

        let event_id = generate_test_event_id();
        let payload = build_stripe_subscription_deleted(
            &event_id,
            &stripe_sub_id,
            &realm_id,
            user_id,
            entitlement_key,
        );
        let response =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(response.status(), StatusCode::OK, "Expected 200 OK");

        let status = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status, "canceled",
            "Subscription status should be 'canceled'"
        );

        let sub_balance = get_subscription_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            sub_balance, 0,
            "subscription_balance should be 0 after subscription deleted (credits revoked)"
        );

        let history_count = count_history_events(ctx, sub_id, "canceled").await;
        assert_eq!(
            history_count, 1,
            "Expected exactly 1 'canceled' history event"
        );
    }

    // =========================================================================
    // Test 18: Stripe checkout.expired idempotent (same event_id twice)
    // =========================================================================

    /// User Story: US-PA-003
    /// Covers: Duplicate checkout.session.expired with same event_id -> no double fail
    ///
    /// Given: Stripe config, a user, and a pending payment attempt
    /// When: checkout.session.expired is sent twice with the SAME event_id
    /// Then: Both return 200 OK, exactly 1 payment_event row, and attempt is "Failed".
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_checkout_expired_idempotent_no_double_fail(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_idemp_expired";
        let realm_id = ctx._realm_id.clone();
        let entitlement_key = "patch-idemp-expired";

        setup_stripe_config(ctx, &realm_id, "sk_test_idemp_exp", webhook_secret).await;
        let mapping_id =
            create_one_time_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-idemp-expired@test.com").await;
        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, "stripe").await;

        // Use the SAME event_id for both sends
        let event_id = format!("evt_idemp_expired_{}", Uuid::now_v7());
        let payload = build_stripe_checkout_expired(&event_id, &realm_id, Some(attempt_id));

        let response1 =
            send_stripe_webhook_with_signature(&app, &realm_id, payload.clone(), webhook_secret)
                .await;
        assert_eq!(
            response1.status(),
            StatusCode::OK,
            "First send should return 200 OK"
        );

        let response2 =
            send_stripe_webhook_with_signature(&app, &realm_id, payload, webhook_secret).await;
        assert_eq!(
            response2.status(),
            StatusCode::OK,
            "Duplicate send should return 200 OK (idempotent)"
        );

        // Exactly 1 payment_event row
        let event_count = count_payment_events(ctx, &event_id).await;
        assert_eq!(
            event_count, 1,
            "Expected exactly 1 payment_event for idempotent expired, got {}",
            event_count
        );

        // Attempt is Failed
        let attempt_status = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist");
        assert_eq!(attempt_status, "Failed", "Payment attempt should be Failed");
    }

    // =========================================================================
    // Test C1: Sequence - checkout.unpaid -> async_payment_succeeded fulfills
    // =========================================================================

    /// User Story: US-PA-003
    /// Covers: checkout.session.completed(unpaid) -> async_payment_succeeded sequence
    ///
    /// Sequence test: checkout.session.completed(unpaid) -> async_payment_succeeded
    ///
    /// Given: A one-time mapping with 500 points, a user + wallet, and a pending payment attempt
    /// When: First, checkout.session.completed arrives with payment_status=unpaid (attempt stays Pending)
    ///       Then, checkout.session.async_payment_succeeded arrives with the same attemptId
    /// Then: After the sequence, attempt status is "Succeeded" and topup_balance = 500.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_checkout_unpaid_then_async_succeeded_fulfills(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_seq_async_ok";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-seq-async-ok";

        setup_stripe_config(ctx, &realm_id, "sk_test_seq_async_ok", webhook_secret).await;
        let mapping_id =
            create_one_time_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-seq-async-ok@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;
        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, "stripe").await;

        // Step 1: checkout.session.completed with payment_status=unpaid -> attempt stays Pending
        let event_id_1 = generate_test_event_id();
        let payload_1 = build_stripe_checkout_completed_unpaid(
            &event_id_1,
            &realm_id,
            user_id,
            client_app_id,
            attempt_id,
            entitlement_key,
        );
        let response1 =
            send_stripe_webhook_with_signature(&app, &realm_id, payload_1, webhook_secret).await;
        assert_eq!(
            response1.status(),
            StatusCode::OK,
            "Step 1 should return 200 OK"
        );

        let attempt_status_1 = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist after step 1");
        assert_eq!(
            attempt_status_1, "Pending",
            "Attempt should remain Pending after unpaid checkout"
        );

        let topup_1 = get_topup_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            topup_1, 0,
            "No points should be granted after unpaid checkout"
        );

        // Step 2: async_payment_succeeded with same attemptId -> attempt becomes Succeeded
        let event_id_2 = generate_test_event_id();
        let payload_2 = build_stripe_async_payment_succeeded(&event_id_2, &realm_id, attempt_id);
        let response2 =
            send_stripe_webhook_with_signature(&app, &realm_id, payload_2, webhook_secret).await;
        assert_eq!(
            response2.status(),
            StatusCode::OK,
            "Step 2 should return 200 OK"
        );

        let attempt_status_2 = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist after step 2");
        assert_eq!(
            attempt_status_2, "Succeeded",
            "Attempt should be Succeeded after async_payment_succeeded"
        );

        let topup_2 = get_topup_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            topup_2, 500,
            "topup_balance should be 500 after async payment succeeded"
        );
    }

    // =========================================================================
    // Test C2: Sequence - checkout.unpaid -> async_payment_failed marks failed
    // =========================================================================

    /// User Story: US-PA-003
    /// Covers: checkout.session.completed(unpaid) -> async_payment_failed sequence
    ///
    /// Sequence test: checkout.session.completed(unpaid) -> async_payment_failed
    ///
    /// Given: A one-time mapping with 500 points, a user + wallet, and a pending payment attempt
    /// When: First, checkout.session.completed arrives with payment_status=unpaid (attempt stays Pending)
    ///       Then, checkout.session.async_payment_failed arrives with the same attemptId
    /// Then: After the sequence, attempt status is "Failed" and no points are granted.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_checkout_unpaid_then_async_failed_marks_failed(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_seq_async_fail";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-seq-async-fail";

        setup_stripe_config(ctx, &realm_id, "sk_test_seq_async_fail", webhook_secret).await;
        let mapping_id =
            create_one_time_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-seq-async-fail@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;
        let attempt_id =
            create_pending_payment_attempt(ctx, &realm_id, user_id, mapping_id, "stripe").await;

        // Step 1: checkout.session.completed with payment_status=unpaid -> attempt stays Pending
        let event_id_1 = generate_test_event_id();
        let payload_1 = build_stripe_checkout_completed_unpaid(
            &event_id_1,
            &realm_id,
            user_id,
            client_app_id,
            attempt_id,
            entitlement_key,
        );
        let response1 =
            send_stripe_webhook_with_signature(&app, &realm_id, payload_1, webhook_secret).await;
        assert_eq!(
            response1.status(),
            StatusCode::OK,
            "Step 1 should return 200 OK"
        );

        let attempt_status_1 = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist after step 1");
        assert_eq!(
            attempt_status_1, "Pending",
            "Attempt should remain Pending after unpaid checkout"
        );

        // Step 2: async_payment_failed with same attemptId -> attempt becomes Failed
        let event_id_2 = generate_test_event_id();
        let payload_2 = build_stripe_async_payment_failed(&event_id_2, &realm_id, attempt_id);
        let response2 =
            send_stripe_webhook_with_signature(&app, &realm_id, payload_2, webhook_secret).await;
        assert_eq!(
            response2.status(),
            StatusCode::OK,
            "Step 2 should return 200 OK"
        );

        let attempt_status_2 = get_payment_attempt_status(ctx, attempt_id)
            .await
            .expect("Payment attempt should exist after step 2");
        assert_eq!(
            attempt_status_2, "Failed",
            "Attempt should be Failed after async_payment_failed"
        );

        let topup_2 = get_topup_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            topup_2, 0,
            "No points should be granted after async payment failed"
        );
    }

    // =========================================================================
    // Test C3: Sequence - Active -> Dispute -> Won -> Active (credits preserved)
    // =========================================================================

    /// User Story: US-BILL-DISPUTE
    /// Covers: charge.dispute.created -> charge.dispute.closed(won) sequence
    ///
    /// Sequence test: Active -> Dispute -> Won -> Active
    ///
    /// Given: A recurring mapping with 500 points, a user + wallet with ledger-backed credits,
    ///        and an active subscription
    /// When: First, charge.dispute.created arrives (status -> dispute)
    ///       Then, charge.dispute.closed with status=won arrives (status -> active)
    /// Then: After the sequence, subscription is "active" with 2 history events:
    ///       1 "disputed" and 1 "reactivated". Credits are preserved.
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_stripe_dispute_full_lifecycle_active_to_dispute_to_won(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let webhook_secret = "whsec_patch_seq_dispute";
        let realm_id = ctx._realm_id.clone();
        let client_app_id = Uuid::parse_str(&ctx._client_app_id).unwrap();
        let entitlement_key = "patch-seq-dispute";

        setup_stripe_config(ctx, &realm_id, "sk_test_seq_dispute", webhook_secret).await;
        create_recurring_mapping(ctx, &realm_id, "stripe", entitlement_key, 500).await;
        let user_id = create_test_user(ctx, &realm_id, "patch-seq-dispute@test.com").await;
        create_points_wallet(ctx, user_id, &realm_id).await;

        let ext_sub_id = format!("sub_stripe_{}", Uuid::now_v7());
        let sub_id = pre_create_subscription(
            ctx,
            &realm_id,
            user_id,
            client_app_id,
            &ext_sub_id,
            &format!("prod_stripe_{}", entitlement_key),
            "stripe",
            "active",
            entitlement_key,
        )
        .await;

        create_subscription_credit_with_ledger(
            ctx,
            user_id,
            &realm_id,
            500,
            &format!("{}:{}", entitlement_key, sub_id),
        )
        .await;

        // Step 1: charge.dispute.created -> status becomes "dispute"
        let event_id_1 = generate_test_event_id();
        let payload_1 = build_stripe_dispute_created(&event_id_1, sub_id);
        let response1 =
            send_stripe_webhook_with_signature(&app, &realm_id, payload_1, webhook_secret).await;
        assert_eq!(
            response1.status(),
            StatusCode::OK,
            "Step 1 should return 200 OK"
        );

        let status_1 = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status_1, "dispute",
            "Subscription should be in 'dispute' status"
        );

        // Step 2: charge.dispute.closed with status=won -> status becomes "active"
        let event_id_2 = generate_test_event_id();
        let payload_2 = build_stripe_dispute_closed(&event_id_2, sub_id, "won");
        let response2 =
            send_stripe_webhook_with_signature(&app, &realm_id, payload_2, webhook_secret).await;
        assert_eq!(
            response2.status(),
            StatusCode::OK,
            "Step 2 should return 200 OK"
        );

        let status_2 = get_subscription_status(ctx, sub_id).await;
        assert_eq!(
            status_2, "active",
            "Subscription should be back to 'active' after dispute won"
        );

        let sub_balance = get_subscription_balance(ctx, user_id, &realm_id).await;
        assert_eq!(
            sub_balance, 500,
            "Credits should be preserved after dispute won"
        );

        let disputed_count = count_history_events(ctx, sub_id, "disputed").await;
        assert_eq!(
            disputed_count, 1,
            "Expected exactly 1 'disputed' history event"
        );

        let reactivated_count = count_history_events(ctx, sub_id, "reactivated").await;
        assert_eq!(
            reactivated_count, 1,
            "Expected exactly 1 'reactivated' history event"
        );
    }

    // =========================================================================
    // 审计 run-2 回归：charge.refunded 元数据订阅跨 realm 不受检
    // =========================================================================

    /// 回归（审计 run-2：charge-refunded-metadata-subscription-realm-unchecked）：
    /// charge.refunded 的 Herald 元数据分支曾用无 realm 谓词的主键查找解析
    /// subscription —— 用 B realm 的 webhook 密钥签名、metadata 指向 A realm
    /// 订阅的事件会对 A 的订阅执行取消并写入 refunded 历史（跨租户写）。
    /// 修复后：元数据分支套用与外部 id 分支相同的 realm 过滤，跨 realm 的
    /// id 与未知 id 不可区分 —— 事件按"无订阅"路由并 400，A 的订阅与历史
    /// 原封不动。
    #[test_context(WebhookPatchTestContext)]
    #[tokio::test]
    async fn test_charge_refunded_foreign_realm_metadata_subscription_is_ignored(
        ctx: &mut WebhookPatchTestContext,
    ) {
        let app = ctx.create_unified_test_router();
        let realm_a = ctx._realm_id.clone();
        let webhook_secret = "whsec_patch_foreign_realm";

        // Realm B：接收方 realm，持有自己的 Stripe webhook 密钥。
        let realm_b = format!("realm-b-{}", Uuid::now_v7().simple());
        sqlx::query("INSERT INTO realm (id, name) VALUES ($1, 'Foreign Realm')")
            .bind(&realm_b)
            .execute(&ctx.app_state.pool)
            .await
            .expect("realm B should insert");
        setup_stripe_config(ctx, &realm_b, "sk_test_foreign_b", webhook_secret).await;

        // Realm A 的用户与活跃订阅（内部主键将被塞进 B 的事件元数据）。
        let user_a = create_test_user(ctx, &realm_a, "foreign-realm-a@test.com").await;
        let sub_internal_id = Uuid::now_v7();
        let external_sub_id = format!("sub_stripe_{}", Uuid::now_v7());
        sqlx::query(
            "INSERT INTO subscription
                (id, realm_id, user_id, external_subscription_id, external_product_id,
                 payment_provider, status, entitlement_key, synced_at,
                 current_period_start, current_period_end, cancel_at_period_end,
                 cancel_at, created_at, updated_at, billing_type)
             VALUES ($1, $2, $3, $4, 'prod_stripe_patch-foreign',
                     'stripe', 'active', 'patch-foreign', NOW(),
                     NOW(), NOW() + INTERVAL '30 days', false,
                     NULL, NOW(), NOW(), 'recurring')",
        )
        .bind(sub_internal_id)
        .bind(&realm_a)
        .bind(user_a)
        .bind(&external_sub_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("realm A subscription should insert");

        let payload = json!({
            "id": generate_test_event_id(),
            "object": "event",
            "type": "charge.refunded",
            "api_version": "2020-08-27",
            "created": chrono::Utc::now().timestamp(),
            "data": {
                "object": {
                    "id": format!("ch_{}", Uuid::now_v7()),
                    "object": "charge",
                    "amount": 1000,
                    "amount_refunded": 1000,
                    "currency": "usd",
                    "payment_intent": format!("pi_{}", Uuid::now_v7()),
                    "metadata": {
                        "herald_subscription_id": sub_internal_id.to_string(),
                    }
                }
            }
        });

        let response =
            send_stripe_webhook_with_signature(&app, &realm_b, payload, webhook_secret).await;
        // 修复后：跨 realm 元数据不可解析 → 按无订阅路由 → 400（诚实失败），
        // 绝不能对 A 的订阅产生任何效果。
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "a foreign-realm metadata subscription must not be accepted"
        );

        let (status,): (String,) = sqlx::query_as("SELECT status FROM subscription WHERE id = $1")
            .bind(sub_internal_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
        assert_eq!(
            status, "active",
            "realm A's subscription must stay untouched"
        );

        let history: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM subscription_history WHERE subscription_id = $1",
        )
        .bind(sub_internal_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
        assert_eq!(
            history, 0,
            "no refunded history row may be written onto the foreign realm's subscription"
        );
    }
}
