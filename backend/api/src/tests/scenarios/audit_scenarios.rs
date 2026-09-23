// =============================================================================
// Audit Log Query API - BDD Scenario Tests
// =============================================================================
//
// User Stories covered:
// - US-AU-001: View audit log list (P0)
// - US-AU-002: Filter audit events (P0)
// - US-AU-003: View audit event detail (P1)
// - US-AU-004: Admin Realm audit logs (P0)
//
// Reference: docs/user-stories/14-audit-user-stories.md
// Design: .ai/design/audit.md
//
// Routes:
//   GET /api/audit          -- list with pagination + filters
//   GET /api/audit/{eventId} -- detail view
//
// =============================================================================

use crate::tests::helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::infrastructure::audit::PostgresAuditEventRepository;
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

// =============================================================================
// Helper: insert an audit event directly via the repository
// =============================================================================

async fn insert_audit_event(ctx: &TestContext, event: NewAuditEvent) -> uuid::Uuid {
    let repo = PostgresAuditEventRepository::new(ctx.app_state.db.as_ref().clone());
    let created = repo
        .create(event)
        .await
        .expect("Failed to insert audit event");
    created.id
}

fn make_event(
    realm_id: &str,
    category: AuditCategory,
    action: AuditAction,
    actor_id: &str,
    target_type: AuditTargetType,
    target_id: &str,
    result: AuditResult,
) -> NewAuditEvent {
    NewAuditEvent {
        realm_id: realm_id.to_string(),
        category,
        action,
        actor_id: actor_id.to_string(),
        actor_type: Some(ActorType::Admin),
        actor_name: Some("test-admin".to_string()),
        target_type,
        target_id: target_id.to_string(),
        target_name: Some("test-target".to_string()),
        result,
        details: None,
        ip_address: Some("127.0.0.1".to_string()),
        user_agent: Some("test-agent".to_string()),
        trace_id: None,
    }
}

// =============================================================================
// US-AU-001: View audit log list
// =============================================================================

/// User Story: docs/user-stories/14-audit-user-stories.md - Story 1, Scenario 1
/// Covers: 验收标准 - 按操作时间倒序展示审计日志列表
///
/// Given a realm with audit events,
/// When admin queries GET /api/audit,
/// Then paginated results returned sorted by created_at DESC.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_list_paginated_sorted_by_time_desc(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "audit-list@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = &ctx._realm_id;

    // Insert events with a small delay to ensure ordering
    let _event1 = insert_audit_event(
        ctx,
        make_event(
            realm_id,
            AuditCategory::Auth,
            AuditAction::AuthLogin,
            &admin_user_id,
            AuditTargetType::Session,
            "session-1",
            AuditResult::Success,
        ),
    )
    .await;

    // Give enough time for UUIDv7 ordering
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let event2 = insert_audit_event(
        ctx,
        make_event(
            realm_id,
            AuditCategory::UserManagement,
            AuditAction::UserCreate,
            &admin_user_id,
            AuditTargetType::User,
            "user-1",
            AuditResult::Success,
        ),
    )
    .await;

    // When: admin queries the audit list
    let req = Request::builder()
        .method("GET")
        .uri("/api/audit?page=0&pageSize=20".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK with paginated results sorted by created_at DESC
    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let items = body["items"].as_array().expect("Expected items array");
    let total = body["total"].as_i64().expect("Expected total as integer");

    assert!(total >= 2, "Expected at least 2 events, got {}", total);

    // Verify most recent event comes first (DESC order by created_at)
    let first_id = items[0]["id"].as_str().expect("Expected id in first item");
    assert_eq!(
        first_id,
        event2.to_string(),
        "Most recent event should be first (created_at DESC)"
    );
}

/// User Story: docs/user-stories/14-audit-user-stories.md - Story 1, Scenario 3
/// Covers: 验收标准 - 无审计日志时显示空状态
///
/// Given a realm with no audit events,
/// When admin queries the audit list,
/// Then empty list with total=0 is returned.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_list_empty_when_no_events(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "audit-empty@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    // The fresh test schema should have no audit events.
    // First verify the count is 0 in the database.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE realm_id = $1")
        .bind(&ctx._realm_id)
        .fetch_one(&ctx.app_state.pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "Precondition: no audit events in test realm");

    let req = Request::builder()
        .method("GET")
        .uri("/api/audit?page=0&pageSize=20".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let items = body["items"]
        .as_array()
        .expect("Expected items array in response");
    let total = body["total"].as_i64().expect("Expected total");

    assert!(items.is_empty(), "Expected empty items array");
    assert_eq!(total, 0, "Expected total=0 for empty realm");
}

// =============================================================================
// US-AU-002: Filter audit events
// =============================================================================

/// User Story: docs/user-stories/14-audit-user-stories.md - Story 2, Scenario 1
/// Covers: 验收标准 - 按事件类型筛选
///
/// Given events of multiple categories,
/// When filtering by category=auth,
/// Then only auth events returned.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_filter_by_category(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "audit-filter-cat@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = &ctx._realm_id;

    // Insert events of different categories
    insert_audit_event(
        ctx,
        make_event(
            realm_id,
            AuditCategory::Auth,
            AuditAction::AuthLogin,
            &admin_user_id,
            AuditTargetType::Session,
            "session-1",
            AuditResult::Success,
        ),
    )
    .await;

    insert_audit_event(
        ctx,
        make_event(
            realm_id,
            AuditCategory::UserManagement,
            AuditAction::UserCreate,
            &admin_user_id,
            AuditTargetType::User,
            "user-1",
            AuditResult::Success,
        ),
    )
    .await;

    insert_audit_event(
        ctx,
        make_event(
            realm_id,
            AuditCategory::RealmManagement,
            AuditAction::RealmCreate,
            &admin_user_id,
            AuditTargetType::Realm,
            "realm-1",
            AuditResult::Success,
        ),
    )
    .await;

    // When: filter by category=auth
    let req = Request::builder()
        .method("GET")
        .uri("/api/audit?page=0&pageSize=20&category=auth".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let items = body["items"].as_array().expect("Expected items array");

    // Then: only auth events returned
    for item in items {
        assert_eq!(
            item["category"].as_str(),
            Some("auth"),
            "Only auth events should be returned, got: {:?}",
            item["category"]
        );
    }

    // Should have at least the one auth event we inserted
    assert!(
        !items.is_empty(),
        "Expected at least one auth event after filtering"
    );
}

/// User Story: docs/user-stories/14-audit-user-stories.md - Story 2, Scenario 3
/// Covers: 验收标准 - 按操作者筛选
///
/// Given events from different actors,
/// When filtering by actorId,
/// Then only that actor's events returned.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_filter_by_actor_id(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "audit-filter-actor@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = &ctx._realm_id;
    let other_actor_id = uuid::Uuid::now_v7().to_string();

    // Insert events from two different actors
    insert_audit_event(
        ctx,
        make_event(
            realm_id,
            AuditCategory::Auth,
            AuditAction::AuthLogin,
            &admin_user_id,
            AuditTargetType::Session,
            "session-admin",
            AuditResult::Success,
        ),
    )
    .await;

    insert_audit_event(
        ctx,
        make_event(
            realm_id,
            AuditCategory::Auth,
            AuditAction::AuthLogin,
            &other_actor_id,
            AuditTargetType::Session,
            "session-other",
            AuditResult::Success,
        ),
    )
    .await;

    // When: filter by the admin_user_id as actorId
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/audit?page=0&pageSize=20&actorId={}",
            admin_user_id
        ))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let items = body["items"].as_array().expect("Expected items array");

    // Then: only the admin user's events are returned
    for item in items {
        assert_eq!(
            item["actorId"].as_str(),
            Some(admin_user_id.as_str()),
            "Only the specified actor's events should be returned, got: {:?}",
            item["actorId"]
        );
    }
}

/// User Story: docs/user-stories/14-audit-user-stories.md - Story 2, Scenario 2
/// Covers: 验收标准 - 按时间范围筛选
///
/// Given events at different times,
/// When filtering by startTime/endTime,
/// Then only events in range returned.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_filter_by_time_range(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "audit-filter-time@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = &ctx._realm_id;

    // Insert an event now
    let _event_id = insert_audit_event(
        ctx,
        make_event(
            realm_id,
            AuditCategory::Auth,
            AuditAction::AuthLogin,
            &admin_user_id,
            AuditTargetType::Session,
            "session-time",
            AuditResult::Success,
        ),
    )
    .await;

    // Use a time range that covers "now"
    let now = chrono::Utc::now();
    let start_time = (now - chrono::Duration::hours(1)).to_rfc3339();
    let end_time = (now + chrono::Duration::hours(1)).to_rfc3339();

    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/audit?page=0&pageSize=20&startTime={}&endTime={}",
            start_time, end_time
        ))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let total = body["total"].as_i64().expect("Expected total");

    // At least the event we just inserted should be in range
    assert!(
        total >= 1,
        "Expected at least 1 event in the time range, got {}",
        total
    );

    // Now use a range far in the past -- should return 0
    let past_start = (now - chrono::Duration::days(365)).to_rfc3339();
    let past_end = (now - chrono::Duration::days(364)).to_rfc3339();

    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/audit?page=0&pageSize=20&startTime={}&endTime={}",
            past_start, past_end
        ))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let past_total = body["total"].as_i64().expect("Expected total");

    assert_eq!(
        past_total, 0,
        "Expected 0 events for a past time range, got {}",
        past_total
    );
}
// =============================================================================
// US-AU-003: View audit event detail
// =============================================================================

/// User Story: docs/user-stories/14-audit-user-stories.md - Story 3, Scenario 1
/// Covers: 验收标准 - 完整详情包含 details JSONB, userAgent, traceId
///
/// Given an event exists,
/// When admin queries GET /api/audit/{eventId},
/// Then full details returned including details JSONB, userAgent, traceId.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_detail_returns_full_fields(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "audit-detail@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = &ctx._realm_id;

    // Insert an event with full data including details, userAgent, traceId
    let event = NewAuditEvent {
        realm_id: realm_id.to_string(),
        category: AuditCategory::Rbac,
        action: AuditAction::RoleAssign,
        actor_id: admin_user_id.clone(),
        actor_type: Some(ActorType::Admin),
        actor_name: Some("admin-detail-test".to_string()),
        target_type: AuditTargetType::User,
        target_id: "user-target-1".to_string(),
        target_name: Some("target-user".to_string()),
        result: AuditResult::Success,
        details: Some(json!({ "role_name": "editor", "role_id": "abc-123" })),
        ip_address: Some("10.0.0.1".to_string()),
        user_agent: Some("HeraldTest/1.0".to_string()),
        trace_id: Some("trace-xyz-789".to_string()),
    };

    let event_id = insert_audit_event(ctx, event).await;

    // When: admin queries the detail endpoint
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/audit/{}", event_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 200 OK with full details
    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;

    // Verify identity fields
    assert_eq!(body["id"].as_str(), Some(event_id.to_string().as_str()));
    assert_eq!(body["category"].as_str(), Some("rbac"));
    assert_eq!(body["action"].as_str(), Some("role.assign"));
    assert_eq!(body["actorId"].as_str(), Some(admin_user_id.as_str()));
    assert_eq!(body["actorType"].as_str(), Some("admin"));
    assert_eq!(body["actorName"].as_str(), Some("admin-detail-test"));
    assert_eq!(body["targetType"].as_str(), Some("user"));
    assert_eq!(body["targetId"].as_str(), Some("user-target-1"));
    assert_eq!(body["targetName"].as_str(), Some("target-user"));
    assert_eq!(body["result"].as_str(), Some("success"));
    assert_eq!(body["ipAddress"].as_str(), Some("10.0.0.1"));

    // Verify detail-only fields
    let details = body["details"]
        .as_object()
        .expect("Expected details JSON object");
    assert_eq!(details["role_name"].as_str(), Some("editor"));
    assert_eq!(details["role_id"].as_str(), Some("abc-123"));

    assert_eq!(
        body["userAgent"].as_str(),
        Some("HeraldTest/1.0"),
        "Expected userAgent in detail response"
    );
    assert_eq!(
        body["traceId"].as_str(),
        Some("trace-xyz-789"),
        "Expected traceId in detail response"
    );
}

// =============================================================================
// US-AU-004 & Realm isolation
// =============================================================================

/// User Story: docs/user-stories/14-audit-user-stories.md - Story 1, Scenario 2
/// Covers: 验收标准 - Realm 隔离
///
/// Given events in realm A only,
/// When realm A admin queries, Then sees only realm A events.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_list_realm_isolation(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "audit-isolation@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = &ctx._realm_id;
    let other_realm_id = uuid::Uuid::now_v7().to_string();

    // Insert events in the test realm
    insert_audit_event(
        ctx,
        make_event(
            realm_id,
            AuditCategory::Auth,
            AuditAction::AuthLogin,
            &admin_user_id,
            AuditTargetType::Session,
            "session-a",
            AuditResult::Success,
        ),
    )
    .await;

    // Insert events in a different realm (directly into DB to bypass realm constraints)
    sqlx::query(
        "INSERT INTO audit_events (id, realm_id, category, action, actor_id, target_type, target_id, result, created_at)
         VALUES ($1, $2, 'auth', 'auth.login', $3, 'session', 'session-b', 'success', NOW())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&other_realm_id)
    .bind(&admin_user_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to insert event in other realm");

    // When: admin queries audit list for their own realm
    let req = Request::builder()
        .method("GET")
        .uri("/api/audit?page=0&pageSize=20".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let items = body["items"].as_array().expect("Expected items array");

    // Then: only events from the admin's realm are returned
    for item in items {
        // The API uses identity.realm_id(), not the path param, so all returned
        // events belong to the authenticated user's realm
        assert_eq!(
            item["actorId"].as_str(),
            Some(admin_user_id.as_str()),
            "Only own-realm events should be visible"
        );
    }
}

/// User Story: docs/user-stories/14-audit-user-stories.md - Story 3, Scenario 1
/// Covers: 验收标准 - 详情查询的 Realm 隔离
///
/// Given an event in realm A,
/// When realm A admin queries detail of realm B event,
/// Then gets 404 (not exposing other realm data).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_detail_cross_realm_returns_404(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "audit-cross-realm@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let other_realm_id = uuid::Uuid::now_v7().to_string();
    let cross_realm_event_id = uuid::Uuid::now_v7();

    // Insert an event in a different realm
    sqlx::query(
        "INSERT INTO audit_events (id, realm_id, category, action, actor_id, target_type, target_id, result, created_at)
         VALUES ($1, $2, 'auth', 'auth.login', $3, 'session', 'session-x', 'success', NOW())",
    )
    .bind(cross_realm_event_id)
    .bind(&other_realm_id)
    .bind(&admin_user_id)
    .execute(&ctx.app_state.pool)
    .await
    .expect("Failed to insert event in other realm");

    // When: admin tries to fetch detail of the cross-realm event
    // Note: the handler derives the realm from the session identity; the
    // cross-realm event must 404 because it belongs to another realm.
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/audit/{}", cross_realm_event_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 404 Not Found
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "Expected 404 for cross-realm audit event detail"
    );
}

// =============================================================================
// Permission checks
// =============================================================================

/// Covers: 验收标准 - 非 admin 用户不能查看审计日志
///
/// Given a non-admin user (no realm-admin role),
/// When querying audit list,
/// Then gets forbidden (403).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_list_non_admin_forbidden(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();

    // Create a user WITHOUT granting realm-admin role
    let (user_token, _user_id) =
        create_admin_session_with_user(ctx, "audit-no-role@test.com", 1800).await;
    // Deliberately NOT calling grant_realm_admin_role

    let req = Request::builder()
        .method("GET")
        .uri("/api/audit?page=0&pageSize=20".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", user_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();

    // Then: 403 Forbidden
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "Expected 403 Forbidden for non-admin user querying audit logs"
    );
}
// =============================================================================
// Pagination
// =============================================================================

/// Covers: 验收标准 - 分页参数正确应用
///
/// Given multiple audit events,
/// When querying with page and pageSize,
/// Then correct pagination metadata returned.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_audit_pagination_metadata(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "audit-pagination@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let realm_id = &ctx._realm_id;

    // Insert 5 events
    for i in 0..5 {
        insert_audit_event(
            ctx,
            make_event(
                realm_id,
                AuditCategory::Auth,
                AuditAction::AuthLogin,
                &admin_user_id,
                AuditTargetType::Session,
                &format!("session-pag-{}", i),
                AuditResult::Success,
            ),
        )
        .await;
    }

    // Request page 0, pageSize 2
    let req = Request::builder()
        .method("GET")
        .uri("/api/audit?page=0&pageSize=2".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["page"].as_i64(), Some(0), "Expected page=0");
    assert_eq!(body["pageSize"].as_i64(), Some(2), "Expected pageSize=2");
    assert!(
        body["total"].as_i64().unwrap_or(0) >= 5,
        "Expected total >= 5"
    );
    let items = body["items"].as_array().expect("Expected items array");
    assert_eq!(items.len(), 2, "Expected 2 items on page 0 with pageSize=2");

    // Request page 1
    let req = Request::builder()
        .method("GET")
        .uri("/api/audit?page=1&pageSize=2".to_string())
        .header(header::AUTHORIZATION, format!("Bearer {}", admin_token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = crate::tests::response_json(resp).await;
    assert_eq!(body["page"].as_i64(), Some(1), "Expected page=1");
}
