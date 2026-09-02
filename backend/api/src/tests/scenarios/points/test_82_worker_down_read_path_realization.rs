// =============================================================================
// worker-down + read-path realization + fail-loud
// =============================================================================
//
// Encodes US-FU-004 scenario 1.1 (read-path realization when the worker
// never runs) plus the realization write-failure fail-loud contract; the
// realization logic is pinned by the invariants below.
//
// CORE INVARIANTS under test (the hardest scenarios in the whole feature):
//
//  1. WORKER-DOWN STILL USABLE: with the worker NOT invoked at all, a due
//     free-periodic schedule is realized on the READ path — calling
//     `PointsService::get_balance` (or `consume_points`) grants the due
//     period and the derived balance reflects it. No worker tick is needed
//     (worker is preheat, not a correctness boundary).
//
//  2. N=3 CAP: realization grants at most 3 due schedules in one call
//     (`find_due_free_grant_schedules_for_user(... limit=3)`); the schedule
//     advances by exactly one period per call.
//
//  3. IDEMPOTENT: calling `get_balance` twice (or concurrently from many
//     tasks) does NOT double-grant — `points_grant_records(schedule_id,
//     period_number)` UNIQUE + schedule row `FOR UPDATE`.
//
//  4. subscription_id IS NULL ONLY: a subscription-bound schedule is NOT
//     realized by the free read-path (`find_due_free_grant_schedules_for_user`
//     filters `subscription_id IS NULL`).
//
//  5. lead_time=0: realization only catches up to `next_grant_time <= now`
//     (a schedule with `next_grant_time > now` is NOT realized early).
//
//  6. FAIL-LOUD: when realization WRITE fails (real DB constraint
//     violation injected by `points_per_period = 0` schedule →
//     `points_credit_ledger.granted_amount > 0` CHECK fails on INSERT),
//     `get_balance`/`consume_points` return `CoreError::DatabaseError`
//     (which maps to HTTP 500 INTERNAL_SERVER_ERROR). They do NOT degrade
//     to `BadRequest("Insufficient points balance...")` (which is the
//     400-mapping `CoreError::insufficient_points` variant) — masking a
//     system write fault as user "low balance" is precisely the
//     anti-pattern the fail-loud contract forbids.
//
// All balance assertions use the derived-predicate helpers
// (`assert_derived_balance` / `get_derived_balance_by_credit_type`) — these
// mirror production `compute_available_balance` verbatim and NEVER read
// `points_wallets.total_balance` (physically removed).
//
// Worker is NEVER started or invoked in this file. Clock progression is
// simulated via SQL UPDATE on schedule rows (`advance_schedule` helper) or
// by seeding `next_grant_time` at the desired offset, mirroring the
// virtual-clock idiom established in test_80.
//
// Provenance labels for fail-loud (evidence strength):
//   * test_realization_failure_fail_loud_get_balance_5xx
//       → service-direct Err propagation (CoreError::DatabaseError).
//     The HTTP 500 mapping is verifiable in `app_errors.rs::status_code()`
//     (`DatabaseError → INTERNAL_SERVER_ERROR`). The test does NOT round-trip
//     through the HTTP handler because the only HTTP balance route
//     (`get_wallet`) does NOT call `reconcile_due_for_user`; realization is
//     exposed only via the service method `get_balance`. This is a
//     service-level fail-loud assertion.
//   * test_realization_failure_consume_fail_loud
//       → same service-direct Err propagation via `consume_points`.
//
// =============================================================================

use crate::tests::helpers::points_helpers::{
    assert_derived_balance, create_free_grant_schedule, create_subscription_grant_schedule,
    create_test_third_party_identity, grant_record_exists,
};
use crate::tests::scenarios::points::fixtures::create_test_user;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use chrono::{Duration, Utc};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::points::dtos::ConsumePointsInput;
use herald_core::domain::points::entities::CreditType;
use std::sync::Arc;
use test_context::test_context;
use uuid::Uuid;

// =============================================================================
// Scenario "worker 未运行仍可用" (子用例 b) + US-FU-004 场景 1.1
// =============================================================================

// User Story: US-FU-004 scenario 1.1 (free user receives each period's free
// credits on time even when the worker never runs).
//
// Covers the free-periodic worker-down rule: `get_balance`/consume entries
// synchronously backfill the due period's row via `reconcile_due_for_user`
// (single-user, idempotent, lead_time=0, subscription_id IS NULL only,
// fail-loud 5xx) — the worker is preheat, not correctness.
//
// WHY this test exists: this is THE central point-time correctness backstop
// for free users. The GrantScheduler (`process_due_schedules`) was dead code
// before this feature and remains a warming path; if the worker is never
// started (deployment misconfiguration, crash, scheduler stall), a free user
// whose period has started would see zero balance on every request UNLESS the
// READ path reconciles the due schedule inline. We seed a due schedule and
// call `PointsService::get_balance` WITHOUT constructing or invoking any
// GrantScheduler — if the read path did not reconcile, the derived balance
// would stay 0 and the test would fail. The fact that it grants the period
// inline and exposes the balance immediately is the correctness claim.
#[test_context(TestContext)]
#[tokio::test]
async fn test_free_periodic_worker_down_read_path_realization(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(
        &ctx.app_state.pool,
        &realm_id,
        "be-t08-fu004-getbalance@exam.com",
    )
    .await;

    let now = Utc::now();
    let points_per_period: i64 = 100;
    let validity_days: i64 = 30;

    // Seed a due free-periodic schedule: next_grant_time is 1h in the past,
    // granted_periods = 0 ⟹ period 1 is the next grant. subscription_id is
    // NULL by construction of `create_free_grant_schedule`.
    let schedule_id = create_free_grant_schedule(
        ctx,
        user_id,
        &realm_id,
        "monthly",
        points_per_period,
        validity_days,
        now - Duration::hours(1),
        0,
        "",
    )
    .await;

    // Precondition: nothing granted yet.
    assert_derived_balance(ctx, user_id, &realm_id, CreditType::FreePeriodicCredit, 0).await;
    assert!(
        !grant_record_exists(ctx, schedule_id, 1).await,
        "precondition: no grant_record for period 1 yet"
    );

    // === THE core gesture: call `get_balance` WITHOUT touching the worker.
    // Realization (`reconcile_due_for_user`) runs inline before the derived
    // SUM, writing period 1. ===
    let identity = create_test_third_party_identity(&realm_id);
    let balance = ctx
        .app_state
        .points_service
        .get_balance(identity, &realm_id, user_id)
        .await
        .expect("get_balance must succeed — read-path realization is the backstop");

    // (a) The period-level business idempotency row was written.
    assert!(
        grant_record_exists(ctx, schedule_id, 1).await,
        "read-path realization must write the period-1 grant_record"
    );

    // (b) The realized row is immediately available in the derived balance
    // (effective_at <= now ⟹ predicate includes it). The user sees the
    // grant on the SAME request — no worker, no second call.
    assert_eq!(
        balance.free_periodic_balance, points_per_period,
        "free_periodic_balance must reflect the just-realized period"
    );
    assert_derived_balance(
        ctx,
        user_id,
        &realm_id,
        CreditType::FreePeriodicCredit,
        points_per_period,
    )
    .await;
}

// ----------------------------------------------------------------------------
// Scenario: read-path realization — consume entry-point also triggers
// realization, so a consume request immediately after period start succeeds
// without any worker tick.
// ----------------------------------------------------------------------------

// User Story: US-FU-004 scenario 1.1 (consume after period start succeeds
// because consume also reconciles inline).
//
// Covers the free-periodic read-path rule for the consume entry plus its
// ordering guarantee: consume runs reconcile_due_for_user BEFORE
// find_active_ledgers_for_update.
//
// WHY this test exists: realization sits at BOTH read entries
// (`get_balance` and `consume_points`). The consume path is the more
// critical one — if it silently skipped realization, a free user with zero
// balance would see "Insufficient points" on the FIRST consume of a new
// period even though their grant is due, undermining the whole backstop.
// This test pins that consume reconciles BEFORE the consume transaction
// opens, so a due period is fulfilled and the consume succeeds.
#[test_context(TestContext)]
#[tokio::test]
async fn test_free_periodic_consume_triggers_realization(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(
        &ctx.app_state.pool,
        &realm_id,
        "be-t08-fu004-consume@exam.com",
    )
    .await;

    let now = Utc::now();
    let points_per_period: i64 = 200;
    let validity_days: i64 = 30;
    let consume_amount: i64 = 50; // <= points_per_period so consume succeeds

    let schedule_id = create_free_grant_schedule(
        ctx,
        user_id,
        &realm_id,
        "monthly",
        points_per_period,
        validity_days,
        now - Duration::hours(1),
        0,
        "",
    )
    .await;

    // Precondition: zero balance.
    assert_derived_balance(ctx, user_id, &realm_id, CreditType::FreePeriodicCredit, 0).await;

    // Consume calls reconcile_due_for_user FIRST → period 1 is realized →
    // consume transaction opens and selects the now-available row.
    let identity = create_test_third_party_identity(&realm_id);
    let input = ConsumePointsInput {
        user_id: user_id.to_string(),
        client_app_id: ctx._client_app_id.clone(),
        amount: consume_amount,
        description: Some("be-t08 consume triggers realization".to_string()),
    };
    let txns = ctx
        .app_state
        .points_service
        .consume_points(identity, &realm_id, input)
        .await
        .expect("consume must succeed — realization runs before the consume transaction");

    // Realization happened: grant_record for period 1 exists, and the
    // consume produced at least one per-bucket transaction.
    assert!(
        grant_record_exists(ctx, schedule_id, 1).await,
        "consume must trigger read-path realization for period 1"
    );
    assert!(
        !txns.is_empty(),
        "consume should produce per-bucket transactions"
    );

    // Derived balance reflects the realized period minus what was consumed.
    assert_derived_balance(
        ctx,
        user_id,
        &realm_id,
        CreditType::FreePeriodicCredit,
        points_per_period - consume_amount,
    )
    .await;
}

// =============================================================================
// Scenario: read-path realization — concurrent get_balance calls do NOT
// double-grant (idempotency).
// =============================================================================

// User Story: US-FU-004 (correctness backstop must not double-grant under
// concurrent requests — free credits are bounded and double-granting would
// break accounting invariants).
//
// Covers "幂等：points_grant_records(schedule_id, period_number)
// UNIQUE + schedule 行 FOR UPDATE，并发请求天然去重" + "并发请求不重复发放".
//
// WHY this test exists: the read path is hit by every balance/consume
// request — including concurrent ones (e.g. multiple tabs, retry storms).
// Without the (schedule_id, period_number) UNIQUE constraint + schedule-row
// FOR UPDATE lock, two concurrent get_balance calls could both observe "no
// grant_record yet" and both insert a ledger row, doubling the grant. This
// test fires N concurrent get_balance futures against the SAME schedule
// and asserts exactly ONE ledger row and ONE grant_record exist for period
// 1 afterwards. The idempotency comes from the DB constraints, not from
// test-side serialization — so this test would catch a regression that
// removed either guard.
#[test_context(TestContext)]
#[tokio::test]
async fn test_realization_concurrent_no_duplicate_grant(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id =
        create_test_user(&ctx.app_state.pool, &realm_id, "be-t08-concurrent@exam.com").await;

    let now = Utc::now();
    let points_per_period: i64 = 100;
    let validity_days: i64 = 30;

    let schedule_id = create_free_grant_schedule(
        ctx,
        user_id,
        &realm_id,
        "monthly",
        points_per_period,
        validity_days,
        now - Duration::minutes(1),
        0,
        "",
    )
    .await;

    // Fire N concurrent get_balance futures from spawned tasks. Each task
    // clones the Arc<points_service> and constructs its own identity — the
    // shared state is the schedule row + grant_records table. Idempotency
    // MUST come from the DB (UNIQUE + FOR UPDATE), not from task ordering.
    let points_service = Arc::clone(&ctx.app_state.points_service);
    let realm_id_clone = realm_id.clone();
    let n = 8;
    let mut handles = Vec::with_capacity(n);
    for _ in 0..n {
        let svc = Arc::clone(&points_service);
        let realm = realm_id_clone.clone();
        handles.push(tokio::spawn(async move {
            let identity = create_test_third_party_identity(&realm);
            // Errors are acceptable here — under contention some callers may
            // see a serialization failure on the UNIQUE insert (the impl
            // surfaces DatabaseError). We only care that EXACTLY ONE grant
            // survives, asserted below. We swallow per-task results.
            let _ = svc.get_balance(identity, &realm, user_id).await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }

    // (a) Exactly ONE grant_record for period 1 — UNIQUE constraint won.
    let grant_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM points_grant_records
         WHERE schedule_id = $1 AND period_number = 1",
    )
    .bind(schedule_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("count grant_records");
    assert_eq!(
        grant_count, 1,
        "concurrent realization must produce exactly ONE grant_record for period 1, got {}",
        grant_count
    );

    // (b) Exactly ONE free_periodic ledger row from this schedule — no
    // duplicate grants leaked into the ledger. The read-path realization
    // grants through `execute_scheduled_fixed_in_tx` → `write_rule_ledger_in_tx`,
    // which writes `source_id = 'distribution:{event_id}'` (not the legacy
    // `schedule:{id}:period:{n}` shape), so scope by the distribution prefix.
    let ledger_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM points_credit_ledger
         WHERE user_id = $1 AND realm_id = $2
           AND credit_type = 'free_periodic_credit'
           AND source_id LIKE 'distribution:%'",
    )
    .bind(user_id)
    .bind(&realm_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("count free_periodic ledger");
    assert_eq!(
        ledger_count, 1,
        "concurrent realization must produce exactly ONE ledger row for period 1, got {}",
        ledger_count
    );

    // (c) Derived balance reflects the single grant exactly once.
    assert_derived_balance(
        ctx,
        user_id,
        &realm_id,
        CreditType::FreePeriodicCredit,
        points_per_period,
    )
    .await;
}

// =============================================================================
// Scenario "兑现期数上限 N (默认 3)"
// =============================================================================

// User Story: US-FU-004 (the read-path backstop must stay bounded — a single
// request must not write an unbounded number of ledger rows when many
// schedules are overdue).
//
// Covers "兑现上限 N (默认 3 期/请求): worker 长宕机时不一次性
// 放出大量历史期" + "兑现期数上限 N 生效".
//
// WHY this test exists: if the worker is down for a long time and a user
// accumulates many due schedules (one per realm/bucket/entitlement
// configuration change, etc.), an unbounded realization loop in the request
// path would write a large batch of rows under a single request — increasing
// lock contention, transaction size, and tail latency. The impl caps the
// `find_due_free_grant_schedules_for_user` query at `limit = N = 3`, so at
// most 3 schedules are realized per call. This test seeds 5 due schedules
// for the same user, calls `get_balance` once, and asserts ≤ 3 grant_records
// were written in that single call.
//
// Note on the cap granularity: the impl grants ONE period per schedule per
// call (period_number = granted_periods + 1). The N=3 cap is therefore
// expressed as "≤ 3 schedules processed per call", and the natural way to
// exercise it is to seed many due schedules, not to make one schedule many
// periods overdue (a single overdue schedule yields exactly 1 ledger row
// per call regardless of how many months it's been due).
#[test_context(TestContext)]
#[tokio::test]
async fn test_realization_period_cap_n(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(&ctx.app_state.pool, &realm_id, "be-t08-cap-n@exam.com").await;

    let now = Utc::now();
    let points_per_period: i64 = 50;
    let validity_days: i64 = 30;

    // Seed 5 due free schedules for the same user. All have distinct ids but
    // share the realm's legacy bucket (matches registration-service shape).
    let mut schedule_ids = Vec::with_capacity(5);
    for i in 0..5 {
        let schedule_id = create_free_grant_schedule(
            ctx,
            user_id,
            &realm_id,
            "monthly",
            points_per_period,
            validity_days,
            // Stagger next_grant_time slightly so the ORDER BY next_grant_time
            // ASC in find_due_free_grant_schedules_for_user produces a
            // deterministic pick; all are still <= now.
            now - Duration::hours(2) - Duration::minutes(i),
            0,
            "",
        )
        .await;
        schedule_ids.push(schedule_id);
    }

    // Single get_balance call — realization processes at most N=3 schedules.
    let identity = create_test_third_party_identity(&realm_id);
    let _ = ctx
        .app_state
        .points_service
        .get_balance(identity, &realm_id, user_id)
        .await
        .expect("get_balance must succeed (5 due schedules, all valid)");

    // Count period-1 grant_records across all 5 seeded schedules. The cap
    // guarantees ≤ 3 in this single call.
    let granted_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM points_grant_records
         WHERE user_id = $1 AND realm_id = $2 AND period_number = 1
           AND schedule_id = ANY($3)",
    )
    .bind(user_id)
    .bind(&realm_id)
    .bind(&schedule_ids)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("count grant_records across seeded schedules");

    assert!(
        granted_count <= 3,
        "read-path realization cap (N=3) violated: {} schedules granted in one call, expected ≤ 3",
        granted_count
    );
    assert!(
        granted_count >= 1,
        "read-path realization should have made progress on at least one schedule, got {}",
        granted_count
    );

    // The remaining schedules are still due and will be picked up on a
    // subsequent call — the cap bounds per-call work, not total progress.
    let identity2 = create_test_third_party_identity(&realm_id);
    let _ = ctx
        .app_state
        .points_service
        .get_balance(identity2, &realm_id, user_id)
        .await
        .expect("second get_balance must succeed");
    let granted_count_after_second: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM points_grant_records
         WHERE user_id = $1 AND realm_id = $2 AND period_number = 1
           AND schedule_id = ANY($3)",
    )
    .bind(user_id)
    .bind(&realm_id)
    .bind(&schedule_ids)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("count grant_records after second call");
    assert!(
        granted_count_after_second >= granted_count,
        "second call should not regress granted count"
    );
}

// =============================================================================
// Scenario "兑现 lead_time=0"
// =============================================================================

// User Story: US-FU-004 (realization is a catch-up path, not a pre-grant
// path — it must not grant future periods early).
//
// Covers "lead_time=0: 兑现只补 next_grant_time<=now 的已到期
// (不提前), 提前预生成由 worker PointsPreGrantJob 负责" + "兑现 lead_time=0".
//
// WHY this test exists: the worker pre-grants with a `lead_time_map` (Daily
// = 1h, Monthly = 24h, etc.) — it may write a row for the NEXT period before
// it starts. The read-path realization MUST NOT do this: it only catches up
// already-due periods, never grants ahead. Otherwise a request immediately
// after sign-up would silently grant next month's credits early, breaking
// the period cadence and the analytics anchored to `next_grant_time`. This
// test seeds a schedule with `next_grant_time = now + 1h` and asserts the
// realization path leaves it untouched.
#[test_context(TestContext)]
#[tokio::test]
async fn test_realization_lead_time_zero_no_advance(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id =
        create_test_user(&ctx.app_state.pool, &realm_id, "be-t08-lead-zero@exam.com").await;

    let now = Utc::now();
    let points_per_period: i64 = 100;
    let validity_days: i64 = 30;

    // Schedule is NOT yet due — next_grant_time is 1h in the future.
    let schedule_id = create_free_grant_schedule(
        ctx,
        user_id,
        &realm_id,
        "daily",
        points_per_period,
        validity_days,
        now + Duration::hours(1),
        0,
        "",
    )
    .await;

    let identity = create_test_third_party_identity(&realm_id);
    let balance = ctx
        .app_state
        .points_service
        .get_balance(identity, &realm_id, user_id)
        .await
        .expect("get_balance must succeed (no due schedule ⟹ pure read)");

    // (a) No grant_record written — realization skipped because not due.
    assert!(
        !grant_record_exists(ctx, schedule_id, 1).await,
        "lead_time=0: realization must NOT pre-grant a not-yet-due schedule"
    );

    // (b) Balance stays at 0 — the future-due row was not written.
    assert_eq!(
        balance.free_periodic_balance, 0,
        "no balance should be visible for a not-yet-due schedule"
    );
    assert_derived_balance(ctx, user_id, &realm_id, CreditType::FreePeriodicCredit, 0).await;
}

// =============================================================================
// Scenario "兑现仅 subscription_id IS NULL"
// =============================================================================

// User Story: US-PU-009 (subscription grant fulfillment must NOT be guessed
// by the request path — it relies on event-driven chained pre-grant, not on
// read-path reconciliation).
//
// Covers "订阅不在读路径兑现: reconcile_due_for_user 只选择
// subscription_id IS NULL 的免费周期 schedule" + "兑现仅
// subscription_id IS NULL".
//
// WHY this test exists: subscriptions have a real provider (Stripe/Creem)
// behind them. The request path has no way to know whether a renewal has
// actually been paid — guessing "the schedule is due, so grant it" would
// hand out subscription credits to users whose renewal failed silently, or
// whose webhook is still in flight. The contract therefore routes
// subscriptions through event-driven chained pre-grant
// (handle_subscription_paid writes period N + pre-grants period N+1) and
// EXCLUDES subscription_id IS NOT NULL schedules from read-path realization.
// This test seeds a due subscription schedule and asserts the read path
// leaves it alone.
#[test_context(TestContext)]
#[tokio::test]
async fn test_realization_skips_subscription_schedule(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id =
        create_test_user(&ctx.app_state.pool, &realm_id, "be-t08-skip-sub@exam.com").await;

    let now = Utc::now();
    let points_per_period: i64 = 500;
    let subscription_id = Uuid::now_v7();

    // Seed a SUBSCRIPTION-bound schedule that is due (next_grant_time < now).
    let schedule_id = create_subscription_grant_schedule(
        ctx,
        user_id,
        &realm_id,
        subscription_id,
        "ent:pro-monthly",
        points_per_period,
        now - Duration::hours(1), // due
        now - Duration::hours(1), // base_time
        0,                        // granted_periods
    )
    .await;

    let identity = create_test_third_party_identity(&realm_id);
    let balance = ctx
        .app_state
        .points_service
        .get_balance(identity, &realm_id, user_id)
        .await
        .expect("get_balance must succeed (no due FREE schedule ⟹ pure read)");

    // (a) The subscription schedule was NOT realized by the read path.
    assert!(
        !grant_record_exists(ctx, schedule_id, 1).await,
        "read-path realization must NOT grant a subscription-bound schedule"
    );

    // (b) Subscription-credit balance stays 0 — no ledger row written.
    assert_eq!(
        balance.subscription_balance, 0,
        "subscription_balance must remain 0 — read path does not realize subscription schedules"
    );
    assert_derived_balance(ctx, user_id, &realm_id, CreditType::SubscriptionCredit, 0).await;
}

// =============================================================================
// Scenario: read-path realization write-failure fail-loud — get_balance
// =============================================================================

// User Story: US-FU-004 (system write faults must surface as system errors,
// not be masked as user-visible "low balance").
//
// Covers the pinned failure semantics: if a due schedule exists and the
// realization write fails, get_balance/consume must fail loud — never
// silently degrade to the stale balance or InsufficientBalance.
//
// WHY this test exists: this is the most important defensive assertion in
// the realization feature. If a write fault (DB error, constraint violation,
// lost connection) inside `pregrant_next_period_atomic` were swallowed and
// the read path returned the OLD balance, the user would silently see
// stale data. Worse, if the error were rewritten to `InsufficientBalance`,
// the system would be telling the user "you don't have credits" when in
// reality the SYSTEM failed to grant them — masking an infrastructure
// fault behind a business error. The error contract therefore pins the
// semantics: write failures propagate verbatim as CoreError::DatabaseError
// (HTTP 500), NEVER rewritten to BadRequest("Insufficient...").
//
// HOW the write failure is injected:
//   We seed a due free schedule with `points_per_period = 0`. The schedule
//   itself accepts 0 (its CHECK is `points_per_period >= 0`), but the
//   realization INSERTs a `points_credit_ledger` row with
//   `granted_amount = 0`, which violates the ledger CHECK
//   `granted_amount > 0` (20260211_billing.sql:263). The infra impl
//   (`pregrant_next_period_atomic`) wraps that INSERT in
//   `CoreError::DatabaseError(...)` (postgres_repository.rs:4986 →
//   create_ledger_in_tx). `reconcile_due_for_user` propagates the error
//   unchanged (service.rs:266-287, fail-loud), and `get_balance` returns
//   it to the caller. This is a REAL DB-level constraint violation driving
//   a REAL CoreError::DatabaseError — not a mock.
//
// Failure propagation: service-direct Err. The test calls
// `PointsService::get_balance` directly (the HTTP `get_wallet` route does
// not invoke `reconcile_due_for_user`); the asserted variant
// (`CoreError::DatabaseError`) maps to HTTP 500 INTERNAL_SERVER_ERROR via
// `app_errors.rs::status_code()`. This should be treated as
// service-level fail-loud evidence; HTTP-level fail-loud would require an
// additional test exercising a route that calls reconcile_due_for_user
// (the consume SDK route is the natural candidate — see the next test).
#[test_context(TestContext)]
#[tokio::test]
async fn test_realization_failure_fail_loud_get_balance_5xx(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(
        &ctx.app_state.pool,
        &realm_id,
        "be-t08-fail-loud-get@exam.com",
    )
    .await;

    let now = Utc::now();
    let validity_days: i64 = 30;

    // Seed a due free schedule whose realization will violate the ledger
    // CHECK constraint (granted_amount > 0). See the test-level comment for
    // the full mechanism.
    let schedule_id = create_free_grant_schedule(
        ctx,
        user_id,
        &realm_id,
        "monthly",
        0, // ← drives the INSERT to violate granted_amount > 0
        validity_days,
        now - Duration::hours(1),
        0,
        "",
    )
    .await;

    // The grant_record existence check happens BEFORE the failing INSERT
    // (see pregrant_next_period_atomic step 2). For period 1 no prior
    // grant_record exists, so the impl proceeds to the INSERT and hits the
    // CHECK violation. Sanity: precondition holds.
    assert!(
        !grant_record_exists(ctx, schedule_id, 1).await,
        "precondition: no grant_record yet"
    );

    let identity = create_test_third_party_identity(&realm_id);
    let result = ctx
        .app_state
        .points_service
        .get_balance(identity, &realm_id, user_id)
        .await;

    // (a) get_balance returns Err — it did NOT swallow the realization fault.
    let err = result.expect_err(
        "get_balance MUST fail loud when realization write fails — silently returning the old \
         balance would mask a system fault as stale data",
    );

    // (b) The error variant is DatabaseError (maps to HTTP 500 INTERNAL_SERVER_ERROR).
    //     It is NOT BadRequest (which is the variant `insufficient_points` maps to).
    match &err {
        CoreError::DatabaseError(_) => { /* expected */ }
        CoreError::BadRequest(msg) => panic!(
            "fail-loud violation: get_balance returned BadRequest (400) with message {:?} — \
             DatabaseError (500) was expected. This means the realization write fault was \
             rewritten to a user-visible business error, masking a system fault.",
            msg
        ),
        other => panic!(
            "fail-loud violation: expected CoreError::DatabaseError (maps to HTTP 500), got {:?}. \
             The error variant determines the HTTP status code; only DatabaseError surfaces as 5xx.",
            other
        ),
    }

    // (c) The error message must NOT mention "insufficient" — that wording
    //     is the user-visible signal for "low balance" (InsufficientBalance
    //     semantics). A write fault must not be disguised as such.
    let msg = match err {
        CoreError::DatabaseError(m) => m,
        _ => unreachable!("checked above"),
    };
    let lower = msg.to_lowercase();
    assert!(
        !lower.contains("insufficient"),
        "fail-loud violation: DatabaseError message must not contain 'insufficient' (would \
         masquerade as InsufficientBalance). Got message: {:?}",
        msg
    );

    // (d) No grant_record was written — the transaction rolled back.
    assert!(
        !grant_record_exists(ctx, schedule_id, 1).await,
        "no grant_record should survive a failed realization (transaction must roll back)"
    );
}

// ----------------------------------------------------------------------------
// Scenario: read-path realization write-failure fail-loud — consume entry-point
// ----------------------------------------------------------------------------

// User Story: US-PU-009 (the SDK consume path must also fail loud — it must
// not fabricate an `InsufficientBalance` response when the real cause is a
// realization write fault).
//
// Covers "SDK 消费入口返回既有 infra 错误码 (不产出
// InsufficientBalance)" + "SDK 消费入口同样 fail-loud".
//
// WHY this test exists: the consume path runs `reconcile_due_for_user` and
// THEN opens the consume transaction. If realization fails inside that
//前置 step, the consume path must surface the realization error (5xx), NOT
// proceed and report "insufficient points" (which would be the natural
// response if realization were silently skipped and the user had 0 balance).
// This test seeds the same `points_per_period = 0` fault and asserts the
// consume returns DatabaseError, not insufficient_points / BadRequest.
//
// Failure propagation: service-direct Err. Same mechanism and justification
// as the get_balance fail-loud test above.
#[test_context(TestContext)]
#[tokio::test]
async fn test_realization_failure_consume_fail_loud(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let user_id = create_test_user(
        &ctx.app_state.pool,
        &realm_id,
        "be-t08-fail-loud-consume@exam.com",
    )
    .await;

    let now = Utc::now();
    let validity_days: i64 = 30;

    // Same fault-injection shape as the get_balance test: due schedule with
    // points_per_period = 0 → realization INSERT violates granted_amount > 0.
    let schedule_id = create_free_grant_schedule(
        ctx,
        user_id,
        &realm_id,
        "monthly",
        0,
        validity_days,
        now - Duration::hours(1),
        0,
        "",
    )
    .await;

    // Sanity: precondition (no prior grant_record).
    assert!(
        !grant_record_exists(ctx, schedule_id, 1).await,
        "precondition: no grant_record yet"
    );

    let identity = create_test_third_party_identity(&realm_id);
    let input = ConsumePointsInput {
        user_id: user_id.to_string(),
        client_app_id: ctx._client_app_id.clone(),
        amount: 10, // any positive amount; consume will never reach the
        // availability check because realization fails first
        description: Some("be-t08 consume fail-loud".to_string()),
    };
    let result = ctx
        .app_state
        .points_service
        .consume_points(identity, &realm_id, input)
        .await;

    // (a) consume returns Err — it did NOT swallow the realization fault
    //     and report "insufficient points" (which would be the response if
    //     realization were silently skipped and the user had 0 balance).
    let err = result.expect_err(
        "consume MUST fail loud when the前置 realization write fails — returning InsufficientBalance \
         would disguise the system fault as the user's 'low balance'",
    );

    // (b) The error variant is DatabaseError (HTTP 500), not BadRequest
    //     (the variant `insufficient_points` → 400 maps to).
    match &err {
        CoreError::DatabaseError(_) => { /* expected */ }
        CoreError::BadRequest(msg) => {
            let lower = msg.to_lowercase();
            if lower.contains("insufficient") {
                panic!(
                    "fail-loud violation: consume returned InsufficientBalance (BadRequest 400) \
                     with message {:?} when the actual cause was a realization write fault. \
                     DatabaseError (500) was expected.",
                    msg
                );
            }
            panic!(
                "fail-loud violation: consume returned BadRequest (400) with message {:?}; \
                 DatabaseError (500) was expected for a realization write fault.",
                msg
            );
        }
        other => panic!(
            "fail-loud violation: expected CoreError::DatabaseError (HTTP 500), got {:?}",
            other
        ),
    }

    // (c) No grant_record survived the rollback.
    assert!(
        !grant_record_exists(ctx, schedule_id, 1).await,
        "no grant_record should survive a failed realization"
    );
}
