// User Stories: US-PU-010, US-CB-008
// Subscription/free-periodic credits now use quota entitlements, not ledger-row
// reclaim. Revoke removes the entitlement from the active window set; already
// written consume rows remain as audit/usage history and are not reversed.

use crate::tests::helpers::points_helpers::*;
use crate::tests::schema_test_context::SchemaTestContext;
use chrono::{Duration, Utc};
use herald_core::domain::points::entities::{CreditType, QuotaSourceType, QuotaWindow};
use test_context::test_context;
use uuid::Uuid;

fn daily_window(limit: i64) -> Vec<QuotaWindow> {
    vec![QuotaWindow {
        window_seconds: 86_400,
        limit,
        key: "day".to_string(),
    }]
}

async fn grant_subscription_quota(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    source_id: &str,
    limit: i64,
) {
    let period_start = Utc::now();
    ctx.app_state
        .subscription_service
        .grant_quota_entitlement(
            realm_id,
            user_id,
            bucket_id,
            CreditType::SubscriptionCredit,
            QuotaSourceType::SubscriptionInitial,
            source_id.to_string(),
            daily_window(limit),
            period_start,
            Some(period_start + Duration::days(30)),
            format!("sub:{source_id}:period:{}", period_start.timestamp()),
        )
        .await
        .expect("quota grant should succeed");
}

async fn revoke_subscription_quota(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    source_id: &str,
) {
    ctx.app_state
        .subscription_service
        .revoke_quota_entitlement(
            realm_id,
            user_id,
            bucket_id,
            CreditType::SubscriptionCredit,
            source_id,
            Utc::now(),
        )
        .await
        .expect("revoke should succeed");
}

async fn count_transaction(ctx: &SchemaTestContext, transaction_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM points_transactions WHERE id = $1")
        .bind(transaction_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .expect("failed to count transaction")
}

async fn assert_subscription_revoked(
    ctx: &SchemaTestContext,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    source_id: &str,
) {
    assert_eq!(
        count_active_quota_entitlements(
            ctx,
            realm_id,
            user_id,
            bucket_id,
            CreditType::SubscriptionCredit,
        )
        .await,
        0,
        "revoked entitlement must leave the active window set"
    );
    assert_window_available(
        ctx,
        realm_id,
        user_id,
        bucket_id,
        CreditType::SubscriptionCredit,
        0,
    )
    .await;
    assert_eq!(
        quota_entitlement_status(
            ctx,
            realm_id,
            user_id,
            bucket_id,
            CreditType::SubscriptionCredit,
            source_id,
        )
        .await,
        ("revoked".to_string(), true),
        "revoke should mark status and set effective_until"
    );
}

async fn assert_subscription_source_revoked(
    ctx: &SchemaTestContext,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    source_id: &str,
) {
    assert_eq!(
        quota_entitlement_status(
            ctx,
            realm_id,
            user_id,
            bucket_id,
            CreditType::SubscriptionCredit,
            source_id,
        )
        .await,
        ("revoked".to_string(), true),
        "old entitlement source should be revoked even when a replacement entitlement is active"
    );
}

async fn setup_subscription_quota(
    ctx: &mut SchemaTestContext,
    email: &str,
    source_id: &str,
    limit: i64,
) -> (String, Uuid, Uuid) {
    let realm_id = ctx._realm_id.clone();
    let (user_id, bucket_id) = create_user_wallet_and_bucket_for_test(ctx, &realm_id, email).await;
    grant_subscription_quota(ctx, &realm_id, user_id, bucket_id, source_id, limit).await;
    (realm_id, user_id, bucket_id)
}

async fn grant_free_quota(
    ctx: &SchemaTestContext,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    source_id: &str,
    limit: i64,
) {
    grant_quota_entitlement_for_test(
        ctx,
        realm_id,
        user_id,
        bucket_id,
        CreditType::FreePeriodicCredit,
        QuotaSourceType::FreePeriodicGrant,
        source_id,
        &[(86_400, limit, "day")],
        Utc::now(),
        None,
    )
    .await;
}

async fn revoke_free_quota(
    ctx: &mut SchemaTestContext,
    realm_id: &str,
    user_id: Uuid,
    bucket_id: Uuid,
    source_id: &str,
) {
    ctx.app_state
        .subscription_service
        .revoke_quota_entitlement(
            realm_id,
            user_id,
            bucket_id,
            CreditType::FreePeriodicCredit,
            source_id,
            Utc::now(),
        )
        .await
        .expect("free quota revoke should succeed");
}

async fn assert_grant_idempotency(ctx: &mut SchemaTestContext, email: &str) {
    let realm_id = ctx._realm_id.clone();
    let (user_id, bucket_id) = create_user_wallet_and_bucket_for_test(ctx, &realm_id, email).await;
    let subscription_id = Uuid::now_v7();
    let period_start = Utc::now();
    let period_end = period_start + Duration::days(30);
    let idempotency_key = format!("sub:{subscription_id}:period:{}", period_start.timestamp());

    let first = ctx
        .app_state
        .subscription_service
        .grant_quota_entitlement(
            &realm_id,
            user_id,
            bucket_id,
            CreditType::SubscriptionCredit,
            QuotaSourceType::SubscriptionInitial,
            subscription_id.to_string(),
            daily_window(100),
            period_start,
            Some(period_end),
            idempotency_key.clone(),
        )
        .await
        .expect("first quota grant should succeed");

    let second = ctx
        .app_state
        .subscription_service
        .grant_quota_entitlement(
            &realm_id,
            user_id,
            bucket_id,
            CreditType::SubscriptionCredit,
            QuotaSourceType::SubscriptionInitial,
            subscription_id.to_string(),
            daily_window(100),
            period_start,
            Some(period_end),
            idempotency_key,
        )
        .await
        .expect("duplicate quota grant should replay existing row");

    assert_eq!(
        first.id, second.id,
        "duplicate grant must return the same entitlement"
    );
    assert_eq!(
        count_all_quota_entitlements(
            ctx,
            &realm_id,
            user_id,
            bucket_id,
            CreditType::SubscriptionCredit,
        )
        .await,
        1,
        "idempotency key must prevent duplicate quota grants"
    );
}

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_grant_quota_entitlement_is_idempotent(ctx: &mut SchemaTestContext) {
    assert_grant_idempotency(ctx, "quota-idem@example.com").await;
}

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_revoke_quota_entitlement_zeroes_window_without_reversing_consumes(
    ctx: &mut SchemaTestContext,
) {
    let subscription_id = Uuid::now_v7();
    let source_id = subscription_id.to_string();
    let (realm_id, user_id, bucket_id) =
        setup_subscription_quota(ctx, "quota-revoke@example.com", &source_id, 100).await;

    assert_window_available(
        ctx,
        &realm_id,
        user_id,
        bucket_id,
        CreditType::SubscriptionCredit,
        100,
    )
    .await;

    let consume_id = seed_quota_consume_for_test(
        ctx,
        &realm_id,
        user_id,
        bucket_id,
        CreditType::SubscriptionCredit,
        40,
        Utc::now(),
    )
    .await;
    assert_window_available(
        ctx,
        &realm_id,
        user_id,
        bucket_id,
        CreditType::SubscriptionCredit,
        60,
    )
    .await;

    revoke_subscription_quota(ctx, &realm_id, user_id, bucket_id, &source_id).await;
    revoke_subscription_quota(ctx, &realm_id, user_id, bucket_id, &source_id).await;
    assert_subscription_revoked(ctx, &realm_id, user_id, bucket_id, &source_id).await;
    assert_eq!(
        count_transaction(ctx, consume_id).await,
        1,
        "revoke must not delete or reverse existing consume history"
    );
}

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_lifecycle_revoke_upgrade_grant_new(ctx: &mut SchemaTestContext) {
    let old_source_id = Uuid::now_v7().to_string();
    let new_source_id = Uuid::now_v7().to_string();
    let (realm_id, user_id, bucket_id) =
        setup_subscription_quota(ctx, "quota-upgrade@example.com", &old_source_id, 100).await;
    revoke_subscription_quota(ctx, &realm_id, user_id, bucket_id, &old_source_id).await;
    grant_subscription_quota(ctx, &realm_id, user_id, bucket_id, &new_source_id, 250).await;

    assert_subscription_source_revoked(ctx, &realm_id, user_id, bucket_id, &old_source_id).await;
    assert_window_available(
        ctx,
        &realm_id,
        user_id,
        bucket_id,
        CreditType::SubscriptionCredit,
        250,
    )
    .await;
}

#[test_context(SchemaTestContext)]
#[tokio::test]
async fn test_lifecycle_free_revoke_on_paid_upgrade(ctx: &mut SchemaTestContext) {
    let realm_id = ctx._realm_id.clone();
    let (user_id, bucket_id) =
        create_user_wallet_and_bucket_for_test(ctx, &realm_id, "quota-free-upgrade@example.com")
            .await;
    let free_source_id = format!("registration:{user_id}");
    let sub_source_id = Uuid::now_v7().to_string();

    grant_free_quota(ctx, &realm_id, user_id, bucket_id, &free_source_id, 50).await;
    assert_window_available(
        ctx,
        &realm_id,
        user_id,
        bucket_id,
        CreditType::FreePeriodicCredit,
        50,
    )
    .await;
    revoke_free_quota(ctx, &realm_id, user_id, bucket_id, &free_source_id).await;
    grant_subscription_quota(ctx, &realm_id, user_id, bucket_id, &sub_source_id, 100).await;

    assert_window_available(
        ctx,
        &realm_id,
        user_id,
        bucket_id,
        CreditType::FreePeriodicCredit,
        0,
    )
    .await;
    assert_window_available(
        ctx,
        &realm_id,
        user_id,
        bucket_id,
        CreditType::SubscriptionCredit,
        100,
    )
    .await;
}
