// =============================================================================
// Scenario Tests: Legal domain HTTP API (public + self-service consent + admin)
// =============================================================================
//
// Exercises the legal HTTP endpoints end-to-end over the unified test router
// (`ctx.create_unified_test_router()` + `tower::ServiceExt::oneshot`). All tables
// come from the BE-D01 migration (`legal_agreement_version` +
// `user_agreement_consent` + the seeded platform-default rows for both
// agreement types); NO second DDL is maintained here.
//
// User stories (docs/user-stories/core/legal-consent-account-deletion.md):
//   - US-RU-012  version change → reconsent
//   - US-RU-013  public view of agreements (no login)
//   - US-RU-015  consent to current version (204 / 409 stale / 401 unauth)
//   - US-RA-019  admin publish/revert/view + cross-realm isolation + perms
//
// Wire format: `legal/mod.rs` DTOs serialize as **snake_case** JSON field names
// (`version_id`, `version_no`, `effective_at`, `agreement_type`,
// `version_label`, `needs_reconsent`, `current_version_id`,
// `consented_version_id`, `source`). Assert JSON bodies with snake_case keys.
// =============================================================================

use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use herald_core::domain::authorization::{PermissionService, principal_types};
use serde_json::{Value, json};
use test_context::test_context;
use tower::ServiceExt;
use uuid::Uuid;

// =============================================================================
// Helpers
// =============================================================================

/// Build a bare (unauthenticated) request to the unified test router.
///
/// Public legal endpoints (`/api/legal/{realmId}/agreements*`) take no
/// Bearer authentication, so this is the canonical request shape for them.
fn build_request(method: &str, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::empty())
        .expect("failed to build request")
}

/// Build an authenticated request with an Authorization Bearer token.
///
/// Used for self-service consent (Bearer authentication) and admin (Bearer
/// authentication + require_permission) endpoints — both gated on a valid token.
fn authed_request(method: &str, path: &str, token: &str, body: Option<String>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("x-forwarded-for", "203.0.113.10")
        .header(header::USER_AGENT, "legal-http-scene-test/1.0");
    if let Some(b) = body {
        builder = builder.header("content-length", b.len().to_string());
        builder
            .body(Body::from(b))
            .expect("failed to build bodyful request")
    } else {
        builder
            .body(Body::empty())
            .expect("failed to build bodyless request")
    }
}

/// Grant a single permission policy (`settings`, `view`|`manage`) to a user by
/// creating a dedicated role with that one policy and binding the user to it.
///
/// Mirrors the `permission_security_scenarios.rs` pattern: realm-admin carries
/// every permission (too coarse for permission-boundary tests), so we mint a
/// bespoke role carrying exactly the requested `settings.<action>` policy. Cache
/// is invalidated so the grant takes effect on the next request.
async fn grant_settings_role(ctx: &TestContext, user_id: &str, realm_id: &str, action: &str) {
    let role_name = format!("legal-settings-{action}-role");
    let maybe_role_id: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM roles WHERE realm_id = $1 AND name = $2")
            .bind(realm_id)
            .bind(&role_name)
            .fetch_optional(&ctx.app_state.pool)
            .await
            .expect("failed to look up settings role");

    let role_id = match maybe_role_id {
        Some(id) => id,
        None => {
            // Role does not exist yet — create it with the single settings.<action>
            // policy inline (no ON CONFLICT: this is a fresh schema-isolated test).
            let client_id = sqlx::query_scalar::<_, String>(
                "SELECT client_id FROM client_app WHERE realm_id = $1 AND client_id = 'admin-web-console' LIMIT 1",
            )
            .bind(realm_id)
            .fetch_one(&ctx.app_state.pool)
            .await
            .expect("admin-web-console client_app must exist for realm");

            let new_role_id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
                 VALUES ($1, $2, $3, $4, $5, false)",
            )
            .bind(new_role_id)
            .bind(&role_name)
            .bind(format!(
                "Role with settings.{action} only (legal scene tests)"
            ))
            .bind(realm_id)
            .bind(&client_id)
            .execute(&ctx.app_state.pool)
            .await
            .expect("failed to create settings role");

            sqlx::query(
                "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
                 VALUES ($1, $2, $3, 'settings', $4)",
            )
            .bind(Uuid::now_v7())
            .bind(new_role_id)
            .bind(realm_id)
            .bind(action)
            .execute(&ctx.app_state.pool)
            .await
            .expect("failed to add settings policy");

            new_role_id
        }
    };

    let user_uuid =
        Uuid::parse_str(user_id).expect("user_id must be a valid uuid in grant_settings_role");
    let client_id = sqlx::query_scalar::<_, String>(
        "SELECT client_id FROM client_app WHERE realm_id = $1 AND client_id = 'admin-web-console' LIMIT 1",
    )
    .bind(realm_id)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("admin-web-console client_app must exist for realm");

    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
         VALUES ($1, $2, $3, $4, $5, $6, $2::text)
         ON CONFLICT DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .bind(user_uuid)
    .bind(role_id)
    .bind(realm_id)
    .bind(&client_id)
    .bind(principal_types::USER)
    .execute(&ctx.app_state.pool)
    .await
    .expect("failed to bind user to settings role");

    let _ = ctx
        .app_state
        .permission_checker
        .invalidate_user_role_cache(realm_id, user_id)
        .await;
}

/// Seed a second realm (B) plus its `admin-web-console` client_app so a user
/// belonging to realm-A has a real realm-B row to be refused access to.
///
/// Returns realm-B's id. Does NOT create any agreement rows for realm-B, so its
/// effective agreement resolution falls back to the platform default templates.
async fn seed_extra_realm(ctx: &TestContext, realm_id2: &str) {
    sqlx::query(
        "INSERT INTO realm (id, name) VALUES ($1, $2)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(realm_id2)
    .bind(format!("Legal isolation realm {realm_id2}"))
    .execute(&ctx.app_state.pool)
    .await
    .expect("failed to seed extra realm");

    // The FirstParty admin console must exist for the extra realm.
    sqlx::query(
        "INSERT INTO client_app (id, realm_id, client_id, name, description, redirect_uris, enabled, browser_refresh_absolute_ttl_seconds, client_secret, is_first_party)
         VALUES ($1, $2, 'admin-web-console', 'legal-scene-test', 'Legal scene test console', '[\"http://localhost:3000/callback\"]'::jsonb, true, 86400, 'test-secret', true)
         ON CONFLICT (realm_id, client_id) DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .bind(realm_id2)
    .execute(&ctx.app_state.pool)
    .await
    .expect("failed to seed admin-web-console for extra realm");
}

/// Find the consent-status item for `agreement_type` inside a parsed
/// `ConsentStatusResponse.items` array. Panics with a clear message if absent.
fn status_item<'a>(body: &'a Value, agreement_type: &str) -> &'a Value {
    body["items"]
        .as_array()
        .unwrap_or_else(|| panic!("consent/status body missing `items` array: {body}"))
        .iter()
        .find(|v| v["agreement_type"].as_str() == Some(agreement_type))
        .unwrap_or_else(|| panic!("consent/status missing item for {agreement_type}: {body}"))
}

/// Find the admin agreement view for `agreement_type` inside a parsed
/// `AdminAgreementsResponse.agreements` array.
fn admin_view<'a>(body: &'a Value, agreement_type: &str) -> &'a Value {
    body["agreements"]
        .as_array()
        .unwrap_or_else(|| panic!("admin body missing `agreements` array: {body}"))
        .iter()
        .find(|v| v["agreement_type"].as_str() == Some(agreement_type))
        .unwrap_or_else(|| panic!("admin body missing view for {agreement_type}: {body}"))
}

/// Return the highest `version_no` of custom (`source = 'custom'`) agreements
/// for `(realm_id, agreement_type)`. Returns 0 when no custom version exists yet.
///
/// WHY: `version_no` is scoped by `(COALESCE(realm_id,''), agreement_type)`,
/// so a realm's first custom publish starts at 1, the same as the platform default.
/// Monotonicity must therefore be checked against the realm's own custom history,
/// not against the effective default template.
async fn max_custom_version_no(ctx: &TestContext, realm_id: &str, agreement_type: &str) -> i64 {
    sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(version_no) FROM legal_agreement_version
         WHERE realm_id = $1 AND agreement_type = $2 AND source = 'custom'",
    )
    .bind(realm_id)
    .bind(agreement_type)
    .fetch_one(&ctx.app_state.pool)
    .await
    .expect("failed to read max custom version_no")
    .unwrap_or(0)
}

// =============================================================================
// Scenario 1: public list agreements without login
// =============================================================================

/// User Story: US-RU-013 (public view of agreements, no login required)
/// Covers: the agreements list endpoint is PUBLIC (no
/// Bearer authentication requirement). An anonymous request must resolve and return both
/// agreement types' current effective summaries from the seeded platform
/// defaults.
///
/// WHY this matters: pre-login surfaces (login page, signup footer) render the
/// binding agreement text without a session. Gating this on identity would
/// force every anonymous visitor into a login wall before they could even read
/// what they would be agreeing to.
#[test_context(TestContext)]
#[tokio::test]
async fn test_public_list_agreements_without_login(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let resp = app
        .oneshot(build_request(
            "GET",
            &format!("/api/legal/{realm_id}/agreements"),
        ))
        .await
        .expect("request must dispatch");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "public list must be 200 without login"
    );

    let body: Value = crate::tests::response_json(resp).await;
    let agreements = body["agreements"]
        .as_array()
        .expect("agreements must be an array");

    let types: Vec<&str> = agreements
        .iter()
        .map(|a| {
            a["agreement_type"]
                .as_str()
                .expect("agreement_type must be a string")
        })
        .collect();
    assert!(
        types.contains(&"terms_of_service"),
        "ToS summary must be present, got: {types:?}"
    );
    assert!(
        types.contains(&"privacy_policy"),
        "Privacy summary must be present, got: {types:?}"
    );

    // Every summary must carry the stable version identifiers a client uses to
    // pin consent: version_id (the consent token), version_no (monotonic),
    // effective_at (the legally material effective date).
    for a in agreements {
        assert!(a["version_id"].is_string(), "version_id must be present");
        assert!(a["version_no"].is_i64(), "version_no must be an integer");
        assert!(
            a["effective_at"].is_string(),
            "effective_at must be present"
        );
    }
}

// =============================================================================
// Scenario 2: public get single agreement by type returns detail
// =============================================================================

/// User Story: US-RU-013 (public view of a single agreement detail)
/// Covers: public detail endpoint returns the localized body
/// plus the version identifiers. The seeded default carries a `zh-CN` body.
///
/// WHY this matters: a summary (Scenario 1) is not enough to obtain informed
/// consent — the user must read the full text, and the version identifiers let
/// the client pin exactly which text they consented to.
#[test_context(TestContext)]
#[tokio::test]
async fn test_public_get_single_agreement_by_type(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let resp = app
        .oneshot(build_request(
            "GET",
            &format!("/api/legal/{realm_id}/agreements/terms_of_service"),
        ))
        .await
        .expect("request must dispatch");

    assert_eq!(resp.status(), StatusCode::OK, "public detail must be 200");

    let body: Value = crate::tests::response_json(resp).await;
    assert_eq!(body["agreement_type"], "terms_of_service");
    assert!(body["version_id"].is_string(), "version_id must be present");
    assert!(body["version_no"].is_i64(), "version_no must be an integer");
    assert!(
        body["effective_at"].is_string(),
        "effective_at must be present"
    );

    // `content` is the locale→body map; pick_locale falls back to the default
    // locale (zh-CN for the seed) when no ?locale= is given.
    let content = &body["content"];
    assert!(
        content.is_object() || content.is_string(),
        "content must resolve to a locale body (object) or a body string, got: {content}"
    );
}

// =============================================================================
// Scenario 3: public get agreement unknown type → 400
// =============================================================================

/// User Story: US-RU-013 (negative path)
/// Covers: an unknown `agreementType` path segment must surface
/// as 400, not a silent 404-on-missing-row nor a 500. `AgreementType::try_from`
/// rejects anything other than `terms_of_service`/`privacy_policy`.
///
/// WHY this matters: a 404 here would be ambiguous (could mean "no effective
/// version deployed"), and a 500 would leak a parse error; 400 is the contract
/// that lets the client distinguish a bad path from a missing deployment.
#[test_context(TestContext)]
#[tokio::test]
async fn test_public_get_agreement_unknown_type_is_400(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let resp = app
        .oneshot(build_request(
            "GET",
            &format!("/api/legal/{realm_id}/agreements/not_a_type"),
        ))
        .await
        .expect("request must dispatch");

    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "unknown agreement type must be 400, not 404/500"
    );
}

// =============================================================================
// Scenario 4: cross-realm agreements are isolated
// =============================================================================

/// User Story: US-RA-019 (cross-realm isolation of custom agreements)
/// Covers: realm isolation — realm-A publishes a custom ToS;
/// realm-B never publishes. Reading realm-A's ToS must return the custom body,
/// reading realm-B's must return the platform default. The two must not bleed.
///
/// WHY this matters: a realm's published text is its legally binding one. If a
/// custom publish in realm-A leaked into realm-B, realm-B's users would be
/// bound by text their admin never approved — and vice versa, a custom publish
/// that silently fell back to default would void the realm's own legal text.
#[test_context(TestContext)]
#[tokio::test]
async fn test_cross_realm_agreements_isolated(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_a = ctx._realm_id.clone();

    // Realm-A: admin publishes a custom ToS with a recognizable body.
    let (admin_token, admin_user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-crossrealm-admin@test.com",
            1800,
        )
        .await;
    grant_settings_role(ctx, &admin_user_id, &realm_a, "manage").await;

    let custom_body = json!({ "en": "realm-A custom ToS body (isolated)" });
    let publish_req = authed_request(
        "PUT",
        &format!("/api/legal/admin/{realm_a}/agreements/terms_of_service"),
        &admin_token,
        Some(json!({ "content": custom_body }).to_string()),
    );
    let publish_resp = app
        .clone()
        .oneshot(publish_req)
        .await
        .expect("PUT must dispatch");
    assert_eq!(
        publish_resp.status(),
        StatusCode::OK,
        "realm-A publish must succeed"
    );

    // Realm-B: a second realm with no custom publish (resolves to seed default).
    let realm_b = format!(
        "legal-isolation-b-{}",
        chrono::Utc::now().timestamp_millis() % 1_000_000
    );
    seed_extra_realm(ctx, &realm_b).await;

    // Realm-A read: custom body must surface.
    let resp_a = app
        .clone()
        .oneshot(build_request(
            "GET",
            &format!("/api/legal/{realm_a}/agreements/terms_of_service"),
        ))
        .await
        .expect("GET realm-A must dispatch");
    assert_eq!(resp_a.status(), StatusCode::OK);
    let body_a: Value = crate::tests::response_json(resp_a).await;
    // The public detail endpoint localizes `content`: with no `?locale=` it
    // returns the default locale body as a string, not the full locale map.
    assert_eq!(
        body_a["content"].as_str(),
        Some("realm-A custom ToS body (isolated)"),
        "realm-A detail must carry its custom body"
    );

    // Realm-B read: must fall back to the platform default, NOT realm-A's custom.
    let resp_b = app
        .oneshot(build_request(
            "GET",
            &format!("/api/legal/{realm_b}/agreements/terms_of_service"),
        ))
        .await
        .expect("GET realm-B must dispatch");
    assert_eq!(resp_b.status(), StatusCode::OK);
    let body_b: Value = crate::tests::response_json(resp_b).await;
    assert_ne!(
        body_b["content"].as_str(),
        Some("realm-A custom ToS body (isolated)"),
        "realm-B must NOT inherit realm-A's custom body (realm isolation)"
    );
    // The seed default's body is the zh-CN template, so the localized content
    // must be present (non-empty string) and different from realm-A's custom.
    assert!(
        body_b["content"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "realm-B detail must fall back to the seeded zh-CN default body"
    );
}

// =============================================================================
// Scenario 5: consent/status needsReconsent=true after admin publish
// =============================================================================

/// User Story: US-RU-012 / US-RA-019 (publish triggers user reconsent, HTTP side)
/// Covers: a user who already consented to the current
/// ToS must see `needs_reconsent=true` over HTTP after an admin publishes a
/// newer custom version.
///
/// WHY this matters: this is the reconsent gate observable on the wire. If a
/// publish did not flip this flag, users would never be re-prompted after a
/// material change, defeating the legal "explicit re-consent" requirement.
#[test_context(TestContext)]
#[tokio::test]
async fn test_consent_status_needs_reconsent_true_after_admin_publish(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    // A regular user logs in and consents to the current (seed default) ToS.
    let (user_token, _user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-reconsent-user@test.com",
            1800,
        )
        .await;

    let current_version_id = {
        let resp = app
            .clone()
            .oneshot(authed_request(
                "GET",
                &format!("/api/legal/{realm_id}/agreements/terms_of_service"),
                "",
                None,
            ))
            .await
            .expect("GET detail must dispatch");
        // Public endpoint — token irrelevant; read the seed default version_id.
        let detail: Value = crate::tests::response_json(resp).await;
        detail["version_id"]
            .as_str()
            .expect("version_id must be present")
            .to_string()
    };

    let consent_req = authed_request(
        "POST",
        &format!("/api/legal/{realm_id}/consent"),
        &user_token,
        Some(json!({
            "agreements": [{ "agreement_type": "terms_of_service", "version_id": current_version_id }]
        }).to_string()),
    );
    let consent_resp = app
        .clone()
        .oneshot(consent_req)
        .await
        .expect("POST must dispatch");
    assert_eq!(
        consent_resp.status(),
        StatusCode::NO_CONTENT,
        "initial consent must be 204"
    );

    // Admin publishes a newer custom ToS → version_id changes.
    let (admin_token, admin_user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-reconsent-admin@test.com",
            1800,
        )
        .await;
    grant_settings_role(ctx, &admin_user_id, &realm_id, "manage").await;

    let publish_req = authed_request(
        "PUT",
        &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service"),
        &admin_token,
        Some(json!({ "content": { "en": "amended ToS body" } }).to_string()),
    );
    let publish_resp = app
        .clone()
        .oneshot(publish_req)
        .await
        .expect("PUT must dispatch");
    assert_eq!(
        publish_resp.status(),
        StatusCode::OK,
        "admin publish must succeed"
    );

    // The user's consent/status must now flag ToS as needsReconsent.
    let status_resp = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/{realm_id}/consent/status"),
            &user_token,
            None,
        ))
        .await
        .expect("GET status must dispatch");
    assert_eq!(status_resp.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(status_resp).await;
    let tos = status_item(&body, "terms_of_service");
    assert!(
        tos["needs_reconsent"].as_bool() == Some(true),
        "ToS needs_reconsent must be true after a publish, got: {tos}"
    );
}

// =============================================================================
// Scenario 6: consent/status needsReconsent=false when up to date (regression)
// =============================================================================

/// User Story: US-RU-012 (do not re-prompt up-to-date users — regression)
/// Covers: when the user has consented to the current effective
/// version and nothing newer has been published, every item must report
/// `needs_reconsent=false`.
///
/// WHY this matters: a false positive here traps users in an infinite reconsent
/// loop on every request; the gate must be stable when nothing changed.
#[test_context(TestContext)]
#[tokio::test]
async fn test_consent_status_needs_reconsent_false_when_consented_latest(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (user_token, _user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-uptodate-user@test.com",
            1800,
        )
        .await;

    // Resolve the current effective version for BOTH types, then consent to each.
    let tos_id = read_effective_version_id(&app, &realm_id, "terms_of_service").await;
    let pp_id = read_effective_version_id(&app, &realm_id, "privacy_policy").await;

    let consent_resp = app
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/api/legal/{realm_id}/consent"),
            &user_token,
            Some(
                json!({
                    "agreements": [
                        { "agreement_type": "terms_of_service", "version_id": tos_id },
                        { "agreement_type": "privacy_policy", "version_id": pp_id }
                    ]
                })
                .to_string(),
            ),
        ))
        .await
        .expect("POST must dispatch");
    assert_eq!(
        consent_resp.status(),
        StatusCode::NO_CONTENT,
        "consent must be 204"
    );

    let status_resp = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/{realm_id}/consent/status"),
            &user_token,
            None,
        ))
        .await
        .expect("GET status must dispatch");
    assert_eq!(status_resp.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(status_resp).await;
    for item in body["items"].as_array().expect("items must be an array") {
        assert!(
            item["needs_reconsent"].as_bool() == Some(false),
            "up-to-date item must report needs_reconsent=false, got: {item}"
        );
    }
}

// =============================================================================
// Scenario 7: POST consent 204 on current version
// =============================================================================

/// User Story: US-RU-015 (consent to current version succeeds)
/// Covers: POST /consent with the current effective version_id
/// returns 204 (no content). The upsert is idempotent, so re-consenting to the
/// same version also yields 204.
///
/// WHY this matters: 204 is the success signal that unblocks a registration /
/// login consent gate. Returning 200 with a body would break no-content clients;
/// returning anything other than 2xx would leave the gate stuck.
#[test_context(TestContext)]
#[tokio::test]
async fn test_post_consent_returns_204_on_current_version(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (user_token, _user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-consent-204-user@test.com",
            1800,
        )
        .await;

    let current_version_id = read_effective_version_id(&app, &realm_id, "terms_of_service").await;

    let resp = app
        .oneshot(authed_request(
            "POST",
            &format!("/api/legal/{realm_id}/consent"),
            &user_token,
            Some(
                json!({
                    "agreements": [{
                        "agreement_type": "terms_of_service",
                        "version_id": current_version_id
                    }]
                })
                .to_string(),
            ),
        ))
        .await
        .expect("POST must dispatch");

    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "current-version consent must be 204"
    );
}

// =============================================================================
// Scenario 8: POST consent 409 on stale version
// =============================================================================

/// User Story: US-RU-012 (stale version is refused — gate side)
/// Covers: POST /consent with a `version_id` that is NOT the
/// current effective one must return 409, so the client re-reads the effective
/// version and re-prompts. The service layer surfaces StaleVersion as a
/// Conflict-class error mapped to 409.
///
/// WHY this matters: the version id is the consent token. Accepting a stale
/// token would let a user "consent" to an obsolete version and silently bypass
/// the reconsent gate after an admin published a newer one — exactly the attack
/// the gate exists to prevent.
#[test_context(TestContext)]
#[tokio::test]
async fn test_post_consent_returns_409_on_stale_version(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();
    let svc = ctx.app_state.legal_service.clone();
    use herald_core::domain::legal::entities::AgreementType;

    // Publish V_old, then V_current, so V_old is provably stale.
    let v_old = svc
        .publish_custom(
            &realm_id,
            AgreementType::TermsOfService,
            json!({ "en": "old version body" }),
            None,
            "admin@scene",
            herald_core::domain::audit::AuditContext {
                actor_id: "admin@scene".to_string(),
                actor_type: Some(herald_core::domain::audit::ActorType::Admin),
                actor_name: None,
                ip_address: None,
                user_agent: None,
                trace_id: None,
            },
        )
        .await
        .expect("first publish must succeed");

    svc.publish_custom(
        &realm_id,
        AgreementType::TermsOfService,
        json!({ "en": "newer version body" }),
        None,
        "admin@scene",
        herald_core::domain::audit::AuditContext {
            actor_id: "admin@scene".to_string(),
            actor_type: Some(herald_core::domain::audit::ActorType::Admin),
            actor_name: None,
            ip_address: None,
            user_agent: None,
            trace_id: None,
        },
    )
    .await
    .expect("second publish must succeed");

    let (user_token, _user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-consent-409-user@test.com",
            1800,
        )
        .await;

    let resp = app
        .oneshot(authed_request(
            "POST",
            &format!("/api/legal/{realm_id}/consent"),
            &user_token,
            Some(
                json!({
                    "agreements": [{
                        "agreement_type": "terms_of_service",
                        "version_id": v_old.id
                    }]
                })
                .to_string(),
            ),
        ))
        .await
        .expect("POST must dispatch");

    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "stale version_id must be rejected with 409, not 204"
    );
}

// =============================================================================
// Scenario 9: POST consent requires login (401 without session)
// =============================================================================

/// User Story: US-RU-015 (consent is a self-service, identity-bound action)
/// Covers: consent is self-service and gated behind
/// Bearer authentication. An unauthenticated POST must return 401 before any write
/// happens, so an anonymous client cannot fabricate consent on someone's behalf.
///
/// WHY this matters: consent is a legal commitment attributed to a specific
/// user. Accepting it anonymously would either crash (no identity to attribute
/// to) or, worse, attribute it to whoever the body claims — neither is
/// acceptable.
#[test_context(TestContext)]
#[tokio::test]
async fn test_post_consent_requires_login(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    // No Authorization header — the token identity layer must short-circuit.
    let resp = app
        .oneshot(build_request_bodyful(
            "POST",
            &format!("/api/legal/{realm_id}/consent"),
            json!({
                "agreements": [{
                    "agreement_type": "terms_of_service",
                    "version_id": Uuid::nil()
                }]
            })
            .to_string(),
        ))
        .await
        .expect("POST must dispatch");

    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "unauthenticated consent POST must be 401"
    );
}

// =============================================================================
// Scenario 10: admin publish creates new version + triggers reconsent
// =============================================================================

/// User Story: US-RA-019 (admin publish produces a new version)
/// Covers: admin PUT — an admin with `settings.manage` +
/// `has_access_to_realm` PUTs a custom ToS; the response carries a fresh
/// `{version_id, version_no, effective_at}` whose `version_no` is strictly
/// greater than any previous *custom* version for that `(realm, type)` scope.
/// Afterward the realm's users see `needs_reconsent=true`.
///
/// WHY this matters: the new `version_id` is what makes the publish observable
/// as a real version change — without a fresh id, existing user consent would
/// silently match and reconsent would never fire. `version_no` monotonicity
/// within the realm/type scope is the DB-side ordering invariant the resolver
/// relies on (`version_no` is scoped by
/// `(COALESCE(realm_id,''), agreement_type)`).
#[test_context(TestContext)]
#[tokio::test]
async fn test_admin_publish_creates_new_version_and_triggers_reconsent(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    // Admin (settings.manage) for this realm.
    let (admin_token, admin_user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-publish-admin@test.com",
            1800,
        )
        .await;
    grant_settings_role(ctx, &admin_user_id, &realm_id, "manage").await;

    // A user consents to the current version first.
    let (user_token, _user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-publish-user@test.com",
            1800,
        )
        .await;
    let initial_version_id = read_effective_version_id(&app, &realm_id, "terms_of_service").await;
    let consent_resp = app
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/api/legal/{realm_id}/consent"),
            &user_token,
            Some(
                json!({
                    "agreements": [{
                        "agreement_type": "terms_of_service",
                        "version_id": initial_version_id
                    }]
                })
                .to_string(),
            ),
        ))
        .await
        .expect("POST must dispatch");
    assert_eq!(consent_resp.status(), StatusCode::NO_CONTENT);

    // Capture the pre-publish highest custom version_no for this realm/type scope.
    // The platform default template has version_no=1 but is scoped to realm_id IS NULL,
    // so the realm's first custom publish also starts at 1. Monotonicity must be
    // judged against the realm's own custom history, not the default template.
    let pre_custom_max = max_custom_version_no(ctx, &realm_id, "terms_of_service").await;

    // Admin publishes.
    let publish_req = authed_request(
        "PUT",
        &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service"),
        &admin_token,
        Some(
            json!({
                "content": { "en": "v1 custom body", "zh-CN": "v1 中文正文" },
                "version_label": "v1"
            })
            .to_string(),
        ),
    );
    let publish_resp = app
        .clone()
        .oneshot(publish_req)
        .await
        .expect("PUT must dispatch");
    assert_eq!(publish_resp.status(), StatusCode::OK, "publish must be 200");

    let published: Value = crate::tests::response_json(publish_resp).await;
    assert!(
        published["version_id"].is_string(),
        "version_id must be present"
    );
    assert!(
        published["version_no"].as_i64() > Some(pre_custom_max),
        "published version_no ({}) must be greater than the realm's previous custom max ({})",
        published["version_no"],
        pre_custom_max
    );
    assert!(
        published["effective_at"].is_string(),
        "effective_at must be present"
    );

    // The realm user's consent/status must now flag ToS as needsReconsent.
    let status_resp = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/{realm_id}/consent/status"),
            &user_token,
            None,
        ))
        .await
        .expect("GET status must dispatch");
    assert_eq!(status_resp.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(status_resp).await;
    let tos = status_item(&body, "terms_of_service");
    assert!(
        tos["needs_reconsent"].as_bool() == Some(true),
        "publish must flip needs_reconsent=true, got: {tos}"
    );
}

// =============================================================================
// Scenario 11: admin revert appends a live-default follow marker
// =============================================================================

/// User Story: US-RA-019 (revert = live-default follow marker, HTTP side)
/// Covers: DELETE admin .../custom — after a realm has a
/// custom ToS (version_no = N), DELETE .../custom must return a NEW marker
/// version (version_no = N+1, source=default) and effective resolution then
/// follows the live platform default (not a detached snapshot). No prior row
/// is deleted; the new version_id triggers user reconsent.
///
/// WHY this matters: revert is NOT a row deletion — it is itself a version
/// change. Rewinding the id or deleting rows would let an old consent silently
/// match and bypass the reconsent gate; the follow-marker semantic makes
/// revert observable as a binding new version while keeping later
/// platform-default updates visible to the realm.
#[test_context(TestContext)]
#[tokio::test]
async fn test_admin_revert_snapshots_default_into_new_version(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (admin_token, admin_user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-revert-admin@test.com",
            1800,
        )
        .await;
    grant_settings_role(ctx, &admin_user_id, &realm_id, "manage").await;

    // Baseline: the effective version BEFORE any custom publish is the live
    // platform default — after revert, effective resolution must return to
    // exactly this row (live-default following, not a detached snapshot).
    let baseline_effective: Value = {
        let r = app
            .clone()
            .oneshot(build_request(
                "GET",
                &format!("/api/legal/{realm_id}/agreements/terms_of_service"),
            ))
            .await
            .expect("baseline GET must dispatch");
        crate::tests::response_json(r).await
    };
    let baseline_version_id = baseline_effective["version_id"]
        .as_str()
        .expect("baseline version_id must be present")
        .to_string();
    let baseline_content = baseline_effective["content"].clone();

    // Establish a realm custom ToS (version_no = N).
    let first_publish: Value = {
        let r = app
            .clone()
            .oneshot(authed_request(
                "PUT",
                &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service"),
                &admin_token,
                Some(json!({ "content": { "en": "realm custom N body" } }).to_string()),
            ))
            .await
            .expect("first PUT must dispatch");
        assert_eq!(r.status(), StatusCode::OK);
        crate::tests::response_json(r).await
    };
    let n = first_publish["version_no"]
        .as_i64()
        .expect("version_no must be int");
    let prior_version_id = first_publish["version_id"]
        .as_str()
        .expect("version_id must be present")
        .to_string();

    // A user consents to the custom version N.
    let (user_token, _user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-revert-user@test.com",
            1800,
        )
        .await;
    let consent_resp = app
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/api/legal/{realm_id}/consent"),
            &user_token,
            Some(json!({
                "agreements": [{ "agreement_type": "terms_of_service", "version_id": prior_version_id }]
            }).to_string()),
        ))
        .await
        .expect("POST must dispatch");
    assert_eq!(consent_resp.status(), StatusCode::NO_CONTENT);

    // Revert.
    let revert_resp = app
        .clone()
        .oneshot(authed_request(
            "DELETE",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service/custom"),
            &admin_token,
            None,
        ))
        .await
        .expect("DELETE must dispatch");
    assert_eq!(revert_resp.status(), StatusCode::OK, "revert must be 200");

    let reverted: Value = crate::tests::response_json(revert_resp).await;
    assert_eq!(
        reverted["version_no"].as_i64(),
        Some(n + 1),
        "revert must advance version_no to N+1, not rewind"
    );
    let new_version_id = reverted["version_id"]
        .as_str()
        .expect("version_id must be present");
    assert_ne!(
        new_version_id, prior_version_id,
        "revert must mint a fresh version_id, never reuse a prior one"
    );

    // The now-effective version is the live platform default row itself —
    // the marker delegates effective resolution to the default, it is not a
    // detached snapshot of it.
    let effective: Value = {
        let r = app
            .clone()
            .oneshot(build_request(
                "GET",
                &format!("/api/legal/{realm_id}/agreements/terms_of_service"),
            ))
            .await
            .expect("GET must dispatch");
        crate::tests::response_json(r).await
    };
    assert_eq!(
        effective["version_id"].as_str(),
        Some(baseline_version_id.as_str()),
        "effective resolution must follow the live platform default after the marker"
    );
    assert_eq!(
        effective["content"], baseline_content,
        "the effective body must be the live platform default template"
    );

    // And the realm user must now be flagged for reconsent.
    let status_resp = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/{realm_id}/consent/status"),
            &user_token,
            None,
        ))
        .await
        .expect("GET status must dispatch");
    let body: Value = crate::tests::response_json(status_resp).await;
    let tos = status_item(&body, "terms_of_service");
    assert!(
        tos["needs_reconsent"].as_bool() == Some(true),
        "revert must flip needs_reconsent=true, got: {tos}"
    );
}

// =============================================================================
// Scenario 12: admin view shows source + history
// =============================================================================

/// User Story: US-RA-019 (admin view distinguishes default/custom + shows history)
/// Covers: admin GET — after a realm has published custom ToS,
/// the admin view reports `source=custom` and a `history` whose custom entries
/// are ordered by `version_no` DESC (most recent first).
///
/// WHY this matters: the `source` flag is the admin's signal that a realm
/// override exists (and therefore that revert is meaningful). The history is
/// the audit trail of what text was effective when; mis-ordering it would
/// mislead the admin about which version is currently binding.
#[test_context(TestContext)]
#[tokio::test]
async fn test_admin_view_shows_source_and_history(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (admin_token, admin_user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-view-admin@test.com",
            1800,
        )
        .await;
    grant_settings_role(ctx, &admin_user_id, &realm_id, "manage").await;
    // view is also needed for the admin GET; grant both so the GET is authorized.
    grant_settings_role(ctx, &admin_user_id, &realm_id, "view").await;

    // Publish two custom ToS versions so the history is non-trivial.
    for label in ["history-v1", "history-v2"] {
        let r = app
            .clone()
            .oneshot(authed_request(
                "PUT",
                &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service"),
                &admin_token,
                Some(json!({ "content": { "en": label }, "version_label": label }).to_string()),
            ))
            .await
            .expect("PUT must dispatch");
        assert_eq!(r.status(), StatusCode::OK, "publish {label} must succeed");
    }

    let resp = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/admin/{realm_id}/agreements"),
            &admin_token,
            None,
        ))
        .await
        .expect("GET admin must dispatch");
    assert_eq!(resp.status(), StatusCode::OK, "admin view must be 200");

    let body: Value = crate::tests::response_json(resp).await;
    let tos = admin_view(&body, "terms_of_service");

    assert_eq!(
        tos["source"].as_str(),
        Some("custom"),
        "realm with a custom publish must report source=custom"
    );
    assert!(
        tos["current_version"]["version_id"].is_string(),
        "current_version.version_id must be present"
    );

    // history must be non-empty and ordered by version_no DESC within the
    // custom rows (the repository returns custom-first, then default fallback).
    let history = tos["history"].as_array().expect("history must be an array");
    assert!(
        !history.is_empty(),
        "history must list at least the custom versions"
    );

    let custom_version_nos: Vec<i64> = history
        .iter()
        .filter(|h| h["source"].as_str() == Some("custom"))
        .map(|h| h["version_no"].as_i64().expect("version_no must be int"))
        .collect();
    assert!(
        !custom_version_nos.is_empty(),
        "history must include custom entries"
    );

    let mut sorted = custom_version_nos.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(
        custom_version_nos, sorted,
        "custom history entries must be ordered version_no DESC (newest first)"
    );
}

// =============================================================================
// Scenario 13: admin cross-realm forbidden (403)
// =============================================================================

/// User Story: US-RA-019 (cross-realm isolation of admin endpoints)
/// Covers: an admin belonging to realm-A (with full settings perms
/// on A) must be refused (403) when targeting realm-B's admin endpoints, because
/// `has_access_to_realm(B)` is false. The settings permission check never even
/// runs.
///
/// WHY this matters: an admin of one realm must not be able to read or mutate
/// another realm's legal text. `has_access_to_realm` is the perimeter that
/// prevents a realm-A admin from publishing (or reverting) realm-B's binding
/// agreement.
#[test_context(TestContext)]
#[tokio::test]
async fn test_admin_cross_realm_forbidden(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_a = ctx._realm_id.clone();
    let realm_b = format!(
        "legal-cross-b-{}",
        chrono::Utc::now().timestamp_millis() % 1_000_000
    );
    seed_extra_realm(ctx, &realm_b).await;

    // Admin of realm-A (settings.view on A, NOT a member of B).
    let (admin_token, admin_user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-crossrealm-a-admin@test.com",
            1800,
        )
        .await;
    grant_settings_role(ctx, &admin_user_id, &realm_a, "view").await;

    let resp = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/admin/{realm_b}/agreements"),
            &admin_token,
            None,
        ))
        .await
        .expect("GET must dispatch");

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "admin of realm-A must be 403 on realm-B (has_access_to_realm false)"
    );
}

// =============================================================================
// Scenario 14: admin publish requires settings.manage
// =============================================================================

/// User Story: US-RA-019 (permission boundary: settings.manage required to publish)
/// Covers: a user with `settings.view` only (no `manage`) +
/// `has_access_to_realm` must be refused (403) on PUT admin publish. The view
/// permission is read-only; publishing mutates binding legal text.
///
/// WHY this matters: a viewer must not be able to publish a new binding
/// agreement. Conflating view and manage would let any read-only operator
/// rewrite the legal text the realm's users are bound by.
#[test_context(TestContext)]
#[tokio::test]
async fn test_admin_publish_requires_settings_manage(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (token, user_id) = crate::tests::helpers::auth_helpers::create_admin_session_with_user(
        ctx,
        "legal-viewonly-admin@test.com",
        1800,
    )
    .await;
    // view only — NO manage.
    grant_settings_role(ctx, &user_id, &realm_id, "view").await;

    let resp = app
        .oneshot(authed_request(
            "PUT",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service"),
            &token,
            Some(json!({ "content": { "en": "should not publish" } }).to_string()),
        ))
        .await
        .expect("PUT must dispatch");

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "settings.view-only user must be 403 on publish (requires settings.manage)"
    );
}

// =============================================================================
// Scenario 15: admin view requires settings.view
// =============================================================================

/// User Story: US-RA-019 (permission boundary: settings.view required to read admin view)
/// Covers: an authenticated user with `has_access_to_realm` but NO
/// `settings.view` permission must be refused (403) on GET admin. The admin view
/// exposes agreement history and source flags; it is not public data.
///
/// WHY this matters: the admin view discloses operational detail (custom vs
/// default, full version history) that a plain realm member should not see.
/// Requiring settings.view keeps it gated to operators with an explicit read
/// grant.
#[test_context(TestContext)]
#[tokio::test]
async fn test_admin_view_requires_settings_view(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    // A plain realm member (session established, belongs to realm, NO settings perm).
    let (token, _user_id) = crate::tests::helpers::auth_helpers::create_admin_session_with_user(
        ctx,
        "legal-no-settings-user@test.com",
        1800,
    )
    .await;

    let resp = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/admin/{realm_id}/agreements"),
            &token,
            None,
        ))
        .await
        .expect("GET must dispatch");

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "user without settings.view must be 403 on admin GET"
    );
}

// =============================================================================
// Internal request helpers (variants not exposed at module top)
// =============================================================================

/// Build a bare request carrying a JSON body (used for the unauthenticated
/// POST consent test — `build_request` above is bodyless).
fn build_request_bodyful(method: &str, path: &str, body: String) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header("content-length", body.len().to_string())
        .body(Body::from(body))
        .expect("failed to build bodyful request")
}

/// Read the current effective `version_id` for an agreement type via the public
/// detail endpoint (no auth required). Used to seed consent payloads with the
/// real current token rather than guessing.
async fn read_effective_version_id(
    app: &axum::Router,
    realm_id: &str,
    agreement_type: &str,
) -> String {
    let resp = app
        .clone()
        .oneshot(build_request(
            "GET",
            &format!("/api/legal/{realm_id}/agreements/{agreement_type}"),
        ))
        .await
        .expect("GET detail must dispatch");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "effective version must resolve"
    );
    let detail: Value = crate::tests::response_json(resp).await;
    detail["version_id"]
        .as_str()
        .expect("version_id must be present on the detail response")
        .to_string()
}

// =============================================================================
// Scenario: admin draft lifecycle (save / get / discard / publish-from-draft)
// =============================================================================
//
// Covers the draft feature end-to-end over HTTP:
//   - GET    .../agreements/{type}/draft      → 404 when no draft, 200 after save
//   - PUT    .../agreements/{type}/draft      → upsert, body echoed back
//   - DELETE .../agreements/{type}/draft      → 204, idempotent on missing draft
//   - POST   .../agreements/{type}/publish    → publishes from draft, clears draft
//
// WHY this matters: drafts are staged in a separate table and must NEVER affect
// end-user resolution, the source indicator, version_no sequence, or the consent
// gate. Only POST /publish flips those. These tests encode that invariant.

#[test_context(TestContext)]
#[tokio::test]
async fn test_draft_save_get_and_discard_does_not_publish(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (admin_token, admin_user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-draft-admin@test.com",
            1800,
        )
        .await;
    grant_settings_role(ctx, &admin_user_id, &realm_id, "manage").await;

    // No draft yet → GET returns 404.
    let get_before = app
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service/draft"),
            &admin_token,
            None,
        ))
        .await
        .expect("GET draft must dispatch");
    assert_eq!(
        get_before.status(),
        StatusCode::NOT_FOUND,
        "missing draft must be 404"
    );

    // Capture the effective version BEFORE saving a draft.
    let pre_effective = read_effective_version_id(&app, &realm_id, "terms_of_service").await;

    // Save a draft.
    let save_resp = app
        .clone()
        .oneshot(authed_request(
            "PUT",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service/draft"),
            &admin_token,
            Some(
                json!({
                    "content": { "en": "draft body, not yet live" },
                    "version_label": "draft v1"
                })
                .to_string(),
            ),
        ))
        .await
        .expect("PUT draft must dispatch");
    assert_eq!(save_resp.status(), StatusCode::OK, "save draft must be 200");
    let saved: Value = crate::tests::response_json(save_resp).await;
    assert_eq!(saved["version_label"].as_str(), Some("draft v1"));
    assert_eq!(
        saved["content"]["en"].as_str(),
        Some("draft body, not yet live")
    );

    // GET now returns the draft.
    let get_after = app
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service/draft"),
            &admin_token,
            None,
        ))
        .await
        .expect("GET draft must dispatch");
    assert_eq!(get_after.status(), StatusCode::OK);
    let fetched: Value = crate::tests::response_json(get_after).await;
    assert_eq!(
        fetched["content"]["en"].as_str(),
        Some("draft body, not yet live")
    );

    // CRITICAL INVARIANT: a saved draft must NOT change the effective version
    // (drafts live in a separate table and never feed version resolution).
    let post_effective = read_effective_version_id(&app, &realm_id, "terms_of_service").await;
    assert_eq!(
        pre_effective, post_effective,
        "saving a draft must not change the effective agreement version"
    );

    // Discard the draft.
    let discard_resp = app
        .clone()
        .oneshot(authed_request(
            "DELETE",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service/draft"),
            &admin_token,
            None,
        ))
        .await
        .expect("DELETE draft must dispatch");
    assert_eq!(discard_resp.status(), StatusCode::NO_CONTENT);

    // Discard again is idempotent.
    let discard_again = app
        .oneshot(authed_request(
            "DELETE",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service/draft"),
            &admin_token,
            None,
        ))
        .await
        .expect("DELETE draft must dispatch");
    assert_eq!(discard_again.status(), StatusCode::NO_CONTENT);
}

#[test_context(TestContext)]
#[tokio::test]
async fn test_publish_from_draft_publishes_new_version_and_clears_draft(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (admin_token, admin_user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-publish-draft-admin@test.com",
            1800,
        )
        .await;
    grant_settings_role(ctx, &admin_user_id, &realm_id, "manage").await;

    // A user consents to the current version first, so we can assert reconsent
    // flips after publish-from-draft.
    let (user_token, _user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-publish-draft-user@test.com",
            1800,
        )
        .await;
    let initial_version_id = read_effective_version_id(&app, &realm_id, "terms_of_service").await;
    let consent_resp = app
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/api/legal/{realm_id}/consent"),
            &user_token,
            Some(
                json!({
                    "agreements": [{
                        "agreement_type": "terms_of_service",
                        "version_id": initial_version_id
                    }]
                })
                .to_string(),
            ),
        ))
        .await
        .expect("POST consent must dispatch");
    assert_eq!(consent_resp.status(), StatusCode::NO_CONTENT);

    let pre_custom_max = max_custom_version_no(ctx, &realm_id, "terms_of_service").await;

    // Publish with no draft saved → 404. The body is an empty JSON object
    // (publish takes an optional version_label override; none given here).
    let publish_no_draft = app
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service/publish"),
            &admin_token,
            Some(json!({}).to_string()),
        ))
        .await
        .expect("POST publish must dispatch");
    assert_eq!(
        publish_no_draft.status(),
        StatusCode::NOT_FOUND,
        "publish with no draft must be 404"
    );

    // Save a draft, then publish from it.
    let save_resp = app
        .clone()
        .oneshot(authed_request(
            "PUT",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service/draft"),
            &admin_token,
            Some(
                json!({
                    "content": { "en": "published-from-draft body" },
                    "version_label": "draft label"
                })
                .to_string(),
            ),
        ))
        .await
        .expect("PUT draft must dispatch");
    assert_eq!(save_resp.status(), StatusCode::OK);

    let publish_resp = app
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service/publish"),
            &admin_token,
            Some(json!({ "version_label": "final override label" }).to_string()),
        ))
        .await
        .expect("POST publish must dispatch");
    assert_eq!(
        publish_resp.status(),
        StatusCode::OK,
        "publish from draft must be 200"
    );

    let published: Value = crate::tests::response_json(publish_resp).await;
    assert!(
        published["version_no"].as_i64() > Some(pre_custom_max),
        "published version_no ({}) must exceed the realm's prior custom max ({})",
        published["version_no"],
        pre_custom_max
    );

    // The draft must be cleared after publish.
    let get_draft_after = app
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service/draft"),
            &admin_token,
            None,
        ))
        .await
        .expect("GET draft must dispatch");
    assert_eq!(
        get_draft_after.status(),
        StatusCode::NOT_FOUND,
        "draft must be cleared after publish"
    );

    // Publish advanced the effective version and flips reconsent.
    let status_resp = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/{realm_id}/consent/status"),
            &user_token,
            None,
        ))
        .await
        .expect("GET status must dispatch");
    let body: Value = crate::tests::response_json(status_resp).await;
    let tos = status_item(&body, "terms_of_service");
    assert!(
        tos["needs_reconsent"].as_bool() == Some(true),
        "publish-from-draft must flip needs_reconsent=true, got: {tos}"
    );
}

// =============================================================================
// Scenario: admin GET version-by-id returns the full body; 404 for unknown id
// =============================================================================

/// User Story: US-RA-019 (admin views a past version's body)
/// Covers: admin endpoint — `GET
/// /api/legal/admin/{realmId}/agreements/versions/{versionId}` returns the
/// full localized `content` for a single history entry (the list endpoint only
/// returns summaries, so the body is fetched on demand for the "view" dialog).
/// Requires `settings.view` + `has_access_to_realm`. An unknown id → 404.
///
/// WHY this matters: this is the only admin path that exposes a historical
/// version's body; returning the wrong body, leaking another realm's, or 500ing
/// on a missing id would all break the audit trail an admin relies on to review
/// what users were bound to at a given time.
#[test_context(TestContext)]
#[tokio::test]
async fn test_admin_get_version_returns_full_body_and_404_for_unknown(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let realm_id = ctx._realm_id.clone();

    let (admin_token, admin_user_id) =
        crate::tests::helpers::auth_helpers::create_admin_session_with_user(
            ctx,
            "legal-view-version@test.com",
            1800,
        )
        .await;
    // GET version requires settings.view; publish (to create a history entry
    // with a known id) requires settings.manage, so grant both.
    grant_settings_role(ctx, &admin_user_id, &realm_id, "view").await;
    grant_settings_role(ctx, &admin_user_id, &realm_id, "manage").await;

    // Publish one custom version so we have a real id + body to fetch.
    let publish_resp = app
        .clone()
        .oneshot(authed_request(
            "PUT",
            &format!("/api/legal/admin/{realm_id}/agreements/terms_of_service"),
            &admin_token,
            Some(
                json!({ "content": { "en": "history body text" }, "version_label": "h1" })
                    .to_string(),
            ),
        ))
        .await
        .expect("PUT publish must dispatch");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let published: Value = crate::tests::response_json(publish_resp).await;
    let version_id = published["version_id"]
        .as_str()
        .expect("publish response must include version_id")
        .to_string();

    // Fetch the version by id → full body, no other realm leakage (path-scoped).
    let resp = app
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/api/legal/admin/{realm_id}/agreements/versions/{version_id}"),
            &admin_token,
            None,
        ))
        .await
        .expect("GET version must dispatch");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "known version id must return 200"
    );
    let body: Value = crate::tests::response_json(resp).await;
    assert_eq!(body["agreement_type"].as_str(), Some("terms_of_service"));
    assert_eq!(
        body["content"]["en"].as_str(),
        Some("history body text"),
        "GET version must return the full localized content body"
    );
    assert_eq!(body["version_label"].as_str(), Some("h1"));

    // Unknown id → 404 (not 500).
    let missing = app
        .oneshot(authed_request(
            "GET",
            &format!(
                "/api/legal/admin/{realm_id}/agreements/versions/{}",
                Uuid::now_v7()
            ),
            &admin_token,
            None,
        ))
        .await
        .expect("GET missing version must dispatch");
    assert_eq!(
        missing.status(),
        StatusCode::NOT_FOUND,
        "unknown version id must return 404"
    );
}
