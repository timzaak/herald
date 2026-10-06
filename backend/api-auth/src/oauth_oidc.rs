//! Shared helpers for the minimal OpenID Connect identity layer layered on
//! the existing Authorization Code + PKCE flow.
//!
//! The only scope semantics Herald implements is presence detection of the
//! literal `openid` token; any other scope tokens are carried through
//! verbatim and never parsed, validated, or rejected.

/// Append the authorization `code` and `state` pair to a downstream redirect
/// URI as properly percent-encoded query parameters.
///
/// `state` is client-controlled (it round-trips through the browser), so raw
/// string concatenation lets `&`/`#`/`%` inside it truncate or rewrite the
/// query. Every login entrance that hands a code back to the OAuth
/// redirect_uri must build the URL through here.
pub fn append_code_and_state(redirect_uri: &str, code: &str, state: &str) -> String {
    match url::Url::parse(redirect_uri) {
        Ok(mut url) => {
            url.query_pairs_mut()
                .append_pair("code", code)
                .append_pair("state", state);
            url.into()
        }
        // The redirect_uri was validated at /authorize time, so this arm is
        // unreachable in practice; keep the legacy concatenation rather than
        // failing the whole login on a defensive path.
        Err(_) => format!("{redirect_uri}?code={code}&state={state}"),
    }
}

/// Whether a space-delimited scope string requests the `openid` scope.
/// OIDC scope tokens are case-sensitive: `OPENID` or `openidprofile` do not
/// count.
pub fn scope_requests_openid(scope: Option<&str>) -> bool {
    scope.is_some_and(|s| s.split_whitespace().any(|token| token == "openid"))
}

/// The optional authorize-transaction parameters that ride the state and
/// authorization-code records: the OIDC `scope`/`nonce` pair and the MCP
/// RFC 8707 `resource` indicator.
#[derive(Debug, Clone, Copy, Default)]
pub struct OptionalAuthorizeParams<'a> {
    pub scope: Option<&'a str>,
    pub nonce: Option<&'a str>,
    pub resource: Option<&'a str>,
}

/// Add the optional OIDC `scope`/`nonce` and MCP `resource` parameters to a
/// JSON record being written.
///
/// Keys are only added when the parameter is present, so a flow that never
/// carried them produces a byte-identical record — the zero-regression
/// guarantee for pre-OIDC clients.
pub fn insert_optional_oidc_fields(
    value: &mut serde_json::Value,
    params: OptionalAuthorizeParams<'_>,
) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for (field, param) in [
        ("scope", params.scope),
        ("nonce", params.nonce),
        ("resource", params.resource),
    ] {
        if let Some(param) = param {
            object.insert(field.to_string(), serde_json::Value::from(param));
        }
    }
}

/// Serialize the authorization-code record stored under `oauth:code:{code}` —
/// the write side of the `/token` read contract. One builder for every login
/// entrance and the OAuth downstream path, so the five base fields plus the
/// optional OIDC/MCP parameters can only drift in one place.
pub fn build_oauth_code_record(
    code_challenge: &str,
    client_id: &str,
    redirect_uri: &str,
    user_id: &str,
    realm_id: &str,
    params: OptionalAuthorizeParams<'_>,
) -> String {
    let mut value = serde_json::json!({
        "code_challenge": code_challenge,
        "client_id": client_id,
        "redirect_uri": redirect_uri,
        "user_id": user_id,
        "realm_id": realm_id,
    });
    insert_optional_oidc_fields(&mut value, params);
    value.to_string()
}

/// [`build_oauth_code_record`] with the OIDC/MCP parameters sourced from the
/// stored authorize-state JSON (the login-entrance shape): `scope`/`nonce`/
/// `resource` are copied from the state record, everything else is passed
/// explicitly. The authorize endpoint only copies these fields from its own
/// server-side state — login pages and callbacks can never grant them.
pub fn build_oauth_code_record_from_state(
    code_challenge: &str,
    client_id: &str,
    redirect_uri: &str,
    user_id: &str,
    realm_id: &str,
    state_data: &serde_json::Value,
) -> String {
    build_oauth_code_record(
        code_challenge,
        client_id,
        redirect_uri,
        user_id,
        realm_id,
        OptionalAuthorizeParams {
            scope: state_data.get("scope").and_then(serde_json::Value::as_str),
            nonce: state_data.get("nonce").and_then(serde_json::Value::as_str),
            resource: state_data
                .get("resource")
                .and_then(serde_json::Value::as_str),
        },
    )
}

/// Re-verify that the OAuth Client App a pending authorization transaction
/// belongs to is still enabled, at login completion. `/authorize` checked it
/// when the flow started, but the state (and the user's browser) can outlive
/// a disable by minutes; a disabled MCP client must not mint codes from
/// in-flight flows. Non-MCP clients keep their existing behavior (the token
/// exchange remains the authoritative enablement gate).
pub async fn ensure_mcp_client_still_enabled(
    state: &herald_api_base::application::http::state::AppState,
    realm_id: &str,
    oauth_client_id: &str,
) -> Result<(), herald_api_base::application::http::server::api_entities::ApiError> {
    use herald_core::domain::client::ports::ClientService;

    if !herald_core::domain::client::is_mcp_client(oauth_client_id) {
        return Ok(());
    }
    let client_app = state
        .service
        .client_service()
        .get_client_app_by_client_id(realm_id, oauth_client_id)
        .await
        .map_err(|error| match error {
            // A missing row means the seed is gone; that reads as "not
            // enabled" for the in-flight flow this guard exists for.
            herald_core::domain::common::entities::app_errors::CoreError::NotFound => {
                herald_api_base::application::http::server::api_entities::ApiError::bad_request(
                    "OAuth client app is not enabled".to_string(),
                )
            }
            // An infrastructure failure must surface as 5xx, not as a
            // permanent-looking "client disabled".
            _ => herald_api_base::application::http::server::api_entities::ApiError::internal(
                "Internal server error".to_string(),
            ),
        })?;
    if !client_app.enabled {
        return Err(
            herald_api_base::application::http::server::api_entities::ApiError::forbidden(
                "Client app is disabled".to_string(),
            ),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // WHY: the detection matrix is the entire scope semantics of the OIDC
    // layer — everything else must pass through untouched. Each row pins a
    // distinct rule: absent vs empty, token boundaries (so "openidprofile"
    // is not a match), and OIDC's case-sensitive scope-token comparison.
    #[test]
    fn scope_detection_matches_only_the_literal_openid_token() {
        assert!(!scope_requests_openid(None));
        assert!(!scope_requests_openid(Some("")));
        assert!(scope_requests_openid(Some("openid")));
        assert!(scope_requests_openid(Some("profile openid")));
        assert!(scope_requests_openid(Some(" openid ")));
        assert!(scope_requests_openid(Some("email profile openid")));
        assert!(!scope_requests_openid(Some("OPENID")));
        assert!(!scope_requests_openid(Some("Openid")));
        assert!(!scope_requests_openid(Some("openidprofile")));
        assert!(!scope_requests_openid(Some("profile")));
    }

    // WHY: the code record is the /token read contract — the builder must copy
    // the OIDC parameters from the state record verbatim and keep every
    // pre-existing base field intact.
    #[test]
    fn state_record_builder_copies_scope_and_nonce_when_present() {
        let state = json!({
            "client_id": "client",
            "scope": "openid profile",
            "nonce": "n-123",
        });
        let record = build_oauth_code_record_from_state(
            "challenge",
            "client",
            "https://app.example/cb",
            "u-1",
            "r-1",
            &state,
        );
        let value: serde_json::Value = serde_json::from_str(&record).unwrap();
        assert_eq!(value["scope"], json!("openid profile"));
        assert_eq!(value["nonce"], json!("n-123"));
        assert_eq!(value["code_challenge"], json!("challenge"));
        assert_eq!(value["client_id"], json!("client"));
        assert_eq!(value["redirect_uri"], json!("https://app.example/cb"));
        assert_eq!(value["user_id"], json!("u-1"));
        assert_eq!(value["realm_id"], json!("r-1"));
    }

    // WHY: this is the zero-regression anchor — an authorization flow without
    // OIDC parameters must serialize to exactly the same code record bytes as
    // before the OIDC layer existed, or existing clients' behavior changes.
    #[test]
    fn state_record_builder_without_oidc_fields_matches_pre_oidc_bytes() {
        let state = json!({
            "client_id": "client",
            "realm_id": "r-1",
            "code_challenge": "challenge",
        });
        let record = build_oauth_code_record_from_state(
            "challenge",
            "client",
            "https://app.example/cb",
            "u-1",
            "r-1",
            &state,
        );
        assert_eq!(
            record,
            r#"{"client_id":"client","code_challenge":"challenge","realm_id":"r-1","redirect_uri":"https://app.example/cb","user_id":"u-1"}"#
        );
    }

    // WHY: a state record whose scope/nonce are JSON null must behave like
    // absent keys — null must never leak into the code record.
    #[test]
    fn state_record_builder_ignores_null_state_values() {
        let state = json!({"scope": serde_json::Value::Null, "nonce": null});
        let record = build_oauth_code_record_from_state(
            "challenge",
            "client",
            "https://app.example/cb",
            "u-1",
            "r-1",
            &state,
        );
        let value: serde_json::Value = serde_json::from_str(&record).unwrap();
        assert!(value.get("scope").is_none());
        assert!(value.get("nonce").is_none());
    }

    // WHY: authorize and the downstream code writer feed Options (not a
    // state object) into the same zero-regression rule — None must add no
    // key, Some must add exactly the given string.
    #[test]
    fn insert_optional_fields_writes_only_present_parameters() {
        let mut value = json!({"client_id": "client"});
        let before = value.to_string();
        insert_optional_oidc_fields(&mut value, OptionalAuthorizeParams::default());
        assert_eq!(value.to_string(), before);

        insert_optional_oidc_fields(
            &mut value,
            OptionalAuthorizeParams {
                scope: Some("openid"),
                nonce: Some("n-1"),
                resource: None,
            },
        );
        assert_eq!(value["scope"], json!("openid"));
        assert_eq!(value["nonce"], json!("n-1"));
        assert!(value.get("resource").is_none());
    }

    // WHY: the resource key is the MCP audience binding the token exchange
    // re-checks — it must ride the state → code path verbatim, and a null
    // state value must behave like an absent key.
    #[test]
    fn state_record_builder_copies_resource_when_present() {
        let state = json!({
            "client_id": "herald-mcp",
            "scope": "mcp:profile:read",
            "resource": "https://herald.example/mcp/acme",
        });
        let record = build_oauth_code_record_from_state(
            "challenge",
            "herald-mcp",
            "http://127.0.0.1:43119/callback",
            "u-1",
            "acme",
            &state,
        );
        let value: serde_json::Value = serde_json::from_str(&record).unwrap();
        assert_eq!(value["resource"], json!("https://herald.example/mcp/acme"));

        let null_resource = json!({"resource": serde_json::Value::Null});
        let record = build_oauth_code_record_from_state(
            "challenge",
            "c",
            "https://app/cb",
            "u-1",
            "r-1",
            &null_resource,
        );
        let value: serde_json::Value = serde_json::from_str(&record).unwrap();
        assert!(value.get("resource").is_none());
    }
}
