// =============================================================================
// Scenario Tests: Credit Bucket directory CRUD + overview + delete intercept
// =============================================================================
//
// Covers design `.ai/design/credit-bucket.md`:
//   - (Bucket directory CRUD + overview + error
//     contracts).
//   - "directory CRUD" testable behaviors:
//       * coverage set must be non-empty (schema/handler fail-loud);
//       * bucket_key format + uniqueness;
//       * delete rejected on in-flight subscriptions or residual balances
//         (409 `bucket_in_use` with `activeSubscriptions` / `holdersWithBalance`);
//       * overview rows + a SEPARATE `grandTotal` field;
//       * NO `is_default` field anywhere.
//   - there is no default Bucket concept — `isDefault` MUST NOT
//     appear in any response JSON.
//
// All scenarios exercise the real production HTTP path through the unified test
// router (`/api/realms/{realmId}/billing/credit-buckets...`) gated on Realm
// Admin `points.manage`. Direct-DB seed helpers (these helpers) materialize
// the in-flight subscription / residual wallet rows the delete intercept reads.
//
// Per authoring rules: tests target the intended design contract. Runtime gaps
// (if any) are recorded inline; the runner triages runtime failures.
//
// =============================================================================

#![allow(clippy::too_many_arguments)]

use crate::tests::helpers::billing_helpers::{
    setup_billing_admin_session, setup_test_entitlement_mapping,
};
use crate::tests::helpers::credit_bucket_helpers::{
    CreditBucketOpts, attach_bucket_client_app, auth_admin_request_via_api,
    create_test_credit_bucket, seed_active_subscription_on_bucket,
    seed_granted_credit_ledger_on_bucket, seed_wallet_with_balance_on_bucket,
};
use crate::tests::scenarios::points::fixtures::{create_test_client_app, create_test_user};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::http::StatusCode;
use serde_json::{Value, json};
use test_context::test_context;
use uuid::Uuid;

// =============================================================================
// Local helpers
// =============================================================================

/// Extract the `code` field from an error JSON body, or None if absent.
fn error_code(body: &Option<Value>) -> Option<&str> {
    body.as_ref()
        .and_then(|v| v.get("code"))
        .and_then(|c| c.as_str())
}

/// Extract an integer field from a JSON body.
fn int_field(body: &Option<Value>, field: &str) -> Option<i64> {
    body.as_ref()
        .and_then(|v| v.get(field))
        .and_then(|n| n.as_i64())
}

/// Recursively check that NO key named `isDefault` (camelCase) or `is_default`
/// (snake_case) appears anywhere in the JSON value (negative
/// regression). Returns the dotted path of the first offending occurrence, if any.
fn find_is_default_key(value: &Value, path: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let next_path = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", path, k)
                };
                if k == "isDefault" || k == "is_default" {
                    return Some(next_path);
                }
                if let Some(found) = find_is_default_key(v, &next_path) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let next_path = format!("{}[{}]", path, i);
                if let Some(found) = find_is_default_key(v, &next_path) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// Assert the body has NO `isDefault` / `is_default` key anywhere.
fn assert_no_is_default(body: &Option<Value>, context: &str) {
    if let Some(v) = body {
        assert!(
            find_is_default_key(v, "").is_none(),
            "regression: found `isDefault`/`is_default` in {} — design removed the default-bucket concept; body: {}",
            context,
            v
        );
    }
}

/// POST a new Bucket via the real handler and return (status, body).
async fn create_bucket_via_api(
    ctx: &TestContext,
    realm_id: &str,
    token: &str,
    bucket_key: &str,
    name: &str,
    client_app_ids: &[Uuid],
) -> (StatusCode, Option<Value>) {
    let body = json!({
        "bucketKey": bucket_key,
        "name": name,
        "clientAppIds": client_app_ids.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
    });
    auth_admin_request_via_api(
        ctx,
        "POST",
        &format!("/api/realms/{}/billing/credit-buckets", realm_id),
        token,
        Some(&body),
    )
    .await
}

// =============================================================================
// Scenario 1: list returns empty array for a fresh realm
// =============================================================================

/// User Story: US-CB-001 (admin lists the realm's Buckets).
/// Covers:
///   - `GET .../credit-buckets` → `Bucket[]`. An empty Realm
///     yields `[]` (not null, not 404).
///   - response items have NO `isDefault` field.
#[test_context(TestContext)]
#[tokio::test]
async fn list_credit_buckets_empty(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_list_empty@example.com").await;

    let (status, body) = auth_admin_request_via_api(
        ctx,
        "GET",
        &format!("/api/realms/{}/billing/credit-buckets", realm_id),
        &token,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "list empty: {:?} body={:?}",
        status,
        body
    );
    let arr = body
        .as_ref()
        .and_then(|v| v.as_array())
        .expect("list must return a JSON array");
    assert!(
        arr.is_empty(),
        "fresh realm must list zero buckets: {:?}",
        body
    );
    assert_no_is_default(&body, "list empty response");
}

// =============================================================================
// Scenario 2: list returns rows with the list-item fields
// =============================================================================

/// User Story: US-CB-001 (admin lists Buckets with coverage/mapping counts).
/// Covers:
///   - list-item fields: `bucketKey`, `name`, `displayOrder`,
///     `enabled`, `ruleReferenceCount`, `coveredClientAppCount`.
///   - NO `isDefault`.
#[test_context(TestContext)]
#[tokio::test]
async fn list_credit_buckets_with_data(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_list_data@example.com").await;
    let pool = &ctx.app_state.pool;

    let client_app = create_test_client_app(pool, &realm_id).await;
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("List Bucket".into()),
            bucket_key: Some(format!("list-bucket-{}", Uuid::now_v7())),
            display_order: Some(7),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app).await;

    let (status, body) = auth_admin_request_via_api(
        ctx,
        "GET",
        &format!("/api/realms/{}/billing/credit-buckets", realm_id),
        &token,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "list with data: {:?} body={:?}",
        status,
        body
    );
    let arr = body
        .as_ref()
        .and_then(|v| v.as_array())
        .expect("list must return a JSON array");
    assert!(!arr.is_empty(), "seeded bucket must appear in list");

    let row = arr
        .iter()
        .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(&bucket.to_string()))
        .expect("seeded bucket must be in the list");

    for field in [
        "bucketKey",
        "name",
        "displayOrder",
        "enabled",
        "ruleReferenceCount",
        "coveredClientAppCount",
    ] {
        assert!(
            row.get(field).is_some(),
            "list-item must expose `{}`, row: {:?}",
            field,
            row
        );
    }
    assert_eq!(
        row.get("coveredClientAppCount").and_then(|v| v.as_i64()),
        Some(1),
        "coveredClientAppCount must reflect attached coverage row"
    );
    assert_no_is_default(&body, "list with data response");
}

// =============================================================================
// Scenario 3: get detail → 404 when missing
// =============================================================================

/// User Story: US-CB-001 (admin fetches a Bucket; missing id → 404).
/// Covers:
///   - `GET .../{bucketId}` 404 when the bucket does not exist.
#[test_context(TestContext)]
#[tokio::test]
async fn get_credit_bucket_detail_404_when_missing(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_get_404@example.com").await;

    let missing_id = Uuid::now_v7();
    let (status, body) = auth_admin_request_via_api(
        ctx,
        "GET",
        &format!(
            "/api/realms/{}/billing/credit-buckets/{}",
            realm_id, missing_id
        ),
        &token,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "missing bucket detail must be 404, got {}: {:?}",
        status,
        body
    );
}

// =============================================================================
// Scenario 4: get detail returns clientApps + ruleReferences arrays
// =============================================================================

/// User Story: US-CB-002 + US-CB-003 (detail surfaces coverage set + rules).
/// Covers:
///   - `BucketDetailResponse` shape: bucket fields + `clientApps[]` +
///     `ruleReferences[]` (the distribution rules targeting this bucket; design
///     §4.2.1). The old `entitlementMappings[]` field was replaced by
///     `ruleReferences[]` when grant config moved from the mapping to
///     distribution rules.
///   - NO `isDefault`.
#[test_context(TestContext)]
#[tokio::test]
async fn get_credit_bucket_detail_returns_client_apps_and_rule_references(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_get_detail@example.com").await;
    let pool = &ctx.app_state.pool;

    let client_app = create_test_client_app(pool, &realm_id).await;
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Detail Bucket".into()),
            bucket_key: Some(format!("detail-bucket-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app).await;

    let (status, body) = auth_admin_request_via_api(
        ctx,
        "GET",
        &format!("/api/realms/{}/billing/credit-buckets/{}", realm_id, bucket),
        &token,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "detail get: {:?} body={:?}",
        status,
        body
    );
    let body = body.expect("detail body must be present");
    assert!(
        body.get("clientApps").and_then(|v| v.as_array()).is_some(),
        "detail must expose `clientApps[]`: {:?}",
        body
    );
    assert!(
        body.get("ruleReferences")
            .and_then(|v| v.as_array())
            .is_some(),
        "detail must expose `ruleReferences[]` (distribution rules targeting \
         this bucket; replaced the old entitlementMappings[]): {:?}",
        body
    );
    assert_no_is_default(&Some(body.clone()), "detail response");
}

// =============================================================================
// Scenario 5: create with empty coverage set → 400
// =============================================================================

/// User Story: US-CB-002 (coverage set must be non-empty — fail-loud).
/// Covers:
///   - `clientAppIds=[]` → 400 (coverage set empty).
#[test_context(TestContext)]
#[tokio::test]
async fn create_credit_bucket_requires_at_least_one_client_app(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_create_empty@example.com").await;

    let (status, body) = create_bucket_via_api(
        ctx,
        &realm_id,
        &token,
        &format!("no-coverage-{}", Uuid::now_v7()),
        "No Coverage",
        &[],
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "empty coverage set must be 400, got {}: {:?}",
        status,
        body
    );
}

// =============================================================================
// Scenario 6: create with invalid bucketKey format → 400
// =============================================================================

/// User Story: US-CB-001 (bucketKey must match `^[a-z0-9-]{1,64}$`).
/// Covers:
///   - `validate_bucket_key`: uppercase / spaces / punctuation
///     → 400.
#[test_context(TestContext)]
#[tokio::test]
async fn create_credit_bucket_rejects_invalid_bucket_key_format(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_create_badkey@example.com").await;
    let pool = &ctx.app_state.pool;
    let client_app = create_test_client_app(pool, &realm_id).await;

    let (status, body) = create_bucket_via_api(
        ctx,
        &realm_id,
        &token,
        "Invalid Key!",
        "Bad Key Bucket",
        &[client_app],
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "invalid bucketKey must be 400, got {}: {:?}",
        status,
        body
    );
}

// =============================================================================
// Scenario 7: create with duplicate bucketKey in same realm → 400/409
// =============================================================================

/// User Story: US-CB-001 (bucketKey is unique within a realm).
/// Covers:
///   - a second bucket with the same `bucketKey` in the
///     same Realm is rejected with 400 `bucket_key_duplicate`. This is the
///     exact error contract surfaced via `map_bucket_error(BucketKeyDuplicate)`
///     after `classify_bucket_insert_error` translates the underlying
///     `UNIQUE(realm_id, bucket_key)` violation. Pins P0-1: a regression
///     that drops back to 500 (constraint-name mismatch) will fail here.
#[test_context(TestContext)]
#[tokio::test]
async fn create_credit_bucket_rejects_duplicate_bucket_key(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_create_dup@example.com").await;
    let pool = &ctx.app_state.pool;
    let client_app = create_test_client_app(pool, &realm_id).await;

    let dup_key = format!("dup-key-{}", Uuid::now_v7());

    // First create succeeds.
    let (status1, body1) =
        create_bucket_via_api(ctx, &realm_id, &token, &dup_key, "First", &[client_app]).await;
    assert_eq!(
        status1,
        StatusCode::CREATED,
        "first create should succeed, got {}: {:?}",
        status1,
        body1
    );

    // Second create with same key in the same realm must be rejected.
    let (status2, body2) =
        create_bucket_via_api(ctx, &realm_id, &token, &dup_key, "Second", &[client_app]).await;

    // A duplicate bucketKey is 400 `bucket_key_duplicate`.
    // Pins P0-1: must not regress to a generic 500 from a missed
    // constraint-name match in `classify_bucket_insert_error`.
    assert_eq!(
        status2,
        StatusCode::BAD_REQUEST,
        "duplicate bucketKey must be 400 bucket_key_duplicate, got {}: {:?}",
        status2,
        body2
    );
    assert_eq!(
        error_code(&body2),
        Some("bucket_key_duplicate"),
        "expected bucket_key_duplicate, got: {:?}",
        body2
    );
}

// =============================================================================
// Scenario 8: create without points.manage → 403
// =============================================================================

/// User Story: US-CB-001 (write requires Realm Admin `points.manage`).
/// Covers:
///   - a caller without `points.manage` is rejected 403.
///   - This scenario uses a freshly created NON-admin user session (no Realm
///     Admin role granted) to prove the permission gate fires.
#[test_context(TestContext)]
#[tokio::test]
async fn create_credit_bucket_without_points_manage_returns_403(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();

    // A plain user session with NO Realm Admin role (= no `points.manage`).
    // Acquire it BEFORE borrowing ctx.app_state.pool (session setup borrows ctx mutably).
    let (plain_token, _plain_user) = plain_user_session(ctx, "cb_t04_no_perm@example.com").await;

    let pool = &ctx.app_state.pool;
    let client_app = create_test_client_app(pool, &realm_id).await;
    let (status, body) = create_bucket_via_api(
        ctx,
        &realm_id,
        &plain_token,
        &format!("no-perm-{}", Uuid::now_v7()),
        "No Perm Bucket",
        &[client_app],
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "create without points.manage must be 403, got {}: {:?}",
        status,
        body
    );
}

// =============================================================================
// Scenario 9: update changes fields; clearing coverage set → 400
// =============================================================================

/// User Story: US-CB-001 + US-CB-002 (update mutates fields; coverage set must
/// stay non-empty).
/// Covers:
///   - PUT: name/displayOrder/enabled/coverage are fully replaced;
///     clearing the coverage set (`clientAppIds=[]`) → 400.
#[test_context(TestContext)]
#[tokio::test]
async fn update_credit_bucket_changes_name_order_enabled_coverage(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_update@example.com").await;
    let pool = &ctx.app_state.pool;

    let client_app_a = create_test_client_app(pool, &realm_id).await;
    let client_app_b = create_test_client_app(pool, &realm_id).await;

    // Seed a bucket directly then drive the full PUT path twice.
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Original".into()),
            bucket_key: Some(format!("upd-bucket-{}", Uuid::now_v7())),
            display_order: Some(1),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app_a).await;

    // --- PUT: rename + reorder + swap coverage to a different client app. ---
    let put_body = json!({
        "name": "Renamed",
        "displayOrder": 42,
        "enabled": false,
        "clientAppIds": [client_app_b],
    });
    let (status, body) = auth_admin_request_via_api(
        ctx,
        "PUT",
        &format!("/api/realms/{}/billing/credit-buckets/{}", realm_id, bucket),
        &token,
        Some(&put_body),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "update should succeed, got {}: {:?}",
        status,
        body
    );
    let body = body.expect("update body");
    assert_eq!(body.get("name").and_then(|v| v.as_str()), Some("Renamed"));
    assert_eq!(body.get("displayOrder").and_then(|v| v.as_i64()), Some(42));
    assert_eq!(body.get("enabled").and_then(|v| v.as_bool()), Some(false));
    assert_no_is_default(&Some(body.clone()), "update response");

    // --- PUT with empty coverage set → 400. --------------------------------
    let bad_body = json!({
        "name": "Bad",
        "clientAppIds": [],
    });
    let (status_bad, body_bad) = auth_admin_request_via_api(
        ctx,
        "PUT",
        &format!("/api/realms/{}/billing/credit-buckets/{}", realm_id, bucket),
        &token,
        Some(&bad_body),
    )
    .await;
    assert_eq!(
        status_bad,
        StatusCode::BAD_REQUEST,
        "clearing coverage set must be 400, got {}: {:?}",
        status_bad,
        body_bad
    );
}

// =============================================================================
// Scenario 11: PUT attaching a mapping increases entitlementMappingCount;
//              removing an attached mapping is rejected (bucket_orphan_mapping)
// =============================================================================
//
// Regression for the count-stale gap (DE-D06 Gap #4 / US-CB-003 "count must
// increase"). Root cause was NOT a stale read — `update_credit_bucket` detached
// mappings via `SET bucket_id = NULL`, illegal under the NOT NULL `bucket_id`
// constraint (commit aa6cc2da): any reassignment of a bucket that already held
// mappings 500'd, so the count never grew. The list count query itself is
// correct (a plain auto-commit LEFT JOIN/GROUP BY; read-after-write is
// immediate under READ COMMITTED).
//
// Under the NOT NULL model (no default bucket) a mapping may JOIN a
// bucket (move-in) but cannot be removed via PUT (detaching would orphan it).
// This scenario asserts both halves:
//   - PUT with `entitlementMappingIds=[M]` → 200 and the list count for this
//     bucket becomes 1 (Gap #4 regression: count must increase, no staleness).
//   - PUT with `entitlementMappingIds=[]` (drop M) → 400 `bucket_orphan_mapping`
//     listing M (fail-loud; never a silent no-op).
//
/// User Story: US-CB-003 (assign ≥1 mapping to a Bucket; count must increase).
///
/// DISABLED — no new-model equivalent. This scenario exercised bucket↔mapping
/// attachment via the bucket PUT endpoint: sending `entitlementMappingIds`,
/// reading back `entitlementMappings[]` / `entitlementMappingCount`, and the
/// `bucket_orphan_mapping` 400. The distribution-rules refactor removed all of
/// that: mappings no longer carry `bucket_id`, the production
/// `UpdateCreditBucketRequest` DTO has no `entitlementMappingIds` field, the
/// detail/list responses surface `ruleReferences[]` / `ruleReferenceCount`
/// instead, and `bucket_orphan_mapping` no longer exists. Linking a bucket to
/// grant routing is now done via distribution rules on the mapping endpoint
/// (covered by `entitlement_mapping_crud_scenarios` +
/// `multi_wallet_grant_rule_scenarios`). Re-enable only if bucket-level mapping
/// attachment is re-introduced.
#[test_context(TestContext)]
#[tokio::test]
#[ignore = "obsolete: bucket<->mapping attachment via PUT was removed by the distribution-rules refactor; no new-model equivalent on the bucket endpoint"]
async fn update_credit_bucket_attaching_mapping_increases_count_and_removal_rejected(
    ctx: &mut TestContext,
) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_mapping_count@example.com").await;
    let pool = &ctx.app_state.pool;

    let client_app = create_test_client_app(pool, &realm_id).await;
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Mapping Bucket".into()),
            bucket_key: Some(format!("mapping-bucket-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app).await;

    // Mapping is created bound to the realm's legacy test bucket (NOT NULL
    // bucket_id); the PUT below moves it onto `bucket`.
    let mapping = setup_test_entitlement_mapping(
        ctx,
        &realm_id,
        "creem",
        &format!("prod-{}", Uuid::now_v7()),
        &format!("ent-{}", Uuid::now_v7()),
    )
    .await;

    // --- PUT: attach the mapping → 200. -------------------------------------
    let put_body = json!({
        "name": "Mapping Bucket",
        "displayOrder": 0,
        "enabled": true,
        "clientAppIds": [client_app],
        "entitlementMappingIds": [mapping],
    });
    let (status, body) = auth_admin_request_via_api(
        ctx,
        "PUT",
        &format!("/api/realms/{}/billing/credit-buckets/{}", realm_id, bucket),
        &token,
        Some(&put_body),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "attach mapping should succeed, got {}: {:?}",
        status,
        body
    );
    let body = body.expect("attach response body");
    assert!(
        body.get("entitlementMappings")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .any(|m| m.get("id").and_then(|v| v.as_str()) == Some(&mapping.to_string()))
            })
            .unwrap_or(false),
        "detail must echo the attached mapping: {:?}",
        body
    );

    // Read-after-write: the list count MUST reflect the just-committed attach
    // (Gap #4 regression assertion — count must increase; no staleness).
    let (list_status, list_body) = auth_admin_request_via_api(
        ctx,
        "GET",
        &format!("/api/realms/{}/billing/credit-buckets", realm_id),
        &token,
        None,
    )
    .await;
    assert_eq!(
        list_status,
        StatusCode::OK,
        "list after attach: {:?}",
        list_body
    );
    let row = list_body
        .as_ref()
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(&bucket.to_string()))
        })
        .expect("bucket must appear in list after attach");
    assert_eq!(
        row.get("entitlementMappingCount").and_then(|v| v.as_i64()),
        Some(1),
        "entitlementMappingCount must be 1 after attach (Gap #4 regression): {:?}",
        row
    );

    // --- PUT: drop the mapping (empty set) → 400 bucket_orphan_mapping. ------
    let drop_body = json!({
        "name": "Mapping Bucket",
        "displayOrder": 0,
        "enabled": true,
        "clientAppIds": [client_app],
        "entitlementMappingIds": [],
    });
    let (drop_status, drop_body) = auth_admin_request_via_api(
        ctx,
        "PUT",
        &format!("/api/realms/{}/billing/credit-buckets/{}", realm_id, bucket),
        &token,
        Some(&drop_body),
    )
    .await;
    assert_eq!(
        drop_status,
        StatusCode::BAD_REQUEST,
        "removing an attached mapping must be 400 bucket_orphan_mapping, got {}: {:?}",
        drop_status,
        drop_body
    );
    assert_eq!(
        error_code(&drop_body),
        Some("bucket_orphan_mapping"),
        "removal must surface bucket_orphan_mapping code: {:?}",
        drop_body
    );
    let orphan_ids = drop_body
        .as_ref()
        .and_then(|v| v.get("orphanMappingIds"))
        .and_then(|v| v.as_array())
        .expect("400 must list orphanMappingIds");
    assert!(
        orphan_ids
            .iter()
            .any(|id| id.as_str() == Some(&mapping.to_string())),
        "orphan list must contain the mapping we tried to drop: {:?}",
        drop_body
    );
}

// =============================================================================
// Scenario 11: delete rejected when active subscriptions exist → 409 bucket_in_use
// =============================================================================

/// User Story: US-CB-001 (delete refuses in-flight subscriptions).
///
/// The `delete_credit_bucket` subscription guard is anchored on the
/// rule-result linkage: an active subscription that has a `subscription_credit`
/// grant result (`points_quota_entitlements` / `points_credit_ledger` with
/// `source_id = subscription_id`) in this bucket blocks the delete, independent
/// of the residual-balance guard (`delete_credit_bucket_rejected_when_holders_with_balance_exist`).
#[test_context(TestContext)]
#[tokio::test]
async fn delete_credit_bucket_rejected_when_active_subscriptions_exist(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_del_sub@example.com").await;
    let pool = &ctx.app_state.pool;

    let client_app = create_test_client_app(pool, &realm_id).await;
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Del-Sub Bucket".into()),
            bucket_key: Some(format!("del-sub-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app).await;

    // Seed an in-flight subscription bound to this bucket.
    seed_active_subscription_on_bucket(pool, &realm_id, bucket).await;

    let (status, body) = auth_admin_request_via_api(
        ctx,
        "DELETE",
        &format!("/api/realms/{}/billing/credit-buckets/{}", realm_id, bucket),
        &token,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "delete with active subscriptions must be 409, got {}: {:?}",
        status,
        body
    );
    assert_eq!(
        error_code(&body),
        Some("bucket_in_use"),
        "expected bucket_in_use, got: {:?}",
        body
    );
    assert!(
        int_field(&body, "activeSubscriptions")
            .map(|n| n >= 1)
            .unwrap_or(false),
        "activeSubscriptions must be >= 1, got: {:?}",
        body
    );
}

// =============================================================================
// Scenario 12: delete rejected when holders with balance exist → 409 bucket_in_use
// =============================================================================

/// User Story: US-CB-001 (delete refuses residual balances).
/// Covers:
///   - DELETE 409 `bucket_in_use` with `holdersWithBalance >= 1`.
///   - `delete_credit_bucket` counts `points_wallets.bucket_id` rows with
///     `total_balance > 0`; the seed helper materializes such a wallet.
#[test_context(TestContext)]
#[tokio::test]
async fn delete_credit_bucket_rejected_when_holders_with_balance_exist(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_del_holder@example.com").await;
    let pool = &ctx.app_state.pool;

    let client_app = create_test_client_app(pool, &realm_id).await;
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Del-Holder Bucket".into()),
            bucket_key: Some(format!("del-holder-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app).await;

    // Seed a wallet with residual balance for this bucket.
    let holder = create_test_user(pool, &realm_id, "cb_t04_holder@example.com").await;
    seed_wallet_with_balance_on_bucket(pool, &realm_id, holder, bucket, 500).await;

    let (status, body) = auth_admin_request_via_api(
        ctx,
        "DELETE",
        &format!("/api/realms/{}/billing/credit-buckets/{}", realm_id, bucket),
        &token,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "delete with residual balance must be 409, got {}: {:?}",
        status,
        body
    );
    assert_eq!(
        error_code(&body),
        Some("bucket_in_use"),
        "expected bucket_in_use, got: {:?}",
        body
    );
    assert!(
        int_field(&body, "holdersWithBalance")
            .map(|n| n >= 1)
            .unwrap_or(false),
        "holdersWithBalance must be >= 1, got: {:?}",
        body
    );
}

// =============================================================================
// Scenario 13: delete succeeds when unused → 204
// =============================================================================

/// User Story: US-CB-001 (delete removes an unused Bucket).
/// Covers:
///   - DELETE 204 when no in-flight subscriptions and no residual
///     balances.
///   - DB check: the `credit_buckets` row is gone post-delete.
#[test_context(TestContext)]
#[tokio::test]
async fn delete_credit_bucket_succeeds_when_unused(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_del_ok@example.com").await;
    let pool = &ctx.app_state.pool;

    let client_app = create_test_client_app(pool, &realm_id).await;
    let bucket = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Del-OK Bucket".into()),
            bucket_key: Some(format!("del-ok-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket, client_app).await;

    let (status, body) = auth_admin_request_via_api(
        ctx,
        "DELETE",
        &format!("/api/realms/{}/billing/credit-buckets/{}", realm_id, bucket),
        &token,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "delete unused must be 204, got {}: {:?}",
        status,
        body
    );

    // DB check: row is gone.
    let still_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM credit_buckets WHERE id = $1")
            .bind(bucket)
            .fetch_optional(pool)
            .await
            .expect("check bucket gone");
    assert!(
        still_exists.is_none(),
        "credit_buckets row must be deleted after 204"
    );
}

// =============================================================================
// Scenario 14 (overview): rows + SEPARATE grandTotal (negative regression combined)
// =============================================================================

/// User Story: US-CB-001 (admin views the bucket×credit-type overview matrix).
/// Covers:
///   - overview `{ rows: OverviewRow[], grandTotal: ByCreditType }`:
///     `grandTotal` is a SEPARATE top-level field, NOT appended to `rows`.
///   - Each row exposes `byCreditType{}` + `bucketTotal`.
///   - negative regression: the overview response (and every other response
///     exercised above) contains NO `isDefault`/`is_default` key anywhere. This
///     final scenario explicitly re-asserts the absence on the overview payload,
///     which is the most structurally distinct response in the directory API.
#[test_context(TestContext)]
#[tokio::test]
async fn credit_bucket_overview_returns_rows_and_grand_total(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "cb_t04_overview@example.com").await;
    let pool = &ctx.app_state.pool;

    // Seed two buckets + wallets with residual balances so the matrix is
    // non-trivial.
    let client_app = create_test_client_app(pool, &realm_id).await;
    let bucket_a = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Overview A".into()),
            bucket_key: Some(format!("overview-a-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket_a, client_app).await;

    let bucket_b = create_test_credit_bucket(
        pool,
        &realm_id,
        CreditBucketOpts {
            name: Some("Overview B".into()),
            bucket_key: Some(format!("overview-b-{}", Uuid::now_v7())),
            ..Default::default()
        },
    )
    .await;
    attach_bucket_client_app(pool, &realm_id, bucket_b, client_app).await;

    let holder_a = create_test_user(pool, &realm_id, "cb_t04_ov_a@example.com").await;
    let holder_b = create_test_user(pool, &realm_id, "cb_t04_ov_b@example.com").await;
    seed_wallet_with_balance_on_bucket(pool, &realm_id, holder_a, bucket_a, 100).await;
    seed_wallet_with_balance_on_bucket(pool, &realm_id, holder_b, bucket_b, 250).await;
    // Under point-time the overview total is a derived SUM over
    // `points_credit_ledger`, so seed real `granted_credit`
    // ledger rows — otherwise `grandTotal.granted` stays 0 despite the wallet
    // analytics rows above.
    seed_granted_credit_ledger_on_bucket(pool, &realm_id, holder_a, bucket_a, 100).await;
    seed_granted_credit_ledger_on_bucket(pool, &realm_id, holder_b, bucket_b, 250).await;

    let (status, body) = auth_admin_request_via_api(
        ctx,
        "GET",
        &format!("/api/realms/{}/billing/credit-buckets/overview", realm_id),
        &token,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "overview: {:?} body={:?}",
        status,
        body
    );
    let body = body.expect("overview body must be present");

    // rows[] present and each row has byCreditType{} + bucketTotal.
    let rows = body
        .get("rows")
        .and_then(|v| v.as_array())
        .expect("overview must expose `rows[]`");
    assert!(
        rows.len() >= 2,
        "overview must list at least the 2 seeded buckets"
    );
    for (i, row) in rows.iter().enumerate() {
        assert!(
            row.get("byCreditType")
                .and_then(|v| v.as_object())
                .is_some(),
            "row {} missing byCreditType: {:?}",
            i,
            row
        );
        assert!(
            row.get("bucketTotal").and_then(|v| v.as_i64()).is_some(),
            "row {} missing bucketTotal: {:?}",
            i,
            row
        );
    }

    // grandTotal is a SEPARATE field (not a synthesized extra row).
    assert!(
        body.get("grandTotal").and_then(|v| v.as_object()).is_some(),
        "overview must expose a SEPARATE `grandTotal` field: {:?}",
        body
    );
    let grand_total = body
        .get("grandTotal")
        .and_then(|v| v.get("granted"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    assert_eq!(
        grand_total, 350,
        "grandTotal.granted must sum both buckets (100 + 250), got {}",
        grand_total
    );

    // negative regression: NO isDefault / is_default key anywhere.
    assert_no_is_default(&Some(body.clone()), "overview response");
}

// =============================================================================
// Local fixture: plain (non-admin) user session for the 403 scenario
// =============================================================================

/// Create a plain user session (NO Realm Admin role) so the caller does NOT
/// hold `points.manage`. Returns (token, user_id). `create_admin_session_with_user`
/// only stores a session — it does NOT grant any role; granting is the caller's
/// responsibility (see `grant_realm_admin_role`), so omitting it leaves the
/// caller without `points.manage`.
async fn plain_user_session(ctx: &mut TestContext, email: &str) -> (String, Uuid) {
    use crate::tests::helpers::create_admin_session_with_user;
    let (token, user_id_str) = create_admin_session_with_user(ctx, email, 1800).await;
    let user_uuid = Uuid::parse_str(&user_id_str).unwrap_or_else(|_| Uuid::nil());
    (token, user_uuid)
}

#[test_context(TestContext)]
#[tokio::test]
async fn dream_check_inactive_quota_reference_blocks_bucket_delete(ctx: &mut TestContext) {
    let realm = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "dream-quota@test.com").await;
    let bucket =
        create_test_credit_bucket(&ctx.app_state.pool, &realm, CreditBucketOpts::default()).await;
    seed_active_subscription_on_bucket(&ctx.app_state.pool, &realm, bucket).await;
    sqlx::query("UPDATE subscription SET status = 'canceled' WHERE realm_id = $1")
        .bind(&realm)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE points_quota_entitlements SET status = 'revoked' WHERE bucket_id = $1")
        .bind(bucket)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    let (status, body) = auth_admin_request_via_api(
        ctx,
        "DELETE",
        &format!("/api/realms/{realm}/billing/credit-buckets/{bucket}"),
        &token,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "historical quota references must return a useful conflict: {body:?}"
    );
    assert_eq!(error_code(&body), Some("bucket_in_use"));
}

#[test_context(TestContext)]
#[tokio::test]
async fn dream_check_disabled_rule_reference_blocks_bucket_delete(ctx: &mut TestContext) {
    let realm = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "dream-rule@test.com").await;
    let mapping =
        setup_test_entitlement_mapping(ctx, &realm, "stripe", "prod-dream", "dream-rule").await;
    let bucket =
        create_test_credit_bucket(&ctx.app_state.pool, &realm, CreditBucketOpts::default()).await;
    sqlx::query("INSERT INTO points_distribution_rules (id, realm_id, owner_type, entitlement_mapping_id, bucket_id, trigger_sources, grant_mode, points_amount, validity_days, enabled, display_order) VALUES ($1, $2, 'entitlement_mapping', $3, $4, ARRAY['subscription_initial'], 'fixed', 100, 0, false, 0)")
        .bind(Uuid::now_v7()).bind(&realm).bind(mapping).bind(bucket)
        .execute(&ctx.app_state.pool).await.unwrap();
    let (status, body) = auth_admin_request_via_api(
        ctx,
        "DELETE",
        &format!("/api/realms/{realm}/billing/credit-buckets/{bucket}"),
        &token,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "disabled rules still reference their bucket: {body:?}"
    );
    assert_eq!(error_code(&body), Some("bucket_in_use"));
}

#[test_context(TestContext)]
#[tokio::test]
async fn dream_check_legacy_policy_delete_protects_builtin_permissions(ctx: &mut TestContext) {
    use herald_core::domain::user::RolePolicyRepository;
    let realm = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "dream-policy@test.com").await;
    let (role, policy, resource, action): (Uuid, Uuid, String, String) = sqlx::query_as(
        "SELECT r.id, rp.id, rp.resource, rp.action FROM roles r JOIN role_policies rp ON rp.role_id = r.id JOIN permissions p ON p.realm_id = r.realm_id AND p.resource = rp.resource AND p.action = rp.action WHERE r.realm_id = $1 AND r.is_builtin AND p.is_builtin LIMIT 1",
    ).bind(&realm).fetch_one(&ctx.app_state.pool).await.unwrap();
    let result = ctx
        .app_state
        .role_policy_repository
        .delete_role_policy(role, &resource, &action)
        .await;
    assert!(matches!(
        result,
        Err(herald_core::domain::user::UserAdminError::PermissionDenied(
            _
        ))
    ));
    let (status, _) = auth_admin_request_via_api(
        ctx,
        "DELETE",
        &format!("/api/permission/roles/{role}/policies/{policy}"),
        &token,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "legacy HTTP deletion must use the same builtin guard"
    );
}

#[test_context(TestContext)]
#[tokio::test]
async fn dream_check_single_patch_cannot_disable_active_mapping(ctx: &mut TestContext) {
    let realm = ctx._realm_id.clone();
    let token = setup_billing_admin_session(ctx, "dream-mapping@test.com").await;
    let mapping =
        setup_test_entitlement_mapping(ctx, &realm, "creem", "prod-seed", "dream-mapping").await;
    sqlx::query("UPDATE provider_entitlement_mappings SET enabled = true WHERE id = $1")
        .bind(mapping)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    let bucket =
        create_test_credit_bucket(&ctx.app_state.pool, &realm, CreditBucketOpts::default()).await;
    seed_active_subscription_on_bucket(&ctx.app_state.pool, &realm, bucket).await;
    for body in [
        json!({"enabled": false}),
        json!({"enabled": false, "pointRules": []}),
    ] {
        let (status, response) = auth_admin_request_via_api(
            ctx,
            "PATCH",
            &format!("/api/bill/{realm}/entitlement-mappings/{mapping}"),
            &token,
            Some(&body),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "both PATCH paths must protect live subscriptions: {response:?}"
        );
        let enabled: bool =
            sqlx::query_scalar("SELECT enabled FROM provider_entitlement_mappings WHERE id = $1")
                .bind(mapping)
                .fetch_one(&ctx.app_state.pool)
                .await
                .unwrap();
        assert!(enabled, "rejected changes must roll back");
    }
}
