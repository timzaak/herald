// =============================================================================
// Provider Product Sync Scenario Tests (sync-payment feature)
// =============================================================================
//
// Deterministic, in-process tests for `ProviderProductSyncService` covering the
// Stripe product/price `metadata` propagation branch (design §6.1) and the
// "重复同步以最新 metadata 为准" contract.
//
// These tests do NOT touch the network: a test-only fake `ProviderApiPort`
// (`FakeProviderApi`) returns canned `ProviderProduct`s, so the sync loop is
// exercised end-to-end against the real DB-backed repository + policy, with
// assertions on the resulting `provider_product_info` JSONB shape and row count.
//
// BE-T01 owns this file: the `#[cfg(test)] mod tests` scaffolding, the
// `FakeProviderApi` seam, and the `build_sync_service` helper. BE-T02 / BE-T03
// only APPEND additional `#[test_context]` test functions here and reuse these
// definitions — they must NOT redefine them or re-register the module.
//
// User Story: US-BL-SYNC-001
// Covers: Stripe product/price metadata propagation into provider_product_info
//         JSONB; re-sync takes the latest metadata.
//
// =============================================================================

#![allow(dead_code)]

#[cfg(test)]
mod tests {
    use crate::tests::helpers::points_helpers::create_test_identity;
    use crate::tests::schema_test_context::SchemaTestContext;
    use herald_core::domain::authentication::Identity;
    use herald_core::domain::billing::{
        ProviderApiPort, ProviderPrice, ProviderProduct, ProviderProductSyncService, SyncStatus,
    };
    use herald_core::domain::common::entities::app_errors::CoreError;
    use herald_core::infrastructure::authorization::policies::PermissionBasedBillingPolicy;
    use serde_json::{Value, json};
    use std::collections::HashMap;
    use std::sync::Arc;
    use test_context::test_context;

    use SchemaTestContext as SyncTestContext;

    // =========================================================================
    // Test-only fake ProviderApiPort — the deterministic sync seam (no network)
    // =========================================================================

    /// In-process fake provider API. Returns the canned `products` vec for any
    /// `(realm_id, payment_provider)` pair, so the sync loop can be exercised
    /// without a real Stripe/Creem HTTP call.
    ///
    /// BE-T02 / BE-T03 reuse this directly; do NOT redefine.
    #[derive(Debug, Clone)]
    pub(super) struct FakeProviderApi {
        pub products: Vec<ProviderProduct>,
    }

    impl ProviderApiPort for FakeProviderApi {
        fn fetch_products(
            &self,
            _realm_id: &str,
            _payment_provider: &str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<ProviderProduct>, CoreError>>
                    + Send
                    + '_,
            >,
        > {
            // Owned clone keeps the future `'static`-ish over the borrow;
            // the canned vec is cheap to clone.
            let products = self.products.clone();
            Box::pin(async move { Ok(products) })
        }
    }

    // =========================================================================
    // Service-construction helper — mirrors schema_test_context.rs:193-207
    // =========================================================================

    /// Build a `ProviderProductSyncService` wired to the test context but with a
    /// `FakeProviderApi` (canned `products`) in place of the real
    /// `ConfiguredProviderProductApi`. The repository and policy are the same
    /// real, DB-backed instances the app uses.
    ///
    /// BE-T02 / BE-T03 reuse this directly; do NOT redefine.
    pub(super) async fn build_sync_service(
        ctx: &SyncTestContext,
        products: Vec<ProviderProduct>,
    ) -> Arc<
        ProviderProductSyncService<
            herald_core::infrastructure::billing::PostgresBillingRepository,
            PermissionBasedBillingPolicy,
            FakeProviderApi,
        >,
    > {
        let repository = ctx.app_state.billing_repository.clone();
        let policy = Arc::new(PermissionBasedBillingPolicy::new(
            ctx.app_state.permission_checker.clone(),
        ));
        let provider_api = Arc::new(FakeProviderApi { products });
        Arc::new(ProviderProductSyncService::new(
            repository,
            policy,
            provider_api,
        ))
    }

    /// Fetch the single `provider_product_info` JSONB for a given
    /// `(external_product_id, external_price_id)` mapping row, asserting exactly
    /// one row exists. Returns the parsed JSON for downstream shape assertions.
    async fn fetch_single_product_info(
        ctx: &SyncTestContext,
        realm_id: &str,
        external_product_id: &str,
        external_price_id: Option<&str>,
    ) -> Value {
        let row: (Value,) = if let Some(pid) = external_price_id {
            sqlx::query_as(
                "SELECT provider_product_info
                 FROM provider_entitlement_mappings
                 WHERE realm_id = $1
                   AND external_product_id = $2
                   AND external_price_id = $3",
            )
            .bind(realm_id)
            .bind(external_product_id)
            .bind(pid)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("mapping row must exist")
        } else {
            sqlx::query_as(
                "SELECT provider_product_info
                 FROM provider_entitlement_mappings
                 WHERE realm_id = $1
                   AND external_product_id = $2
                   AND external_price_id IS NULL",
            )
            .bind(realm_id)
            .bind(external_product_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("mapping row must exist")
        };
        row.0
    }

    /// Count mapping rows matching a given `external_product_id` in the realm.
    async fn count_mappings_for_product(
        ctx: &SyncTestContext,
        realm_id: &str,
        external_product_id: &str,
    ) -> i64 {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM provider_entitlement_mappings
             WHERE realm_id = $1 AND external_product_id = $2",
        )
        .bind(realm_id)
        .bind(external_product_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("count query must succeed");
        count
    }

    // =========================================================================
    // Tests
    // =========================================================================

    /// User Story: US-BL-SYNC-001
    /// Covers: Stripe product/price metadata propagation into provider_product_info JSONB
    #[test_context(SyncTestContext)]
    #[tokio::test]
    async fn test_sync_stripe_product_metadata_propagates_into_jsonb(ctx: &mut SyncTestContext) {
        let realm_id = ctx._realm_id.clone();
        // The sync service requires a registration-pool credit bucket to bind
        // newly-created draft mappings (bucket_id is NOT NULL). Seed one for the
        // realm before sync — idempotent (ON CONFLICT DO NOTHING).
        crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            &realm_id,
        )
        .await;

        // Realm-admin role carries points.manage, which satisfies
        // PermissionBasedBillingPolicy (can_manage_billing). We need the
        // user_id to build the Identity the sync service consumes.
        let (_token, user_id) =
            crate::tests::helpers::billing_helpers::setup_billing_admin_session_with_user(
                ctx,
                "sync-stripe-meta-prop@test.com",
            )
            .await;

        let identity: Identity = create_test_identity(user_id, &realm_id);

        let product = ProviderProduct {
            external_product_id: "prod_meta_1".to_string(),
            name: "Pro Plan".to_string(),
            description: Some("Pro tier".to_string()),
            product_metadata: Some(HashMap::from([
                ("tier".to_string(), "pro".to_string()),
                ("internal_id".to_string(), "abc".to_string()),
            ])),
            prices: vec![ProviderPrice {
                external_price_id: Some("price_1".to_string()),
                price: Some(1999),
                currency: Some("usd".to_string()),
                billing_type: Some("recurring".to_string()),
                billing_period: Some("month".to_string()),
                price_metadata: Some(HashMap::from([(
                    "nickname".to_string(),
                    "Monthly".to_string(),
                )])),
            }],
        };

        let service = build_sync_service(ctx, vec![product]).await;
        let result = service
            .sync_provider_products(identity, &realm_id, "stripe")
            .await
            .expect("sync must complete");

        assert_eq!(result.sync_status, SyncStatus::Completed);
        assert_eq!(result.products_synced, 1);

        assert_eq!(
            count_mappings_for_product(ctx, &realm_id, "prod_meta_1").await,
            1,
            "expected exactly one mapping row"
        );

        let info = fetch_single_product_info(ctx, &realm_id, "prod_meta_1", Some("price_1")).await;

        assert_eq!(
            info["product_metadata"],
            json!({"tier": "pro", "internal_id": "abc"}),
            "product_metadata must propagate into JSONB"
        );
        assert_eq!(
            info["price_metadata"],
            json!({"nickname": "Monthly"}),
            "price_metadata must propagate into JSONB"
        );

        assert_eq!(info["name"], "Pro Plan");
        assert!(info["description"].is_string());
        assert_eq!(info["price"], 1999);
        assert_eq!(info["currency"], "usd");
    }

    /// User Story: US-BL-SYNC-001
    /// Covers: Stripe metadata null when provider returns None
    #[test_context(SyncTestContext)]
    #[tokio::test]
    async fn test_sync_stripe_metadata_null_when_provider_returns_none(ctx: &mut SyncTestContext) {
        let realm_id = ctx._realm_id.clone();
        // Seed the realm's registration-pool credit bucket so the sync service
        // can bind newly-created draft mappings (idempotent).
        crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            &realm_id,
        )
        .await;

        let (_token, user_id) =
            crate::tests::helpers::billing_helpers::setup_billing_admin_session_with_user(
                ctx,
                "sync-stripe-meta-null@test.com",
            )
            .await;
        let identity: Identity = create_test_identity(user_id, &realm_id);

        let product = ProviderProduct {
            external_product_id: "prod_nometa_1".to_string(),
            name: "No-Meta Plan".to_string(),
            description: None,
            product_metadata: None,
            prices: vec![ProviderPrice {
                external_price_id: Some("price_nometa_1".to_string()),
                price: Some(499),
                currency: Some("usd".to_string()),
                billing_type: Some("recurring".to_string()),
                billing_period: Some("month".to_string()),
                price_metadata: None,
            }],
        };

        let service = build_sync_service(ctx, vec![product]).await;
        let result = service
            .sync_provider_products(identity, &realm_id, "stripe")
            .await
            .expect("sync must complete");

        assert_eq!(result.sync_status, SyncStatus::Completed);

        let info =
            fetch_single_product_info(ctx, &realm_id, "prod_nometa_1", Some("price_nometa_1"))
                .await;

        // None serializes to JSON null (key emitted, value null — not omitted).
        assert!(
            info["product_metadata"].is_null(),
            "product_metadata must be null when provider returns None"
        );
        assert!(
            info["price_metadata"].is_null(),
            "price_metadata must be null when provider returns None"
        );
    }

    /// User Story: US-BL-SYNC-001
    /// Covers: Re-sync updates metadata to latest (重复同步以最新 metadata 为准)
    #[test_context(SyncTestContext)]
    #[tokio::test]
    async fn test_sync_stripe_resync_updates_metadata_to_latest(ctx: &mut SyncTestContext) {
        let realm_id = ctx._realm_id.clone();
        // Seed the realm's registration-pool credit bucket so the sync service
        // can bind newly-created draft mappings (idempotent).
        crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            &realm_id,
        )
        .await;

        let (_token, user_id) =
            crate::tests::helpers::billing_helpers::setup_billing_admin_session_with_user(
                ctx,
                "sync-stripe-resync@test.com",
            )
            .await;

        // First sync: product_metadata = {"v": "1"}.
        let first_product = ProviderProduct {
            external_product_id: "prod_resync_1".to_string(),
            name: "Resync Plan".to_string(),
            description: None,
            product_metadata: Some(HashMap::from([("v".to_string(), "1".to_string())])),
            prices: vec![ProviderPrice {
                external_price_id: Some("price_resync_1".to_string()),
                price: Some(999),
                currency: Some("usd".to_string()),
                billing_type: Some("recurring".to_string()),
                billing_period: Some("month".to_string()),
                price_metadata: None,
            }],
        };
        let service = build_sync_service(ctx, vec![first_product]).await;
        let identity: Identity = create_test_identity(user_id, &realm_id);
        service
            .sync_provider_products(identity, &realm_id, "stripe")
            .await
            .expect("first sync must complete");

        let info_after_first =
            fetch_single_product_info(ctx, &realm_id, "prod_resync_1", Some("price_resync_1"))
                .await;
        assert_eq!(info_after_first["product_metadata"], json!({"v": "1"}));

        // Second sync: same external ids, updated product_metadata = {"v": "2"}.
        let second_product = ProviderProduct {
            external_product_id: "prod_resync_1".to_string(),
            name: "Resync Plan".to_string(),
            description: None,
            product_metadata: Some(HashMap::from([("v".to_string(), "2".to_string())])),
            prices: vec![ProviderPrice {
                external_price_id: Some("price_resync_1".to_string()),
                price: Some(999),
                currency: Some("usd".to_string()),
                billing_type: Some("recurring".to_string()),
                billing_period: Some("month".to_string()),
                price_metadata: None,
            }],
        };
        let service_v2 = build_sync_service(ctx, vec![second_product]).await;
        let identity2: Identity = create_test_identity(user_id, &realm_id);
        let result = service_v2
            .sync_provider_products(identity2, &realm_id, "stripe")
            .await
            .expect("second sync must complete");
        assert_eq!(result.sync_status, SyncStatus::Completed);

        assert_eq!(
            count_mappings_for_product(ctx, &realm_id, "prod_resync_1").await,
            1,
            "re-sync must update in place, not duplicate"
        );

        let info_after_second =
            fetch_single_product_info(ctx, &realm_id, "prod_resync_1", Some("price_resync_1"))
                .await;
        assert_eq!(
            info_after_second["product_metadata"],
            json!({"v": "2"}),
            "re-sync must update product_metadata to the latest value"
        );
    }

    // =========================================================================
    // Creem price-field scenarios (BE-T02 / US-BL-SYNC-003)
    // =========================================================================
    //
    // Creem's product fetch surfaces price info at the product level rather than
    // as a Stripe-style price object: it has no `external_price_id`. Backend/dev
    // BE-D02 made the Creem fetch return a real `ProviderPrice`
    // (`external_price_id = None`) when price fields are present, and an empty
    // prices vec only when ALL four price fields are absent (→ the sync loop's
    // NULL_PRICE fallback writes a single NULL-price row). These tests pin that
    // contract via the fake provider seam — no real Creem HTTP call.
    //
    // User Story: US-BL-SYNC-003
    // Covers: Creem real price fields propagation; empty-prices NULL fallback.

    /// Fetch the concrete column values (`external_price_id`, `billing_type`,
    /// `billing_period`) for the single mapping row matching a given
    /// `(external_product_id, external_price_id)` — the NULL-safe variant is
    /// used when `external_price_id` is `None` (Creem rows). Asserts exactly one
    /// row exists. The columns are stored as nullable text: `billing_type` is
    /// written via `BillingType::as_str()` (e.g. `"recurring"`), so the
    /// assertion compares against the parsed/stored text, not the source
    /// `ProviderPrice.billing_type` string verbatim.
    async fn fetch_mapping_columns(
        ctx: &SyncTestContext,
        realm_id: &str,
        external_product_id: &str,
        external_price_id: Option<&str>,
    ) -> (Option<String>, Option<String>, Option<String>) {
        let row: (Option<String>, Option<String>, Option<String>) =
            if let Some(pid) = external_price_id {
                sqlx::query_as(
                    "SELECT external_price_id, billing_type, billing_period
                 FROM provider_entitlement_mappings
                 WHERE realm_id = $1
                   AND external_product_id = $2
                   AND external_price_id = $3",
                )
                .bind(realm_id)
                .bind(external_product_id)
                .bind(pid)
                .fetch_one(&ctx.app_state.pool)
                .await
                .expect("mapping row must exist")
            } else {
                sqlx::query_as(
                    "SELECT external_price_id, billing_type, billing_period
                 FROM provider_entitlement_mappings
                 WHERE realm_id = $1
                   AND external_product_id = $2
                   AND external_price_id IS NULL",
                )
                .bind(realm_id)
                .bind(external_product_id)
                .fetch_one(&ctx.app_state.pool)
                .await
                .expect("mapping row must exist")
            };
        row
    }

    /// User Story: US-BL-SYNC-003
    /// Covers: Creem real price fields propagation; empty-prices NULL fallback
    ///
    /// A Creem product WITH price/currency/billing_type/billing_period present
    /// (but no `external_price_id`) lands exactly ONE mapping row carrying those
    /// fields, with `external_price_id IS NULL`.
    #[test_context(SyncTestContext)]
    #[tokio::test]
    async fn test_sync_creem_product_with_price_fields_lands_single_real_price_row(
        ctx: &mut SyncTestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        // Seed the realm's registration-pool credit bucket so the sync service
        // can bind newly-created draft mappings (idempotent).
        crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            &realm_id,
        )
        .await;

        let (_token, user_id) =
            crate::tests::helpers::billing_helpers::setup_billing_admin_session_with_user(
                ctx,
                "sync-creem-full@test.com",
            )
            .await;
        let identity: Identity = create_test_identity(user_id, &realm_id);

        // Creem product with all four price fields present, no external_price_id
        // (Creem is product-level only). The fake returns ONE real ProviderPrice.
        let product = ProviderProduct {
            external_product_id: "creem_prod_full".to_string(),
            name: "Creem Pro".to_string(),
            description: Some("Creem monthly".to_string()),
            product_metadata: None,
            prices: vec![ProviderPrice {
                external_price_id: None,
                price: Some(1999),
                currency: Some("usd".to_string()),
                billing_type: Some("recurring".to_string()),
                billing_period: Some("every-month".to_string()),
                price_metadata: None,
            }],
        };

        let service = build_sync_service(ctx, vec![product]).await;
        let result = service
            .sync_provider_products(identity, &realm_id, "creem")
            .await
            .expect("sync must complete");

        assert_eq!(result.sync_status, SyncStatus::Completed);
        assert_eq!(result.products_synced, 1);
        assert_eq!(result.prices_synced, 1);

        assert_eq!(
            count_mappings_for_product(ctx, &realm_id, "creem_prod_full").await,
            1,
            "expected exactly one mapping row for the Creem product"
        );

        // Columns: external_price_id IS NULL, billing fields propagated.
        // billing_type is stored via BillingType::as_str() → "recurring".
        let (ext_price_id, billing_type, billing_period) =
            fetch_mapping_columns(ctx, &realm_id, "creem_prod_full", None).await;
        assert!(
            ext_price_id.is_none(),
            "external_price_id must be NULL for a Creem (product-level) price row"
        );
        assert_eq!(
            billing_type.as_deref(),
            Some("recurring"),
            "billing_type must be parsed and stored as the BillingType text value"
        );
        assert_eq!(
            billing_period.as_deref(),
            Some("every-month"),
            "billing_period must propagate verbatim"
        );

        let info = fetch_single_product_info(ctx, &realm_id, "creem_prod_full", None).await;
        assert_eq!(info["price"], 1999);
        assert_eq!(info["currency"], "usd");
        assert_eq!(info["billing_type"], "recurring");
        assert_eq!(info["billing_period"], "every-month");
    }

    /// User Story: US-BL-SYNC-003
    /// Covers: Creem real price fields propagation; empty-prices NULL fallback
    ///
    /// A Creem product with NONE of the four price fields present falls back to
    /// a single NULL-price row (the sync loop's empty-prices → NULL_PRICE path).
    #[test_context(SyncTestContext)]
    #[tokio::test]
    async fn test_sync_creem_product_without_price_fields_falls_back_to_null_price_row(
        ctx: &mut SyncTestContext,
    ) {
        let realm_id = ctx._realm_id.clone();
        // Seed the realm's registration-pool credit bucket so the sync service
        // can bind newly-created draft mappings (idempotent).
        crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            &realm_id,
        )
        .await;

        let (_token, user_id) =
            crate::tests::helpers::billing_helpers::setup_billing_admin_session_with_user(
                ctx,
                "sync-creem-empty@test.com",
            )
            .await;
        let identity: Identity = create_test_identity(user_id, &realm_id);

        // Empty prices vec simulates the Creem fetch's all-four-fields-absent
        // case (infra returns no ProviderPrice → sync loop uses NULL_PRICE).
        let product = ProviderProduct {
            external_product_id: "creem_prod_empty".to_string(),
            name: "Creem Legacy".to_string(),
            description: None,
            product_metadata: None,
            prices: vec![],
        };

        let service = build_sync_service(ctx, vec![product]).await;
        let result = service
            .sync_provider_products(identity, &realm_id, "creem")
            .await
            .expect("sync must complete");

        assert_eq!(result.sync_status, SyncStatus::Completed);
        assert_eq!(result.products_synced, 1);
        // NULL_PRICE fallback still counts as one price-level upsert row.
        assert_eq!(result.prices_synced, 1);

        assert_eq!(
            count_mappings_for_product(ctx, &realm_id, "creem_prod_empty").await,
            1,
            "expected exactly one NULL-price mapping row"
        );

        let (ext_price_id, billing_type, billing_period) =
            fetch_mapping_columns(ctx, &realm_id, "creem_prod_empty", None).await;
        assert!(ext_price_id.is_none(), "external_price_id must be NULL");
        assert!(billing_type.is_none(), "billing_type must be NULL");
        assert!(billing_period.is_none(), "billing_period must be NULL");

        let info = fetch_single_product_info(ctx, &realm_id, "creem_prod_empty", None).await;
        assert!(info["price"].is_null(), "price must be null in JSONB");
        assert!(
            info["billing_period"].is_null(),
            "billing_period must be null in JSONB"
        );
    }

    /// User Story: US-BL-SYNC-003
    /// Covers: Creem real price fields propagation; empty-prices NULL fallback
    ///
    /// Re-syncing the same Creem product (same external_product_id, NULL
    /// external_price_id) must NOT create a duplicate row — dedup is by
    /// `(realm, provider, external_product_id, external_price_id=NULL)` via the
    /// NULLS NOT DISTINCT unique constraint.
    #[test_context(SyncTestContext)]
    #[tokio::test]
    async fn test_sync_creem_does_not_create_duplicate_rows_on_resync(ctx: &mut SyncTestContext) {
        let realm_id = ctx._realm_id.clone();
        // Seed the realm's registration-pool credit bucket so the sync service
        // can bind newly-created draft mappings (idempotent).
        crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            &realm_id,
        )
        .await;

        let (_token, user_id) =
            crate::tests::helpers::billing_helpers::setup_billing_admin_session_with_user(
                ctx,
                "sync-creem-resync@test.com",
            )
            .await;

        let make_product = || ProviderProduct {
            external_product_id: "creem_prod_full".to_string(),
            name: "Creem Pro".to_string(),
            description: Some("Creem monthly".to_string()),
            product_metadata: None,
            prices: vec![ProviderPrice {
                external_price_id: None,
                price: Some(1999),
                currency: Some("usd".to_string()),
                billing_type: Some("recurring".to_string()),
                billing_period: Some("every-month".to_string()),
                price_metadata: None,
            }],
        };

        let service = build_sync_service(ctx, vec![make_product()]).await;
        let identity: Identity = create_test_identity(user_id, &realm_id);
        service
            .sync_provider_products(identity, &realm_id, "creem")
            .await
            .expect("first sync must complete");
        assert_eq!(
            count_mappings_for_product(ctx, &realm_id, "creem_prod_full").await,
            1,
            "first sync must land exactly one row"
        );

        // Second sync — same external_product_id, NULL external_price_id.
        let service_v2 = build_sync_service(ctx, vec![make_product()]).await;
        let identity2: Identity = create_test_identity(user_id, &realm_id);
        let result = service_v2
            .sync_provider_products(identity2, &realm_id, "creem")
            .await
            .expect("second sync must complete");
        assert_eq!(result.sync_status, SyncStatus::Completed);

        // Still exactly one row — NULLS NOT DISTINCT dedup updated in place.
        assert_eq!(
            count_mappings_for_product(ctx, &realm_id, "creem_prod_full").await,
            1,
            "re-sync must not create a duplicate row (NULLS NOT DISTINCT dedup)"
        );
    }

    /// User Story: US-BL-SYNC-003
    /// Covers: re-sync preserves the mapping's billing_type when the provider
    /// reports no recognizable one
    ///
    /// PRD subscription §4.1 pins mapping config as Herald-managed ("sync must
    /// not override Herald-managed entitlement/points/quota policy"), so a
    /// full re-sync must not clobber a stored `billing_type` to NULL — a
    /// NULLed billing_type makes every later purchase fail with "has no
    /// billing_type set". Two preserve paths are pinned: (a) the product stops
    /// reporting price fields entirely (→ the NULL_PRICE fallback row), and
    /// (b) the provider reports a word form the domain cannot parse.
    #[test_context(SyncTestContext)]
    #[tokio::test]
    async fn test_sync_creem_resync_preserves_configured_billing_type(ctx: &mut SyncTestContext) {
        let realm_id = ctx._realm_id.clone();
        // Seed the realm's registration-pool credit bucket so the sync service
        // can bind newly-created draft mappings (idempotent).
        crate::tests::helpers::points_helpers::ensure_test_bucket_for_realm(
            &ctx.app_state.pool,
            &realm_id,
        )
        .await;

        let (_token, user_id) =
            crate::tests::helpers::billing_helpers::setup_billing_admin_session_with_user(
                ctx,
                "sync-creem-bt-preserve@test.com",
            )
            .await;

        // First sync: both products carry parseable price fields, so each
        // mapping lands with billing_type = "recurring".
        let make_product = |id: &str| ProviderProduct {
            external_product_id: id.to_string(),
            name: format!("Creem {id}"),
            description: None,
            product_metadata: None,
            prices: vec![ProviderPrice {
                external_price_id: None,
                price: Some(1999),
                currency: Some("usd".to_string()),
                billing_type: Some("recurring".to_string()),
                billing_period: Some("every-month".to_string()),
                price_metadata: None,
            }],
        };
        let service = build_sync_service(
            ctx,
            vec![
                make_product("creem_prod_bt_nullprice"),
                make_product("creem_prod_bt_unknown"),
            ],
        )
        .await;
        let identity: Identity = create_test_identity(user_id, &realm_id);
        service
            .sync_provider_products(identity, &realm_id, "creem")
            .await
            .expect("first sync must complete");
        for pid in ["creem_prod_bt_nullprice", "creem_prod_bt_unknown"] {
            let (_, billing_type, _) = fetch_mapping_columns(ctx, &realm_id, pid, None).await;
            assert_eq!(
                billing_type.as_deref(),
                Some("recurring"),
                "{pid}: first sync must land billing_type = recurring"
            );
        }

        // Re-sync: (a) stops reporting price fields entirely (→ NULL_PRICE
        // fallback row); (b) reports an unparseable word form. Both must
        // PRESERVE the stored billing_type instead of writing NULL.
        let mut null_price = make_product("creem_prod_bt_nullprice");
        null_price.prices.clear();
        let mut unknown_form = make_product("creem_prod_bt_unknown");
        unknown_form.prices[0].billing_type = Some("lifetime".to_string());
        unknown_form.prices[0].billing_period = None;
        let service_v2 = build_sync_service(ctx, vec![null_price, unknown_form]).await;
        let identity2: Identity = create_test_identity(user_id, &realm_id);
        let result = service_v2
            .sync_provider_products(identity2, &realm_id, "creem")
            .await
            .expect("re-sync must complete");
        assert_eq!(result.sync_status, SyncStatus::Completed);

        let (_, bt_nullprice, _) =
            fetch_mapping_columns(ctx, &realm_id, "creem_prod_bt_nullprice", None).await;
        assert_eq!(
            bt_nullprice.as_deref(),
            Some("recurring"),
            "a NULL_PRICE re-sync must preserve the stored billing_type"
        );
        let (_, bt_unknown, _) =
            fetch_mapping_columns(ctx, &realm_id, "creem_prod_bt_unknown", None).await;
        assert_eq!(
            bt_unknown.as_deref(),
            Some("recurring"),
            "an unparseable provider word form must preserve the stored billing_type"
        );
    }

    // =========================================================================
    // Batch-update preserves synced billing_period (BE-T03 / US-BL-SYNC-004)
    // =========================================================================
    //
    // These are DB-backed integration tests through the REAL HTTP batch PUT
    // endpoint (PUT /api/bill/{realmId}/entitlement-mappings/batch) — they do
    // NOT use the FakeProviderApi seam above. They assert that a batch update
    // no longer writes `billing_period`: a previously-synced value survives a
    // batch update that does not target it (concern #3 / design §6.1 + §6.3
    // regression for the checkout reader at api-billing/src/handlers.rs:540).

    /// Build an authenticated request with the admin auth cookie — mirrors the
    /// `auth_request` pattern in `entitlement_mapping_crud_scenarios.rs`.
    fn batch_auth_request(
        method: &str,
        uri: String,
        token: &str,
        body: Option<axum::body::Body>,
    ) -> axum::http::Request<axum::body::Body> {
        use axum::http::{Request, header};
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", token));
        if let Some(b) = body {
            builder = builder.header("Content-Type", "application/json");
            builder.body(b).unwrap()
        } else {
            builder.body(axum::body::Body::empty()).unwrap()
        }
    }

    /// User Story: US-BL-SYNC-004
    /// Covers: batch update no longer writes billing_period; synced value preserved (regression for checkout reader handlers.rs:540)
    ///
    /// Seed a mapping with `billing_period = 'every-month'`, send a batch PUT
    /// that updates ONLY non-`billing_period` fields, and assert the DB
    /// `billing_period` is UNCHANGED.
    #[test_context(SyncTestContext)]
    #[tokio::test]
    async fn test_batch_update_preserves_synced_billing_period(ctx: &mut SyncTestContext) {
        use tower::ServiceExt;

        let realm_id = ctx._realm_id.clone();
        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "sync-batch-bp-preserve@test.com",
        )
        .await;

        // Seed a mapping with a known synced billing_period.
        let mapping_id =
            crate::tests::helpers::billing_helpers::setup_test_entitlement_mapping_full(
                ctx,
                &realm_id,
                "stripe",
                "prod_bp_1",
                Some("price_bp_1"),
                "initial-key",
                Some("recurring"),
                Some("every-month"),
                Some(100),
                None,
                None,
                true,
                None,
                true,
                None,
            )
            .await;

        let (.., seeded_bp) =
            fetch_mapping_columns(ctx, &realm_id, "prod_bp_1", Some("price_bp_1")).await;
        assert_eq!(
            seeded_bp.as_deref(),
            Some("every-month"),
            "seeded billing_period must be the synced value before the batch update"
        );

        let payload = serde_json::json!({
            "paymentProvider": "stripe",
            "externalProductId": "prod_bp_1",
            "updates": [{
                "mappingId": mapping_id,
                "entitlementKey": "updated-key",
                "pointsPerPeriod": 200,
                "enabled": true
            }]
        });

        let app = ctx.create_unified_test_router();
        let response = app
            .clone()
            .oneshot(batch_auth_request(
                "PUT",
                format!("/api/bill/{}/entitlement-mappings/batch", realm_id),
                &token,
                Some(axum::body::Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        // The handler returns StatusCode::CREATED (201) on success.
        assert_eq!(
            response.status(),
            axum::http::StatusCode::CREATED,
            "batch update must succeed"
        );

        let (.., billing_period) =
            fetch_mapping_columns(ctx, &realm_id, "prod_bp_1", Some("price_bp_1")).await;
        assert_eq!(
            billing_period.as_deref(),
            Some("every-month"),
            "billing_period must be UNCHANGED after a batch update that does not target it"
        );
    }

    /// User Story: US-BL-SYNC-004
    /// Covers: batch update no longer writes billing_period; synced value preserved (regression for checkout reader handlers.rs:540)
    ///
    /// Regression guard: `billing_period` is NOT part of the batch update
    /// contract (it is provider-sync-owned), so it is absent from
    /// `PriceMappingUpdate`. A client that nonetheless sends `billingPeriod`
    /// cannot override the synced value: the key is dropped as an unknown field
    /// and the batch SQL never writes `billing_period`.
    #[test_context(SyncTestContext)]
    #[tokio::test]
    async fn test_batch_update_with_billing_period_in_payload_still_preserves_synced_value(
        ctx: &mut SyncTestContext,
    ) {
        use tower::ServiceExt;

        let realm_id = ctx._realm_id.clone();
        let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
            ctx,
            "sync-batch-bp-ignored@test.com",
        )
        .await;

        let mapping_id =
            crate::tests::helpers::billing_helpers::setup_test_entitlement_mapping_full(
                ctx,
                &realm_id,
                "stripe",
                "prod_bp_2",
                Some("price_bp_2"),
                "initial-key",
                Some("recurring"),
                Some("every-month"),
                Some(100),
                None,
                None,
                true,
                None,
                true,
                None,
            )
            .await;

        // billing_period is not on PriceMappingUpdate, so the JSON key is
        // dropped as an unknown field and the batch SQL never writes it.
        let payload = serde_json::json!({
            "paymentProvider": "stripe",
            "externalProductId": "prod_bp_2",
            "updates": [{
                "mappingId": mapping_id,
                "entitlementKey": "updated-key-2",
                "billingPeriod": "every-year",
                "pointsPerPeriod": 300,
                "enabled": true
            }]
        });

        let app = ctx.create_unified_test_router();
        let response = app
            .clone()
            .oneshot(batch_auth_request(
                "PUT",
                format!("/api/bill/{}/entitlement-mappings/batch", realm_id),
                &token,
                Some(axum::body::Body::from(payload.to_string())),
            ))
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::CREATED,
            "batch update must succeed even with the deprecated billing_period field present"
        );

        let (.., billing_period) =
            fetch_mapping_columns(ctx, &realm_id, "prod_bp_2", Some("price_bp_2")).await;
        assert_eq!(
            billing_period.as_deref(),
            Some("every-month"),
            "billing_period must remain the synced value even when the payload tries to change it"
        );
    }
}
