// OAuth resource-server authentication + scope preflight for /mcp/{realmId}.
//
// Transport form: `Authorization: Bearer <opaque MCP user token>` only. A
// Bearer credential is never interpreted as a Client API Key (that path is
// gone), and browser credentials are rejected by the audience gate inside
// `authenticate_bearer_for` — the MCP face requires the exact canonical
// resource URI and a current token generation.
//
// Pipeline (backend.md §9.4): realm/canonical resolution → OAuth Bearer
// verification → per-user rate limit → (POST) bounded body buffering →
// strict JSON-RPC preflight → restore the same body and inject
// Identity/Context/ScopePreflight → rmcp. Failures before rmcp render as
// RFC 9728-aware challenges (WWW-Authenticate with resource_metadata) so
// compliant agent clients can bootstrap the browser authorization flow.

use axum::{
    body::Body,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use herald_api_base::application::http::auth::identity_middleware::authenticate_bearer_for;
use herald_api_base::application::http::common::public_helper::{
    realm_exists, realm_public_url_parts,
};
use herald_api_base::application::http::rate_limit::{RateLimitConfig, rate_limit};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{CredentialScope, ExpectedTokenAudience};
use herald_core::domain::client::{mcp_prm_uri, mcp_resource_uri};
use serde::Deserializer as _;
use serde_json::json;
use std::collections::HashSet;
use tracing::{info, warn};

/// Per-user quota: 60 requests / 60s. Read-only, agent-paced traffic —
/// comfortably above interactive use, far below abuse throughput. The key is
/// the authenticated user id scoped to the realm, so quotas never leak
/// across users or realms. Enforced in dev/test so scenario suites exercise
/// the real path.
pub const MCP_RATE_LIMIT: RateLimitConfig = RateLimitConfig {
    max_requests: 60,
    window_secs: 60,
    enforce_in_dev: true,
};

/// JSON-RPC POST body buffer ceiling, unified with rmcp's own limit.
const MCP_MAX_BODY_BYTES: usize = 1024 * 1024;

/// Marker inserted for an authenticated `tools/call` whose scope preflight
/// already passed. The matching tool handler re-checks tool-name equality —
/// a missing or drifted marker is an internal contract error (zero reads),
/// never a silent downgrade to a tool-level message.
#[derive(Debug, Clone)]
pub struct ScopePreflight {
    pub tool: &'static str,
}

#[derive(Debug)]
enum PreflightError {
    /// Unparseable JSON envelope.
    Malformed,
    /// JSON-RPC batch array: rejected whole, nothing dispatched.
    Batch,
    /// Duplicate object keys anywhere in the envelope (outside/rmcp
    /// interpretation drift).
    DuplicateKeys,
    /// A known self tool called without its required scope. Carries the
    /// required scope wire token.
    InsufficientScope(&'static str),
}

/// The four self-face tools and their required scopes. Admin tools and any
/// unknown name require no transport-level scope (their gate is the
/// tool-layer RBAC check / rmcp's unknown-tool error).
fn required_scope_for_tool(name: &str) -> Option<(&'static str, CredentialScope)> {
    match name {
        "get_my_profile" => Some(("get_my_profile", CredentialScope::McpProfileRead)),
        "get_my_points_balance" => Some(("get_my_points_balance", CredentialScope::McpPointsRead)),
        "list_my_points_transactions" => Some((
            "list_my_points_transactions",
            CredentialScope::McpTransactionsRead,
        )),
        "list_my_subscriptions" => Some((
            "list_my_subscriptions",
            CredentialScope::McpSubscriptionsRead,
        )),
        _ => None,
    }
}

/// Extract the realm path segment from `/mcp/{realmId}`. Anything else (no
/// realm, empty, multi-segment) is not this endpoint's shape — the router
/// already rejects most of these; the middleware re-checks defensively.
fn realm_from_path(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/mcp/")?;
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(rest)
}

/// Per-request DNS-rebinding gate for the transport, replacing rmcp's static
/// allowed-hosts list (which cannot track custom domains enabled at
/// runtime): the request Host must be the realm's canonical origin host —
/// the same `realm_public_url_parts` result the audience binding uses — or
/// a loopback literal for dev/test. The Host never feeds the audience; this
/// only stops rebound or foreign hosts from reaching the protocol layer.
fn host_is_acceptable(host_header: &str, canonical_origin: &str) -> bool {
    let Ok(authority) = axum::http::uri::Authority::try_from(host_header.trim()) else {
        return false;
    };
    let request_host = authority.host().to_ascii_lowercase();
    if request_host == "localhost" {
        return true;
    }
    // `Authority::host()` keeps the brackets on IPv6 literals while
    // `IpAddr::from_str` rejects them — unbracket only for the parse; the
    // origin comparison below stays bracketed on both sides (url's
    // `host_str()` keeps brackets too).
    let ip_literal = request_host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(&request_host);
    if let Ok(ip) = ip_literal.parse::<std::net::IpAddr>()
        && ip.is_loopback()
    {
        return true;
    }
    let Ok(origin_url) = url::Url::parse(canonical_origin) else {
        return false;
    };
    origin_url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case(&request_host))
}

fn bearer_challenge(resource_metadata: &str, error: Option<&str>, scope: Option<&str>) -> String {
    let mut value = format!("Bearer resource_metadata=\"{resource_metadata}\"");
    if let Some(error) = error {
        value.push_str(&format!(", error=\"{error}\""));
    }
    if let Some(scope) = scope {
        value.push_str(&format!(", scope=\"{scope}\""));
    }
    value
}

fn challenge_body(error: &str, description: &str) -> Body {
    Body::from(
        json!({
            "error": error,
            "error_description": description,
        })
        .to_string(),
    )
}

/// Assemble a challenge response; the WWW-Authenticate value embeds the
/// configured origin, which an operator could set to a non-ASCII string —
/// answering 500 beats panicking the connection on every unauthenticated
/// request, so the header value is validated instead of expected.
fn challenge_response(
    status: StatusCode,
    challenge: String,
    error: &str,
    description: &str,
) -> Response {
    match header::HeaderValue::from_str(&challenge) {
        Ok(value) => Response::builder()
            .status(status)
            .header(header::WWW_AUTHENTICATE, value)
            .header(header::CONTENT_TYPE, "application/json")
            .body(challenge_body(error, description))
            .expect("static challenge response parts"),
        Err(_) => plain_json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "The request could not be completed. Please retry later.",
        ),
    }
}

/// 401 with the RFC 9728 bootstrap challenge. A missing token on the initial
/// connection also hints the minimal scope so the client can request the
/// smallest useful authorization.
fn unauthorized_challenge(resource_metadata: &str, token_was_present: bool) -> Response {
    let (error, description, scope_hint) = if token_was_present {
        (
            Some("invalid_token"),
            "The access token is invalid, expired, revoked, or issued for a different resource.",
            None,
        )
    } else {
        (
            None,
            "A bearer access token for this MCP resource is required.",
            Some(herald_core::domain::authentication::MCP_DEFAULT_SCOPE),
        )
    };
    let challenge = bearer_challenge(resource_metadata, error, scope_hint);
    challenge_response(
        StatusCode::UNAUTHORIZED,
        challenge,
        error.unwrap_or("invalid_request"),
        description,
    )
}

/// 403 with the RFC 6750 insufficient_scope challenge: the client must
/// re-authorize with the named scope; no data has been read.
fn insufficient_scope_challenge(resource_metadata: &str, required_scope: &str) -> Response {
    let challenge = bearer_challenge(
        resource_metadata,
        Some("insufficient_scope"),
        Some(required_scope),
    );
    challenge_response(
        StatusCode::FORBIDDEN,
        challenge,
        "insufficient_scope",
        "The access token is missing a required scope; restart the authorization flow with the requested scope.",
    )
}

fn plain_json_error(status: StatusCode, error: &str, description: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(challenge_body(error, description))
        .expect("static error response parts")
}

pub async fn mcp_oauth_auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(realm_id) = realm_from_path(req.uri().path()) else {
        return plain_json_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "Unknown MCP endpoint shape.",
        );
    };
    let realm_id = realm_id.to_string();

    // Unknown realm: 404 before any challenge, so an agent never bootstraps
    // an authorization flow against a nonexistent tenant. An infrastructure
    // failure is NOT a missing realm — collapsing the two would tell
    // RFC 9728 clients the tenant permanently does not exist.
    match realm_exists(&state.pool, &realm_id).await {
        Ok(true) => {}
        Ok(false) => {
            return plain_json_error(StatusCode::NOT_FOUND, "not_found", "Unknown realm.");
        }
        Err(error) => {
            warn!(realm_id = %realm_id, error = %error, "MCP realm lookup failed");
            return plain_json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "The request could not be completed. Please retry later.",
            );
        }
    }

    // Canonical resource and PRM from the configured/enabled-custom-domain
    // origin — never from request Host headers.
    let origin = match realm_public_url_parts(&state, &realm_id).await {
        Ok((origin, _)) => origin,
        Err(_) => {
            return plain_json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "The request could not be completed. Please retry later.",
            );
        }
    };
    let canonical = mcp_resource_uri(&origin, &realm_id);
    let prm = mcp_prm_uri(&origin, &realm_id);

    // Host gate (see host_is_acceptable): the transport's static list cannot
    // follow custom domains enabled at runtime, so the check lives here with
    // the same live canonical the audience uses.
    let request_host = req
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !host_is_acceptable(request_host, &origin) {
        warn!(realm_id = %realm_id, host = %request_host, "MCP request Host rejected");
        return plain_json_error(
            StatusCode::FORBIDDEN,
            "invalid_host",
            "The request Host is not an accepted origin for this realm's MCP endpoint.",
        );
    }

    // OAuth Bearer verification against the MCP face. The token was never an
    // API key; a Bearer missing entirely is the bootstrap case.
    let token_was_present = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| !value.trim().is_empty());
    let (identity, credential_context) = match authenticate_bearer_for(
        &state,
        req.headers(),
        &ExpectedTokenAudience::McpResource(canonical),
    )
    .await
    {
        Ok(pair) => pair,
        Err(_) => {
            warn!(realm_id = %realm_id, "MCP request failed OAuth bearer verification");
            return unauthorized_challenge(&prm, token_was_present);
        }
    };

    // Per-user rate limit after successful auth (quota key is the
    // authenticated user, scoped to the realm).
    if let Err(e) = rate_limit(
        &state,
        format!("rl:mcp:{realm_id}:{}", identity.user_id()),
        MCP_RATE_LIMIT,
    )
    .await
    {
        warn!(
            realm_id = %realm_id,
            user_id = %identity.user_id(),
            "MCP rate limit exceeded"
        );
        return e.into_response();
    }

    let mut req = req;
    req.extensions_mut().insert(identity);
    req.extensions_mut().insert(credential_context.clone());

    // POST carries the JSON-RPC envelope: buffer it (bounded), run the
    // strict preflight, then hand the SAME bytes to rmcp.
    if req.method() == axum::http::Method::POST {
        let (mut parts, body) = req.into_parts();
        let bytes = match axum::body::to_bytes(body, MCP_MAX_BODY_BYTES).await {
            Ok(bytes) => bytes,
            Err(_) => {
                return plain_json_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "request_too_large",
                    "The request body exceeds the 1 MiB limit.",
                );
            }
        };

        match preflight(&bytes, &credential_context.allowed_scopes) {
            Ok(marker) => {
                if let Some(marker) = marker {
                    parts.extensions.insert(marker);
                }
            }
            Err(PreflightError::InsufficientScope(required_scope)) => {
                // Rejected before any business read — the client can only
                // proceed by re-authorizing with the scope.
                return insufficient_scope_challenge(&prm, required_scope);
            }
            Err(PreflightError::Batch) => {
                return plain_json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "JSON-RPC batch requests are not supported on this endpoint.",
                );
            }
            Err(PreflightError::DuplicateKeys) => {
                return plain_json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "The JSON-RPC envelope contains duplicate keys.",
                );
            }
            Err(PreflightError::Malformed) => {
                return plain_json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "The request body is not a valid JSON-RPC envelope.",
                );
            }
        }

        req = Request::from_parts(parts, Body::from(bytes));
    }

    info!(realm_id = %realm_id, "MCP request authenticated");
    next.run(req).await
}

/// Strict JSON-RPC envelope preflight (POST bodies only).
///
/// * the body must parse as one JSON object (arrays = batch, rejected whole;
///   scalars = malformed);
/// * no object anywhere in the envelope may carry duplicate keys — serde's
///   `Value` would silently collapse them, creating an interpretation gap
///   between this preflight and rmcp's typed parse;
/// * for `method == "tools/call"` with a known self tool name, the required
///   scope must already be on the credential, else `InsufficientScope`
///   (rendered as the 403 challenge by the caller, before any read);
/// * malformed `method`/`params` shapes are NOT judged here — rmcp's own
///   invalid-params error is the response contract for those.
fn preflight(
    body: &[u8],
    scopes: &HashSet<CredentialScope>,
) -> Result<Option<ScopePreflight>, PreflightError> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| PreflightError::Malformed)?;
    let Some(object) = value.as_object() else {
        return Err(match value {
            serde_json::Value::Array(_) => PreflightError::Batch,
            _ => PreflightError::Malformed,
        });
    };

    // The JSON is syntactically valid at this point, so any strict-scan
    // failure is a duplicate key (serde's `Value` above already collapsed
    // it — the scan reads the raw bytes to catch what it hid).
    strict_no_duplicate_keys(body)?;

    if object.get("method").and_then(|m| m.as_str()) != Some("tools/call") {
        return Ok(None);
    }
    let Some(name) = object
        .get("params")
        .and_then(|p| p.as_object())
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
    else {
        return Ok(None);
    };

    match required_scope_for_tool(name) {
        Some((_tool, scope)) if !scopes.contains(&scope) => Err(PreflightError::InsufficientScope(
            scope.mcp_wire().unwrap_or_default(),
        )),
        Some((tool, _)) => Ok(Some(ScopePreflight { tool })),
        None => Ok(None),
    }
}

/// Walk the raw JSON with a custom visitor so duplicate keys surface as
/// errors before serde's `Value` collapses them.
fn strict_no_duplicate_keys(body: &[u8]) -> Result<(), PreflightError> {
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    deserializer
        .deserialize_any(StrictKeysVisitor)
        .map_err(|_| PreflightError::DuplicateKeys)?;
    deserializer.end().map_err(|_| PreflightError::Malformed)?;
    Ok(())
}

/// Recursing stand-in for `serde::de::IgnoredAny`: nested values must go
/// through the same strict visitor, or duplicate keys one level down (e.g.
/// inside `params`) would be consumed unseen.
#[derive(Debug)]
struct StrictJson;

impl<'de> serde::Deserialize<'de> for StrictJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer
            .deserialize_any(StrictKeysVisitor)
            .map(|()| StrictJson)
    }
}

struct StrictKeysVisitor;

impl<'de> serde::de::Visitor<'de> for StrictKeysVisitor {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("any JSON value without duplicate object keys")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        while seq.next_element::<StrictJson>()?.is_some() {}
        Ok(())
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut seen = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key) {
                return Err(serde::de::Error::custom("duplicate object key"));
            }
            map.next_value::<StrictJson>()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn scopes(list: &[CredentialScope]) -> HashSet<CredentialScope> {
        list.iter().copied().collect()
    }

    // WHY: a scope-less call reaching the tool layer would turn the
    // challenge contract into a 200 tool message — the exact regression the
    // transport-side preflight exists to prevent.
    #[test]
    fn preflight_rejects_self_tool_without_scope() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_my_profile","arguments":{}}}"#;
        let err = preflight(body, &scopes(&[])).unwrap_err();
        match err {
            PreflightError::InsufficientScope("mcp:profile:read") => {}
            other => panic!("expected insufficient_scope, got {other:?}"),
        }
    }

    #[test]
    fn preflight_passes_self_tool_with_scope_and_marks_it() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_my_subscriptions","arguments":{}}}"#;
        let marker = preflight(body, &scopes(&[CredentialScope::McpSubscriptionsRead]))
            .unwrap()
            .expect("marker required");
        assert_eq!(marker.tool, "list_my_subscriptions");
    }

    // WHY: admin tools gate on tool-layer RBAC, not transport scope — a
    // scope-less-but-authenticated caller must reach the tool to get the
    // agent-readable permission guidance.
    #[test]
    fn preflight_leaves_admin_tools_and_discovery_alone() {
        let admin = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"query_users","arguments":{}}}"#;
        assert!(preflight(admin, &scopes(&[])).unwrap().is_none());

        let initialize = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        assert!(preflight(initialize, &scopes(&[])).unwrap().is_none());

        let tools_list = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        assert!(preflight(tools_list, &scopes(&[])).unwrap().is_none());
    }

    // WHY: a batch run partially server-side would execute some calls before
    // failing the rest — the whole batch is refused before dispatch.
    #[test]
    fn preflight_rejects_batches_whole() {
        let body = br#"[{"jsonrpc":"2.0","id":1,"method":"tools/list"},{"jsonrpc":"2.0","id":2,"method":"ping"}]"#;
        assert!(matches!(
            preflight(body, &scopes(&[])),
            Err(PreflightError::Batch)
        ));
    }

    // WHY: duplicate keys parse differently for this preflight (serde Value
    // collapses to the last) and for rmcp's typed parse — rejecting them raw
    // closes the interpretation gap.
    #[test]
    fn preflight_rejects_duplicate_keys_at_envelope_and_params_level() {
        let envelope = br#"{"jsonrpc":"2.0","jsonrpc":"2.0","id":1,"method":"ping"}"#;
        assert!(matches!(
            preflight(envelope, &scopes(&[])),
            Err(PreflightError::DuplicateKeys)
        ));

        let params = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_my_profile","name":"query_users","arguments":{}}}"#;
        assert!(matches!(
            preflight(params, &scopes(&[])),
            Err(PreflightError::DuplicateKeys)
        ));
    }

    #[test]
    fn preflight_treats_malformed_envelope_as_malformed() {
        for body in [
            &b"not json"[..],
            &b"42"[..],
            &b"\"string\""[..],
            &b"null"[..],
        ] {
            assert!(matches!(
                preflight(body, &scopes(&[])),
                Err(PreflightError::Malformed)
            ));
        }
    }

    // WHY: a malformed method/params shape is rmcp's contract (invalid
    // params), not this preflight's — string method + non-object params must
    // pass through untouched.
    #[test]
    fn preflight_passes_malformed_params_to_rmcp() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":"not-an-object"}"#;
        assert!(preflight(body, &scopes(&[])).unwrap().is_none());

        let no_method = br#"{"jsonrpc":"2.0","id":1}"#;
        assert!(preflight(no_method, &scopes(&[])).unwrap().is_none());
    }

    // WHY: tools/call on an unknown tool name must surface rmcp's
    // unknown-tool error, not a scope challenge for a name outside the
    // fixed table.
    #[test]
    fn preflight_ignores_unknown_tool_names() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"future_tool","arguments":{}}}"#;
        assert!(preflight(body, &scopes(&[])).unwrap().is_none());
    }

    #[test]
    fn realm_segment_extraction() {
        assert_eq!(realm_from_path("/mcp/acme"), Some("acme"));
        assert_eq!(realm_from_path("/mcp/"), None);
        assert_eq!(realm_from_path("/mcp/acme/extra"), None);
        assert_eq!(realm_from_path("/other/acme"), None);
    }

    // WHY: the WWW-Authenticate header is the contract agent clients parse;
    // its parameter order and quoting are pinned here.
    #[test]
    fn bearer_challenge_parameter_shape() {
        assert_eq!(
            bearer_challenge(
                "https://h.example/.well-known/oauth-protected-resource/mcp/acme",
                None,
                None
            ),
            "Bearer resource_metadata=\"https://h.example/.well-known/oauth-protected-resource/mcp/acme\""
        );
        assert_eq!(
            bearer_challenge(
                "https://prm",
                Some("insufficient_scope"),
                Some("mcp:points:read")
            ),
            "Bearer resource_metadata=\"https://prm\", error=\"insufficient_scope\", scope=\"mcp:points:read\""
        );
    }

    // WHY: custom domains are enabled at runtime, so the Host gate must
    // accept the live canonical host (rmcp's static list cannot) while still
    // rejecting rebound/foreign hosts — the DNS-rebinding protection the
    // transport expects does not disappear with the static list.
    #[test]
    fn host_gate_accepts_canonical_loopback_and_custom_domain() {
        let origin = "https://api.example.com";
        assert!(host_is_acceptable("api.example.com", origin));
        assert!(host_is_acceptable("API.EXAMPLE.COM", origin));
        assert!(host_is_acceptable("api.example.com:443", origin));
        assert!(host_is_acceptable("localhost", origin));
        assert!(host_is_acceptable("127.0.0.1:9", origin));
        assert!(host_is_acceptable("[::1]:9", origin));

        let custom = "https://login.acme.com";
        assert!(host_is_acceptable("login.acme.com", custom));
    }

    #[test]
    fn host_gate_rejects_foreign_and_rebound_hosts() {
        let origin = "https://api.example.com";
        assert!(!host_is_acceptable("evil.example.net", origin));
        // Unbracketing for the loopback parse must not widen the gate: a
        // bracketed non-loopback IPv6 literal is neither loopback nor the
        // canonical host.
        assert!(!host_is_acceptable("[2001:db8::1]", origin));
        // A loopback Host with a non-loopback canonical origin is dev/test
        // traffic; a PUBLIC Host against a loopback canonical origin is the
        // classic rebinding shape and must fail.
        assert!(!host_is_acceptable(
            "evil.example.net",
            "http://localhost:8080"
        ));
        assert!(!host_is_acceptable("", origin));
        assert!(!host_is_acceptable("not a host", origin));
    }
}
