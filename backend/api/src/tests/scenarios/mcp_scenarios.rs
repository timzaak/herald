// =============================================================================
// MCP Server Scenario Tests (OAuth user-credential era)
// =============================================================================
//
// End-to-end coverage of the /mcp/{realmId} endpoint after the API-Key -> OAuth
// migration: a real rmcp client transport (Streamable HTTP) drives a real
// unified test router over an ephemeral TCP listener for the success paths,
// and raw HTTP POSTs pin the transport-side contracts (RFC 9728 challenges,
// strict envelope preflight, per-user rate limit, body ceiling) that live
// before rmcp.
//
// Coverage map (verified contract source: docs/user-stories/integration/mcp-server.md):
// - connection + tools/list: nine read-only tools, never hidden by permissions
// - 401 challenge states: missing token, forged token, browser token, both
//   Client API Key forms (the MCP face no longer parses API keys)
// - scope preflight: 403 challenge before any read; batch/duplicate-key/
//   malformed envelopes rejected whole
// - admin-face RBAC: view-only suffices, denial guidance, cross-realm targets
//   read as not_found, missing targets never misreport as zero balances
// - self-face: scope-only (no RBAC), userId structurally rejected
// - V6 output: realm balance summary, minimized transaction/audit/config/
//   subscription surfaces, page-independent subscription summary
// - V5 lifecycle: standard refresh with scope inheritance, family reuse
//   detection, binding rejections, disable->re-enable never revives
//   (generation), concurrent refresh single winner, user family revocation
// - V3 isolation: MCP credentials rejected on every browser face, probes
//   never introspect them
//
// Tests are independent: each seeds its own users/subscriptions under its own
// schema-isolated context.
//
// Reference: docs/user-stories/integration/mcp-server.md
//
// =============================================================================

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use rmcp::model::{CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock};
use rmcp::service::{ClientInitializeError, RoleClient, RunningService, ServiceExt as _};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde_json::{Value, json};
use test_context::test_context;
use tower::ServiceExt;

use crate::tests::helpers::auth_helpers::{create_admin_session_with_user, grant_realm_admin_role};
use crate::tests::helpers::client_helpers::create_test_api_key;
use crate::tests::helpers::mcp_oauth_helpers::{
    MCP_ALL_SCOPES, MCP_PROFILE_SCOPE, browser_refresh_request, mcp_canonical_resource,
    mcp_prm_url, mcp_refresh_request, obtain_mcp_tokens,
};
use crate::tests::helpers::oauth_pkce_helpers::{
    compute_code_challenge, extract_auth_code_from_redirect, generate_code_verifier,
    generate_state, login_with_oauth, oauth_authorize, oauth_token_exchange,
};
use crate::tests::helpers::subscription_test_helpers::create_test_subscription_for_user;
use crate::tests::helpers::test_setup_helpers::{create_test_user, login_user};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use herald_core::domain::authentication::BrowserTokenService;
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::authorization::principal_types;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;

type McpClient = RunningService<RoleClient, ()>;

// =============================================================================
// Local helpers
// =============================================================================

/// Spawn the full production router (create_api_routes) on an ephemeral
/// loopback port and return the MCP base URL (`http://{addr}/mcp`). The rmcp
/// client transport needs a real URL; oneshot cannot host it. The audience
/// check uses the configured canonical resource, so the ephemeral address is
/// fine as the connection target.
async fn spawn_mcp_server(ctx: &TestContext) -> String {
    let router = ctx.create_unified_test_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind ephemeral listener");
    let addr = listener.local_addr().expect("Failed to read local addr");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("MCP test server failed");
    });
    format!("http://{addr}/mcp")
}

/// Connect an rmcp client authenticated with an MCP OAuth access token.
/// `auth_header` takes the bare token — the transport renders it as
/// `Authorization: Bearer <token>` on every request (initialize, POST and
/// the SSE GET alike).
async fn connect_mcp(url: &str, access_token: &str) -> Result<McpClient, ClientInitializeError> {
    let transport = StreamableHttpClientTransport::with_client(
        rmcp_test_reqwest::Client::new(),
        StreamableHttpClientTransportConfig::with_uri(url.to_string())
            .auth_header(access_token.to_string()),
    );
    ().serve(transport).await
}

async fn call_tool(client: &McpClient, name: &str, args: serde_json::Value) -> CallToolResult {
    let mut params = CallToolRequestParams::new(name.to_string());
    if let serde_json::Value::Object(map) = args {
        params.arguments = Some(map);
    }
    match client
        .call_tool_once(params)
        .await
        .expect("tools/call transport-level failure")
    {
        CallToolResponse::Complete(result) => result,
        other => panic!("expected a completed tool result, got {other:?}"),
    }
}

fn result_text(result: &CallToolResult) -> String {
    match result.content.first() {
        Some(ContentBlock::Text(text)) => text.text.clone(),
        other => panic!("expected text content block, got {other:?}"),
    }
}

fn result_json(result: &CallToolResult) -> serde_json::Value {
    serde_json::from_str(&result_text(result)).expect("tool output is valid JSON")
}

/// Assert a tool-level business error: isError + "<code>: ..." prefix.
fn assert_tool_error(result: &CallToolResult, code: &str) {
    assert_eq!(
        result.is_error,
        Some(true),
        "expected isError=true, content: {}",
        result_text(result)
    );
    let text = result_text(result);
    assert!(
        text.starts_with(&format!("{code}: ")),
        "expected '{code}: ' prefix, got: {text}"
    );
}

/// A minimal single-request JSON-RPC envelope for `tools/call`.
fn tool_call_envelope(name: &str, arguments: serde_json::Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    })
    .to_string()
}

/// Raw POST against the spawned MCP server. Returns the reqwest response so
/// callers can pin status, headers and body of the transport-side contracts
/// (everything that happens before rmcp). The Accept header satisfies the
/// streamable-HTTP transport's content negotiation once a request survives
/// the middleware — without it rmcp answers 406 before any tool runs, which
/// would mask the contract under test.
async fn raw_mcp_post(
    url: &str,
    authorization: Option<&str>,
    api_key: Option<&str>,
    body: String,
) -> rmcp_test_reqwest::Response {
    let mut request = rmcp_test_reqwest::Client::new()
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(token) = authorization {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(key) = api_key {
        request = request.header("x-api-key", key);
    }
    request
        .body(body)
        .send()
        .await
        .expect("request must complete")
}

async fn response_body(response: rmcp_test_reqwest::Response) -> Value {
    let text = response.text().await.expect("body must be readable");
    serde_json::from_str(&text).unwrap_or_else(|error| {
        panic!("expected a JSON body, got '{text}' ({error})");
    })
}

/// Prove a token is live on the MCP face with one full rmcp client
/// round-trip (initialize → get_my_profile). A raw POST cannot make this
/// claim: the streamable transport requires an initialize request as the
/// first message of every session, so a bare tools/call is refused 422
/// before any tool runs.
async fn mcp_token_works(url: &str, token: &str) -> bool {
    let Ok(mut client) = connect_mcp(url, token).await else {
        return false;
    };
    let result = call_tool(&client, "get_my_profile", json!({})).await;
    let works = result.is_error == Some(false);
    let _ = client.close().await;
    works
}

/// The dead-credential counterpart of [`mcp_token_works`]: the middleware's
/// 401 challenge surfaces as a client-initialize transport error.
async fn mcp_token_is_dead(url: &str, token: &str) -> bool {
    connect_mcp(url, token).await.is_err()
}

fn www_authenticate(response: &rmcp_test_reqwest::Response) -> String {
    response
        .headers()
        .get("www-authenticate")
        .and_then(|value| value.to_str().ok())
        .expect("challenge responses must carry WWW-Authenticate")
        .to_string()
}

/// Grant ONE permission to a USER principal via a dedicated role (the tool
/// layer checks RBAC on the OAuth user, never on an API key) and drop the
/// user's cached role bindings so the grant is visible immediately.
async fn grant_mcp_user_permission(
    ctx: &TestContext,
    user_id: uuid::Uuid,
    resource: &str,
    action: &str,
) {
    let role_uuid = uuid::Uuid::now_v7();
    // roles.name is unique per (realm, client_id); the suffix uses the UUIDv7
    // random tail because its leading chars encode the timestamp and collide
    // for same-millisecond generations.
    sqlx::query(
        "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
         VALUES ($1, $2, $3, $4, $5, false)",
    )
    .bind(role_uuid)
    .bind(format!(
        "mcp-user-role-{}-{}-{}",
        resource,
        action,
        &role_uuid.to_string()[24..]
    ))
    .bind(format!("MCP test role for {}.{}", resource, action))
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to create MCP test role");

    sqlx::query(
        "INSERT INTO role_policies (id, role_id, realm_id, resource, action)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(role_uuid)
    .bind(&ctx._realm_id)
    .bind(resource)
    .bind(action)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to add policy to MCP test role");

    sqlx::query(
        "INSERT INTO user_roles (id, user_id, role_id, realm_id, client_id, principal_type, principal_id)
         VALUES ($1, $2, $3, $4, $5, $6, $2::text)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(user_id)
    .bind(role_uuid)
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .bind(principal_types::USER)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to bind MCP test role to user");

    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_user_role_cache(&ctx._realm_id, &user_id.to_string())
        .await;
}

async fn seed_audit_event(ctx: &TestContext, category: &str, action: &str, actor_id: &str) {
    sqlx::query(
        "INSERT INTO audit_events (id, realm_id, category, action, actor_id, target_type, target_id, result, created_at)
         VALUES ($1, $2, $3, $4, $5, 'user', $6, 'success', NOW())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&ctx._realm_id)
    .bind(category)
    .bind(action)
    .bind(actor_id)
    .bind(uuid::Uuid::now_v7().to_string())
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed audit event");
}

/// Seed one subscription row for a user. Each row gets its own client app
/// (subscription.client_app_id is UNIQUE) and a controlled created_at so
/// list_my_subscriptions page ordering is deterministic.
async fn seed_user_subscription(
    ctx: &mut TestContext,
    realm_id: &str,
    user_id: uuid::Uuid,
    entitlement_key: &str,
    status: &str,
    age_hours: i32,
) -> uuid::Uuid {
    let client_app_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO client_app (id, realm_id, client_id, name, enabled, created_at, updated_at)
         VALUES ($1, $2, $3, $4, true, NOW(), NOW())",
    )
    .bind(client_app_id)
    .bind(realm_id)
    .bind(format!("mcp-sub-app-{client_app_id}"))
    .bind(format!("MCP subscription fixture {client_app_id}"))
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed subscription client app");

    let subscription_id = create_test_subscription_for_user(
        ctx,
        realm_id,
        client_app_id,
        user_id,
        entitlement_key,
        &format!("price_{entitlement_key}"),
        "creem",
        status,
    )
    .await;

    sqlx::query(
        "UPDATE subscription SET created_at = NOW() - make_interval(hours => $1) WHERE id = $2",
    )
    .bind(age_hours)
    .bind(subscription_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to backdate subscription");
    subscription_id
}

async fn herald_mcp_client_app_id(ctx: &TestContext, realm_id: &str) -> uuid::Uuid {
    sqlx::query_scalar("SELECT id FROM client_app WHERE realm_id = $1 AND client_id = 'herald-mcp'")
        .bind(realm_id)
        .fetch_one(&ctx._app_state.pool)
        .await
        .expect("the per-realm built-in herald-mcp client must exist")
}

/// Toggle the built-in MCP client through the real admin API (the same path
/// an administrator uses), asserting the update succeeded.
async fn set_herald_mcp_enabled(
    ctx: &TestContext,
    admin_token: &str,
    client_app_id: uuid::Uuid,
    enabled: bool,
) {
    let request = Request::builder()
        .method("PUT")
        .uri(format!("/api/client/{client_app_id}"))
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(Body::from(json!({ "enabled": enabled }).to_string()))
        .unwrap();
    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "admin {enabled} update on the built-in MCP client must succeed"
    );
}

// =============================================================================
// Scenario 1: connection + tools/list (US-MCP-001)
// =============================================================================

// Given a user holding all four MCP scopes (and NO admin RBAC roles),
// When an agent client connects and lists tools,
// Then the handshake succeeds and exactly the nine read-only tools are
// listed — discovery is never filtered by the caller's permissions, so the
// agent can see (and be told it lacks) management tools.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: US-MCP-001 连接成功并展示完整查询工具清单
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_connect_lists_all_nine_tools(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let email = "mcp-connect@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;

    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);
    let mut client = connect_mcp(&url, &tokens.access_token)
        .await
        .expect("connect with a valid MCP OAuth token must succeed");

    let tools = client
        .list_tools(None)
        .await
        .expect("tools/list must succeed after initialize");

    let mut names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "get_my_points_balance",
            "get_my_profile",
            "get_points_balance",
            "get_realm_config_status",
            "list_audit_logs",
            "list_my_points_transactions",
            "list_my_subscriptions",
            "list_points_transactions",
            "query_users",
        ],
        "the tool list must be exactly the nine read-only tools"
    );

    let _ = client.close().await;
}

// =============================================================================
// Scenario 2: 401 challenge states (US-MCP-001, bidirectional isolation)
// =============================================================================

// Given the MCP face accepts ONLY OAuth MCP tokens,
// When raw HTTP POSTs arrive with (a) no credential, (b) a forged token,
// (c) a genuine browser token from the normal PKCE flow, (d) a Client API
// Key in X-API-Key form, (e) a Client API Key in Bearer form,
// Then every case is a 401 whose WWW-Authenticate points at this realm's
// RFC 9728 protected-resource metadata — the API-Key parsing path is gone,
// and a browser credential is just another wrong-audience token.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: US-MCP-001 凭证无效/非 MCP 凭证被拒绝并给出标准挑战
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_unauthorized_challenges_for_non_mcp_credentials(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let origin = ctx
        ._app_state
        .public_base_url
        .trim_end_matches('/')
        .to_string();
    let prm = mcp_prm_url(ctx, &realm_id);
    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);
    let envelope = tool_call_envelope("get_my_profile", json!({}));

    // (a) No credential: the bootstrap challenge also hints the minimal scope.
    let response = raw_mcp_post(&url, None, None, envelope.clone()).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        www_authenticate(&response),
        format!(r#"Bearer resource_metadata="{prm}", scope="mcp:profile:read""#),
        "the missing-token challenge must carry the PRM and the minimal scope hint"
    );

    // (b) Forged bearer: same challenge plus error="invalid_token".
    let response = raw_mcp_post(&url, Some("not-a-real-token"), None, envelope.clone()).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        www_authenticate(&response),
        format!(r#"Bearer resource_metadata="{prm}", error="invalid_token""#),
        "a presented-but-invalid token must be challenged with invalid_token"
    );

    // (c) A genuine browser token from the ordinary PKCE flow (no resource,
    // browser credential class): audience mismatch, not a missing token.
    let email = "mcp-browser-token@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let browser_redirect_uri = format!("{origin}/callback");
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();
    let authorize_response = oauth_authorize(
        ctx,
        &realm_id,
        "admin-web-console",
        &browser_redirect_uri,
        &state,
        &code_challenge,
        "S256",
    )
    .await;
    assert_eq!(authorize_response.status(), StatusCode::FOUND);
    let login_response = login_with_oauth(
        ctx,
        &realm_id,
        email,
        password,
        "admin-web-console",
        &browser_redirect_uri,
        &state,
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let login_json: Value = crate::tests::response_json(login_response).await;
    let redirect_to = login_json["redirectTo"].as_str().expect("redirectTo");
    let code = extract_auth_code_from_redirect(redirect_to).expect("auth code");
    let token_response = oauth_token_exchange(
        ctx,
        &realm_id,
        "authorization_code",
        &code,
        &browser_redirect_uri,
        "admin-web-console",
        &code_verifier,
    )
    .await;
    assert_eq!(token_response.status(), StatusCode::OK);
    let token_json: Value = crate::tests::response_json(token_response).await;
    let browser_token = token_json["access_token"].as_str().expect("browser token");

    let response = raw_mcp_post(&url, Some(browser_token), None, envelope.clone()).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        www_authenticate(&response),
        format!(r#"Bearer resource_metadata="{prm}", error="invalid_token""#),
        "a browser credential must read as invalid on the MCP face"
    );

    // (d)+(e) Client API Keys in both transport forms: the MCP face no longer
    // parses API keys at all — both are just unauthenticated callers.
    let (api_key, _entity) = create_test_api_key(ctx, "mcp-legacy-key", true, None).await;

    let response = raw_mcp_post(&url, None, Some(&api_key), envelope.clone()).await;
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "X-API-Key must not authenticate on the MCP face"
    );
    assert_eq!(
        www_authenticate(&response),
        format!(r#"Bearer resource_metadata="{prm}", scope="mcp:profile:read""#),
        "a key-only request is the missing-credential bootstrap case"
    );

    let response = raw_mcp_post(&url, Some(&api_key), None, envelope).await;
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "a Bearer sk-... API key must not authenticate on the MCP face"
    );
    assert_eq!(
        www_authenticate(&response),
        format!(r#"Bearer resource_metadata="{prm}", error="invalid_token""#)
    );
}

// Given an MCP request addressed to a realm that does not exist,
// When the request arrives,
// Then the response is 404 — never a 401 challenge — so an agent never
// bootstraps an authorization flow against a nonexistent tenant, and a 404
// can never be mistaken for an authentication verdict.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: US-MCP-001 未知租户地址返回 404
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_unknown_realm_returns_404(ctx: &mut TestContext) {
    let url = format!(
        "{}/no-such-realm-{}",
        spawn_mcp_server(ctx).await,
        uuid::Uuid::now_v7().simple()
    );
    let response = raw_mcp_post(&url, None, None, tool_call_envelope("ping", json!({}))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response_body(response).await;
    assert_eq!(body["error"], "not_found");
}

// =============================================================================
// Scenario 3: scope preflight + strict envelope (US-MCP-008)
// =============================================================================

// Given a token carrying ONLY mcp:profile:read,
// When get_my_points_balance is called,
// Then the transport rejects it with the RFC 6750 insufficient_scope
// challenge BEFORE any business read — a scope shortfall must be a
// re-authorization prompt, never a 200 tool message — while the same token
// calling get_my_profile (whose scope it holds) succeeds;
// And batch arrays, duplicate keys (envelope and params levels) and
// non-JSON bodies are each rejected whole with 400 invalid_request: the
// strict preflight exists to close interpretation gaps with rmcp's typed
// parse, so none of these may reach dispatch.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-008
// Covers: US-MCP-008 缺少 scope 时得到 403 挑战且零读取；协议面 envelope 负例
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_scope_preflight_challenges_and_envelope_rejections(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let prm = mcp_prm_url(ctx, &realm_id);
    let email = "mcp-preflight@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_PROFILE_SCOPE)).await;

    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);

    // 403 challenge: exact header shape, and a body with nothing but the
    // error pair — no business data may leak alongside the challenge.
    let response = raw_mcp_post(
        &url,
        Some(&tokens.access_token),
        None,
        tool_call_envelope("get_my_points_balance", json!({})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        www_authenticate(&response),
        format!(
            r#"Bearer resource_metadata="{prm}", error="insufficient_scope", scope="mcp:points:read""#
        ),
        "the challenge must name the exact missing scope and the PRM"
    );
    let body = response_body(response).await;
    assert_eq!(body["error"], "insufficient_scope");
    let keys: Vec<&str> = body
        .as_object()
        .expect("challenge body is a JSON object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec!["error", "error_description"],
        "a scope challenge must carry no business payload: {body}"
    );

    // The same token holds mcp:profile:read, so the profile tool succeeds —
    // the preflight is per-scope, not per-connection.
    let mut client = connect_mcp(&url, &tokens.access_token)
        .await
        .expect("profile-only token must connect");
    let profile = call_tool(&client, "get_my_profile", json!({})).await;
    assert_eq!(profile.is_error, Some(false));
    assert_eq!(result_json(&profile)["email"], email);

    // Batch: rejected whole, nothing dispatched (partial execution would run
    // some reads before failing the rest).
    let batch = json!([
        { "jsonrpc": "2.0", "id": 1, "method": "tools/list" },
        { "jsonrpc": "2.0", "id": 2, "method": "ping" }
    ])
    .to_string();
    let response = raw_mcp_post(&url, Some(&tokens.access_token), None, batch).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_body(response).await;
    assert_eq!(body["error"], "invalid_request");

    // Duplicate keys at envelope level: serde's Value would silently collapse
    // them — rejecting raw closes the outside/rmcp interpretation gap.
    let duplicated_envelope =
        r#"{"jsonrpc":"2.0","jsonrpc":"2.0","id":1,"method":"ping"}"#.to_string();
    let response = raw_mcp_post(&url, Some(&tokens.access_token), None, duplicated_envelope).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_body(response).await["error"], "invalid_request");

    // Duplicate keys at params level (nested objects must recurse).
    let duplicated_params = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_my_profile","name":"query_users","arguments":{}}}"#.to_string();
    let response = raw_mcp_post(&url, Some(&tokens.access_token), None, duplicated_params).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_body(response).await["error"], "invalid_request");

    // Non-JSON body.
    let response = raw_mcp_post(
        &url,
        Some(&tokens.access_token),
        None,
        "not json".to_string(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_body(response).await["error"], "invalid_request");

    let _ = client.close().await;
}

// =============================================================================
// Scenario 4: admin-face RBAC — view suffices (US-MCP-002)
// =============================================================================

// Given a user whose ONLY permission is users.view (no manage, no other
// resources),
// When query_users lists and fetches a detail,
// Then both succeed — the MCP admin queries are read-only and gated on the
// view action alone, mirroring the least-privilege console viewer.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-002
// Covers: US-MCP-002 列表与详情查询成功（仅 users.view 即可）
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_query_users_succeeds_with_view_only_permission(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let email = "mcp-users-viewer@test.com";
    let password = "password123";
    let actor_id = create_test_user(ctx, email, password).await;
    grant_mcp_user_permission(ctx, actor_id, "users", "view").await;

    let target_email = "mcp-users-target@test.com";
    let target_id = create_test_user(ctx, target_email, "password123").await;

    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;
    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);
    let mut client = connect_mcp(&url, &tokens.access_token)
        .await
        .expect("connect");

    let list = call_tool(
        &client,
        "query_users",
        json!({ "page": 1, "pageSize": 100 }),
    )
    .await;
    assert_eq!(list.is_error, Some(false), "list must succeed");
    let list_body = result_json(&list);
    let users = list_body["users"].as_array().expect("users array");
    assert!(
        users
            .iter()
            .any(|u| u["id"].as_str() == Some(target_id.to_string().as_str())),
        "the seeded target must appear: {list_body}"
    );

    let detail = call_tool(
        &client,
        "query_users",
        json!({ "userId": target_id.to_string() }),
    )
    .await;
    assert_eq!(detail.is_error, Some(false), "detail must succeed");
    let user = &result_json(&detail)["users"][0];
    assert_eq!(user["id"].as_str(), Some(target_id.to_string().as_str()));
    assert_eq!(user["email"].as_str(), Some(target_email));
    for field in ["nickname", "status", "createdAt"] {
        assert!(
            user.get(field).is_some(),
            "detail must include {field}: {user}"
        );
    }

    let _ = client.close().await;
}

// Given a user with NO roles at all,
// When admin tools are called,
// Then each denial is an agent-readable permission_denied that names the
// missing permission and points at the realm administrator — the agent can
// relay the remedy instead of retrying login or widening its scopes.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-002
// Covers: US-MCP-002/003/004/005/006 缺权限时的指引文案
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_admin_tool_denial_guides_to_realm_administrator(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let email = "mcp-no-roles@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;

    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;
    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);
    let mut client = connect_mcp(&url, &tokens.access_token)
        .await
        .expect("connect");

    let denied = call_tool(&client, "query_users", json!({})).await;
    assert_tool_error(&denied, "permission_denied");
    assert_eq!(
        result_text(&denied),
        "permission_denied: Your user does not have the 'users.view' permission. \
Ask your realm administrator to grant a role with this permission.",
        "the denial text is the agent-facing contract"
    );

    let target_id = create_test_user(ctx, "mcp-denial-target@test.com", "password123").await;
    let denied = call_tool(
        &client,
        "get_points_balance",
        json!({ "userId": target_id.to_string() }),
    )
    .await;
    assert_tool_error(&denied, "permission_denied");
    let text = result_text(&denied);
    assert!(
        text.contains("'points.view'"),
        "names the permission: {text}"
    );
    assert!(
        text.contains("Ask your realm administrator"),
        "points at the remedy: {text}"
    );
    assert!(
        !text.contains("topupBalance"),
        "a denial must not carry balance data: {text}"
    );

    let _ = client.close().await;
}

// Given targets that live in another realm or match no user at all,
// When admin tools query them,
// Then every case reads as not_found — realm comes from the credential so a
// cross-realm read is structurally inexpressible, and a missing user can
// never misreport as a zero balance (get_balance synthesizes zeros for
// wallet-less users; the existence pre-check is what keeps that honest).
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-002
// Covers: US-MCP-002 场景 4/5（跨租户与不存在目标）
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_cross_realm_and_missing_targets_read_as_not_found(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let email = "mcp-targets@test.com";
    let password = "password123";
    let actor_id = create_test_user(ctx, email, password).await;
    grant_mcp_user_permission(ctx, actor_id, "users", "view").await;
    grant_mcp_user_permission(ctx, actor_id, "points", "view").await;

    // A user that exists only in a foreign realm.
    let foreign_user_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, 'mcp-foreign@other-realm.test', NULL, 1)",
    )
    .bind(foreign_user_id)
    .bind(uuid::Uuid::now_v7().to_string())
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed foreign-realm user");

    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;
    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);
    let mut client = connect_mcp(&url, &tokens.access_token)
        .await
        .expect("connect");

    let result = call_tool(
        &client,
        "query_users",
        json!({ "userId": foreign_user_id.to_string() }),
    )
    .await;
    assert_tool_error(&result, "not_found");
    assert!(
        result_text(&result).contains("was not found in this realm"),
        "a foreign-realm target reads as realm-local not_found: {}",
        result_text(&result)
    );

    let random_user = uuid::Uuid::now_v7().to_string();
    let result = call_tool(
        &client,
        "get_points_balance",
        json!({ "userId": random_user }),
    )
    .await;
    assert_tool_error(&result, "not_found");

    let result = call_tool(
        &client,
        "list_points_transactions",
        json!({ "userId": random_user }),
    )
    .await;
    assert_tool_error(&result, "not_found");

    let _ = client.close().await;
}

// Given a permissioned caller,
// When a tool receives a structurally invalid argument (a non-UUID userId),
// Then the tool answers invalid_argument naming the offending field — an
// agent can self-correct the id instead of reading a protocol-level opaque
// failure, and a lenient parse regression (silently succeeding or drifting
// to not_found) fails here.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-002
// Covers: US-MCP-002 invalid_argument 参数自纠契约
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_invalid_argument_on_malformed_uuid(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let email = "mcp-badarg@test.com";
    let password = "password123";
    let actor_id = create_test_user(ctx, email, password).await;
    grant_mcp_user_permission(ctx, actor_id, "users", "view").await;
    grant_mcp_user_permission(ctx, actor_id, "points", "view").await;

    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;
    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);
    let mut client = connect_mcp(&url, &tokens.access_token)
        .await
        .expect("connect");

    for tool in [
        "query_users",
        "get_points_balance",
        "list_points_transactions",
    ] {
        let result = call_tool(&client, tool, json!({ "userId": "not-a-uuid" })).await;
        assert_tool_error(&result, "invalid_argument");
        assert!(
            result_text(&result).contains("'userId'"),
            "the rejection must name the offending field for {tool}: {}",
            result_text(&result)
        );
    }

    // Swapped time bounds are a parameter error, not an empty period: the
    // agent must be told instead of reasoning over zero rows.
    let swapped = call_tool(
        &client,
        "list_my_points_transactions",
        json!({ "startTime": "2026-10-06", "endTime": "2026-10-01" }),
    )
    .await;
    assert_tool_error(&swapped, "invalid_argument");
    assert!(
        result_text(&swapped).contains("startTime"),
        "the swapped-bounds rejection must name the fields: {}",
        result_text(&swapped)
    );

    let _ = client.close().await;
}

// Given a user with NO RBAC roles but all four MCP scopes,
// When self tools are called,
// Then they succeed — the self face is gated by scopes only, RBAC plays no
// part — and the outputs are pinned to the verified identity: the balance
// summary belongs to the caller, and a userId argument is structurally
// rejected (deny_unknown_fields) so reading another user is inexpressible.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-007, US-MCP-008, US-MCP-009
// Covers: US-MCP-007/008/009 self 面仅凭 scope 成功；userId 参数被拒绝
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_self_tools_are_scope_gated_and_reject_user_id_arguments(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let email = "mcp-self@test.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;

    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;
    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);
    let mut client = connect_mcp(&url, &tokens.access_token)
        .await
        .expect("connect");

    let profile = call_tool(&client, "get_my_profile", json!({})).await;
    assert_eq!(profile.is_error, Some(false));
    let profile_body = result_json(&profile);
    assert_eq!(
        profile_body["id"].as_str(),
        Some(user_id.to_string().as_str())
    );
    assert_eq!(profile_body["email"].as_str(), Some(email));

    let balance = call_tool(&client, "get_my_points_balance", json!({})).await;
    assert_eq!(
        balance.is_error,
        Some(false),
        "self tools must not require any RBAC role: {}",
        result_text(&balance)
    );
    let balance_body = result_json(&balance);
    assert_eq!(
        balance_body["userId"].as_str(),
        Some(user_id.to_string().as_str()),
        "self outputs are pinned to the verified identity"
    );
    assert_eq!(balance_body["scope"].as_str(), Some("realm"));
    for field in [
        "balance",
        "topupBalance",
        "subscriptionBalance",
        "grantedBalance",
        "registrationBalance",
        "freePeriodicBalance",
        "updatedAt",
    ] {
        assert!(
            balance_body.get(field).is_some(),
            "realm balance summary must expose {field}: {balance_body}"
        );
    }

    // An empty own-transaction list is a normal result, not an error.
    let transactions = call_tool(&client, "list_my_points_transactions", json!({})).await;
    assert_eq!(transactions.is_error, Some(false));
    let transactions_body = result_json(&transactions);
    assert_eq!(
        transactions_body["transactions"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(transactions_body["total"], 0);

    // deny_unknown_fields: a userId on a self tool is rejected at the
    // argument layer (rmcp renders the deserialization failure as an
    // isError tool result naming the unknown field). Argument-less self
    // tools carry an empty deny_unknown_fields input for the same reason —
    // silently ignoring the argument would let a client believe it queried
    // another user.
    let rejected = call_tool(
        &client,
        "list_my_points_transactions",
        json!({ "userId": user_id.to_string() }),
    )
    .await;
    assert_eq!(
        rejected.is_error,
        Some(true),
        "a userId argument on a self tool must be rejected: {}",
        result_text(&rejected)
    );
    assert!(
        result_text(&rejected).contains("unknown field `userId`"),
        "the rejection must name the structurally-absent field: {}",
        result_text(&rejected)
    );

    let rejected_profile = call_tool(
        &client,
        "get_my_profile",
        json!({ "userId": user_id.to_string() }),
    )
    .await;
    assert_eq!(
        rejected_profile.is_error,
        Some(true),
        "an argument-less self tool must not silently swallow a userId: {}",
        result_text(&rejected_profile)
    );

    let _ = client.close().await;
}

// =============================================================================
// Scenario 5: V6 admin output surfaces (US-MCP-003, US-MCP-004)
// =============================================================================

// Given a points.view-only viewer (NOT the target, and without
// points.manage) and a seeded target wallet with a recharge and a consume,
// When the balance and transactions of the TARGET are queried,
// Then the balance is the realm-summed figure with every bucket field, and
// the transaction rows belong to the target (the direct repository read must
// NOT narrow a non-manage viewer to their own rows), amounts are signed, and
// the ledger attribution fields never appear — agents may carry tool output
// into third-party models.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-003, US-MCP-004
// Covers: US-MCP-003/004 管理面余额与流水输出（目标非本人、字段最小化）
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_admin_balance_summary_and_cross_user_transactions(ctx: &mut TestContext) {
    use crate::tests::scenarios::points::fixtures::{
        create_test_points_wallet, create_test_transaction,
    };

    let realm_id = ctx._realm_id.clone();
    let email = "mcp-points-viewer@test.com";
    let password = "password123";
    let actor_id = create_test_user(ctx, email, password).await;
    grant_mcp_user_permission(ctx, actor_id, "points", "view").await;

    let target_id = create_test_user(ctx, "mcp-points-target@test.com", "password123").await;
    let wallet_id = create_test_points_wallet(&ctx._app_state.pool, target_id, 500).await;
    create_test_transaction(
        &ctx._app_state.pool,
        wallet_id,
        target_id,
        "recharge",
        1000,
        1000,
        Some("top up"),
        None,
    )
    .await;
    create_test_transaction(
        &ctx._app_state.pool,
        wallet_id,
        target_id,
        "consume",
        -200,
        800,
        Some("spend"),
        None,
    )
    .await;

    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;
    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);
    let mut client = connect_mcp(&url, &tokens.access_token)
        .await
        .expect("connect");

    let balance = call_tool(
        &client,
        "get_points_balance",
        json!({ "userId": target_id.to_string() }),
    )
    .await;
    assert_eq!(
        balance.is_error,
        Some(false),
        "a points.view viewer must read another user's balance: {}",
        result_text(&balance)
    );
    let balance_body = result_json(&balance);
    assert_eq!(
        balance_body["userId"].as_str(),
        Some(target_id.to_string().as_str())
    );
    assert_eq!(
        balance_body["balance"].as_i64(),
        Some(500),
        "{balance_body}"
    );
    assert_eq!(balance_body["scope"].as_str(), Some("realm"));
    assert!(balance_body.get("updatedAt").is_some());

    let filtered = call_tool(
        &client,
        "list_points_transactions",
        json!({
            "userId": target_id.to_string(),
            "transactionType": "consume",
        }),
    )
    .await;
    assert_eq!(filtered.is_error, Some(false));
    let filtered_body = result_json(&filtered);
    let rows = filtered_body["transactions"].as_array().expect("rows");
    assert_eq!(
        rows.len(),
        1,
        "only the consume row matches: {filtered_body}"
    );
    assert_eq!(rows[0]["amount"].as_i64(), Some(-200), "amounts are signed");

    let full = call_tool(
        &client,
        "list_points_transactions",
        json!({ "userId": target_id.to_string() }),
    )
    .await;
    let full_body = result_json(&full);
    assert_eq!(
        full_body["total"], 2,
        "the target's rows are visible cross-user"
    );

    for row in full_body["transactions"]
        .as_array()
        .expect("transactions array")
    {
        for field in [
            "walletId",
            "bucketId",
            "correlationId",
            "externalRefId",
            "subscriptionId",
            "clientAppId",
        ] {
            assert!(
                row.get(field).is_none(),
                "minimized ledger field '{field}' must not appear: {row}"
            );
        }
    }

    let _ = client.close().await;
}

// Given an audit.view + settings.view viewer and seeded audit events,
// When audit logs are listed with a category filter and the config status is
// read,
// Then the filter holds and the audit payload omits ip / user agent / trace
// id / details (the most sensitive read surface), and the config status
// reports per-entry enablement without any configValue — values never leave,
// not even non-sensitive ones.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-005, US-MCP-006
// Covers: US-MCP-005/006 审计与配置状态输出字段最小化
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_admin_audit_and_config_status_minimized_output(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let email = "mcp-audit-settings@test.com";
    let password = "password123";
    let actor_id = create_test_user(ctx, email, password).await;
    grant_mcp_user_permission(ctx, actor_id, "audit", "view").await;
    grant_mcp_user_permission(ctx, actor_id, "settings", "view").await;

    let actor_id_str = uuid::Uuid::now_v7().to_string();
    seed_audit_event(ctx, "user_management", "user.create", &actor_id_str).await;
    seed_audit_event(ctx, "auth", "auth.login", &actor_id_str).await;

    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;
    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);
    let mut client = connect_mcp(&url, &tokens.access_token)
        .await
        .expect("connect");

    let audit = call_tool(
        &client,
        "list_audit_logs",
        json!({ "category": "user_management" }),
    )
    .await;
    assert_eq!(audit.is_error, Some(false));
    let audit_body = result_json(&audit);
    let events = audit_body["events"].as_array().expect("events array");
    assert!(
        !events.is_empty(),
        "the seeded user_management event must be returned: {audit_body}"
    );
    for event in events {
        assert_eq!(
            event["category"].as_str(),
            Some("user_management"),
            "the category filter must hold: {event}"
        );
        for field in ["ip", "ipAddress", "userAgent", "traceId", "details"] {
            assert!(
                event.get(field).is_none(),
                "sensitive audit field '{field}' must not appear: {event}"
            );
        }
    }

    let config = call_tool(&client, "get_realm_config_status", json!({})).await;
    assert_eq!(
        config.is_error,
        Some(false),
        "the connectivity self-check must succeed: {}",
        result_text(&config)
    );
    let config_body = result_json(&config);
    assert_eq!(config_body["realmId"].as_str(), Some(realm_id.as_str()));
    assert!(
        config_body["configs"].is_array(),
        "configs must be an array: {config_body}"
    );
    for entry in config_body["configs"].as_array().expect("configs array") {
        assert!(
            entry.get("configValue").is_none(),
            "config values must never leave: {entry}"
        );
    }

    let _ = client.close().await;
}

// Given a user with no subscriptions, and a user with an active and two
// expired subscriptions in a controlled creation order,
// When list_my_subscriptions is called,
// Then the no-subscription answer is false/false/[]/total 0 (a normal
// result), expired rows stay visible with hasAccess=false and empty
// activeEntitlements, the active row reports hasAccess=true with
// activeEntitlements containing exactly its own entitlement key, and
// hasActiveSubscription is computed over ALL rows — page 2 shows only an
// expired row yet still reports true. Payment external ids never surface.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-010
// Covers: US-MCP-010 空列表/expired 可见/active 权益/分页汇总与字段最小化
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_my_subscriptions_lifecycle_and_page_independent_summary(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);

    // Fresh user: no subscription rows at all.
    let empty_email = "mcp-subs-empty@test.com";
    let empty_password = "password123";
    let _empty_user = create_test_user(ctx, empty_email, empty_password).await;
    let empty_tokens = obtain_mcp_tokens(
        ctx,
        &realm_id,
        empty_email,
        empty_password,
        Some(MCP_ALL_SCOPES),
    )
    .await;
    let mut empty_client = connect_mcp(&url, &empty_tokens.access_token)
        .await
        .expect("connect");
    let result = call_tool(&empty_client, "list_my_subscriptions", json!({})).await;
    assert_eq!(result.is_error, Some(false));
    let body = result_json(&result);
    assert_eq!(body["hasSubscription"], false, "{body}");
    assert_eq!(body["hasActiveSubscription"], false, "{body}");
    assert_eq!(
        body["subscriptions"].as_array().map(Vec::len),
        Some(0),
        "{body}"
    );
    assert_eq!(body["total"], 0);
    let _ = empty_client.close().await;

    // Populated user: newest-first order is [expired_newest, expired_mid,
    // active_oldest] via backdated created_at, so page 1 (size 2) shows only
    // expired rows while the active row sits on page 2.
    let email = "mcp-subs-populated@test.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;
    seed_user_subscription(ctx, &realm_id, user_id, "mcp-pro-plan", "active", 3).await;
    seed_user_subscription(ctx, &realm_id, user_id, "mcp-old-plan", "expired", 2).await;
    seed_user_subscription(ctx, &realm_id, user_id, "mcp-older-plan", "expired", 1).await;

    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;
    let mut client = connect_mcp(&url, &tokens.access_token)
        .await
        .expect("connect");

    let page1 = call_tool(
        &client,
        "list_my_subscriptions",
        json!({ "page": 1, "pageSize": 2 }),
    )
    .await;
    assert_eq!(page1.is_error, Some(false));
    let page1_body = result_json(&page1);
    assert_eq!(page1_body["total"], 3, "{page1_body}");
    assert_eq!(page1_body["hasSubscription"], true);
    let page1_rows = page1_body["subscriptions"].as_array().expect("rows");
    assert_eq!(page1_rows.len(), 2);
    for row in page1_rows {
        assert_eq!(
            row["status"].as_str(),
            Some("expired"),
            "newest rows are the expired ones: {row}"
        );
        assert_eq!(row["hasAccess"], false, "expired grants nothing: {row}");
        assert_eq!(
            row["activeEntitlements"].as_array().map(Vec::len),
            Some(0),
            "no entitlement while access is false: {row}"
        );
    }

    // The active row is alone on page 2; the summary stays true because it is
    // computed over the full row set, not the current page.
    assert_eq!(
        page1_body["hasActiveSubscription"], true,
        "the summary must not depend on which rows the page shows: {page1_body}"
    );

    let page2 = call_tool(
        &client,
        "list_my_subscriptions",
        json!({ "page": 2, "pageSize": 2 }),
    )
    .await;
    assert_eq!(page2.is_error, Some(false));
    let page2_body = result_json(&page2);
    assert_eq!(
        page2_body["hasActiveSubscription"], true,
        "page 2 shows only an expired row, the user still holds an active subscription: {page2_body}"
    );
    let page2_rows = page2_body["subscriptions"].as_array().expect("rows");
    assert_eq!(page2_rows.len(), 1);
    let active_row = &page2_rows[0];
    assert_eq!(active_row["status"].as_str(), Some("active"));
    assert_eq!(active_row["entitlementKey"].as_str(), Some("mcp-pro-plan"));
    assert_eq!(active_row["hasAccess"], true);
    assert_eq!(
        active_row["activeEntitlements"],
        json!(["mcp-pro-plan"]),
        "an accessing row lists exactly its own entitlement key: {active_row}"
    );

    for row in page1_rows.iter().chain(page2_rows.iter()) {
        for field in [
            "externalSubscriptionId",
            "externalProductId",
            "providerMetadata",
            "clientAppId",
        ] {
            assert!(
                row.get(field).is_none(),
                "payment/external field '{field}' must not appear: {row}"
            );
        }
    }

    let _ = client.close().await;
}

// =============================================================================
// Scenario 6: V5 lifecycle — refresh, reuse detection, bindings
// =============================================================================

// Given a full-chain MCP family,
// When the standard refresh grant runs,
// Then rotation succeeds with the scope set inherited verbatim (absent
// scope) or accepted when explicitly equal; a DIFFERENT explicit scope is
// invalid_scope and consumes nothing; and reusing an already-rotated refresh
// token is invalid_grant that revokes the whole family — the successor
// access token dies with it (reuse detection must not leave a live branch).
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: US-MCP-001 标准刷新继承绑定与 family 复用检测
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_refresh_rotates_inherits_scope_and_reuse_kills_family(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let resource = mcp_canonical_resource(ctx, &realm_id);
    let email = "mcp-refresh@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;

    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);

    // Absent scope: inherited verbatim from the family.
    let response = mcp_refresh_request(
        ctx,
        &realm_id,
        &tokens.refresh_token,
        "herald-mcp",
        &resource,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let rotated: Value = crate::tests::response_json(response).await;
    assert_eq!(rotated["scope"].as_str(), Some(MCP_ALL_SCOPES));
    let access2 = rotated["access_token"].as_str().unwrap().to_string();
    let refresh2 = rotated["refresh_token"].as_str().unwrap().to_string();

    // The rotated access token is live on the MCP face.
    assert!(
        mcp_token_works(&url, &access2).await,
        "the rotated access token must work"
    );

    // Explicit equal scope: accepted.
    let response = mcp_refresh_request(
        ctx,
        &realm_id,
        &refresh2,
        "herald-mcp",
        &resource,
        Some(MCP_ALL_SCOPES),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "an explicit scope equal to the granted set must be accepted"
    );
    let rotated: Value = crate::tests::response_json(response).await;
    let refresh3 = rotated["refresh_token"].as_str().unwrap().to_string();

    // Explicit different scope: rejected, and the token is NOT consumed.
    let response = mcp_refresh_request(
        ctx,
        &realm_id,
        &refresh3,
        "herald-mcp",
        &resource,
        Some(MCP_PROFILE_SCOPE),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = crate::tests::response_json(response).await;
    assert_eq!(
        body["error"], "invalid_scope",
        "the refresh grant can never change the granted scope set"
    );

    let response =
        mcp_refresh_request(ctx, &realm_id, &refresh3, "herald-mcp", &resource, None).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a rejected attempt must not consume the refresh token"
    );
    let rotated: Value = crate::tests::response_json(response).await;
    let refresh4 = rotated["refresh_token"].as_str().unwrap().to_string();

    // Reuse of an already-rotated refresh token: invalid_grant + family
    // revocation — the successor access token must die with the family.
    let response =
        mcp_refresh_request(ctx, &realm_id, &refresh4, "herald-mcp", &resource, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let rotated: Value = crate::tests::response_json(response).await;
    let access5 = rotated["access_token"].as_str().unwrap().to_string();

    let response =
        mcp_refresh_request(ctx, &realm_id, &refresh4, "herald-mcp", &resource, None).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = crate::tests::response_json(response).await;
    assert_eq!(body["error"], "invalid_grant");

    let response = raw_mcp_post(
        &url,
        Some(&access5),
        None,
        tool_call_envelope("get_my_profile", json!({})),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "reuse detection must revoke the successor access token too"
    );
}

// Given a live MCP family,
// When the refresh grant is attempted with a wrong resource, a non-MCP
// client_id, or against another realm's token path,
// Then each is rejected with its specific OAuth error and none of them
// consumes the token — the bindings (realm/client/resource) come from the
// family record, and a request may only repeat them.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: US-MCP-001 刷新绑定负例（invalid_target/unsupported_grant_type/realm mismatch）
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_refresh_binding_rejections_leave_token_unconsumed(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let resource = mcp_canonical_resource(ctx, &realm_id);
    let email = "mcp-refresh-bindings@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;

    // Wrong resource: invalid_target.
    let wrong_resource = format!("{resource}-attacker");
    let response = mcp_refresh_request(
        ctx,
        &realm_id,
        &tokens.refresh_token,
        "herald-mcp",
        &wrong_resource,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = crate::tests::response_json(response).await;
    assert_eq!(body["error"], "invalid_target");

    // A client other than the built-in MCP client: the grant is not for them.
    let response = mcp_refresh_request(
        ctx,
        &realm_id,
        &tokens.refresh_token,
        "admin-web-console",
        &resource,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = crate::tests::response_json(response).await;
    assert_eq!(body["error"], "unsupported_grant_type");

    // Another realm's token path: the family realm binding refuses.
    let other_realm = format!("other-realm-{}", uuid::Uuid::now_v7().simple());
    let response = mcp_refresh_request(
        ctx,
        &other_realm,
        &tokens.refresh_token,
        "herald-mcp",
        &resource,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = crate::tests::response_json(response).await;
    assert_eq!(body["error"], "invalid_grant");
    assert_eq!(
        body["error_description"].as_str(),
        Some("realm mismatch"),
        "a cross-realm refresh attempt is named for what it is: {body}"
    );

    // None of the rejections consumed the refresh token.
    let response = mcp_refresh_request(
        ctx,
        &realm_id,
        &tokens.refresh_token,
        "herald-mcp",
        &resource,
        None,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "rejected binding attempts must leave the refresh token unconsumed"
    );
}

// Given a live MCP family,
// When an administrator disables the built-in herald-mcp client and later
// re-enables it,
// Then the disable immediately kills the existing access token (401) and
// refresh token (invalid_grant), and the RE-ENABLE does not revive them —
// the persisted generation was bumped, so old families stay dead by
// construction instead of relying on Redis eviction surviving the round trip.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-011
// Covers: US-MCP-011 停用立即失效；重新启用不复活旧凭证
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_client_disable_then_reenable_never_revives_credentials(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let resource = mcp_canonical_resource(ctx, &realm_id);

    let (admin_token, admin_user_id) =
        create_admin_session_with_user(ctx, "mcp-toggle-admin@test.com", 1800).await;
    grant_realm_admin_role(ctx, &admin_user_id).await;
    let mcp_app_id = herald_mcp_client_app_id(ctx, &realm_id).await;

    let email = "mcp-toggle-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;

    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);

    // Sanity before the toggle: the credential works.
    assert!(
        mcp_token_works(&url, &tokens.access_token).await,
        "the issued access token must work before the toggle"
    );

    set_herald_mcp_enabled(ctx, &admin_token, mcp_app_id, false).await;

    assert!(
        mcp_token_is_dead(&url, &tokens.access_token).await,
        "a disabled MCP client kills issued access tokens immediately"
    );

    let response = mcp_refresh_request(
        ctx,
        &realm_id,
        &tokens.refresh_token,
        "herald-mcp",
        &resource,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = crate::tests::response_json(response).await;
    assert_eq!(
        body["error"], "invalid_grant",
        "the refresh path re-checks the live client row"
    );

    // Re-enabling restores the client for NEW authorizations but must not
    // resurrect the disabled-generation families.
    set_herald_mcp_enabled(ctx, &admin_token, mcp_app_id, true).await;

    assert!(
        mcp_token_is_dead(&url, &tokens.access_token).await,
        "re-enabling must not revive credentials issued before the disable"
    );

    let response = mcp_refresh_request(
        ctx,
        &realm_id,
        &tokens.refresh_token,
        "herald-mcp",
        &resource,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = crate::tests::response_json(response).await;
    assert_eq!(body["error"], "invalid_grant");
}

// Given one valid refresh token,
// When two refresh grants run it CONCURRENTLY,
// Then exactly one rotation succeeds — the atomic Redis rotation is the
// serialization point — and the loser is told invalid_grant. Two winners
// would mean two live families from one token (token replay).
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: US-MCP-001 并发刷新仅一个成功
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_concurrent_refresh_has_exactly_one_winner(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let resource = mcp_canonical_resource(ctx, &realm_id);
    let email = "mcp-concurrent-refresh@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;

    let (first, second) = tokio::join!(
        mcp_refresh_request(
            ctx,
            &realm_id,
            &tokens.refresh_token,
            "herald-mcp",
            &resource,
            None
        ),
        mcp_refresh_request(
            ctx,
            &realm_id,
            &tokens.refresh_token,
            "herald-mcp",
            &resource,
            None
        )
    );

    let mut statuses = [first.status(), second.status()];
    statuses.sort_unstable();
    assert_eq!(
        statuses,
        [StatusCode::OK, StatusCode::BAD_REQUEST],
        "exactly one of the concurrent refreshes may succeed"
    );

    let (winner, loser) = if first.status() == StatusCode::OK {
        (first, second)
    } else {
        (second, first)
    };
    let winner_body: Value = crate::tests::response_json(winner).await;
    let rotated_access = winner_body["access_token"].as_str().unwrap().to_string();
    let loser_body: Value = crate::tests::response_json(loser).await;
    assert_eq!(loser_body["error"], "invalid_grant");

    // The loser's failed attempt is a REPLAY of the consumed refresh token:
    // reuse detection atomically revokes the whole family, so the winner's
    // freshly rotated tokens die with it. That is the strong guarantee —
    // two live families from one refresh token must never coexist, even at
    // the cost of forcing the client back through authorization.
    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);
    assert!(
        mcp_token_is_dead(&url, &rotated_access).await,
        "reuse detection must revoke the family: the winner's rotated token cannot outlive the loser's replay"
    );
}

// Given a live MCP family,
// When the user is disabled / force-logged-out (the admin path revokes all
// of the user's token families through the shared service),
// Then the MCP access token answers 401 and the refresh token invalid_grant
// — user-status enforcement covers the MCP face, not just browser sessions.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: US-MCP-001 用户禁用/强制下线撤销 MCP family
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_user_family_revocation_kills_credentials(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let resource = mcp_canonical_resource(ctx, &realm_id);
    let email = "mcp-revoked-user@test.com";
    let password = "password123";
    let user_id = create_test_user(ctx, email, password).await;
    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;

    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);

    assert!(
        mcp_token_works(&url, &tokens.access_token).await,
        "the access token must work before the revocation"
    );

    // The same revocation primitive the admin user-disable / force-logout
    // paths call.
    RedisBrowserTokenService::new(ctx._app_state.redis_manager.clone())
        .revoke_user_families(&user_id.to_string())
        .await
        .expect("user family revocation must succeed");

    assert!(mcp_token_is_dead(&url, &tokens.access_token).await);

    let response = mcp_refresh_request(
        ctx,
        &realm_id,
        &tokens.refresh_token,
        "herald-mcp",
        &resource,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = crate::tests::response_json(response).await;
    assert_eq!(body["error"], "invalid_grant");
}

// =============================================================================
// Scenario 7: V3 isolation — MCP credentials on browser faces (reverse)
// =============================================================================

// Given a valid MCP access token,
// When it is presented as a Bearer credential on browser-facing endpoints
// (session status, the points REST surface, change-email, OIDC userinfo,
// switch-client),
// Then every one answers 401: the MCP credential face is accepted by exactly
// one resource, so an agent token can never be upgraded into a browser
// session or a first-party UI token.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: US-MCP-001 MCP 凭证在 browser 面全部 401
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_tokens_rejected_on_browser_face(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let email = "mcp-isolation@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;
    let bearer = format!("Bearer {}", tokens.access_token);

    let app = ctx.create_unified_test_router();

    let cases: Vec<(&str, Request<Body>)> = vec![
        (
            "session status",
            Request::builder()
                .uri("/api/auth/status")
                .header(header::AUTHORIZATION, &bearer)
                .body(Body::empty())
                .unwrap(),
        ),
        (
            "points REST",
            Request::builder()
                .uri("/api/points/wallets")
                .header(header::AUTHORIZATION, &bearer)
                .body(Body::empty())
                .unwrap(),
        ),
        (
            "change-email",
            Request::builder()
                .method("POST")
                .uri(format!("/api/auth/{realm_id}/change_email/request"))
                .header(header::AUTHORIZATION, &bearer)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "newEmail": "next@mcp-isolation.test",
                        "reauthToken": "unused",
                    })
                    .to_string(),
                ))
                .unwrap(),
        ),
        (
            "OIDC userinfo",
            Request::builder()
                .uri(format!("/api/oauth/{realm_id}/userinfo"))
                .header(header::AUTHORIZATION, &bearer)
                .body(Body::empty())
                .unwrap(),
        ),
        (
            "switch-client",
            Request::builder()
                .method("POST")
                .uri("/api/auth/browser-token/switch-client")
                .header(header::AUTHORIZATION, &bearer)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "targetClientId": "admin-web-console" }).to_string(),
                ))
                .unwrap(),
        ),
    ];

    for (name, request) in cases {
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "an MCP token must be rejected by the browser face ({name})"
        );
    }
}

// Given a valid MCP refresh token,
// When it is presented to the BROWSER refresh endpoint,
// Then the answer is 401 AND the token is not consumed — the standard MCP
// refresh grant still succeeds afterwards, proving the browser endpoint
// neither rotated nor revoked the family it refused.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: US-MCP-001 MCP refresh token 在 browser 刷新面 401 且不旋转
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_refresh_token_rejected_by_browser_refresh_without_rotation(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let resource = mcp_canonical_resource(ctx, &realm_id);
    let email = "mcp-rt-isolation@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;

    let response = browser_refresh_request(ctx, &tokens.refresh_token).await;
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "the browser refresh endpoint must refuse MCP refresh tokens"
    );

    // Non-consumption proof: the standard MCP grant still rotates it.
    let response = mcp_refresh_request(
        ctx,
        &realm_id,
        &tokens.refresh_token,
        "herald-mcp",
        &resource,
        None,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the browser endpoint must not have consumed the MCP refresh token"
    );
}

// Given an MCP access token,
// When it is probed through the browser permission check (by its own user,
// with a genuine browser session) and through the ext permission check
// (with a valid API key),
// Then both answer allowed=false with NO user id (the ext probe adds
// error="invalid_token") — the probe surfaces must never introspect an
// agent credential into permissions or identity.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: US-MCP-001 权限探测不泄露 MCP 凭证信息
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_permission_probes_do_not_introspect_mcp_tokens(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let email = "mcp-probe-user@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_ALL_SCOPES)).await;

    // Browser probe: the SAME user authenticates with a genuine browser
    // session (plain login — the account already exists) and offers the
    // MCP token as the probed credential.
    let browser_token = login_user(ctx, email, password).await;

    let request = Request::builder()
        .method("POST")
        .uri("/api/permission/check")
        .header(header::AUTHORIZATION, format!("Bearer {browser_token}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "token": tokens.access_token,
                "clientId": ctx._client_id,
                "rules": [{ "resource": "users", "action": "view" }],
            })
            .to_string(),
        ))
        .unwrap();
    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = crate::tests::response_json(response).await;
    let probe = body.get("data").cloned().unwrap_or(body);
    assert_eq!(
        probe["allowed"], false,
        "an MCP token must not introspect as allowed"
    );
    assert!(
        probe.get("userId").is_none(),
        "the probe must not reveal who owns the MCP token: {probe}"
    );

    // Ext probe (SDK surface): a valid API key introspects the MCP token.
    let (api_key, _entity) = create_test_api_key(ctx, "mcp-probe-ext", true, None).await;
    let request = Request::builder()
        .method("POST")
        .uri("/api/ext/permission/check")
        .header("x-api-key", &api_key)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "accessToken": tokens.access_token,
                "rules": [{ "resource": "users", "action": "view" }],
            })
            .to_string(),
        ))
        .unwrap();
    let response = ctx
        .create_unified_test_router()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let probe: Value = crate::tests::response_json(response).await;
    assert_eq!(probe["allowed"], false);
    assert_eq!(
        probe["error"].as_str(),
        Some("invalid_token"),
        "the ext probe names the credential-face mismatch: {probe}"
    );
    assert!(
        probe.get("userId").is_none(),
        "the ext probe must not reveal who owns the MCP token: {probe}"
    );
}

// =============================================================================
// Scenario 8: per-user rate limit + body ceiling
// =============================================================================

// Given the per-user MCP quota of 60 requests / 60s (keyed by the
// AUTHENTICATED user, so quotas never leak across users),
// When the budget is consumed by raw POSTs and a 61st arrives in the same
// window,
// Then the 61st is rejected 429 while a DIFFERENT user's token still passes;
// And a body over the 1 MiB transport buffer is rejected 413 regardless of
// the quota — the ceiling is a pre-parse bound, not a rate decision.
//
// Raw POSTs instead of an rmcp client: the limiter sits in middleware before
// any MCP semantics, so this pins the quota boundary exactly (60 pass, 61st
// 429) without 60 full protocol handshakes.
//
// User Story: docs/user-stories/integration/mcp-server.md US-MCP-001
// Covers: US-MCP-001 per-user 限流 429 与 1 MiB body 上限 413
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_rate_limit_per_user_and_body_ceiling(ctx: &mut TestContext) {
    let realm_id = ctx._realm_id.clone();
    let url = format!("{}/{}", spawn_mcp_server(ctx).await, realm_id);
    let envelope = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string();

    let email = "mcp-ratelimit-a@test.com";
    let password = "password123";
    let _user_id = create_test_user(ctx, email, password).await;
    let tokens = obtain_mcp_tokens(ctx, &realm_id, email, password, Some(MCP_PROFILE_SCOPE)).await;

    for i in 1..=60 {
        let response = raw_mcp_post(&url, Some(&tokens.access_token), None, envelope.clone()).await;
        assert_ne!(
            response.status().as_u16(),
            429,
            "request {i} within the 60/60s budget must not be rate limited"
        );
    }

    let response = raw_mcp_post(&url, Some(&tokens.access_token), None, envelope).await;
    assert_eq!(
        response.status().as_u16(),
        429,
        "the 61st request in the same window must be rejected with 429"
    );

    // The quota is per-user: another authenticated user is unaffected.
    let other_email = "mcp-ratelimit-b@test.com";
    let _other_user = create_test_user(ctx, other_email, password).await;
    let other_tokens = obtain_mcp_tokens(
        ctx,
        &realm_id,
        other_email,
        password,
        Some(MCP_PROFILE_SCOPE),
    )
    .await;
    let response = raw_mcp_post(
        &url,
        Some(&other_tokens.access_token),
        None,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string(),
    )
    .await;
    assert_ne!(
        response.status().as_u16(),
        429,
        "a different user must have an independent budget"
    );

    // Body ceiling: just over 1 MiB is refused before any parsing.
    let oversized = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\",\"pad\":\"{}\"}}",
        "a".repeat(1024 * 1024)
    );
    let response = raw_mcp_post(&url, Some(&other_tokens.access_token), None, oversized).await;
    assert_eq!(
        response.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "bodies over the 1 MiB transport buffer must be rejected"
    );
    assert_eq!(
        response_body(response).await["error"],
        "request_too_large",
        "the 413 body names the transport limit"
    );
}
