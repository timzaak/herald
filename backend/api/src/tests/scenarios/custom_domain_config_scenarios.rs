use crate::application::http::server::create_api_routes;
use crate::tests::helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use herald_core::domain::realm_config::ConfigType;
use serde_json::{Value, json};
use test_context::test_context;
use tower::ServiceExt;

const CUSTOM_DOMAIN_PATH: &str = "/api/realms/{realm}/config/custom-domain";

fn custom_domain_uri(realm_id: &str, suffix: &str) -> String {
    format!(
        "{}{}",
        CUSTOM_DOMAIN_PATH.replace("{realm}", realm_id),
        suffix
    )
}

fn authed_request(method: &str, uri: String, token: &str, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));

    let body = if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };

    builder.body(body).unwrap()
}

/// Fetch a raw `custom_domain` `realm_config.config_value` for the context's realm.
async fn fetch_custom_domain_config(ctx: &TestContext, config_key: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = 'custom_domain' AND config_key = $2",
    )
    .bind(&ctx._realm_id)
    .bind(config_key)
    .fetch_optional(&ctx._app_state.pool)
    .await
    .expect("Failed to fetch custom-domain config")
}

/// Insert a `custom_domain_mapping` row for an arbitrary realm.
///
/// `enabled` defaults true (the unified request-time effectiveness predicate).
/// `cname_verified`/`tls_ready` are surface-only and default false. Used to
/// seed cross-realm hostname occupation (409 conflict) and to simulate a
/// Caddy-issued mapping for the ask endpoint.
async fn insert_custom_domain_mapping(
    ctx: &TestContext,
    realm_id: &str,
    hostname: &str,
    enabled: bool,
) {
    sqlx::query(
        "INSERT INTO custom_domain_mapping (realm_id, hostname, enabled, cname_verified, tls_ready, created_at, updated_at)
         VALUES ($1, $2, $3, false, false, NOW(), NOW())
         ON CONFLICT (hostname)
         DO UPDATE SET realm_id = EXCLUDED.realm_id, enabled = EXCLUDED.enabled, updated_at = NOW()",
    )
    .bind(realm_id)
    .bind(hostname)
    .bind(enabled)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to upsert custom-domain mapping");
}

/// Count enabled mapping rows for a realm.
async fn count_mappings(ctx: &TestContext, realm_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM custom_domain_mapping WHERE realm_id = $1")
        .bind(realm_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("Failed to count mappings")
}

/// User Story: US-CD-001 — Realm Admin custom-domain settings must not be routed to another config type.
#[test]
fn custom_domain_config_type_string_mappings_are_registered() {
    assert_eq!(
        ConfigType::try_from_str("custom_domain"),
        Ok(ConfigType::CustomDomain)
    );
    assert_eq!(String::from(ConfigType::CustomDomain), "custom_domain");
    assert_eq!(ConfigType::CustomDomain.as_ref(), "custom_domain");
}

/// User Story: US-CD-001 — Realm Admin opens custom-domain settings before any configuration exists.
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_get_returns_empty_state_when_unconfigured(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "custom-domain-empty@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let req = authed_request("GET", custom_domain_uri(&ctx._realm_id, ""), &token, None);

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(resp).await;
    // published is CustomDomainConfig::default() — a non-null object with a null hostname.
    assert_eq!(body["published"]["hostname"], Value::Null);
    // No draft / has_previous fields exist in the simplified single-state model.
    assert!(body.get("draft").is_none());
    assert!(body.get("hasPrevious").is_none());
    // cnameTarget is a global config string (empty in the test context) but must be present.
    assert!(body.get("cnameTarget").is_some());
    assert!(body["status"].is_null());
}

/// User Story: US-CD-001 — Regular users cannot view Realm Admin custom-domain state.
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_get_requires_view_and_forbids_plain_user(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, _user_id) =
        create_admin_session_with_user(ctx, "custom-domain-view-plain@test.com", 1800).await;

    let req = authed_request("GET", custom_domain_uri(&ctx._realm_id, ""), &token, None);

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// User Story: US-CD-001 / US-CD-004 — Custom-domain settings are isolated to the user's own Realm.
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_cross_realm_access_is_forbidden(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "custom-domain-cross-realm@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;
    let other_realm_id = uuid::Uuid::now_v7().to_string();

    let get_req = authed_request("GET", custom_domain_uri(&other_realm_id, ""), &token, None);
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::FORBIDDEN);

    let put_req = authed_request(
        "PUT",
        custom_domain_uri(&other_realm_id, ""),
        &token,
        Some(json!({ "hostname": "login.other-realm.com" })),
    );
    let put_resp = app.oneshot(put_req).await.unwrap();
    assert_eq!(put_resp.status(), StatusCode::FORBIDDEN);
}

/// User Story: US-CD-001 — Updating custom-domain config requires `settings.manage` and
/// writes both the settings row and the host→realm mapping in one step.
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_update_requires_manage_and_writes_mapping_and_settings(
    ctx: &mut TestContext,
) {
    let app = ctx.create_unified_test_router();
    let (plain_token, _plain_user_id) =
        create_admin_session_with_user(ctx, "custom-domain-update-plain@test.com", 1800).await;
    let body = json!({ "hostname": "login.example.com" });

    // Without settings.manage → 403.
    let forbidden_req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &plain_token,
        Some(body.clone()),
    );
    let forbidden_resp = app.clone().oneshot(forbidden_req).await.unwrap();
    assert_eq!(forbidden_resp.status(), StatusCode::FORBIDDEN);

    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "custom-domain-update-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &admin_token,
        Some(body),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp_body: Value = crate::tests::response_json(resp).await;
    assert_eq!(resp_body["message"], "Custom-domain configuration updated");
    // update writes the mapping with status pending (enabled, not yet verified).
    assert_eq!(resp_body["status"]["cnameVerified"], false);
    assert_eq!(resp_body["status"]["tlsReady"], false);

    // settings now holds the hostname.
    let settings = fetch_custom_domain_config(ctx, "settings")
        .await
        .expect("settings row must exist after update");
    assert!(settings.contains("login.example.com"));

    // The host→realm mapping reflects the configured hostname in its pending state.
    let row: Option<(bool, bool)> = sqlx::query_as(
        "SELECT cname_verified, tls_ready FROM custom_domain_mapping
         WHERE realm_id = $1 AND hostname = $2",
    )
    .bind(&ctx._realm_id)
    .bind("login.example.com")
    .fetch_optional(&ctx._app_state.pool)
    .await
    .expect("Failed to fetch mapping");
    let (cname_verified, tls_ready) =
        row.expect("update must write a mapping row for the hostname");
    assert!(
        !cname_verified && !tls_ready,
        "freshly written mapping must start pending (cname_verified=false, tls_ready=false)"
    );

    // No draft / previous_settings rows are created.
    assert!(fetch_custom_domain_config(ctx, "draft").await.is_none());
    assert!(
        fetch_custom_domain_config(ctx, "previous_settings")
            .await
            .is_none()
    );
}

/// User Story: US-CD-001 / US-CD-004 — Realm Admin cannot save an unsafe or malformed hostname.
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_update_rejects_invalid_hostname(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "custom-domain-validation@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Wildcard hostnames are rejected.
    let wildcard_req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &token,
        Some(json!({ "hostname": "*.example.com" })),
    );
    let wildcard_resp = app.clone().oneshot(wildcard_req).await.unwrap();
    assert_eq!(wildcard_resp.status(), StatusCode::BAD_REQUEST);

    // Hostnames with a port are rejected.
    let port_req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &token,
        Some(json!({ "hostname": "login.example.com:8443" })),
    );
    let port_resp = app.clone().oneshot(port_req).await.unwrap();
    assert_eq!(port_resp.status(), StatusCode::BAD_REQUEST);

    // A scheme-prefixed URL is rejected (caller pasted a full URL, not a hostname).
    let scheme_req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &token,
        Some(json!({ "hostname": "https://login.example.com" })),
    );
    let scheme_resp = app.clone().oneshot(scheme_req).await.unwrap();
    assert_eq!(scheme_resp.status(), StatusCode::BAD_REQUEST);

    // A valid mixed-case hostname with a trailing dot is accepted and normalized
    // (lowercased, trailing dot stripped).
    let normalized_req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &token,
        Some(json!({ "hostname": "Login.Example.COM." })),
    );
    let normalized_resp = app.oneshot(normalized_req).await.unwrap();
    assert_eq!(normalized_resp.status(), StatusCode::OK);

    let settings = fetch_custom_domain_config(ctx, "settings")
        .await
        .expect("settings row must exist after update");
    assert!(settings.contains("login.example.com"));
}

/// User Story: US-CD-001 — A custom-domain hostname is globally unique across all Realms.
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_update_409_on_hostname_taken_across_realms(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "custom-domain-conflict@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Another realm already occupies the hostname via a mapping row.
    let other_realm_id = uuid::Uuid::now_v7().to_string();
    insert_custom_domain_mapping(ctx, &other_realm_id, "taken.example.com", true).await;

    let req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &token,
        Some(json!({ "hostname": "taken.example.com" })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

/// User Story: US-CD-001 — Re-saving a different hostname switches the mapping; the
/// superseded hostname row is removed (at-most-one-enabled-row-per-realm invariant).
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_update_switches_mapping_to_new_hostname(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "custom-domain-switch@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // First save configures the old hostname.
    let first_req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &token,
        Some(json!({ "hostname": "old.example.com" })),
    );
    let first_resp = app.clone().oneshot(first_req).await.unwrap();
    assert_eq!(first_resp.status(), StatusCode::OK);

    // Second save switches to a new hostname.
    let second_req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &token,
        Some(json!({ "hostname": "new.example.com" })),
    );
    let second_resp = app.clone().oneshot(second_req).await.unwrap();
    assert_eq!(second_resp.status(), StatusCode::OK);

    // The old hostname mapping is gone; the new one is the realm's single mapping.
    let old_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM custom_domain_mapping WHERE realm_id = $1 AND hostname = $2",
    )
    .bind(&ctx._realm_id)
    .bind("old.example.com")
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to count old mapping");
    assert_eq!(
        old_count, 0,
        "switching hostname must delete the old mapping"
    );

    let new_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM custom_domain_mapping WHERE realm_id = $1 AND hostname = $2",
    )
    .bind(&ctx._realm_id)
    .bind("new.example.com")
    .fetch_one(&ctx._app_state.pool)
    .await
    .expect("Failed to count new mapping");
    assert_eq!(
        new_count, 1,
        "switching hostname must write the new mapping"
    );
}

/// User Story: US-CD-001 — Clearing the hostname (null/empty) removes the realm's
/// mapping rows entirely (the realm no longer has a custom domain).
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_update_clearing_hostname_removes_mapping(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "custom-domain-clear@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // Seed an existing hostname.
    let seed_req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &token,
        Some(json!({ "hostname": "seed.example.com" })),
    );
    let seed_resp = app.clone().oneshot(seed_req).await.unwrap();
    assert_eq!(seed_resp.status(), StatusCode::OK);
    assert_eq!(count_mappings(ctx, &ctx._realm_id).await, 1);

    // Clear the hostname with null.
    let clear_req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &token,
        Some(json!({ "hostname": null })),
    );
    let clear_resp = app.oneshot(clear_req).await.unwrap();
    assert_eq!(clear_resp.status(), StatusCode::OK);

    let resp_body: Value = crate::tests::response_json(clear_resp).await;
    assert_eq!(resp_body["status"], Value::Null);

    assert_eq!(
        count_mappings(ctx, &ctx._realm_id).await,
        0,
        "clearing hostname must remove all mapping rows for the realm"
    );

    // settings reflects the cleared state (null hostname).
    let settings = fetch_custom_domain_config(ctx, "settings")
        .await
        .expect("settings row must still exist after clear");
    assert!(settings.contains("null"));
}

/// User Story: US-CD-002 — Updating a custom-domain config must write the hostname into the
/// `custom_domain_mapping` table so Caddy On-Demand TLS issuance (the ask endpoint)
/// and future per-realm lookups reflect it.
///
/// Note: the public host→realmId resolve endpoint and per-domain URL generation
/// were reverted; the retained read surface for the mapping table is the Caddy
/// ask endpoint (`GET /api/internal/custom-domain/authorize`), which we use here
/// to confirm the configured hostname was committed and is effective.
#[test_context(TestContext)]
#[tokio::test]
async fn custom_domain_update_writes_mapping_visible_to_ask_endpoint(ctx: &mut TestContext) {
    // ask-key-gated router: the authorize endpoint needs a non-empty configured
    // key to return 200 (the default test context key is empty → always 401).
    let mut state = (*ctx._app_state).clone();
    state.custom_domain_ask_key = "test-ask-shared-secret".to_string();
    let ask_key = state.custom_domain_ask_key.clone();
    let app = create_api_routes(std::sync::Arc::new(state.clone())).with_state(state);

    let (token, user_id) =
        create_admin_session_with_user(ctx, "custom-domain-update-mapping@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;
    let hostname = "login.update-mapping-example.com";

    let update_req = authed_request(
        "PUT",
        custom_domain_uri(&ctx._realm_id, ""),
        &token,
        Some(json!({ "hostname": hostname })),
    );
    let update_resp = app.clone().oneshot(update_req).await.unwrap();
    assert_eq!(update_resp.status(), StatusCode::OK);

    // The Caddy ask endpoint (retained read surface) must now authorize the
    // just-configured hostname — proving the update wrote the mapping row.
    let ask_uri = format!("/api/internal/custom-domain/authorize?host={hostname}");
    let ask_req = Request::builder()
        .method("GET")
        .uri(ask_uri)
        .header("x-herald-ask-key", &ask_key)
        .body(Body::empty())
        .unwrap();
    let ask_resp = app.oneshot(ask_req).await.unwrap();
    assert_eq!(ask_resp.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(ask_resp).await;
    assert_eq!(body["authorized"], true);
}

#[test_context(TestContext)]
#[tokio::test]
async fn dream_check_custom_domain_save_and_clear_are_audited(ctx: &mut TestContext) {
    let token = crate::tests::helpers::billing_helpers::setup_billing_admin_session(
        ctx,
        "dream-domain@test.com",
    )
    .await;
    let app = ctx.create_unified_test_router();
    for hostname in ["login.example.com", ""] {
        let response = app
            .clone()
            .oneshot(authed_request(
                "PUT",
                custom_domain_uri(&ctx._realm_id, ""),
                &token,
                Some(json!({"hostname": hostname})),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let operations: Vec<String> = sqlx::query_scalar("SELECT details->>'operation' FROM audit_events WHERE realm_id = $1 AND action = 'realm_config.update' AND details->>'config_type' = 'custom_domain' ORDER BY created_at")
        .bind(&ctx._realm_id).fetch_all(&ctx.app_state.pool).await.unwrap();
    assert_eq!(
        operations,
        vec!["saved", "cleared"],
        "both domain ownership changes must be auditable"
    );
}
