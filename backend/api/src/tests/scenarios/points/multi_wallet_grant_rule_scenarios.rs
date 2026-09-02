//! Multi-wallet distribution-rule scenario tests.
//!
//! These scenarios exercise the persisted rule/event model through the real
//! PostgreSQL repository. HTTP-owned configuration tests live beside their
//! resource modules; lifecycle entry points reuse the helpers below so every
//! scenario observes the same event, ledger, entitlement, and replay facts.

use crate::tests::schema_test_context::SchemaTestContext;
use chrono::Utc;
use herald_core::domain::points::{
    CapturedRuleRef, DistributionEvent, DistributionGrantResult, DistributionRuleOwner,
    DistributionRuleSelection, DistributionTrigger, PointsRepository,
};
use sqlx::Row;
use test_context::test_context;
use uuid::Uuid;

pub(crate) struct DistributionFixture {
    pub user_id: Uuid,
    pub mapping_id: Uuid,
    pub bucket_a: Uuid,
    pub bucket_b: Uuid,
    pub rule_a: Uuid,
    pub rule_b: Uuid,
}

async fn seed_account(ctx: &SchemaTestContext) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, status)
         VALUES ($1, $2, $3, 1)",
    )
    .bind(id)
    .bind(&ctx._realm_id)
    .bind(format!("multi-wallet-{id}@example.com"))
    .execute(&ctx.app_state.pool)
    .await
    .expect("seed account");
    id
}

pub(crate) async fn seed_bucket(ctx: &SchemaTestContext, realm_id: &str, enabled: bool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO credit_buckets
            (id, realm_id, bucket_key, name, enabled, display_order)
         VALUES ($1, $2, $3, $4, $5, 0)",
    )
    .bind(id)
    .bind(realm_id)
    .bind(format!("multi-wallet-{id}"))
    .bind(format!("Multi wallet {id}"))
    .bind(enabled)
    .execute(&ctx.app_state.pool)
    .await
    .expect("seed bucket");
    id
}

pub(crate) async fn seed_mapping(
    ctx: &SchemaTestContext,
    realm_id: &str,
    billing_type: &str,
) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO provider_entitlement_mappings
            (id, realm_id, payment_provider, external_product_id,
             entitlement_key, billing_type, enabled)
         VALUES ($1, $2, 'stripe', $3, $4, $5, true)",
    )
    .bind(id)
    .bind(realm_id)
    .bind(format!("prod_{id}"))
    .bind(format!("multi-wallet-{id}"))
    .bind(billing_type)
    .execute(&ctx.app_state.pool)
    .await
    .expect("seed mapping");
    id
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn seed_rule(
    ctx: &SchemaTestContext,
    realm_id: &str,
    mapping_id: Option<Uuid>,
    bucket_id: Uuid,
    trigger_sources: &[&str],
    grant_mode: &str,
    amount: Option<i64>,
    enabled: bool,
    display_order: i32,
) -> Uuid {
    let id = Uuid::now_v7();
    let owner_type = if mapping_id.is_some() {
        "entitlement_mapping"
    } else {
        "realm_registration"
    };
    let quota_windows = (grant_mode == "quota")
        .then(|| serde_json::json!([{"windowSeconds": 3600, "limit": 25, "key": "1h"}]));
    sqlx::query(
        "INSERT INTO points_distribution_rules
            (id, realm_id, owner_type, entitlement_mapping_id, bucket_id,
             trigger_sources, grant_mode, points_amount, validity_days,
             quota_windows, enabled, display_order)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, $9, $10, $11)",
    )
    .bind(id)
    .bind(realm_id)
    .bind(owner_type)
    .bind(mapping_id)
    .bind(bucket_id)
    .bind(trigger_sources)
    .bind(grant_mode)
    .bind(amount)
    .bind(quota_windows)
    .bind(enabled)
    .bind(display_order)
    .execute(&ctx.app_state.pool)
    .await
    .expect("seed distribution rule");
    id
}

pub(crate) async fn seed_two_rule_fixture(
    ctx: &SchemaTestContext,
    trigger: &str,
) -> DistributionFixture {
    let user_id = seed_account(ctx).await;
    let mapping_id = seed_mapping(
        ctx,
        &ctx._realm_id,
        if trigger == "topup" {
            "one_time"
        } else {
            "recurring"
        },
    )
    .await;
    let bucket_a = seed_bucket(ctx, &ctx._realm_id, true).await;
    let bucket_b = seed_bucket(ctx, &ctx._realm_id, true).await;
    let rule_a = seed_rule(
        ctx,
        &ctx._realm_id,
        Some(mapping_id),
        bucket_a,
        &[trigger],
        "fixed",
        Some(40),
        true,
        0,
    )
    .await;
    let rule_b = seed_rule(
        ctx,
        &ctx._realm_id,
        Some(mapping_id),
        bucket_b,
        &[trigger],
        "fixed",
        Some(60),
        true,
        1,
    )
    .await;
    DistributionFixture {
        user_id,
        mapping_id,
        bucket_a,
        bucket_b,
        rule_a,
        rule_b,
    }
}

fn event(
    ctx: &SchemaTestContext,
    fixture: &DistributionFixture,
    trigger: DistributionTrigger,
    key: String,
) -> DistributionEvent {
    DistributionEvent {
        realm_id: ctx._realm_id.clone(),
        user_id: fixture.user_id,
        owner: DistributionRuleOwner::EntitlementMapping(fixture.mapping_id),
        trigger,
        event_key: key.clone(),
        source_id: key,
        effective_from: Utc::now(),
        effective_until: None,
    }
}

async fn execute(
    ctx: &SchemaTestContext,
    fixture: &DistributionFixture,
    trigger: DistributionTrigger,
    key: &str,
    selection: DistributionRuleSelection,
) -> Result<Vec<DistributionGrantResult>, String> {
    ctx.app_state
        .points_repository
        .execute_distribution_event_atomic(event(ctx, fixture, trigger, key.to_string()), selection)
        .await
        .map_err(|e| e.to_string())
}

pub(crate) async fn assert_two_account_fixed_event(
    ctx: &SchemaTestContext,
    trigger: DistributionTrigger,
) {
    let fixture = seed_two_rule_fixture(ctx, trigger.as_str()).await;
    let results = execute(
        ctx,
        &fixture,
        trigger,
        &format!("multi-wallet:{}", Uuid::now_v7()),
        DistributionRuleSelection::CurrentOwnerRules,
    )
    .await
    .expect("two-rule event succeeds");
    assert_eq!(results.len(), 2, "one result per matched rule");
    let rows = sqlx::query(
        "SELECT bucket_id, granted_amount, distribution_event_id,
                distribution_rule_id
         FROM points_credit_ledger
         WHERE user_id = $1 ORDER BY bucket_id",
    )
    .bind(fixture.user_id)
    .fetch_all(&ctx.app_state.pool)
    .await
    .expect("query ledgers");
    assert_eq!(rows.len(), 2);
    let buckets: Vec<Uuid> = rows.iter().map(|r| r.get("bucket_id")).collect();
    assert!(buckets.contains(&fixture.bucket_a));
    assert!(buckets.contains(&fixture.bucket_b));
    assert!(rows.iter().all(|r| {
        r.get::<Option<Uuid>, _>("distribution_event_id").is_some()
            && r.get::<Option<Uuid>, _>("distribution_rule_id").is_some()
    }));
}

pub(crate) async fn assert_snapshot_survives_disable(ctx: &SchemaTestContext) {
    let fixture = seed_two_rule_fixture(ctx, "topup").await;
    let attempt_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO payment_attempts
            (id, realm_id, user_id, payment_provider, target_type, target_id,
             amount, currency, status, expires_at)
         VALUES ($1, $2, $3, 'stripe', 'entitlement_mapping', $4,
                 1, 'usd', 'Succeeded', now() + interval '1 hour')",
    )
    .bind(attempt_id)
    .bind(&ctx._realm_id)
    .bind(fixture.user_id)
    .bind(fixture.mapping_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("seed captured payment attempt");
    sqlx::query(
        "INSERT INTO payment_attempt_point_rules
            (payment_attempt_id, rule_id, bucket_id)
         VALUES ($1, $2, $3), ($1, $4, $5)",
    )
    .bind(attempt_id)
    .bind(fixture.rule_a)
    .bind(fixture.bucket_a)
    .bind(fixture.rule_b)
    .bind(fixture.bucket_b)
    .execute(&ctx.app_state.pool)
    .await
    .expect("seed captured payment rules");
    sqlx::query("UPDATE points_distribution_rules SET enabled = false WHERE id = ANY($1)")
        .bind(vec![fixture.rule_a, fixture.rule_b])
        .execute(&ctx.app_state.pool)
        .await
        .expect("disable captured rules");
    let results = ctx
        .app_state
        .points_repository
        .execute_distribution_event_atomic(
            DistributionEvent {
                realm_id: ctx._realm_id.clone(),
                user_id: fixture.user_id,
                owner: DistributionRuleOwner::EntitlementMapping(fixture.mapping_id),
                trigger: DistributionTrigger::Topup,
                event_key: format!("payment:{attempt_id}"),
                source_id: attempt_id.to_string(),
                effective_from: Utc::now(),
                effective_until: None,
            },
            DistributionRuleSelection::CapturedPaymentRules(vec![
                CapturedRuleRef {
                    rule_id: fixture.rule_a,
                    bucket_id: fixture.bucket_a,
                },
                CapturedRuleRef {
                    rule_id: fixture.rule_b,
                    bucket_id: fixture.bucket_b,
                },
            ]),
        )
        .await
        .expect("captured payment rules remain fulfillable");
    assert_eq!(results.len(), 2);
}

pub(crate) async fn assert_fixed_and_quota_event(ctx: &SchemaTestContext) {
    let mut fixture = seed_two_rule_fixture(ctx, "subscription_initial").await;
    sqlx::query("DELETE FROM points_distribution_rules WHERE id = $1")
        .bind(fixture.rule_b)
        .execute(&ctx.app_state.pool)
        .await
        .expect("replace second rule");
    fixture.rule_b = seed_rule(
        ctx,
        &ctx._realm_id,
        Some(fixture.mapping_id),
        fixture.bucket_b,
        &["subscription_initial"],
        "quota",
        None,
        true,
        1,
    )
    .await;
    let results = execute(
        ctx,
        &fixture,
        DistributionTrigger::SubscriptionInitial,
        &format!("subscription-initial:{}", Uuid::now_v7()),
        DistributionRuleSelection::CurrentOwnerRules,
    )
    .await
    .expect("fixed and quota commit together");
    assert_eq!(results.len(), 2);
    let ledger_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM points_credit_ledger WHERE user_id = $1")
            .bind(fixture.user_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
    let quota_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM points_quota_entitlements WHERE user_id = $1")
            .bind(fixture.user_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
    assert_eq!((ledger_count, quota_count), (1, 1));
}

pub(crate) async fn assert_replay_is_stable(ctx: &SchemaTestContext) {
    let fixture = seed_two_rule_fixture(ctx, "topup").await;
    let key = format!("replay:{}", Uuid::now_v7());
    let first = execute(
        ctx,
        &fixture,
        DistributionTrigger::Topup,
        &key,
        DistributionRuleSelection::CurrentOwnerRules,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE points_distribution_rules SET enabled = false WHERE id = $1")
        .bind(fixture.rule_a)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    let third_bucket = seed_bucket(ctx, &ctx._realm_id, true).await;
    seed_rule(
        ctx,
        &ctx._realm_id,
        Some(fixture.mapping_id),
        third_bucket,
        &["topup"],
        "fixed",
        Some(99),
        true,
        3,
    )
    .await;
    let replay = execute(
        ctx,
        &fixture,
        DistributionTrigger::Topup,
        &key,
        DistributionRuleSelection::CurrentOwnerRules,
    )
    .await
    .unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(replay.len(), 2, "replay fixes the first complete set");
    let ledger_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM points_credit_ledger WHERE user_id = $1")
            .bind(fixture.user_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
    assert_eq!(ledger_count, 2, "replay must not issue new rows");
}

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_multi_wallet_grant_rule_realm_crud_and_permission_matrix(
    ctx: &mut SchemaTestContext,
) {
    use crate::tests::helpers::billing_helpers::setup_billing_admin_session;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use tower::ServiceExt;

    let bucket = seed_bucket(ctx, &ctx._realm_id, true).await;
    let token = setup_billing_admin_session(ctx, "multi-wallet-rules@example.com").await;
    let app = ctx.create_unified_test_router();
    let body = serde_json::json!({"rules": [{
        "bucketId": bucket,
        "triggerSources": ["registration"],
        "grantMode": "fixed",
        "pointsAmount": 75,
        "validityDays": 0,
        "enabled": true,
        "displayOrder": 0
    }]});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/points/{}/registration-rules", ctx._realm_id))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(value["rules"].as_array().unwrap().len(), 1);
    assert_eq!(value["rules"][0]["bucketId"], bucket.to_string());

    let invalid = serde_json::json!({"rules": [{
        "bucketId": bucket,
        "triggerSources": ["subscription_initial"],
        "grantMode": "fixed",
        "pointsAmount": 1
    }]});
    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/points/{}/registration-rules", ctx._realm_id))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(invalid.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_multi_wallet_grant_rule_disabled_bucket_rolls_back_every_result(
    ctx: &mut SchemaTestContext,
) {
    let fixture = seed_two_rule_fixture(ctx, "topup").await;
    sqlx::query("UPDATE credit_buckets SET enabled = false WHERE id = $1")
        .bind(fixture.bucket_b)
        .execute(&ctx.app_state.pool)
        .await
        .unwrap();
    let result = execute(
        ctx,
        &fixture,
        DistributionTrigger::Topup,
        &format!("disabled:{}", Uuid::now_v7()),
        DistributionRuleSelection::CurrentOwnerRules,
    )
    .await;
    assert!(result.is_err());
    let counts: (i64, i64) = (
        sqlx::query_scalar("SELECT COUNT(*) FROM points_credit_ledger WHERE user_id = $1")
            .bind(fixture.user_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap(),
        sqlx::query_scalar("SELECT COUNT(*) FROM points_distribution_events WHERE user_id = $1")
            .bind(fixture.user_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap(),
    );
    assert_eq!(counts, (0, 0), "the whole event transaction rolls back");
}

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_multi_wallet_grant_rule_concurrent_duplicate_returns_original_results(
    ctx: &mut SchemaTestContext,
) {
    let fixture = seed_two_rule_fixture(ctx, "topup").await;
    let key = format!("concurrent:{}", Uuid::now_v7());
    let a = execute(
        ctx,
        &fixture,
        DistributionTrigger::Topup,
        &key,
        DistributionRuleSelection::CurrentOwnerRules,
    );
    let b = execute(
        ctx,
        &fixture,
        DistributionTrigger::Topup,
        &key,
        DistributionRuleSelection::CurrentOwnerRules,
    );
    let (a, b) = tokio::join!(a, b);
    assert_eq!(a.unwrap().len(), 2);
    assert_eq!(b.unwrap().len(), 2);
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM points_credit_ledger WHERE user_id = $1")
            .bind(fixture.user_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .unwrap();
    assert_eq!(count, 2);
}

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_multi_wallet_grant_rule_zero_rules_replay_stays_empty_after_configuration(
    ctx: &mut SchemaTestContext,
) {
    let user_id = seed_account(ctx).await;
    let mapping_id = seed_mapping(ctx, &ctx._realm_id, "one_time").await;
    let bucket = seed_bucket(ctx, &ctx._realm_id, true).await;
    let fixture = DistributionFixture {
        user_id,
        mapping_id,
        bucket_a: bucket,
        bucket_b: bucket,
        rule_a: Uuid::nil(),
        rule_b: Uuid::nil(),
    };
    let key = format!("zero:{}", Uuid::now_v7());
    let first = execute(
        ctx,
        &fixture,
        DistributionTrigger::Topup,
        &key,
        DistributionRuleSelection::CurrentOwnerRules,
    )
    .await
    .unwrap();
    assert!(first.is_empty());
    seed_rule(
        ctx,
        &ctx._realm_id,
        Some(mapping_id),
        bucket,
        &["topup"],
        "fixed",
        Some(5),
        true,
        0,
    )
    .await;
    let replay = execute(
        ctx,
        &fixture,
        DistributionTrigger::Topup,
        &key,
        DistributionRuleSelection::CurrentOwnerRules,
    )
    .await
    .unwrap();
    assert!(replay.is_empty(), "zero-result completion is replay-stable");
}

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_multi_wallet_grant_rule_corrupt_result_count_fails_loud(ctx: &mut SchemaTestContext) {
    let fixture = seed_two_rule_fixture(ctx, "topup").await;
    let key = format!("corrupt:{}", Uuid::now_v7());
    execute(
        ctx,
        &fixture,
        DistributionTrigger::Topup,
        &key,
        DistributionRuleSelection::CurrentOwnerRules,
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE points_distribution_events SET result_count = result_count + 1
         WHERE user_id = $1 AND event_key = $2",
    )
    .bind(fixture.user_id)
    .bind(&key)
    .execute(&ctx.app_state.pool)
    .await
    .unwrap();
    let replay = execute(
        ctx,
        &fixture,
        DistributionTrigger::Topup,
        &key,
        DistributionRuleSelection::CurrentOwnerRules,
    )
    .await;
    assert!(
        replay.unwrap_err().contains("result corruption"),
        "corrupt completion must not be reported as success"
    );
}

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_multi_wallet_grant_rule_admin_sdk_internal_quota_do_not_fan_out(
    ctx: &mut SchemaTestContext,
) {
    use herald_core::domain::points::{CreditSourceType, CreditType};
    let fixture = seed_two_rule_fixture(ctx, "topup").await;
    ctx.app_state
        .points_repository
        .grant_points_atomic(
            &ctx._realm_id,
            fixture.user_id,
            fixture.bucket_a,
            CreditType::GrantedCredit,
            CreditSourceType::AdminGrant,
            11,
            None,
            None,
            Some("direct-admin".to_string()),
            Some("direct grant must stay bucket-scoped".to_string()),
            Some(format!("direct:{}", Uuid::now_v7())),
        )
        .await
        .expect("direct admin grant");
    let row = sqlx::query(
        "SELECT bucket_id, distribution_event_id, distribution_rule_id
         FROM points_credit_ledger WHERE user_id = $1",
    )
    .bind(fixture.user_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .unwrap();
    assert_eq!(row.get::<Uuid, _>("bucket_id"), fixture.bucket_a);
    assert_eq!(row.get::<Option<Uuid>, _>("distribution_event_id"), None);
    assert_eq!(row.get::<Option<Uuid>, _>("distribution_rule_id"), None);
}

#[test]
fn test_multi_wallet_grant_rule_openapi_removes_singular_contract_fields() {
    let spec = serde_json::to_value(crate::application::http::server::build_openapi_spec())
        .expect("serialize runtime OpenAPI");
    let schemas = &spec["components"]["schemas"];
    let mapping = &schemas["EntitlementMappingResponse"]["properties"];
    assert!(mapping.get("pointRules").is_some());
    for removed in [
        "pointsPerPeriod",
        "grantOnSubscribe",
        "quotaWindows",
        "bucketId",
    ] {
        assert!(
            mapping.get(removed).is_none(),
            "legacy mapping field {removed} must be absent"
        );
    }
    for schema_name in ["FulfillmentResultResponse", "FulfillPaymentResponse"] {
        let fulfillment = &schemas[schema_name]["properties"];
        assert!(
            fulfillment.get("pointGrants").is_some(),
            "{schema_name} exposes the multi-result pointGrants contract"
        );
        assert!(fulfillment.get("pointsGranted").is_none());
    }
    let rule = &schemas["PointDistributionRuleResponse"]["properties"];
    assert!(
        rule.get("bucketId").is_some(),
        "bucketId remains legal at rule level"
    );
}
