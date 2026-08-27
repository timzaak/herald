// =============================================================================
// Scenario tests: Realm admin LDAP configuration via the generic configs CRUD
// =============================================================================
//
// LDAP configuration rides the existing /api/configs/{realmId} CRUD surface
// (DEC-005): two rows — `ldap/settings` (validated JSON, non-secret) and
// `ldap/bind_password` (server-forced secret, masked on read, empty submit
// preserves). The public enablement signal is
//   GET /api/auth/{realmId}/ldap/status
//
// Coverage (US-LD-003): save + masked read-back, empty-secret preserve,
// plaintext `ldap://` rejection, cross-realm rejection, disable/delete
// degradation (login 400, existing accounts keep other login methods), and
// the status endpoint's fail-closed behaviour.
//
// =============================================================================

use crate::tests::helpers::ldap_helpers::{
    current_effective_agreements, delete_ldap_settings, disable_ldap, enable_ldap,
    insert_ldap_settings, ldap_login, mock_dir, one_mock_user,
};
use crate::tests::helpers::*;
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::json;
use test_context::test_context;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Local request helpers
// ---------------------------------------------------------------------------

fn batch_upsert_request(realm_id: &str, token: &str, configs: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/configs/{realm_id}/batch"))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(json!({ "configs": configs }).to_string()))
        .unwrap()
}

async fn list_ldap_configs(
    ctx: &TestContext,
    app: &axum::Router,
    token: &str,
) -> Vec<serde_json::Value> {
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/configs/{}/ldap", ctx._realm_id))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(request).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    crate::tests::response_json(resp).await
}

async fn ldap_status(ctx: &TestContext) -> serde_json::Value {
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/auth/{}/ldap/status", ctx._realm_id))
        .body(Body::empty())
        .unwrap();
    let resp = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    crate::tests::response_json(resp).await
}

fn valid_settings_json(url: &str, starttls: bool) -> String {
    json!({
        "enabled": true,
        "url": url,
        "starttls": starttls,
        "baseDn": "dc=example,dc=com",
        "bindDn": "cn=admin,dc=example,dc=com",
        "userFilter": "(&(objectClass=user)(sAMAccountName={login}))",
    })
    .to_string()
}

// =============================================================================
// Scenarios
// =============================================================================

/// User Story: US-LD-003
/// Covers: Design §4.2.3 / DEC-005 — batch-saving settings + bind_password
/// succeeds; the read-back masks the password value regardless of the
/// client-side is_secret flag, while settings echo normally.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_config_save_and_masked_readback(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "ldap-cfg-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    // bind_password submitted with is_secret=false on purpose: the server
    // must force the secret classification anyway.
    let resp = app
        .clone()
        .oneshot(batch_upsert_request(
            &ctx._realm_id,
            &token,
            json!([
                {
                    "configType": "ldap",
                    "configKey": "settings",
                    "configValue": valid_settings_json("ldaps://ldap.example.com:636", false),
                    "isSecret": false,
                    "enabled": true
                },
                {
                    "configType": "ldap",
                    "configKey": "bind_password",
                    "configValue": "svc-secret-pw",
                    "isSecret": false,
                    "enabled": true
                }
            ]),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "batch save must succeed");
    let _: serde_json::Value = crate::tests::response_json(resp).await;

    let configs = list_ldap_configs(ctx, &app, &token).await;
    let settings = configs
        .iter()
        .find(|c| c["configKey"] == "settings")
        .expect("settings row must exist");
    // configValue is the raw settings JSON string; parse it for assertions.
    let settings_value: serde_json::Value = serde_json::from_str(
        settings["configValue"]
            .as_str()
            .expect("settings value echoes"),
    )
    .expect("settings configValue must be valid JSON");
    assert_eq!(settings_value["enabled"], true);
    let bind_pw = configs
        .iter()
        .find(|c| c["configKey"] == "bind_password")
        .expect("bind_password row must exist");
    assert_eq!(
        bind_pw["configValue"],
        serde_json::Value::Null,
        "password value must be masked to null"
    );
    assert_eq!(bind_pw["isSecret"], true, "is_secret must be forced true");

    // Enabled row → public status flips to true.
    assert_eq!(ldap_status(ctx).await["enabled"], true);
}

/// User Story: US-LD-003
/// Covers: Design §4.2.3 — submitting an EMPTY bind_password preserves the
/// stored secret (admin edit without re-entering the password).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_config_empty_password_preserves_stored_secret(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "ldap-cfg-keep@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;
    enable_ldap(ctx).await; // seeds settings + bind_password rows

    // Re-save settings with an empty bind_password row.
    let resp = app
        .clone()
        .oneshot(batch_upsert_request(
            &ctx._realm_id,
            &token,
            json!([
                {
                    "configType": "ldap",
                    "configKey": "bind_password",
                    "configValue": "",
                    "isSecret": true,
                    "enabled": true
                }
            ]),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "empty secret must preserve");

    // The stored password row is untouched (still the seeded value).
    let stored: Option<String> = sqlx::query_scalar(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = 'ldap' AND config_key = 'bind_password'",
    )
    .bind(&ctx._realm_id)
    .fetch_optional(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(
        stored.as_deref(),
        Some("svc-password"),
        "empty submit must preserve the stored secret"
    );
}

/// User Story: US-LD-003
/// Covers: US-LD-003 scenario 5 / PRD §4.1 credential-channel hard rule — a
/// plaintext `ldap://` URL without StartTLS is rejected at save time with a
/// field-level 400 (admin surface: explicit error, no generalization).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_config_plaintext_url_rejected(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "ldap-cfg-plain@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let resp = app
        .clone()
        .oneshot(batch_upsert_request(
            &ctx._realm_id,
            &token,
            json!([{
                "configType": "ldap",
                "configKey": "settings",
                "configValue": valid_settings_json("ldap://ldap.example.com:389", false),
                "isSecret": false,
                "enabled": true
            }]),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "plaintext ldap:// without StartTLS must be rejected"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let message = body["message"].as_str().unwrap_or("");
    assert!(
        message.contains("plaintext"),
        "rejection must name the plaintext-channel rule; got {message:?}"
    );

    // Nothing was persisted.
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM realm_config WHERE realm_id = $1 AND config_type = 'ldap'",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(rows, 0);
    assert_eq!(ldap_status(ctx).await["enabled"], false);
}

/// User Story: US-LD-003
/// Covers: US-LD-003 write-path validation of the optional private-CA trust
/// field — `caCertPem` must be a PEM certificate bundle. A value that is not
/// PEM is rejected with a field-level 400 (admin surface: explicit error);
/// a well-formed PEM is accepted and persisted verbatim.
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_config_bad_ca_cert_pem_rejected(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "ldap-cfg-capem@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let bad_settings = json!({
        "enabled": true,
        "url": "ldaps://ldap.example.com:636",
        "baseDn": "dc=example,dc=com",
        "userFilter": "(uid={login})",
        "caCertPem": "this is not a certificate",
    })
    .to_string();
    let resp = app
        .clone()
        .oneshot(batch_upsert_request(
            &ctx._realm_id,
            &token,
            json!([{
                "configType": "ldap",
                "configKey": "settings",
                "configValue": bad_settings,
                "isSecret": false,
                "enabled": true
            }]),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "non-PEM caCertPem must be rejected at save time"
    );
    let body: serde_json::Value = crate::tests::response_json(resp).await;
    let message = body["message"].as_str().unwrap_or("");
    assert!(
        message.contains("caCertPem"),
        "rejection must name the field; got {message:?}"
    );

    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM realm_config WHERE realm_id = $1 AND config_type = 'ldap'",
    )
    .bind(&ctx._realm_id)
    .fetch_one(&ctx._app_state.pool)
    .await
    .unwrap();
    assert_eq!(rows, 0, "rejected value must not persist");

    // A well-formed PEM is accepted (marker-only shape; the infra adapter
    // does the real parsing at connect time).
    let good_settings = json!({
        "enabled": true,
        "url": "ldaps://ldap.example.com:636",
        "baseDn": "dc=example,dc=com",
        "userFilter": "(uid={login})",
        "caCertPem": "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----",
    })
    .to_string();
    let resp = app
        .oneshot(batch_upsert_request(
            &ctx._realm_id,
            &token,
            json!([{
                "configType": "ldap",
                "configKey": "settings",
                "configValue": good_settings,
                "isSecret": false,
                "enabled": true
            }]),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "valid PEM must be accepted");
}

/// User Story: US-LD-003
/// Covers: US-LD-003 scenario 3 — an admin of realm A cannot manage realm
/// B's LDAP configuration (cross-realm 403 via AdminIdentity).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_config_cross_realm_rejected(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "ldap-cfg-cross@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let other_realm = uuid::Uuid::now_v7().to_string();
    let resp = app
        .clone()
        .oneshot(batch_upsert_request(
            &other_realm,
            &token,
            json!([{
                "configType": "ldap",
                "configKey": "settings",
                "configValue": valid_settings_json("ldaps://ldap.example.com:636", false),
                "isSecret": false,
                "enabled": true
            }]),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "cross-realm config write must be 403"
    );

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/configs/{other_realm}/ldap"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(request).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "cross-realm config read must be 403"
    );
}

/// User Story: US-LD-003
/// Covers: US-LD-003 scenario 2 — disabling (enabled=false) or deleting the
/// settings row flips the public status to false and login becomes 400,
/// while an account previously provisioned via LDAP still logs in with its
/// password (smooth degradation; accounts are never deleted).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_disable_degrades_and_keeps_other_logins(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    enable_ldap(ctx).await;

    // Provision an account via LDAP JIT (gives it a directory link) — the
    // mock directory user carries a mail we can also set a local password on.
    let email = format!("ld003-degrade-{}@test.com", uuid::Uuid::now_v7());
    let dn = "uid=degrade,dc=example,dc=com";
    let agreements = current_effective_agreements(ctx).await;
    let mock = mock_dir(one_mock_user("degrade", dn, Some(&email), "corp-pw"));
    let resp = ldap_login(ctx, &mock, "degrade", "corp-pw", Some(agreements)).await;
    assert_eq!(resp.status(), StatusCode::OK, "JIT provisioning must work");

    // Give the account a real local password so password login is possible.
    let bcrypt_hash =
        bcrypt::hash("local-password-123", bcrypt::DEFAULT_COST).expect("bcrypt hash");
    sqlx::query("UPDATE account SET password = $1 WHERE realm_id = $2 AND email = $3")
        .bind(&bcrypt_hash)
        .bind(&ctx._realm_id)
        .bind(&email)
        .execute(&ctx._app_state.pool)
        .await
        .unwrap();

    // Disable LDAP → status false, LDAP login 400.
    disable_ldap(ctx).await;
    assert_eq!(ldap_status(ctx).await["enabled"], false);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/auth/{}/login/ldap", ctx._realm_id))
                .header("content-type", "application/json")
                .header("x-forwarded-for", "6.6.6.6")
                .body(Body::from(
                    json!({
                        "clientId": ctx._client_id,
                        "username": "degrade",
                        "password": "corp-pw",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Password login still works for the same account.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/auth/{}/login", ctx._realm_id))
                .header("content-type", "application/json")
                .header("x-forwarded-for", "6.6.6.6")
                .body(Body::from(
                    json!({
                        "clientId": ctx._client_id,
                        "email": email,
                        "password": "local-password-123",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "disabled LDAP must not break other login methods for provisioned accounts"
    );

    // Deleting the settings row entirely behaves the same (fail-closed).
    delete_ldap_settings(ctx).await;
    assert_eq!(ldap_status(ctx).await["enabled"], false);
}

/// User Story: US-LD-003
/// Covers: Design §4.2.2 / D2-1 — the public status endpoint is fail-closed:
/// unconfigured → false; enabled config → true; a malformed settings row →
/// false (no 500).
#[test_context(TestContext)]
#[tokio::test]
async fn test_scenario_ldap_status_fail_closed(ctx: &mut TestContext) {
    // Unconfigured realm.
    assert_eq!(ldap_status(ctx).await["enabled"], false);

    // Malformed settings row → still false, not an error.
    insert_ldap_settings(ctx, &json!({ "enabled": true, "oops": true })).await;
    assert_eq!(
        ldap_status(ctx).await["enabled"],
        false,
        "malformed settings must degrade to disabled (fail-closed)"
    );

    // Legacy plaintext row (written before validation existed) → false.
    insert_ldap_settings(
        ctx,
        &json!({
            "enabled": true,
            "url": "ldap://legacy.example.com:389",
            "starttls": false,
            "baseDn": "dc=example,dc=com",
            "userFilter": "(uid={login})",
        }),
    )
    .await;
    assert_eq!(
        ldap_status(ctx).await["enabled"],
        false,
        "plaintext legacy row must be treated as not enabled at read time"
    );

    // Well-formed enabled row → true.
    enable_ldap(ctx).await;
    assert_eq!(ldap_status(ctx).await["enabled"], true);
}
