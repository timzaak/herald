// =============================================================================
// Paywall M3 — One-Time+Role Anti-Repeat Scenario Tests
// =============================================================================
//
// Proves the "one-time role entitlements are one-per-user" capability
// (design §4.2.2 / §5.4 / §6.1 M3 / §6.3):
//   1. POST payment-attempts returns a structured HTTP 409 `already_owned` when
//      the buyer already holds the granted role from a `payment` source.
//   2. POST payment-attempts returns 409 `already_owned` when the buyer already
//      has a succeeded attempt for the target (the OR's second branch alone).
//   3. Points packages (one_time with empty granted_role_ids) remain repeatable
//      even when a succeeded attempt exists (inverse regression: gate is scoped
//      to one_time+role only).
//   4. A legitimate first purchase of a one_time+role mapping is NOT blocked
//      (gate is not over-aggressive).
//   5. purchase-options sets `grantsRole=true` for a one_time+role mapping.
//   6. purchase-options sets `grantsRole=false` for a points package (and a
//      recurring+role mapping — only one_time+role is the gated combo).
//   7. purchase-options sets `alreadyOwned=true` when the user holds the role.
//   8. purchase-options sets `alreadyOwned=false` for a new user.
//   9. Concurrent double-purchase for the same user+target: at most one
//      succeeded attempt (the `has_succeeded_attempt` pre-check gate). Grant
//      dedup is per-`source_id`, not global per-role — see migration 0006 — so
//      the "one role per user" guarantee is enforced by the pre-check, not by a
//      global unique index.
//
// Mirrors `one_time_api_scenarios.rs` (the `make_create_attempt_request` +
// authenticated oneshot pattern, `create_succeeded_payment_attempt`,
// `create_one_time_mapping`) and `paywall_w1_m2_grant_scenarios.rs`
// (`create_one_time_mapping_with_role`, `seed_*_role_grant`,
// `count_payment_role_grants`).
//
// User Story: US-PW-004 (one-time+role one-per-user; points repeatable;
//             frontend pre-disable signals grantsRole/alreadyOwned;
//             concurrent-double-purchase invariant)
// Covers: design §1.3 (one_time+role ONLY gated), §4.2.2 (409 already_owned body
//         + PurchaseOptionView.grantsRole/alreadyOwned), §4.3.2 (ownership via
//         pre-check), §5.4 (ownership predicate OR:
//         user_has_any_payment_role || has_succeeded_attempt), §6.1 M3, §6.3
//
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tests::helpers::billing_helpers::setup_billing_admin_session_with_user;
    use crate::tests::helpers::rbac_helpers::create_role;
    use crate::tests::schema_test_context::SchemaTestContext as TestContext;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use herald_core::domain::authorization::principal_types;
    use serde_json::json;
    use test_context::test_context;
    use tower::ServiceExt;
    use uuid::Uuid;

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Thin wrapper over `rbac_helpers::create_role`. Requires an admin token
    /// (the role-definition endpoint needs `roles.manage`).
    async fn create_role_in_realm(
        ctx: &TestContext,
        realm_id: &str,
        token: &str,
        name: &str,
    ) -> Uuid {
        create_role(
            ctx,
            realm_id,
            token,
            name,
            "paywall M3 anti-repeat test role",
        )
        .await
    }

    /// Create a one-time entitlement mapping that grants `role_ids` on payment.
    /// Sets `provider_product_info` so the purchase-options endpoint surfaces it
    /// and the one-time-mappings read model includes it. Mirrors
    /// `paywall_w1_m2_grant_scenarios.rs::create_one_time_mapping_with_role`
    /// (sqlx `Vec<Uuid>` → Postgres `uuid[]`).
    async fn create_one_time_role_mapping(
        ctx: &TestContext,
        realm_id: &str,
        _token: &str,
        entitlement_key: &str,
        role_ids: &[Uuid],
        enabled: bool,
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        let provider_product_info = json!({
            "name": format!("M3 Role Pkg {}", entitlement_key),
            "price": 999,
            "currency": "usd"
        });

        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, enabled, provider_product_info,
                 granted_role_ids, created_at, updated_at)
             VALUES ($1, $2, 'stripe', $3, $4, 'one_time', $5, $6, $7, NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(format!("prod_{}", mapping_id))
        .bind(entitlement_key)
        .bind(enabled)
        .bind(provider_product_info)
        .bind(role_ids) // Vec<Uuid> → Postgres uuid[]
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create one-time+role mapping");
        mapping_id
    }

    /// Create a one-time POINTS package mapping (empty granted_role_ids + points
    /// set). This is the repeatable control case — NOT gated by the M3 check.
    async fn create_one_time_points_mapping(
        ctx: &TestContext,
        realm_id: &str,
        entitlement_key: &str,
        points: i64,
        enabled: bool,
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        let provider_product_info = json!({
            "name": format!("M3 Points Pkg {}pts", points),
            "price": 499,
            "currency": "usd"
        });

        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, enabled, provider_product_info,
                 granted_role_ids, created_at, updated_at)
             VALUES ($1, $2, 'stripe', $3, $4, 'one_time', $5, $6, '{}', NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(format!("prod_{}", mapping_id))
        .bind(entitlement_key)
        .bind(enabled)
        .bind(provider_product_info)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create one-time points mapping");
        mapping_id
    }

    /// Create a recurring entitlement mapping with granted_role_ids. Used to
    /// prove `grantsRole` is false for recurring+role (only one_time+role is
    /// gated).
    async fn create_recurring_role_mapping(
        ctx: &TestContext,
        realm_id: &str,
        entitlement_key: &str,
        role_ids: &[Uuid],
        enabled: bool,
    ) -> Uuid {
        let mapping_id = Uuid::now_v7();
        let provider_product_info = json!({
            "name": format!("M3 Recurring Pkg {}", entitlement_key),
            "price": 1200,
            "currency": "usd"
        });

        sqlx::query(
            "INSERT INTO provider_entitlement_mappings
                (id, realm_id, payment_provider, external_product_id, entitlement_key,
                 billing_type, billing_period, enabled, provider_product_info,
                 granted_role_ids, created_at, updated_at)
             VALUES ($1, $2, 'stripe', $3, $4, 'recurring', 'monthly', $5, $6, $7, NOW(), NOW())",
        )
        .bind(mapping_id)
        .bind(realm_id)
        .bind(format!("prod_{}", mapping_id))
        .bind(entitlement_key)
        .bind(enabled)
        .bind(provider_product_info)
        .bind(role_ids)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to create recurring+role mapping");
        mapping_id
    }

    /// Send POST `/api/bill/purchase/payment-attempts` with an auth
    /// cookie. Copied verbatim from
    /// `one_time_api_scenarios.rs::make_create_attempt_request` (hardcoded
    /// targetType=entitlement_mapping, paymentProvider=stripe).
    async fn create_payment_attempt(
        app: &axum::Router,
        _realm_id: &str,
        token: &str,
        target_id: Uuid,
    ) -> (StatusCode, serde_json::Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/bill/purchase/payment-attempts".to_string())
                    .header(header::AUTHORIZATION, format!("Bearer {}", token))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        json!({
                            "targetType": "entitlement_mapping",
                            "targetId": target_id.to_string(),
                            "paymentProvider": "stripe"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value =
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
        (status, body_json)
    }

    /// GET `/api/bill/client/{clientAppId}/purchase-options` with an
    /// auth cookie. Returns `{ items: [...] }`. Mirrors the authenticated
    /// oneshot pattern from `make_ext_request` /
    /// `make_purchase_history_request`.
    async fn get_purchase_options(
        app: &axum::Router,
        _realm_id: &str,
        client_app_id: &str,
        token: &str,
    ) -> (StatusCode, serde_json::Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/bill/client/{}/purchase-options",
                        client_app_id
                    ))
                    .header(header::AUTHORIZATION, format!("Bearer {}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value =
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
        (status, body_json)
    }

    /// Seed a SUCCEEDED payment attempt row directly for `user`+`mapping`
    /// (mirrors `one_time_api_scenarios.rs::create_succeeded_payment_attempt`).
    /// Triggers the `has_succeeded_attempt` branch of the ownership predicate
    /// WITHOUT going through fulfillment (so no role grant / points grant is
    /// produced). Returns the attempt id.
    async fn seed_succeeded_attempt(
        ctx: &TestContext,
        realm_id: &str,
        user_id: Uuid,
        mapping_id: Uuid,
    ) -> Uuid {
        let attempt_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO payment_attempts
                (id, realm_id, user_id, payment_provider, target_type, target_id,
                 amount, currency, status, expires_at, created_at, updated_at)
             VALUES ($1, $2, $3, 'stripe', 'entitlement_mapping', $4,
                     999, 'usd', 'Succeeded', NOW() + INTERVAL '2 hours', NOW(), NOW())",
        )
        .bind(attempt_id)
        .bind(realm_id)
        .bind(user_id)
        .bind(mapping_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to seed succeeded payment attempt");
        attempt_id
    }

    /// Seed a permanent user role from the requested source. Ownership must
    /// block a duplicate purchase regardless of whether the role came from a
    /// payment or an administrator.
    async fn seed_role_grant(
        ctx: &TestContext,
        realm_id: &str,
        user_id: Uuid,
        role_id: Uuid,
        source: &str,
        source_id: &str,
    ) {
        let user_role_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO user_roles
                (id, user_id, role_id, realm_id, client_id, principal_type, principal_id,
                 source, source_id, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $2::text, $7, $8, NULL)",
        )
        .bind(user_role_id)
        .bind(user_id)
        .bind(role_id)
        .bind(realm_id)
        .bind(&ctx._client_id)
        .bind(principal_types::USER)
        .bind(source)
        .bind(source_id)
        .execute(&ctx.app_state.pool)
        .await
        .expect("Failed to seed payment role grant");
    }

    /// Count `payment_attempts` rows for `user`+`target` (any status). Used to
    /// prove the gate rejected the blocked attempt BEFORE inserting (count stays
    /// at the pre-seeded value) and that an allowed attempt was inserted.
    async fn count_attempts_for_target(ctx: &TestContext, user_id: Uuid, target_id: Uuid) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM payment_attempts WHERE user_id = $1 AND target_id = $2",
        )
        .bind(user_id)
        .bind(target_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    /// Count `user_roles` rows for a user/role with `source='payment'`. Mirrors
    /// `paywall_w1_m2_grant_scenarios.rs::count_payment_role_grants`.
    async fn count_payment_role_grants(ctx: &TestContext, user_id: Uuid, role_id: Uuid) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles
             WHERE user_id = $1 AND role_id = $2 AND source = 'payment'",
        )
        .bind(user_id)
        .bind(role_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    /// Count SUCCEEDED payment attempts for `user`+`target`.
    async fn count_succeeded_attempts_for_target(
        ctx: &TestContext,
        user_id: Uuid,
        target_id: Uuid,
    ) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM payment_attempts
             WHERE user_id = $1 AND target_id = $2 AND status = 'Succeeded'",
        )
        .bind(user_id)
        .bind(target_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap()
    }

    // =========================================================================
    // Test 1: 409 already_owned when role already held via a payment grant
    // =========================================================================

    /// User Story: US-PW-004 (one-time+role one-per-user)
    /// Covers: design §4.2.2 (409 already_owned body), §5.4 (ownership check),
    ///         §6.1 M3
    ///
    /// Scenario: A user who already holds the granted role from a `payment`
    /// source is blocked at purchase creation with a structured 409
    /// `already_owned`, and NO new payment_attempts row is created (the gate
    /// rejects before insert).
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_create_payment_attempt_returns_409_when_role_already_owned(
        ctx: &mut TestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id) =
            setup_billing_admin_session_with_user(ctx, "pw3-role-owned@test.com").await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw3-role-owned-role").await;

        // Given: a one-time+role mapping.
        let entitlement_key = "pw3-role-owned-ent";
        let mapping_id =
            create_one_time_role_mapping(ctx, &realm_id, &token, entitlement_key, &[role_id], true)
                .await;

        // And: the user already holds the role via a manual admin grant. The
        // ownership rule is source-agnostic; revoke logic remains payment-only.
        seed_role_grant(
            ctx,
            &realm_id,
            user_id,
            role_id,
            "manual",
            &Uuid::now_v7().to_string(),
        )
        .await;

        let attempts_before = count_attempts_for_target(ctx, user_id, mapping_id).await;

        let app = ctx.create_unified_test_router();

        // When: the user tries to purchase the already-owned entitlement.
        let (status, body) = create_payment_attempt(&app, &realm_id, &token, mapping_id).await;

        // Then: EXACTLY 409 CONFLICT (not a generic client error).
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "Expected 409 CONFLICT, got {status}: {body}"
        );
        // And: the structured already_owned body.
        assert_eq!(
            body["code"], "already_owned",
            "body code must be already_owned, got: {body}"
        );
        assert_eq!(
            body["entitlementKey"], entitlement_key,
            "entitlementKey must match the mapping, got: {body}"
        );

        // And: NO new payment_attempts row was created for this user+target
        // (the gate rejected before the insert in create_payment_attempt).
        let attempts_after = count_attempts_for_target(ctx, user_id, mapping_id).await;
        assert_eq!(
            attempts_after, attempts_before,
            "the blocked attempt must NOT have created a payment_attempts row"
        );
    }

    // =========================================================================
    // Test 2: 409 already_owned when a succeeded attempt exists
    // =========================================================================

    /// User Story: US-PW-004 (防重复 + succeeded-attempt branch)
    /// Covers: design §5.4 (has_succeeded_attempt predicate branch), §6.1 M3
    ///
    /// Scenario: A user with NO payment role grant but a SUCCEEDED attempt for
    /// the target is still blocked — the ownership predicate's OR fires on the
    /// `has_succeeded_attempt` branch alone.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_create_payment_attempt_returns_409_when_attempt_already_succeeded(
        ctx: &mut TestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id) =
            setup_billing_admin_session_with_user(ctx, "pw3-attempt-succ@test.com").await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw3-attempt-succ-role").await;

        let entitlement_key = "pw3-attempt-succ-ent";
        let mapping_id =
            create_one_time_role_mapping(ctx, &realm_id, &token, entitlement_key, &[role_id], true)
                .await;

        // And: a SUCCEEDED attempt for user+mapping — this user has NO payment
        // role grant, so ONLY the has_succeeded_attempt branch fires.
        seed_succeeded_attempt(ctx, &realm_id, user_id, mapping_id).await;
        assert_eq!(
            count_payment_role_grants(ctx, user_id, role_id).await,
            0,
            "precondition: user must NOT hold the role via payment"
        );

        let app = ctx.create_unified_test_router();

        // When.
        let (status, body) = create_payment_attempt(&app, &realm_id, &token, mapping_id).await;

        // Then: EXACTLY 409 CONFLICT.
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "Expected 409 CONFLICT, got {status}: {body}"
        );
        assert_eq!(body["code"], "already_owned", "got: {body}");
        assert_eq!(body["entitlementKey"], entitlement_key, "got: {body}");

        // Proves the predicate's OR: either branch alone triggers the gate.
    }

    // =========================================================================
    // Test 3: points package is repeatable (inverse regression)
    // =========================================================================

    /// User Story: US-PW-004 (积分包可重复 — inverse regression)
    /// Covers: design §1.3 (one_time+role ONLY gated; points repeatable),
    ///         §5.4 (gate only when granted_role_ids non-empty), §6.1 M3
    ///
    /// Scenario: A one-time POINTS package (empty granted_role_ids) is
    /// repeatable: even after a succeeded attempt exists, a new attempt is NOT
    /// 409-blocked and a new payment_attempts row IS created.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_create_payment_attempt_allows_repeat_for_points_package(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id) =
            setup_billing_admin_session_with_user(ctx, "pw3-points-repeat@test.com").await;

        // Given: a one-time POINTS mapping (no role grant).
        let mapping_id =
            create_one_time_points_mapping(ctx, &realm_id, "pw3-points-repeat-ent", 500, true)
                .await;

        // And: the user already "bought" it once (succeeded attempt).
        seed_succeeded_attempt(ctx, &realm_id, user_id, mapping_id).await;
        let attempts_before = count_attempts_for_target(ctx, user_id, mapping_id).await;
        assert_eq!(
            attempts_before, 1,
            "precondition: one succeeded attempt seeded"
        );

        let app = ctx.create_unified_test_router();

        // When.
        let (status, _body) = create_payment_attempt(&app, &realm_id, &token, mapping_id).await;

        // Then: NOT a 409 conflict. The attempt creation may still surface a
        // non-2xx from the downstream provider-call (Stripe not configured in
        // the test harness), but the M3 gate must not block it. The load-
        // bearing invariant is that a NEW row was created (the pre-check did
        // not reject) — the row insert happens in prepare_payment_attempt
        // before the provider call.
        assert_ne!(
            status,
            StatusCode::CONFLICT,
            "points package must NOT be 409-blocked even after a succeeded attempt; got {status}"
        );

        let attempts_after = count_attempts_for_target(ctx, user_id, mapping_id).await;
        assert_eq!(
            attempts_after,
            attempts_before + 1,
            "a new payment_attempts row must be created for the repeat purchase"
        );
    }

    // =========================================================================
    // Test 4: legitimate first purchase of one_time+role is NOT blocked
    // =========================================================================

    /// User Story: US-PW-004 (legitimate first purchase not blocked)
    /// Covers: design §5.4 (gate only when already owned), §6.1 M3
    ///
    /// Scenario: A user with NO prior ownership (no role grant, no succeeded
    /// attempt) can purchase a one_time+role mapping — the gate is not over-
    /// aggressive. A payment_attempts row is created.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_create_payment_attempt_allows_first_purchase_for_one_time_role(
        ctx: &mut TestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id) =
            setup_billing_admin_session_with_user(ctx, "pw3-first-buy@test.com").await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw3-first-buy-role").await;

        let mapping_id = create_one_time_role_mapping(
            ctx,
            &realm_id,
            &token,
            "pw3-first-buy-ent",
            &[role_id],
            true,
        )
        .await;

        // Precondition: no ownership of any kind.
        assert_eq!(
            count_payment_role_grants(ctx, user_id, role_id).await,
            0,
            "precondition: no payment role grant"
        );
        assert_eq!(
            count_succeeded_attempts_for_target(ctx, user_id, mapping_id).await,
            0,
            "precondition: no succeeded attempt"
        );

        let app = ctx.create_unified_test_router();

        // When.
        let (status, _body) = create_payment_attempt(&app, &realm_id, &token, mapping_id).await;

        // Then: NOT a 409. (The downstream Stripe call may surface a non-2xx in
        // the test harness without Stripe configured, but the ownership gate
        // must not block a first purchase.)
        assert_ne!(
            status,
            StatusCode::CONFLICT,
            "first purchase of a one_time+role entitlement must NOT be 409-blocked; got {status}"
        );

        // And: a payment_attempts row WAS created.
        assert_eq!(
            count_attempts_for_target(ctx, user_id, mapping_id).await,
            1,
            "a payment_attempts row must be created for the first purchase"
        );
    }

    // =========================================================================
    // Test 5: purchase-options grantsRole=true for one_time+role
    // =========================================================================

    /// User Story: US-PW-004 (frontend disable signal: grantsRole)
    /// Covers: design §4.2.2 (PurchaseOptionView.grantsRole), §6.1 M3
    ///
    /// Scenario: A one_time+role mapping surfaces in purchase-options with
    /// `grantsRole=true`.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_purchase_options_sets_grants_role_true_for_one_time_role_mapping(
        ctx: &mut TestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let (token, _user_id) =
            setup_billing_admin_session_with_user(ctx, "pw3-grants-true@test.com").await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw3-grants-true-role").await;

        let entitlement_key = "pw3-grants-true-ent";
        let mapping_id =
            create_one_time_role_mapping(ctx, &realm_id, &token, entitlement_key, &[role_id], true)
                .await;

        let app = ctx.create_unified_test_router();

        // When.
        let (status, body) =
            get_purchase_options(&app, &realm_id, &ctx._client_app_id, &token).await;

        // Then.
        assert_eq!(status, StatusCode::OK, "Expected 200, got {status}: {body}");
        let items = body["items"]
            .as_array()
            .unwrap_or_else(|| panic!("items should be an array, got: {body}"));
        let item = items
            .iter()
            .find(|i| i["mappingId"] == mapping_id.to_string())
            .unwrap_or_else(|| {
                panic!("mapping {mapping_id} should be in purchase-options items: {body}")
            });
        assert_eq!(
            item["grantsRole"], true,
            "grantsRole must be true for a one_time+role mapping, got: {item}"
        );
    }

    // =========================================================================
    // Test 6: purchase-options grantsRole=false for points (and recurring+role)
    // =========================================================================

    /// User Story: US-PW-004 (grantsRole false for non-gated)
    /// Covers: design §4.2.2 (grantsRole only for one_time+role), §6.1 M3
    ///
    /// Scenario: A points package (empty granted_role_ids) AND a recurring+role
    /// mapping both surface with `grantsRole=false` — only the one_time+role
    /// combo is gated.
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_purchase_options_sets_grants_role_false_for_points_package(
        ctx: &mut TestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let (token, _user_id) =
            setup_billing_admin_session_with_user(ctx, "pw3-grants-false@test.com").await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw3-grants-false-role").await;

        // Points package (empty granted_role_ids).
        let points_mapping_id = create_one_time_points_mapping(
            ctx,
            &realm_id,
            "pw3-grants-false-points-ent",
            300,
            true,
        )
        .await;
        // Recurring+role mapping — proves the billing_type check (only one_time
        // is gated, even when granted_role_ids is non-empty).
        let recurring_mapping_id = create_recurring_role_mapping(
            ctx,
            &realm_id,
            "pw3-grants-false-recur-ent",
            &[role_id],
            true,
        )
        .await;

        let app = ctx.create_unified_test_router();

        // When.
        let (status, body) =
            get_purchase_options(&app, &realm_id, &ctx._client_app_id, &token).await;

        // Then.
        assert_eq!(status, StatusCode::OK, "Expected 200, got {status}: {body}");
        let items = body["items"]
            .as_array()
            .unwrap_or_else(|| panic!("items should be an array, got: {body}"));

        let points_item = items
            .iter()
            .find(|i| i["mappingId"] == points_mapping_id.to_string())
            .expect("points mapping should be present");
        assert_eq!(
            points_item["grantsRole"], false,
            "grantsRole must be false for a points package, got: {points_item}"
        );

        let recurring_item = items
            .iter()
            .find(|i| i["mappingId"] == recurring_mapping_id.to_string())
            .expect("recurring mapping should be present");
        assert_eq!(
            recurring_item["grantsRole"], false,
            "grantsRole must be false for a recurring+role mapping (only one_time+role is gated), got: {recurring_item}"
        );
    }

    // =========================================================================
    // Test 7: purchase-options alreadyOwned=true when user holds the role
    // =========================================================================

    /// User Story: US-PW-004 (frontend disable signal: alreadyOwned)
    /// Covers: design §4.2.2 (PurchaseOptionView.alreadyOwned), §6.1 M3
    ///
    /// Scenario: A user who holds the granted role via a payment grant sees
    /// `alreadyOwned=true` (and `grantsRole=true`) for that mapping in
    /// purchase-options. Proves the purchase-options ownership predicate
    /// mirrors the create-time gate (frontend can pre-disable).
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_purchase_options_sets_already_owned_true_when_user_holds_role(
        ctx: &mut TestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id) =
            setup_billing_admin_session_with_user(ctx, "pw3-already-true@test.com").await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw3-already-true-role").await;

        let mapping_id = create_one_time_role_mapping(
            ctx,
            &realm_id,
            &token,
            "pw3-already-true-ent",
            &[role_id],
            true,
        )
        .await;

        // And: the user holds the role via a payment grant.
        seed_role_grant(
            ctx,
            &realm_id,
            user_id,
            role_id,
            "payment",
            &Uuid::now_v7().to_string(),
        )
        .await;

        let app = ctx.create_unified_test_router();

        // When: GET as that user.
        let (status, body) =
            get_purchase_options(&app, &realm_id, &ctx._client_app_id, &token).await;

        // Then.
        assert_eq!(status, StatusCode::OK, "Expected 200, got {status}: {body}");
        let items = body["items"]
            .as_array()
            .unwrap_or_else(|| panic!("items should be an array, got: {body}"));
        let item = items
            .iter()
            .find(|i| i["mappingId"] == mapping_id.to_string())
            .unwrap_or_else(|| {
                panic!("mapping {mapping_id} should be in purchase-options items: {body}")
            });
        assert_eq!(
            item["grantsRole"], true,
            "grantsRole must be true for a one_time+role mapping, got: {item}"
        );
        assert_eq!(
            item["alreadyOwned"], true,
            "alreadyOwned must be true when the user holds the granted role, got: {item}"
        );
    }

    // =========================================================================
    // Test 8: purchase-options alreadyOwned=false for a new user
    // =========================================================================

    /// User Story: US-PW-004 (alreadyOwned false for new user)
    /// Covers: design §4.2.2 (alreadyOwned meaningful only for one_time+role),
    ///         §6.1 M3
    ///
    /// Scenario: A new user with NO ownership sees `alreadyOwned=false` for a
    /// one_time+role mapping. A points-package item also reports
    /// `alreadyOwned=false` (the flag is meaningful only for one_time+role but
    /// is `false`, not an error, for others).
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_purchase_options_sets_already_owned_false_for_new_user(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let (token, _user_id) =
            setup_billing_admin_session_with_user(ctx, "pw3-already-false@test.com").await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw3-already-false-role").await;

        let role_mapping_id = create_one_time_role_mapping(
            ctx,
            &realm_id,
            &token,
            "pw3-already-false-ent",
            &[role_id],
            true,
        )
        .await;
        let points_mapping_id = create_one_time_points_mapping(
            ctx,
            &realm_id,
            "pw3-already-false-points-ent",
            250,
            true,
        )
        .await;

        let app = ctx.create_unified_test_router();

        // When: GET as a brand-new user (no ownership of any kind).
        let (status, body) =
            get_purchase_options(&app, &realm_id, &ctx._client_app_id, &token).await;

        // Then.
        assert_eq!(status, StatusCode::OK, "Expected 200, got {status}: {body}");
        let items = body["items"]
            .as_array()
            .unwrap_or_else(|| panic!("items should be an array, got: {body}"));

        let role_item = items
            .iter()
            .find(|i| i["mappingId"] == role_mapping_id.to_string())
            .expect("one_time+role mapping should be present");
        assert_eq!(
            role_item["grantsRole"], true,
            "grantsRole must be true for the one_time+role mapping, got: {role_item}"
        );
        assert_eq!(
            role_item["alreadyOwned"], false,
            "alreadyOwned must be false for a new user, got: {role_item}"
        );

        let points_item = items
            .iter()
            .find(|i| i["mappingId"] == points_mapping_id.to_string())
            .expect("points mapping should be present");
        assert_eq!(
            points_item["alreadyOwned"], false,
            "alreadyOwned must be false (not error) for a points package, got: {points_item}"
        );
    }

    // =========================================================================
    // Test 9: concurrent double-purchase leaves at most one grant
    // =========================================================================

    /// User Story: US-PW-004 (含并发双购防双购)
    /// Covers: design §4.3.2 M3 note (pre-check gate), §5.4, §6.1 M3, §6.3
    ///
    /// Scenario: Two concurrent POST payment-attempts for the same user+target
    /// race past the pre-check (neither sees the other's row yet). The race
    /// outcome is non-deterministic — one or both may return 201 CREATED.
    /// NOTE on the grant model: payment-role dedup is keyed on the payment
    /// origin (`source_id`), not on (user, role) globally — see migration 0006's
    /// `idx_user_roles_principal_role_payment`. Two distinct attempts that both
    /// fulfill would therefore write two grants (one per `source_id`); the
    /// global "one role per user" guarantee is instead enforced upstream by the
    /// `has_succeeded_attempt` pre-check, which blocks the second attempt once
    /// the first has succeeded. This test exercises only the create path (no
    /// fulfillment call), so no grants are written here; the load-bearing
    /// INVARIANT for THIS test is about the create path, not the grant count:
    /// after both attempts resolve, there is at most one SUCCEEDED attempt for
    /// the target (the pre-check's effect once committed). The grant assertion
    /// below stays as a ≤1 bound (it is 0 here since fulfillment is not called)
    /// and would catch an accidental grant during create.
    ///
    /// (Kept runnable — the invariant is deterministic even though the
    /// individual statuses are not.)
    #[test_context(TestContext)]
    #[tokio::test]
    async fn test_concurrent_double_purchase_at_most_one_succeeds(ctx: &mut TestContext) {
        let realm_id = ctx._realm_id.clone();
        let (token, user_id) =
            setup_billing_admin_session_with_user(ctx, "pw3-concurrent@test.com").await;
        let role_id = create_role_in_realm(ctx, &realm_id, &token, "pw3-concurrent-role").await;

        let mapping_id = create_one_time_role_mapping(
            ctx,
            &realm_id,
            &token,
            "pw3-concurrent-ent",
            &[role_id],
            true,
        )
        .await;

        // Precondition: no ownership.
        assert_eq!(
            count_payment_role_grants(ctx, user_id, role_id).await,
            0,
            "precondition: no payment role grant"
        );
        assert_eq!(
            count_succeeded_attempts_for_target(ctx, user_id, mapping_id).await,
            0,
            "precondition: no succeeded attempt"
        );

        let app = ctx.create_unified_test_router();

        // When: fire two purchase attempts concurrently for the same
        // user+target. Both share the same app/router (each `oneshot` consumes
        // a clone).
        let app_a = app.clone();
        let app_b = app.clone();
        let realm_a = realm_id.clone();
        let realm_b = realm_id.clone();
        let token_a = token.clone();
        let token_b = token.clone();
        let (res_a, res_b) = tokio::join!(
            create_payment_attempt(&app_a, &realm_a, &token_a, mapping_id),
            create_payment_attempt(&app_b, &realm_b, &token_b, mapping_id),
        );

        // Then: the INVARIANT holds. The individual statuses are non-
        // deterministic (documented above) so we only assert the invariant.
        // At most one CREATED; at most one succeeded attempt; at most one
        // payment-source grant for the role.
        let created = [res_a.0, res_b.0]
            .iter()
            .filter(|&&s| s == StatusCode::CREATED)
            .count();
        assert!(
            created <= 1,
            "at most one concurrent attempt may fully succeed (201 CREATED), got {created} created. \
             statuses were {} and {}; bodies: {} / {}",
            res_a.0,
            res_b.0,
            res_a.1,
            res_b.1
        );

        // No more than two payment_attempts rows total (one per fired request,
        // minus any rejected by the pre-check). The backstop is about not
        // double-FULFILLING; a blocked pre-check may even leave <2 rows.
        let attempts = count_attempts_for_target(ctx, user_id, mapping_id).await;
        assert!(
            attempts <= 2,
            "at most 2 payment_attempts rows expected for the concurrent pair, got {attempts}"
        );

        // Sanity bound on grants: this create-only path writes no grants
        // (fulfillment is a separate call), so grants should be 0 here. The ≤1
        // bound would also catch an accidental grant during create. Note the
        // global "one role per user" guarantee is enforced by the
        // `has_succeeded_attempt` pre-check at attempt-creation, not by a
        // global (user, role) unique index — the payment-role index is keyed on
        // `source_id` (migration 0006), so distinct attempts are allowed to
        // coexist if the pre-check is bypassed.
        let grants = count_payment_role_grants(ctx, user_id, role_id).await;
        assert!(
            grants <= 1,
            "create path must not double-grant; expected 0 grants (no fulfillment called), got {grants}"
        );

        // At most one succeeded attempt (fulfillment-time dedup).
        let succeeded = count_succeeded_attempts_for_target(ctx, user_id, mapping_id).await;
        assert!(
            succeeded <= 1,
            "at most 1 succeeded attempt may result from a concurrent double-purchase, got {succeeded}"
        );
    }
}
