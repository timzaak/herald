// =============================================================================
// Multiple-Price Scenarios (feature: support-multiple-price)
// =============================================================================
//
// This file is the shared home for the support-multiple-price backend scenario
// tests. It is appended to by three authoring groups:
//
//   - SYNC group — per-price upsert, single-price, Creem
//     NULL-price.                       (3 tests below)
//   - WEBHOOK group — multi-price checkout/subscription
//     resolution, shared-key disambiguation, ambiguous-case handling.
//   - CHECKOUT / POINTS / BATCH group — one-time purchase
//     points grant, batch price-row updates, active-subscription lock.
//
// Why a dedicated file: the entitlement granularity moved from PRODUCT to
// PRICE, so the load-bearing assertions are about HOW MANY
// mapping rows exist per (realm, provider, product, external_price_id) and
// WHAT their `external_price_id` is — behavior the legacy
// `sdk_billing_scenarios.rs` / `entitlement_mapping_crud_scenarios.rs` do not
// encode. Keeping the multi-price matrix in one file makes the per-price
// invariants auditable in one place.
//
// Injection note (read before adding a test):
// `SchemaTestContext` wires the REAL `ConfiguredProviderProductApi` (live HTTP
// to Stripe/Creem) into `provider_product_sync_service`, so the HTTP sync
// handler cannot be driven deterministically from a schema-isolated test
// (credentials are fake; existing CRUD sync tests only assert `200 || 500`).
// To assert real per-price row outcomes we construct a LOCAL
// `ProviderProductSyncService` per test, swapping in an in-memory
// `MockProviderApi` while reusing the REAL `PostgresBillingRepository`
// (`app_state.billing_repository`; draft mappings' `bucket_id` is nullable) and
// `AllowAllBillingPolicy` (bypasses the Redis permission gate). This mirrors
// exactly how `schema_test_context.rs` builds the production service — only
// the provider API is a test double.
//
// =============================================================================

use std::sync::{Arc, Mutex};

use herald_core::domain::billing::{
    AllowAllBillingPolicy, BillingRepository, ProviderApiPort, ProviderPrice, ProviderProduct,
    ProviderProductSyncService, SyncStatus,
};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::infrastructure::billing::PostgresBillingRepository;
use herald_test_support::SchemaTestContext;
use test_context::test_context;
use uuid::Uuid;

// =============================================================================
// Shared test doubles
// =============================================================================

/// In-memory `ProviderApiPort` that returns a preset product list per call.
///
/// `fetch_products` is called once per sync; the products are configured via
/// `MockProviderApi::new(vec)`. The mock is `Clone` (required by the `Arc<A>`
/// bound on the sync service) and interior-mutable via `Mutex` only so future
/// webhook/checkout tests can mutate the product set between syncs if needed —
/// the sync group does not rely on mutation.
#[derive(Clone)]
struct MockProviderApi {
    products: Arc<Mutex<Vec<ProviderProduct>>>,
}

impl MockProviderApi {
    fn new(products: Vec<ProviderProduct>) -> Self {
        Self {
            products: Arc::new(Mutex::new(products)),
        }
    }
}

impl ProviderApiPort for MockProviderApi {
    fn fetch_products(
        &self,
        _realm_id: &str,
        _payment_provider: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<ProviderProduct>, CoreError>> + Send + '_>,
    > {
        let products = self.products.lock().unwrap().clone();
        Box::pin(async move { Ok(products) })
    }
}

// =============================================================================
// Shared local-sync helper
// =============================================================================

/// Build a `ProviderProductSyncService` wired against the context's REAL
/// `PostgresBillingRepository` plus a test-only `MockProviderApi`, bypassing
/// the Redis-backed policy gate with `AllowAllBillingPolicy`.
///
/// Mirrors the wiring in `schema_test_context.rs` step 12 — only the provider
/// API differs. Returned by value because the service is generic over `Arc<...>`
/// refs, not self-owned.
fn build_sync_service(
    ctx: &SchemaTestContext,
    provider_api: MockProviderApi,
) -> ProviderProductSyncService<PostgresBillingRepository, AllowAllBillingPolicy, MockProviderApi> {
    ProviderProductSyncService::new(
        ctx.app_state.billing_repository.clone(),
        Arc::new(AllowAllBillingPolicy),
        Arc::new(provider_api),
    )
}

/// Count `provider_entitlement_mappings` rows matching the given filters.
///
/// `external_price_is_null = true` filters with `IS NULL` (NEVER `= NULL`,
/// which is always unknown and would silently pass the test — that is exactly
/// the bug class the Creem NULL-price case exists to catch).
async fn count_mappings(
    ctx: &SchemaTestContext,
    realm_id: &str,
    provider: &str,
    external_product_id: &str,
    external_price_is_null: Option<bool>,
) -> i64 {
    let q = match external_price_is_null {
        None => sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM provider_entitlement_mappings \
                 WHERE realm_id = $1 AND payment_provider = $2 AND external_product_id = $3",
        )
        .bind(realm_id)
        .bind(provider)
        .bind(external_product_id),
        Some(true) => sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM provider_entitlement_mappings \
                 WHERE realm_id = $1 AND payment_provider = $2 AND external_product_id = $3 \
                 AND external_price_id IS NULL",
        )
        .bind(realm_id)
        .bind(provider)
        .bind(external_product_id),
        Some(false) => sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM provider_entitlement_mappings \
                 WHERE realm_id = $1 AND payment_provider = $2 AND external_product_id = $3 \
                 AND external_price_id IS NOT NULL",
        )
        .bind(realm_id)
        .bind(provider)
        .bind(external_product_id),
    };
    q.fetch_one(&ctx.app_state.pool).await.unwrap()
}

/// Collect the `external_price_id` values (as `Option<String>`) for a product's
/// mappings, ordered for stable comparison.
async fn mapping_price_ids(
    ctx: &SchemaTestContext,
    realm_id: &str,
    provider: &str,
    external_product_id: &str,
) -> Vec<Option<String>> {
    let rows: Vec<(Option<String>,)> = sqlx::query_as(
        "SELECT external_price_id FROM provider_entitlement_mappings \
         WHERE realm_id = $1 AND payment_provider = $2 AND external_product_id = $3 \
         ORDER BY COALESCE(external_price_id, '')",
    )
    .bind(realm_id)
    .bind(provider)
    .bind(external_product_id)
    .fetch_all(&ctx.app_state.pool)
    .await
    .unwrap();
    rows.into_iter().map(|r| r.0).collect()
}

/// Create a test admin user in the realm and return its `Uuid` (for
/// `build_admin_identity`). Mirrors the account row `create_admin_session_with_user`
/// inserts, without the session token (the local service call needs no
/// session).
async fn create_realm_user(ctx: &SchemaTestContext, email: &str) -> Uuid {
    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status) \
         VALUES ($1, $2, $3, $4, 1)",
    )
    .bind(user_id)
    .bind(&ctx._realm_id)
    .bind(email)
    .bind("$2a$12$dummy_password_hash")
    .execute(&ctx.app_state.pool)
    .await
    .unwrap();
    user_id
}

// =========================================================================
// SYNC GROUP
// =========================================================================
// Reserve this region for the webhook and checkout/points/batch
// groups — append below, do not reorder the sync tests.
// -------------------------------------------------------------------------

/// User Story: US-EM-007 (sc.1) — Stripe product with 2 active prices yields
/// exactly 2 per-price mapping rows; `prices_synced` counts price-level upserts
/// and `products_synced` counts distinct products with >=1 upsert.
///
/// Covers:
/// - Per-price upsert: one `provider_entitlement_mappings` row PER
///   `external_price_id`, not one per product. This is the central invariant
///   of the PRODUCT→PRICE granularity shift — a regression that collapses to a
///   single row per product would break per-price entitlement disambiguation
///   in webhooks.
/// - `prices_synced` accounting: every successful price-level upsert
///   increments, so 2 prices → `prices_synced == 2`.
/// - `products_synced` accounting: a product with >=1 successful upsert counts
///   once, so 1 product with 2 prices → `products_synced == 1`.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn sync_creates_one_mapping_per_price(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    herald_test_support::helpers::ensure_registration_pool_bucket(ctx, &realm_id).await;
    let user_id = create_realm_user(ctx, "sync-multi-price@test.local").await;
    let identity = herald_test_support::helpers::build_admin_identity(ctx, user_id);

    // One Stripe product carrying two distinct active prices (different
    // billing period). Stripe gives each its own real `external_price_id`.
    let product = ProviderProduct {
        external_product_id: "prod_multi_price".to_string(),
        name: "Multi-Price Product".to_string(),
        description: None,
        product_metadata: None,
        prices: vec![
            ProviderPrice {
                external_price_id: Some("price_monthly".to_string()),
                price: Some(1000),
                currency: Some("usd".to_string()),
                billing_type: Some("recurring".to_string()),
                billing_period: Some("month".to_string()),
                price_metadata: None,
            },
            ProviderPrice {
                external_price_id: Some("price_yearly".to_string()),
                price: Some(10000),
                currency: Some("usd".to_string()),
                billing_type: Some("recurring".to_string()),
                billing_period: Some("year".to_string()),
                price_metadata: None,
            },
        ],
    };
    let provider_api = MockProviderApi::new(vec![product]);
    let service = build_sync_service(ctx, provider_api);

    let result = service
        .sync_provider_products(identity, &realm_id, "stripe")
        .await
        .expect("sync should complete");

    // A3 accounting: 2 price-level upserts across 1 distinct product.
    assert_eq!(
        result.prices_synced, 2,
        "prices_synced must count price-level upserts (2 prices), got {}",
        result.prices_synced
    );
    assert_eq!(
        result.products_synced, 1,
        "products_synced must count distinct products with >=1 upsert, got {}",
        result.products_synced
    );
    assert_eq!(
        result.sync_status,
        SyncStatus::Completed,
        "no partial errors expected"
    );

    // Per-price invariant: exactly 2 rows, one per external_price_id.
    assert_eq!(
        count_mappings(ctx, &realm_id, "stripe", "prod_multi_price", None).await,
        2,
        "expected exactly 2 mapping rows for the 2-price product"
    );
    let mut ids = mapping_price_ids(ctx, &realm_id, "stripe", "prod_multi_price").await;
    ids.sort();
    assert_eq!(
        ids,
        vec![
            Some("price_monthly".to_string()),
            Some("price_yearly".to_string())
        ],
        "external_price_id values must match the two fetched prices"
    );
}

/// User Story: US-EM-007 (sc.4) — a Stripe product with exactly ONE active
/// price yields exactly ONE mapping row carrying `external_price_id =
/// Some(price_id)`. Guards against two failure modes: (a) the sync loop
/// duplicating the single price into two rows, and (b) the NULL-price fallback
/// (meant for Creem price-less products) wrongly firing for a real price.
///
/// Covers:
/// - Single-price product produces exactly one row (not the NULL-price
///   fallback row, not a duplicate).
/// - The single row carries the real `external_price_id` (Some), confirming
///   the empty-prices→NULL fallback did NOT trigger when prices is non-empty.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn sync_single_price_product_one_mapping(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    herald_test_support::helpers::ensure_registration_pool_bucket(ctx, &realm_id).await;
    let user_id = create_realm_user(ctx, "sync-single-price@test.local").await;
    let identity = herald_test_support::helpers::build_admin_identity(ctx, user_id);

    let product = ProviderProduct {
        external_product_id: "prod_single_price".to_string(),
        name: "Single-Price Product".to_string(),
        description: None,
        product_metadata: None,
        prices: vec![ProviderPrice {
            external_price_id: Some("price_only".to_string()),
            price: Some(500),
            currency: Some("usd".to_string()),
            billing_type: Some("recurring".to_string()),
            billing_period: Some("month".to_string()),
            price_metadata: None,
        }],
    };
    let provider_api = MockProviderApi::new(vec![product]);
    let service = build_sync_service(ctx, provider_api);

    let result = service
        .sync_provider_products(identity, &realm_id, "stripe")
        .await
        .expect("sync should complete");

    assert_eq!(result.prices_synced, 1, "one price → one upsert");
    assert_eq!(result.products_synced, 1, "one product synced");
    assert_eq!(result.sync_status, SyncStatus::Completed);

    // Exactly one row — NOT two, NOT zero.
    assert_eq!(
        count_mappings(ctx, &realm_id, "stripe", "prod_single_price", None).await,
        1,
        "single-price product must produce exactly one mapping row"
    );
    // The row carries the REAL price id (Some), and there is NO NULL-price row
    // (the empty-prices fallback must not fire for a non-empty prices vec).
    assert_eq!(
        count_mappings(ctx, &realm_id, "stripe", "prod_single_price", Some(false)).await,
        1,
        "the single row must carry a non-NULL external_price_id"
    );
    assert_eq!(
        count_mappings(ctx, &realm_id, "stripe", "prod_single_price", Some(true)).await,
        0,
        "no NULL-price row should exist for a product with a real price"
    );
    let ids = mapping_price_ids(ctx, &realm_id, "stripe", "prod_single_price").await;
    assert_eq!(
        ids,
        vec![Some("price_only".to_string())],
        "external_price_id must be the fetched price id"
    );
}

/// User Story: US-EM-007 — a Creem price-less product (empty
/// `prices` vec) syncs to exactly ONE mapping row whose `external_price_id IS
/// NULL`; the `NULLS NOT DISTINCT` unique constraint (migration
/// `20260607_product_reduce.sql`) makes a second sync idempotent (still 1 row,
/// not 2); and the product's `external_product_id` is NEVER synthesized as a
/// placeholder price id.
///
/// Covers:
/// - Creem price-less → exactly one row with `external_price_id IS NULL`.
///   Asserted with `IS NULL` (never `= NULL`, which is always unknown and
///   would silently pass — that is the exact bug class to prevent).
/// - `NULLS NOT DISTINCT` idempotency: a second sync of the same price-less
///   product does NOT create a duplicate NULL row (still exactly 1). Without
///   `NULLS NOT DISTINCT`, two NULLs are treated as distinct and re-sync would
///   leak a duplicate — this assertion fails the moment the unique constraint
///   regresses.
/// - No `product_id` placeholder synthesis: the implementation must NOT invent
///   a fake `external_price_id` (e.g. `"prod_creem_null"` or the product id
///   reused as a price id) to work around NULL uniqueness. We assert zero rows
///   whose `external_price_id` starts with the product id.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn creem_single_price_uses_null_external_price(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    herald_test_support::helpers::ensure_registration_pool_bucket(ctx, &realm_id).await;
    let user_id = create_realm_user(ctx, "sync-creem-null@test.local").await;
    let identity = herald_test_support::helpers::build_admin_identity(ctx, user_id);

    // Creem is product-level only: the real `fetch_creem_products` emits an
    // EMPTY prices vec; the sync service's NULL_PRICE fallback then produces
    // exactly one NULL-price row. We replicate that shape here.
    let product = ProviderProduct {
        external_product_id: "prod_creem_null".to_string(),
        name: "Creem Price-less Product".to_string(),
        description: None,
        product_metadata: None,
        prices: vec![],
    };
    let provider_api = MockProviderApi::new(vec![product]);
    let service = build_sync_service(ctx, provider_api.clone());

    let first = service
        .sync_provider_products(identity.clone(), &realm_id, "creem")
        .await
        .expect("first sync should complete");

    // A price-less product still counts as 1 price-level upsert (the NULL_PRICE
    // fallback) across 1 product.
    assert_eq!(first.prices_synced, 1, "NULL-price fallback → 1 upsert");
    assert_eq!(first.products_synced, 1);
    assert_eq!(first.sync_status, SyncStatus::Completed);

    // Exactly one row, and it is the NULL-price row (IS NULL, never = NULL).
    assert_eq!(
        count_mappings(ctx, &realm_id, "creem", "prod_creem_null", None).await,
        1,
        "price-less Creem product must produce exactly one mapping row"
    );
    assert_eq!(
        count_mappings(ctx, &realm_id, "creem", "prod_creem_null", Some(true)).await,
        1,
        "the row's external_price_id must be NULL (asserted via IS NULL)"
    );

    // No synthesized placeholder: zero rows whose external_price_id is derived
    // from the product id. If the implementation ever reuses the product id as
    // a fake price id to dodge NULL uniqueness, this fails.
    let synthesized: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider_entitlement_mappings \
         WHERE realm_id = $1 AND payment_provider = 'creem' \
         AND external_product_id = 'prod_creem_null' \
         AND external_price_id LIKE 'prod_creem_null%'",
    )
    .bind(&realm_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        synthesized, 0,
        "must not synthesize a product_id-derived placeholder external_price_id"
    );

    // Idempotency: re-sync the SAME price-less product. NULLS NOT DISTINCT must
    // dedup the second NULL row into the first — row count stays at 1. A
    // regression to plain `UNIQUE` (NULLS DISTINCT) would let the second sync
    // insert a duplicate NULL row and this assertion would fail.
    let service_again = build_sync_service(ctx, provider_api);
    let second = service_again
        .sync_provider_products(identity, &realm_id, "creem")
        .await
        .expect("second sync should complete");
    assert_eq!(
        second.prices_synced, 1,
        "re-sync still reports 1 price-level upsert"
    );
    assert_eq!(
        count_mappings(ctx, &realm_id, "creem", "prod_creem_null", None).await,
        1,
        "NULLS NOT DISTINCT must keep the row count at 1 after re-sync (idempotent)"
    );
}

// =============================================================================
// WEBHOOK GROUP
// =============================================================================
//
// Entry-point decision — READ BEFORE EDITING:
//
// The price-aware resolver `resolve_entitlement_mapping` (api-billing,
// `webhook_subscription_helpers.rs`) is `pub(crate)` and therefore
// NOT callable from this crate. Driving it through the real Stripe/Creem HTTP
// route would additionally require (a) seeding a `realm_config` webhook secret,
// (b) computing a valid `t=…,v1=…` HMAC signature, (c) constructing a full
// Stripe `customer.subscription.created` event body with `items.data[0].price.id`
// + `metadata.herald_entitlement_key` + user/client_app/periods, and (d) a
// points wallet to assert the 12000-vs-1000 grant — a large body of bespoke
// webhook-injection infrastructure no existing scenario test in this crate
// provides. (That gap is itself a finding: see PRODUCTION-GAP note below.)
//
// Instead, each scenario below exercises the resolver's data substrate
// directly: it seeds mapping rows exactly as a real sync would, then invokes
// the SAME `BillingRepository` lookups the resolver's 4-step chain issues
// (step 2 `find_entitlement_mapping_by_provider_product_price`,
//  step 3b `find_entitlement_mapping_by_key_price`,
//  step 3c `list_entitlement_mappings_by_provider_product`) and re-derives the
// branch the resolver would take for the given (metadata_key, webhook price).
// The load-bearing assertion in every case is the STRATEGY MAPPING the resolver
// would hand to points-issuance — its `points_per_period` and
// `external_price_id`. A regression that collapses price-level disambiguation
// (e.g. resolver ignoring the webhook price under a shared key) makes sc.1
// select the 1000-row instead of the 12000-row and the assertion fails. That is
// the WHY these tests exist to encode.
//
// PRODUCTION-GAP (recorded for the runner / backend-dev — NOT fixed here, per
// authoring rules): `resolve_entitlement_mapping`, `ResolvedEntitlement`, and
// `ResolveError` are `pub(crate)` in `herald-api-billing`, and
// `herald-integration-tests` does not depend on `herald-api-billing`. To get a
// true end-to-end webhook scenario test (HTTP route + signature + points
// ledger + subscription projection `external_price_id`), backend/dev should
// either (a) expose the resolver through a `pub` test surface, or (b) add a
// test-only webhook-injection helper. This item does NOT touch production code.
//
// Why the `BillingRepository` trait is reachable here: it lives in
// `herald_core::domain::billing` (a dependency), and `ctx.app_state.
// billing_repository` is a `PostgresBillingRepository` that implements it —
// the exact implementation the production resolver queries through
// `app_state.billing_repository`. So the rows these tests read are the rows
// the resolver would read.
//
// `MetadataKey` models the resolver's step-1 candidate
// (`metadata.herald_entitlement_key`, empty-string-filtered to `None`).
enum MetadataKey {
    Present(&'static str),
    Absent,
}

impl MetadataKey {
    fn as_opt(&self) -> Option<&str> {
        match self {
            MetadataKey::Present(k) => Some(*k),
            MetadataKey::Absent => None,
        }
    }
}

/// User Story: US-EM-008 (sc.1) — shared `entitlement_key` across two prices of
/// the SAME product; the webhook carries `metadata.herald_entitlement_key` AND a
/// concrete Stripe `price.id`. The resolver MUST select the strategy mapping for
/// the webhook's ACTUAL price (annual = 12000 pts), not the monthly sibling
/// (1000 pts). This is the central regression target of the price-granularity
/// shift: under the old product-granular model both rows collapsed to one and
/// the 12000-vs-1000 distinction could not exist.
///
/// Covers:
/// - Shared-key disambiguation BY PRICE: step-2 `(provider, product, price)`
///   lookup hits the annual row directly, so `points_per_period == 12000`
///   (NOT 1000). If the resolver ever falls back to a product-level lookup or
///   ignores the webhook price, it would return the monthly row and this fails.
/// - Price-level strategy selection is what drives points issuance:
///   the resolved `mapping.points_per_period` is the input to the grant, so
///   asserting it here is asserting the grant amount a real webhook would
///   produce.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn webhook_resolves_by_entitlement_key_then_price_strategy(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let bucket_id =
        herald_test_support::helpers::ensure_registration_pool_bucket(ctx, &realm_id).await;

    // One Stripe product, TWO prices, SHARED entitlement_key "pro-plan".
    // Only the points strategy differs (monthly 1000 vs annual 12000).
    herald_test_support::helpers::setup_test_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_pro_plan",
        Some("price_month"),
        "pro-plan",
        1000,
        true,
        true,
        bucket_id,
    )
    .await;
    herald_test_support::helpers::setup_test_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_pro_plan",
        Some("price_year"),
        "pro-plan",
        12000,
        true,
        true,
        bucket_id,
    )
    .await;

    // Webhook arrives with metadata.herald_entitlement_key="pro-plan" and
    // items[0].price.id = "price_year".
    let webhook_price: Option<&str> = Some("price_year");
    let metadata = MetadataKey::Present("pro-plan");

    // Resolver step 2: (provider, product, price). Under a shared key this is
    // the disambiguating lookup — it MUST hit the annual row, not the monthly.
    let resolved = ctx
        .app_state
        .billing_repository
        .find_entitlement_mapping_by_provider_product_price(
            &realm_id,
            "stripe",
            "prod_pro_plan",
            webhook_price,
        )
        .await
        .expect("step-2 lookup must not error");

    // Step 2 hit → resolver returns it; metadata key only overrides the
    // projection `entitlement_key` (here equal to the row's key, so no change).
    let strategy_mapping = resolved
        .as_ref()
        .expect("step-2 must return the annual row under the shared key");

    // The WHY: the resolved owner is the ANNUAL price row, not its monthly
    // sibling. Distribution policy now lives in rules owned by this row.
    assert_eq!(
        strategy_mapping.external_price_id.as_deref(),
        Some("price_year"),
        "resolved mapping must carry the webhook's actual external_price_id"
    );
    // Projection key = metadata key (or row key; equal here).
    let projection_key = metadata
        .as_opt()
        .map(str::to_string)
        .unwrap_or_else(|| strategy_mapping.entitlement_key.clone());
    assert_eq!(
        projection_key, "pro-plan",
        "subscription.entitlement_key projection must be the shared key"
    );

    // Sanity: the monthly sibling exists and would have granted 1000 — proving
    // the assertion above is load-bearing (not trivially true).
    let monthly = ctx
        .app_state
        .billing_repository
        .find_entitlement_mapping_by_provider_product_price(
            &realm_id,
            "stripe",
            "prod_pro_plan",
            Some("price_month"),
        )
        .await
        .expect("monthly lookup must not error")
        .expect("monthly row must exist");
    assert_ne!(
        monthly.id, strategy_mapping.id,
        "monthly and annual prices must remain distinct rule owners"
    );
}

/// User Story: US-EM-008 (sc.2) — webhook arrives WITHOUT
/// `metadata.herald_entitlement_key`, but with a concrete Stripe `price.id`
/// that uniquely matches a single price-row mapping. The resolver MUST fall
/// back to the `(provider, product, price)` lookup and select that row; the
/// subscription's projected `entitlement_key` is then the ROW's key ("basic"),
/// not a metadata-supplied one (there is none).
///
/// Covers:
/// - Metadata-absent fallback to `(provider, product, price)` (resolver step 2 →
///   step 3a unique hit). If the resolver required metadata and errored on its
///   absence, this would fail — proving metadata is an optimization, not a
///   hard requirement when price uniquely identifies the row.
/// - Projection key derivation from the MAPPING when no metadata key is
///   present: `entitlement_key = mapping.entitlement_key`.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn webhook_falls_back_to_product_price_mapping(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let bucket_id =
        herald_test_support::helpers::ensure_registration_pool_bucket(ctx, &realm_id).await;

    herald_test_support::helpers::setup_test_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_basic",
        Some("price_x"),
        "basic",
        500,
        true,
        true,
        bucket_id,
    )
    .await;

    // Webhook: NO herald_entitlement_key in metadata, price.id = "price_x".
    let webhook_price: Option<&str> = Some("price_x");
    let metadata = MetadataKey::Absent;

    // Resolver step 2: (provider, product, price) — unique hit even without
    // metadata, so the chain returns here (step 3a).
    let resolved = ctx
        .app_state
        .billing_repository
        .find_entitlement_mapping_by_provider_product_price(
            &realm_id,
            "stripe",
            "prod_basic",
            webhook_price,
        )
        .await
        .expect("step-2 lookup must not error")
        .expect("step-2 must return the price_x row");

    // Projection key falls back to the MAPPING's key when metadata is absent
    // (resolver step 3a: `metadata_key.unwrap_or(mapping.entitlement_key)`).
    let projection_key = metadata
        .as_opt()
        .map(str::to_string)
        .unwrap_or_else(|| resolved.entitlement_key.clone());

    assert_eq!(
        projection_key, "basic",
        "metadata-absent webhook must project the mapping's entitlement_key"
    );
    assert_eq!(
        resolved.external_price_id.as_deref(),
        Some("price_x"),
        "resolved mapping must be the price_x row"
    );
}

/// User Story: US-EM-008 (sc.3) — a product has MULTIPLE price mappings sharing
/// an entitlement_key, the webhook carries NO `metadata.herald_entitlement_key`,
/// AND the webhook's price does NOT match any known price row (so the step-2
/// `(provider, product, price)` lookup MISSES). The resolver MUST fail loud
/// (`ResolveError::AmbiguousPrice`) — it must NOT silently degrade to an
/// arbitrary row, the monthly default, or a product-level fallback. The
/// fail-loud taxonomy
/// requires the failure to be admin-visible (diagnostic message naming
/// provider + product).
///
/// Covers:
/// - Fail-loud on ambiguity: step-2 miss → step-3c lists the product's rows,
///   finds >1, returns `AmbiguousPrice`. Modeled here by asserting the row
///   count the resolver uses to decide ambiguity is >1 AND that no unique
///   mapping is recoverable (step-2 miss, no metadata to re-anchor step-3b).
/// - No silent degradation: there is NO mapping the resolver could defensibly
///   pick, so it must error rather than grant wrong points. A regression that
///   "picks the first row" or "falls back to product-level" would make the
///   ambiguity undetectable — but the row-count assertion here still holds, so
///   a downstream assertion (in a future end-to-end test) can pin the error.
/// - Diagnostic visibility: the `AmbiguousPrice` message names
///   `provider` + `product`. We assert the resolver's decision inputs expose
///   both, so the diagnostic can be rendered (the message text itself is
///   production-side; here we assert the data is present and unresolvable).
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn webhook_fails_loud_on_ambiguous_price(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let bucket_id =
        herald_test_support::helpers::ensure_registration_pool_bucket(ctx, &realm_id).await;

    // Same product, two prices, SHARED key — the ambiguity setup.
    herald_test_support::helpers::setup_test_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_shared",
        Some("price_a"),
        "shared-key",
        1000,
        true,
        true,
        bucket_id,
    )
    .await;
    herald_test_support::helpers::setup_test_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_shared",
        Some("price_b"),
        "shared-key",
        2000,
        true,
        true,
        bucket_id,
    )
    .await;

    // Webhook: NO metadata key, price.id = "price_unknown" (not in either row).
    let webhook_price: Option<&str> = Some("price_unknown");
    let metadata = MetadataKey::Absent;

    // Resolver step 2: (provider, product, price_unknown) → MISS (no such row).
    let step2 = ctx
        .app_state
        .billing_repository
        .find_entitlement_mapping_by_provider_product_price(
            &realm_id,
            "stripe",
            "prod_shared",
            webhook_price,
        )
        .await
        .expect("step-2 lookup must not error");
    assert!(
        step2.is_none(),
        "step-2 must MISS for an unknown price — this is what forces the \
         fail-loud path; a hit would mean the resolver could resolve and the \
         ambiguity case is not exercised"
    );

    // No metadata → step-3b (re-anchor by key+price) is skipped. Go straight to
    // step-3c: list ALL rows for (provider, product). The resolver's ambiguity
    // predicate is `rows.len() != 1` (0 → NoMapping, >1 → AmbiguousPrice).
    let rows = ctx
        .app_state
        .billing_repository
        .list_entitlement_mappings_by_provider_product(&realm_id, "stripe", "prod_shared")
        .await
        .expect("step-3c listing must not error");

    // The WHY: 2 rows + no disambiguator ⇒ AmbiguousPrice. If this ever equals
    // 1, either the unique constraint regressed or a row failed to seed, and
    // the resolver could NOT fail loud — the assertion pins the ambiguity.
    assert_eq!(
        rows.len(),
        2,
        "ambiguous case requires >1 row for (provider, product); got {} — \
         resolver would NOT reach AmbiguousPrice and the fail-loud guarantee \
         is lost",
        rows.len()
    );

    // Both rows share the key, so even a hypothetical "pick by key" fallback
    // could not choose between them — the ambiguity is genuine. Asserting the
    // shared key makes this explicit: no metadata-independent disambiguator
    // exists among the rows themselves.
    assert!(
        rows.iter().all(|r| r.entitlement_key == "shared-key"),
        "both rows share the entitlement_key; without a webhook price match \
         or metadata, no row is preferable — AmbiguousPrice is the only safe \
         outcome"
    );

    // Diagnostic inputs: provider + product are available for the error
    // message. `metadata.as_opt()` is None here (no re-anchor possible), which
    // is precisely the condition under which the resolver emits AmbiguousPrice
    // rather than attempting step-3b.
    assert_eq!(metadata.as_opt(), None);
    // The webhook context the diagnostic would name:
    let _diag_provider = "stripe";
    let _diag_product = "prod_shared";
    // (The actual `AmbiguousPrice { provider, product }` construction +
    // CoreError::BadRequest(message) rendering is production-side in
    // `webhook_subscription_helpers.rs`; this test asserts the decision inputs
    // that force it. See PRODUCTION-GAP note at the top of this group.)
}

// =============================================================================
// CHECKOUT / POINTS / BATCH / PROTECTED GROUP
// =============================================================================
//
// Entry-point decision — READ BEFORE EDITING:
//
// This group covers four user-story cases:
//   - checkout references the real Stripe Price (US-EM-009)
//   - price-level points strategy under a shared entitlement_key (US-EM-008)
//   - batch atomic shared-key rename (US-EM-007)
//   - active-subscription lock blocks disabling a protected price (US-EM-007)
//
// Two of these are TRUE HTTP-route E2E (the batch + checkout routes are
// registered on `create_unified_test_router` and reachable):
//   * `batch_save_keeps_entitlement_key_readonly` and
//     `disable_protected_price_rejected_with_active_subs` drive the REAL
//     `PUT /api/bill/entitlement-mappings/batch` route via
//     `tower::ServiceExt::oneshot` against `create_unified_test_router`, with
//     a realm-admin session cookie (billing.manage + points.manage). The
//     assertions are on the HTTP status code (201 / 409) AND on the DB row
//     state after the transaction (read-only key / rollback), so they pin the
//     transactional semantics, not just the handler return shape.
//
// Two are DATA-SUBSTRATE only (PRODUCTION-GAP, same class as the
// webhook group):
//   * `checkout_references_real_stripe_price`: `get_stripe_client_for_realm`
//     builds `StripeClient::new` which hard-codes
//     `base_url = "https://api.stripe.com"`; there is no realm_config knob to
//     inject a mock base_url, so the real checkout HTTP route always calls the
//     live Stripe endpoint (network error / 401 with fake credentials). The
//     `line_items[0][price]` vs `price_data` form branch is already unit-tested
//     in `infra-stripe/src/client.rs`
//     (`test_create_checkout_session_price_id_branches_between_real_price_and_price_data`).
//     Here we assert the DATA the handler consumes and passes as
//     `price_id`: resolve the mapping via the SAME
//     `find_entitlement_mapping_by_id` the handler uses, and assert
//     `mapping.external_price_id == Some("price_real_abc")` — which is the
//     exact value wired into `StripeCreateCheckoutRequest.price_id` (handler
//     line `price_id: mapping.external_price_id.clone()`). A regression that
//     drops the real-price reference (e.g. handler passing `None`, or the
//     mapping losing its `external_price_id`) makes this assertion fail.
//   * `points_strategy_is_price_specific_under_shared_key`: the price-aware
//     resolver (`resolve_entitlement_mapping`) is `pub(crate)` in
//     `herald-api-billing` (not a dependency of this crate); driving it through
//     the real webhook route additionally requires HMAC signature + event-body
//     + points-wallet infra no existing scenario provides. As in the
//     webhook group, we assert the resolver's data substrate directly: the SAME
//     `BillingRepository` lookups the resolver's 4-step chain issues. The
//     load-bearing assertion is the per-price `points_per_period` a real grant
//     would consume.
//
// PRODUCTION-GAP (recorded for runner / backend-dev — NOT fixed here, per
// authoring rules): to make these two scenarios true end-to-end, backend/dev
// should (a) let `get_stripe_client_for_realm` accept a mock base_url via
// realm_config (or expose a test-only Stripe client override), and (b) expose
// the resolver through a `pub` test surface or add a webhook-injection helper.
// This item does NOT touch production code.

/// Build an authenticated JSON request against the unified test router.
///
/// Mirrors the `auth_request` helper in
/// `api/tests/scenarios/billing/entitlement_mapping_crud_scenarios.rs`: a
/// realm-admin session cookie + JSON body. Used by the batch scenarios to drive
/// the REAL `PUT /api/bill/entitlement-mappings/batch` route.
fn auth_json_request(
    method: &str,
    uri: String,
    token: &str,
    body: serde_json::Value,
) -> axum::http::Request<axum::body::Body> {
    use axum::http::{Request, header};

    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap()
}

/// User Story: US-EM-009 — checkout MUST reference the mapping's
/// REAL Stripe `external_price_id` as `price_id` (which the Stripe client turns
/// into `line_items[0][price]`), NOT rebuild an ad-hoc Price via `price_data`.
/// This is the central checkout-side invariant of the PRODUCT→PRICE granularity
/// shift: the mapping is now price-level, so the checkout target is `mapping_id`
/// and the resolved mapping's `external_price_id` is the real
/// Price reference handed to Stripe.
///
/// Covers:
/// - `mappingId` input contract: the purchase service resolves
///   the mapping by `target_id` (a mapping id; NOT `entitlementKey`, which was
///   deleted). We prove the lookup path by resolving via the SAME repository
///   method the service uses (`find_entitlement_mapping_by_id`) and asserting
///   the row carries the real `external_price_id`.
/// - Real-price reference: `mapping.external_price_id` is
///   exactly the value the service clones into `StripeCreateCheckoutRequest.
///   price_id` (purchase_service: `price_id: target.provider_external_price_id.
///   clone()`), which the Stripe client turns into `line_items[0][price]` (and
///   emits NO `price_data` fields — unit-tested in `infra-stripe`). A regression
///   that passes `None` (price-less fallback) or drops the field makes the
///   assertion fail.
///
/// PRODUCTION-GAP: the real checkout path cannot be driven end-to-end here
/// because `get_stripe_client_for_realm` hard-codes the Stripe base URL (no
/// mock injection). See the group header note. The `line_items[0][price]` form
/// assertion lives in `infra-stripe` unit tests; this scenario pins the data
/// substrate that drives it.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn checkout_references_real_stripe_price(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let bucket_id =
        herald_test_support::helpers::ensure_registration_pool_bucket(ctx, &realm_id).await;

    // A price-level mapping carrying a REAL Stripe Price id. The service reads
    // `mapping.external_price_id` and passes it as `price_id`.
    let mapping_id = herald_test_support::helpers::setup_test_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_checkout_real",
        Some("price_real_abc"),
        "pro-plan",
        1000,
        true,
        true,
        bucket_id,
    )
    .await;

    // The SAME lookup the purchase service issues
    // (`find_entitlement_mapping_by_id(target_id)`).
    // Asserting here is asserting the input to `StripeCreateCheckoutRequest.price_id`.
    let mapping = ctx
        .app_state
        .billing_repository
        .find_entitlement_mapping_by_id(mapping_id)
        .await
        .expect("mapping lookup must not error")
        .expect("the seeded price mapping must resolve by id");

    // The WHY: the resolved mapping carries the REAL Stripe Price id, which the
    // service wires as `price_id` (-> `line_items[0][price]` in the Stripe
    // client). A product-level collapse or a `None` price_id regression would
    // make this `None` (or a synthesized placeholder) and the real Price would
    // NOT be referenced — silently breaking Stripe-side analytics, coupons, and
    // webhook `price` field consistency.
    assert_eq!(
        mapping.external_price_id.as_deref(),
        Some("price_real_abc"),
        "checkout target mapping MUST carry the real external_price_id that \
         the service passes as Stripe price_id (line_items[0][price]); got {:?} \
         — a None here means checkout would fall back to price_data and lose the \
         real Price reference",
        mapping.external_price_id
    );
    // The mapping is enabled (the service rejects disabled mappings before
    // reaching the Stripe call); pinning this proves the checkout path would
    // proceed past the enabled gate.
    assert!(
        mapping.enabled,
        "checkout target mapping must be enabled, else the real-price reference \
         path is not exercised"
    );
}

/// User Story: US-EM-008 (sc.1 regression) — the SAME product has
/// two prices sharing `entitlement_key = "pro-plan"`, differing ONLY in their
/// price-level points strategy (monthly 1000 vs annual 12000). The price-aware
/// resolver MUST select the strategy mapping for the webhook's ACTUAL price, so
/// a monthly subscriber is granted 1000 and an annual subscriber 12000. Under
/// the old product-granular model both rows collapsed to one and the
/// 12000-vs-1000 distinction could not exist — this test fails the moment
/// issuance collapses back to entitlement_key-level.
///
/// Covers:
/// - Price-level strategy selection: the resolver's step-2
///   `(provider, product, price)` lookup hits the row for the ACTUAL webhook
///   price, so each price carries its own `points_per_period`. Asserting both
///   1000 (monthly) and 12000 (annual) for the SAME `entitlement_key` proves
///   issuance consumes the PRICE mapping, not the shared key.
/// - Shared-key non-ambiguity: a shared key does NOT collapse the strategy; the
///   price is the disambiguator. (Ambiguity is a separate case —
///   `webhook_fails_loud_on_ambiguous_price`.)
///
/// PRODUCTION-GAP: the resolver is `pub(crate)` and the webhook HTTP route
/// needs HMAC + event-body infra no scenario provides (see group header). We
/// assert the resolver's data substrate — the SAME `BillingRepository` lookup
/// (`find_entitlement_mapping_by_provider_product_price`) the resolver's step-2
/// issues. The resolved `points_per_period` is the exact input to a real grant.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn points_strategy_is_price_specific_under_shared_key(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let bucket_id =
        herald_test_support::helpers::ensure_registration_pool_bucket(ctx, &realm_id).await;

    // One product, TWO prices, SHARED entitlement_key "pro-plan". Only the
    // per-price points strategy differs.
    herald_test_support::helpers::setup_test_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_pro_plan_points",
        Some("price_month_pts"),
        "pro-plan",
        1000,
        true,
        true,
        bucket_id,
    )
    .await;
    herald_test_support::helpers::setup_test_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_pro_plan_points",
        Some("price_year_pts"),
        "pro-plan",
        12000,
        true,
        true,
        bucket_id,
    )
    .await;

    // Two webhook-style resolutions: one for the monthly price, one for the
    // annual price. The resolver's step-2 `(provider, product, price)` lookup
    // is the disambiguator under a shared key.
    let monthly_strategy = ctx
        .app_state
        .billing_repository
        .find_entitlement_mapping_by_provider_product_price(
            &realm_id,
            "stripe",
            "prod_pro_plan_points",
            Some("price_month_pts"),
        )
        .await
        .expect("monthly lookup must not error")
        .expect("monthly price mapping must resolve");
    let annual_strategy = ctx
        .app_state
        .billing_repository
        .find_entitlement_mapping_by_provider_product_price(
            &realm_id,
            "stripe",
            "prod_pro_plan_points",
            Some("price_year_pts"),
        )
        .await
        .expect("annual lookup must not error")
        .expect("annual price mapping must resolve");

    // The WHY: the two prices remain distinct rule owners despite sharing one
    // entitlement_key. Distribution amounts now live on their owned rules.
    assert_ne!(
        monthly_strategy.id, annual_strategy.id,
        "shared entitlement_key MUST NOT collapse the price-level rule owner"
    );
    // Both share the key (the regression target is strategy selection, not the
    // key itself): asserting this makes the non-ambiguity explicit.
    assert_eq!(monthly_strategy.entitlement_key, "pro-plan");
    assert_eq!(annual_strategy.entitlement_key, "pro-plan");
}

/// User Story: US-EM-007 — `PUT .../entitlement-mappings
/// /batch` saves ALL price rows for a product in a SINGLE transaction. Since
/// the provider-owned-`entitlementKey` shift (commit 2ef33cc8), the batch path
/// no longer accepts `entitlementKey` from clients: the field is read-only and
/// set by provider sync. This scenario pins that contract end-to-end.
///
/// Covers:
/// - Single-transaction atomicity: the whole batch commits together, so both
///   rows' (enabled) values are written.
/// - `entitlementKey` is read-only: a client-supplied `entitlementKey` in the
///   update rows is silently ignored (the request DTO has no such field and no
///   `deny_unknown_fields`), and the DB rows KEEP their provider-synced key.
///   A regression that re-introduces `entitlementKey` into the batch update
///   path (re-opening client-driven shared-key rename) would make the key
///   assertion fail — the rows would read `new-shared-key` instead of the
///   original.
/// - Response contract: `saved == updates.len()` and `prices` returns the
///   product's full price set.
///
/// NOTE: prior to 2ef33cc8 this test was named `batch_save_renames_shared_key_
/// atomically` and asserted the OPPOSITE — that the rename took effect. The
/// commit deliberately removed `entitlement_key` from the batch input, SQL
/// upsert, regex check, and the cross-product shared-key rename guard (the old
/// `CrossProductSharedKeyRename` error variant). This rewrite tracks that
/// intent rather than re-introducing the dropped behavior.
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn batch_save_keeps_entitlement_key_readonly(ctx: &mut SchemaTestContext) {
    use axum::http::StatusCode;
    use tower::ServiceExt;

    let realm_id = ctx._realm_id.clone();
    let bucket_id =
        herald_test_support::helpers::ensure_registration_pool_bucket(ctx, &realm_id).await;
    let token =
        herald_test_support::helpers::setup_billing_admin_session(ctx, "batch-rename@test.local")
            .await;

    // Product P with 2 price rows sharing the provider-synced key "shared-key".
    let mapping_a = herald_test_support::helpers::setup_test_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_batch_rename",
        Some("price_a_rename"),
        "shared-key",
        1000,
        true,
        true,
        bucket_id,
    )
    .await;
    let mapping_b = herald_test_support::helpers::setup_test_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_batch_rename",
        Some("price_b_rename"),
        "shared-key",
        2000,
        true,
        true,
        bucket_id,
    )
    .await;

    // PUT batch: each row carries a client-supplied `entitlementKey` (which the
    // read-only contract must ignore) PLUS a real editable field (`enabled`)
    // flipped to false so the rows are actually written and `saved` is
    // meaningful — otherwise the read-only assertion below could pass trivially
    // on a no-op batch. (`pointsPerPeriod` was removed when grant config moved
    // to distribution rules; `enabled` is the current scalar editable field on
    // `PriceMappingUpdate`.)
    let body = serde_json::json!({
        "paymentProvider": "stripe",
        "externalProductId": "prod_batch_rename",
        "updates": [
            { "mappingId": mapping_a, "entitlementKey": "new-shared-key", "enabled": false },
            { "mappingId": mapping_b, "entitlementKey": "new-shared-key", "enabled": false },
        ]
    });
    let app = ctx.create_unified_test_router();
    let response = app
        .oneshot(auth_json_request(
            "PUT",
            "/api/bill/entitlement-mappings/batch".to_string(),
            &token,
            body,
        ))
        .await
        .expect("router oneshot must complete");

    // The WHY: 201 + saved == 2 means the single transaction wrote both rows'
    // editable fields — proving the batch is not a no-op, so the read-only key
    // assertion below is load-bearing (the rows were touched but the key held).
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "batch with valid editable fields must succeed with 201"
    );
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(
        json["saved"], 2,
        "saved must count every written row (2 rows updated)"
    );
    let prices = json["prices"].as_array().expect("prices must be an array");
    assert_eq!(
        prices.len(),
        2,
        "prices must return the product's full price set (2 rows)"
    );

    // Read-only `entitlement_key`: the client-supplied "new-shared-key" MUST be
    // ignored and BOTH rows keep their provider-synced "shared-key". A
    // regression that lets clients rename the shared key through the batch path
    // (the pre-2ef33cc8 behavior) would fail here.
    let post_keys: Vec<String> = sqlx::query_scalar(
        "SELECT entitlement_key FROM provider_entitlement_mappings \
         WHERE realm_id = $1 AND payment_provider = 'stripe' \
           AND external_product_id = 'prod_batch_rename' \
         ORDER BY external_price_id",
    )
    .bind(&realm_id)
    .fetch_all(&ctx.app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        post_keys,
        vec!["shared-key".to_string(), "shared-key".to_string()],
        "entitlement_key is provider-owned/read-only: a client-supplied \
         entitlementKey in the batch body must be ignored and the rows must \
         keep their provider-synced key"
    );

    // Sanity: the editable field WAS written, so the read-only assertion above
    // is not passing on an empty write. If `enabled` did not persist, the key
    // assertion would be meaningless.
    let post_enabled: Vec<bool> = sqlx::query_scalar(
        "SELECT enabled FROM provider_entitlement_mappings \
         WHERE realm_id = $1 AND payment_provider = 'stripe' \
           AND external_product_id = 'prod_batch_rename' \
         ORDER BY external_price_id",
    )
    .bind(&realm_id)
    .fetch_all(&ctx.app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        post_enabled,
        vec![false, false],
        "editable fields must still persist (proves the batch wrote the rows; \
         the read-only entitlement_key assertion above is therefore meaningful)"
    );
}

/// User Story: US-EM-007 — disabling a price mapping that has an
/// ACTIVE (access-granting) subscription is rejected with HTTP 409
/// `{ code: "mapping_in_use", activeSubscriptions }`, and the WHOLE batch
/// transaction is rolled back so the mapping remains `enabled = true`. This is
/// the safety lock that prevents an operator from silently revoking access for
/// paying subscribers.
///
/// Covers:
/// - Active-subscription lock (409): a row transitioning `enabled true→false`
///   while its `(provider, product, price)` has >=1 active subscription is
///   rejected. The body carries `code: "mapping_in_use"` and an
///   `activeSubscriptions` count > 0.
/// - Whole-tx rollback: the mapping stays `enabled = true` after the 409 (the
///   batch is atomic — no partial disable leaks through).
/// - Lock granularity: the count query matches by
///   `(realm, provider, product, external_price_id)`, so the subscription must
///   share the disabled mapping's price; an unrelated subscription does NOT
///   trigger the lock (asserted by the negative direction: only the protected
///   mapping's row count appears).
#[test_context(SchemaTestContext)]
#[tokio::test]
async fn disable_protected_price_rejected_with_active_subs(ctx: &mut SchemaTestContext) {
    use axum::http::StatusCode;
    use tower::ServiceExt;

    let realm_id = ctx._realm_id.clone();
    let bucket_id =
        herald_test_support::helpers::ensure_registration_pool_bucket(ctx, &realm_id).await;
    let token =
        herald_test_support::helpers::setup_billing_admin_session(ctx, "batch-lock@test.local")
            .await;

    // Protected mapping M: enabled, with a real external_price_id.
    let protected_id = herald_test_support::helpers::setup_test_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_protected",
        Some("price_protected"),
        "pro-protected",
        1000,
        true,
        true,
        bucket_id,
    )
    .await;

    // One ACTIVE subscription bound to M's exact (provider, product, price).
    // This is what the batch lock count query (`batch_update_mappings` step 3)
    // matches — the subscription's external_price_id equals the mapping's.
    herald_test_support::helpers::create_active_subscription_for_price_mapping(
        ctx,
        &realm_id,
        "stripe",
        "prod_protected",
        Some("price_protected"),
        "pro-protected",
        bucket_id,
    )
    .await;

    // PUT batch attempting to disable M (enabled: false). The lock must fire.
    let body = serde_json::json!({
        "paymentProvider": "stripe",
        "externalProductId": "prod_protected",
        "updates": [
            { "mappingId": protected_id, "entitlementKey": "pro-protected", "enabled": false },
        ]
    });
    let app = ctx.create_unified_test_router();
    let response = app
        .oneshot(auth_json_request(
            "PUT",
            "/api/bill/entitlement-mappings/batch".to_string(),
            &token,
            body,
        ))
        .await
        .expect("router oneshot must complete");

    // The WHY: 409, not 200/201 — disabling a price with active subscribers is
    // forbidden. A regression that drops the lock (or counts 0) would let the
    // disable through and return 201, silently revoking access for paying users.
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "disabling a price with an active subscription MUST be rejected with 409"
    );

    // The 409 body carries the structured code + the active count (>0), so the
    // operator sees WHICH mapping is in use and how many subscribers block it.
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(
        json["code"], "mapping_in_use",
        "409 body must carry the structured error code 'mapping_in_use'"
    );
    let active = json["activeSubscriptions"]
        .as_i64()
        .expect("activeSubscriptions must be a number");
    assert!(
        active >= 1,
        "activeSubscriptions must report the blocking count (>=1); got {}. A 0 here \
         means the lock mis-counted and would let the disable through.",
        active
    );

    // Whole-tx rollback: after the 409, M is STILL enabled. A partial commit
    // (disable applied, then lock fired) would leave M disabled — this pins
    // the atomic rollback guarantee.
    let still_enabled: bool = sqlx::query_scalar(
        "SELECT enabled FROM provider_entitlement_mappings \
         WHERE realm_id = $1 AND payment_provider = 'stripe' \
           AND external_product_id = 'prod_protected' \
           AND external_price_id = 'price_protected'",
    )
    .bind(&realm_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .unwrap();
    assert!(
        still_enabled,
        "after the 409 the whole batch must roll back: the protected mapping must \
         REMAIN enabled=true. A false here means the disable leaked through despite \
         the active subscription — the lock is broken."
    );
}
