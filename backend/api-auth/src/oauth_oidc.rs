//! Shared helpers for the minimal OpenID Connect identity layer layered on
//! the existing Authorization Code + PKCE flow.
//!
//! The only scope semantics Herald implements is presence detection of the
//! literal `openid` token; any other scope tokens are carried through
//! verbatim and never parsed, validated, or rejected.

/// Whether a space-delimited scope string requests the `openid` scope.
/// OIDC scope tokens are case-sensitive: `OPENID` or `openidprofile` do not
/// count.
pub fn scope_requests_openid(scope: Option<&str>) -> bool {
    scope.is_some_and(|s| s.split_whitespace().any(|token| token == "openid"))
}

/// Add the optional OIDC `scope`/`nonce` parameters to a JSON record being
/// written.
///
/// Keys are only added when the parameter is present, so a flow that never
/// carried them produces a byte-identical record — the zero-regression
/// guarantee for pre-OIDC clients.
pub fn insert_optional_oidc_fields(
    value: &mut serde_json::Value,
    scope: Option<&str>,
    nonce: Option<&str>,
) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for (field, param) in [("scope", scope), ("nonce", nonce)] {
        if let Some(param) = param {
            object.insert(field.to_string(), serde_json::Value::from(param));
        }
    }
}

/// Serialize the authorization-code record stored under `oauth:code:{code}` —
/// the write side of the `/token` read contract. One builder for every login
/// entrance and the OAuth downstream path, so the five base fields plus the
/// optional OIDC parameters can only drift in one place.
pub fn build_oauth_code_record(
    code_challenge: &str,
    client_id: &str,
    redirect_uri: &str,
    user_id: &str,
    realm_id: &str,
    scope: Option<&str>,
    nonce: Option<&str>,
) -> String {
    let mut value = serde_json::json!({
        "code_challenge": code_challenge,
        "client_id": client_id,
        "redirect_uri": redirect_uri,
        "user_id": user_id,
        "realm_id": realm_id,
    });
    insert_optional_oidc_fields(&mut value, scope, nonce);
    value.to_string()
}

/// [`build_oauth_code_record`] with the OIDC parameters sourced from the
/// stored authorize-state JSON (the login-entrance shape): `scope`/`nonce`
/// are copied from the state record, everything else is passed explicitly.
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
        state_data.get("scope").and_then(serde_json::Value::as_str),
        state_data.get("nonce").and_then(serde_json::Value::as_str),
    )
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
        insert_optional_oidc_fields(&mut value, None, None);
        assert_eq!(value.to_string(), before);

        insert_optional_oidc_fields(&mut value, Some("openid"), Some("n-1"));
        assert_eq!(value["scope"], json!("openid"));
        assert_eq!(value["nonce"], json!("n-1"));
    }
}
