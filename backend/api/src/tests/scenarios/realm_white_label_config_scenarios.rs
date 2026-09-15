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

const WHITE_LABEL_PATH: &str = "/api/realms/{realm}/config/white-label";

fn white_label_uri(realm_id: &str, suffix: &str) -> String {
    format!(
        "{}{}",
        WHITE_LABEL_PATH.replace("{realm}", realm_id),
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

async fn insert_white_label_config(ctx: &TestContext, config_key: &str, config_value: Value) {
    sqlx::query(
        "INSERT INTO realm_config (realm_id, config_type, config_key, config_value, is_secret, enabled, metadata, created_at, updated_at)
         VALUES ($1, 'white_label', $2, $3, false, true, '{}'::jsonb, NOW(), NOW())
         ON CONFLICT (realm_id, config_type, config_key)
         DO UPDATE SET config_value = EXCLUDED.config_value, enabled = EXCLUDED.enabled, updated_at = NOW()",
    )
    .bind(&ctx._realm_id)
    .bind(config_key)
    .bind(config_value.to_string())
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to upsert white-label config");
}

async fn fetch_white_label_config(ctx: &TestContext, config_key: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = 'white_label' AND config_key = $2",
    )
    .bind(&ctx._realm_id)
    .bind(config_key)
    .fetch_optional(&ctx._app_state.pool)
    .await
    .expect("Failed to fetch white-label config")
}

fn white_label_config_value(overrides: &[(&str, Value)]) -> Value {
    let mut value = json!({
        "brandName": null,
        "logoUrl": null,
        "faviconUrl": null,
        "accentColor": null,
        "background": null,
        "footerText": null,
        "loginTitle": null,
        "loginSubtitle": null,
        "registerTitle": null,
        "registerSubtitle": null
    });

    let object = value.as_object_mut().unwrap();
    for (key, override_value) in overrides {
        object.insert((*key).to_string(), override_value.clone());
    }

    value
}

/// User Story: US-WL-001 — Realm Admin white-label settings must not be routed to another config type.
/// Covers: design §5.2 (`ConfigType::WhiteLabel` string mappings)
#[test]
fn white_label_config_type_string_mappings_are_registered() {
    assert_eq!(
        ConfigType::try_from_str("white_label"),
        Ok(ConfigType::WhiteLabel)
    );
    assert_eq!(String::from(ConfigType::WhiteLabel), "white_label");
    assert_eq!(ConfigType::WhiteLabel.as_ref(), "white_label");
}

/// User Story: US-WL-001 — Realm Admin opens branding settings before any configuration exists.
/// Covers: design §4.2.2 (GET management state defaults)
#[test_context(TestContext)]
#[tokio::test]
async fn white_label_get_returns_empty_state_when_unconfigured(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "white-label-empty@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let req = authed_request("GET", white_label_uri(&ctx._realm_id, ""), &token, None);

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(resp).await;
    assert_eq!(body["published"], white_label_config_value(&[]));
    assert!(body["draft"].is_null());
    assert_eq!(body["hasPrevious"], false);
    assert!(body["publishedUpdatedAt"].is_null());
    assert!(body["draftUpdatedAt"].is_null());
}

/// User Story: US-WL-001 — Realm Admin saves an unpublished branding draft.
/// Covers: design §4.2.2 (PUT draft), §4.5 (`settings.manage`)
#[test_context(TestContext)]
#[tokio::test]
async fn white_label_draft_requires_manage_and_does_not_publish(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (plain_token, _plain_user_id) =
        create_admin_session_with_user(ctx, "white-label-draft-plain@test.com", 1800).await;
    let draft = json!({
        "brandName": "Example",
        "logoUrl": "https://cdn.example.com/logo.svg",
        "faviconUrl": "https://cdn.example.com/favicon.ico",
        "accentColor": "#2563eb",
        "footerText": "Example Inc."
    });

    let forbidden_req = authed_request(
        "PUT",
        white_label_uri(&ctx._realm_id, "/draft"),
        &plain_token,
        Some(draft.clone()),
    );
    let forbidden_resp = app.clone().oneshot(forbidden_req).await.unwrap();
    assert_eq!(forbidden_resp.status(), StatusCode::FORBIDDEN);

    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "white-label-draft-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;

    let req = authed_request(
        "PUT",
        white_label_uri(&ctx._realm_id, "/draft"),
        &admin_token,
        Some(draft),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(resp).await;
    assert_eq!(body["message"], "White-label draft saved");
    assert_eq!(body["draft"]["logoUrl"], "https://cdn.example.com/logo.svg");
    assert_eq!(body["draft"]["brandName"], "Example");
    assert_eq!(
        body["draft"]["faviconUrl"],
        "https://cdn.example.com/favicon.ico"
    );
    assert!(fetch_white_label_config(ctx, "draft").await.is_some());
    assert!(
        fetch_white_label_config(ctx, "settings").await.is_none(),
        "Saving draft must not publish settings"
    );
}

/// User Story: US-WL-001 — Regular users cannot view Realm Admin white-label state.
/// Covers: design §4.2.2 (GET management state), §4.5 (`settings.view`)
#[test_context(TestContext)]
#[tokio::test]
async fn white_label_get_requires_view_and_forbids_plain_user(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, _user_id) =
        create_admin_session_with_user(ctx, "white-label-view-plain@test.com", 1800).await;

    let req = authed_request("GET", white_label_uri(&ctx._realm_id, ""), &token, None);

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// User Story: US-WL-001 — White-label settings are isolated to the user's own Realm.
/// Covers: design §4.5 (cross-Realm access forbidden)
#[test_context(TestContext)]
#[tokio::test]
async fn white_label_cross_realm_access_is_forbidden(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "white-label-cross-realm@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;
    let other_realm_id = uuid::Uuid::now_v7().to_string();

    let get_req = authed_request("GET", white_label_uri(&other_realm_id, ""), &token, None);
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::FORBIDDEN);

    let put_req = authed_request(
        "PUT",
        white_label_uri(&other_realm_id, "/draft"),
        &token,
        Some(json!({"logoUrl": "https://cdn.example.com/logo.svg"})),
    );
    let put_resp = app.oneshot(put_req).await.unwrap();
    assert_eq!(put_resp.status(), StatusCode::FORBIDDEN);
}

/// User Story: US-WL-001 — Realm Admin discards drafts without changing published branding.
/// Covers: design §4.2.2 (DELETE draft idempotence)
#[test_context(TestContext)]
#[tokio::test]
async fn white_label_delete_draft_is_idempotent_and_preserves_published(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "white-label-delete-draft@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;
    let published = json!({"logoUrl": "https://cdn.example.com/published.svg"});
    insert_white_label_config(ctx, "settings", published.clone()).await;
    insert_white_label_config(
        ctx,
        "draft",
        json!({"logoUrl": "https://cdn.example.com/draft.svg"}),
    )
    .await;

    for _ in 0..2 {
        let req = authed_request(
            "DELETE",
            white_label_uri(&ctx._realm_id, "/draft"),
            &token,
            None,
        );
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body: Value = crate::tests::response_json(resp).await;
        assert_eq!(body["published"]["logoUrl"], published["logoUrl"]);
        assert!(body["draft"].is_null());
    }

    assert_eq!(
        serde_json::from_str::<Value>(&fetch_white_label_config(ctx, "settings").await.unwrap())
            .unwrap(),
        published
    );
    assert!(fetch_white_label_config(ctx, "draft").await.is_none());
}

/// User Story: US-WL-001 — Realm Admin publishes a draft while keeping one-step rollback available.
/// Covers: design §4.2.2 (POST publish), §5.1 (`settings`/`draft`/`previous_settings`)
#[test_context(TestContext)]
#[tokio::test]
async fn white_label_publish_draft_clears_draft_and_preserves_previous(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "white-label-publish@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;
    let old_settings = json!({"loginTitle": "Old Login"});
    let draft = json!({"loginTitle": "New Login", "accentColor": "#16a34a"});
    insert_white_label_config(ctx, "settings", old_settings.clone()).await;
    insert_white_label_config(ctx, "draft", draft.clone()).await;

    let req = authed_request(
        "POST",
        white_label_uri(&ctx._realm_id, "/publish"),
        &token,
        None,
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(resp).await;
    assert_eq!(body["published"]["loginTitle"], "New Login");
    assert!(body["draft"].is_null());
    assert_eq!(body["hasPrevious"], true);
    assert_eq!(
        serde_json::from_str::<Value>(&fetch_white_label_config(ctx, "settings").await.unwrap())
            .unwrap(),
        white_label_config_value(&[
            ("loginTitle", json!("New Login")),
            ("accentColor", json!("#16a34a")),
        ])
    );
    assert_eq!(
        serde_json::from_str::<Value>(
            &fetch_white_label_config(ctx, "previous_settings")
                .await
                .unwrap()
        )
        .unwrap(),
        white_label_config_value(&[("loginTitle", json!("Old Login"))])
    );
    assert!(fetch_white_label_config(ctx, "draft").await.is_none());
}

/// User Story: US-WL-001 — Publishing requires an explicit body or an existing draft.
/// Covers: design §4.2.2 (POST publish bad request)
#[test_context(TestContext)]
#[tokio::test]
async fn white_label_publish_without_body_or_draft_returns_bad_request(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "white-label-publish-empty@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let req = authed_request(
        "POST",
        white_label_uri(&ctx._realm_id, "/publish"),
        &token,
        None,
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// User Story: US-WL-001 — Realm Admin can restore the previous published white-label settings.
/// Covers: design §4.2.2 (POST restore), §5.1 (`previous_settings`)
#[test_context(TestContext)]
#[tokio::test]
async fn white_label_restore_previous_settings_or_returns_bad_request(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "white-label-restore@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let missing_req = authed_request(
        "POST",
        white_label_uri(&ctx._realm_id, "/restore"),
        &token,
        None,
    );
    let missing_resp = app.clone().oneshot(missing_req).await.unwrap();
    assert_eq!(missing_resp.status(), StatusCode::BAD_REQUEST);

    let current = json!({"loginTitle": "Broken Brand"});
    let previous = json!({"loginTitle": "Stable Brand"});
    insert_white_label_config(ctx, "settings", current.clone()).await;
    insert_white_label_config(ctx, "previous_settings", previous.clone()).await;

    let restore_req = authed_request(
        "POST",
        white_label_uri(&ctx._realm_id, "/restore"),
        &token,
        None,
    );
    let restore_resp = app.oneshot(restore_req).await.unwrap();
    assert_eq!(restore_resp.status(), StatusCode::OK);

    let body: Value = crate::tests::response_json(restore_resp).await;
    assert_eq!(body["published"]["loginTitle"], "Stable Brand");
    assert_eq!(body["hasPrevious"], true);
    assert_eq!(
        serde_json::from_str::<Value>(&fetch_white_label_config(ctx, "settings").await.unwrap())
            .unwrap(),
        white_label_config_value(&[("loginTitle", json!("Stable Brand"))])
    );
    assert_eq!(
        serde_json::from_str::<Value>(
            &fetch_white_label_config(ctx, "previous_settings")
                .await
                .unwrap()
        )
        .unwrap(),
        white_label_config_value(&[("loginTitle", json!("Broken Brand"))])
    );
}

/// User Story: US-WL-004 — Realm Admin cannot save unsafe asset URLs or unsupported background CSS.
/// Covers: design §4.5 (URL scheme and gradient validation)
#[test_context(TestContext)]
#[tokio::test]
async fn white_label_rejects_invalid_asset_url_and_gradient(ctx: &mut TestContext) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "white-label-validation@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;

    let invalid_url_req = authed_request(
        "PUT",
        white_label_uri(&ctx._realm_id, "/draft"),
        &token,
        Some(json!({"logoUrl": "javascript:alert(1)"})),
    );
    let invalid_url_resp = app.clone().oneshot(invalid_url_req).await.unwrap();
    assert_eq!(invalid_url_resp.status(), StatusCode::BAD_REQUEST);

    let invalid_favicon_req = authed_request(
        "PUT",
        white_label_uri(&ctx._realm_id, "/draft"),
        &token,
        Some(json!({"faviconUrl": "data:image/svg+xml,unsafe"})),
    );
    let invalid_favicon_resp = app.clone().oneshot(invalid_favicon_req).await.unwrap();
    assert_eq!(invalid_favicon_resp.status(), StatusCode::BAD_REQUEST);

    let invalid_gradient_req = authed_request(
        "PUT",
        white_label_uri(&ctx._realm_id, "/draft"),
        &token,
        Some(json!({
            "background": {
                "type": "gradient",
                "value": "conic-gradient(red, blue)"
            }
        })),
    );
    let invalid_gradient_resp = app.oneshot(invalid_gradient_req).await.unwrap();
    assert_eq!(invalid_gradient_resp.status(), StatusCode::BAD_REQUEST);
}

/// User Story: US-WL-001 — white-label rows must only flow through the dedicated
/// draft/publish/restore lifecycle, never the generic configs API.
/// Covers: ui-custom PRD §6 capability list (no generic delete) and the
/// reject chain shared by upsert/batch/delete in the configs handler.
///
/// WHY: a `settings.manage` caller deleting `settings` (or the
/// `previous_settings` recovery snapshot) through
/// `DELETE /api/configs/{realm}/white_label/{key}` would clear published
/// branding and break restore without ever passing the dedicated surface's
/// validation — the same bypass the upsert rejection exists to prevent, so
/// every generic write path (PUT and DELETE alike) must refuse the type.
#[test_context(TestContext)]
#[tokio::test]
async fn white_label_rows_cannot_be_written_or_deleted_via_generic_configs_api(
    ctx: &mut TestContext,
) {
    let app = ctx.create_unified_test_router();
    let (token, user_id) =
        create_admin_session_with_user(ctx, "white-label-generic-api@test.com", 1800).await;
    grant_realm_admin_role(ctx, &user_id).await;
    let published = json!({"loginTitle": "Published Brand"});
    insert_white_label_config(ctx, "settings", published.clone()).await;
    insert_white_label_config(ctx, "previous_settings", json!({"loginTitle": "Old Brand"})).await;

    let put_req = authed_request(
        "PUT",
        format!("/api/configs/{}", ctx._realm_id),
        &token,
        Some(json!({
            "configType": "white_label",
            "configKey": "settings",
            "configValue": "{\"loginTitle\":\"Smuggled Brand\"}"
        })),
    );
    let put_resp = app.clone().oneshot(put_req).await.unwrap();
    assert_eq!(put_resp.status(), StatusCode::BAD_REQUEST);

    for key in ["settings", "previous_settings"] {
        let delete_req = authed_request(
            "DELETE",
            format!("/api/configs/{}/white_label/{}", ctx._realm_id, key),
            &token,
            None,
        );
        let delete_resp = app.clone().oneshot(delete_req).await.unwrap();
        assert_eq!(
            delete_resp.status(),
            StatusCode::BAD_REQUEST,
            "generic delete of white_label/{key} must be refused"
        );
    }

    // The bypass must not have touched the lifecycle rows.
    assert_eq!(
        serde_json::from_str::<Value>(&fetch_white_label_config(ctx, "settings").await.unwrap())
            .unwrap(),
        published
    );
    assert!(
        fetch_white_label_config(ctx, "previous_settings")
            .await
            .is_some(),
        "recovery snapshot must survive the refused deletes"
    );
}
