// =============================================================================
// MCP Server Scenario Tests
// =============================================================================
//
// End-to-end coverage of the /mcp endpoint: a real rmcp client transport
// (Streamable HTTP) drives a real unified test router over an ephemeral TCP
// listener, so every hop is exercised — routing, the protocol-level API-key
// middleware, the MCP Streamable HTTP service, tool-level RBAC, and the
// domain services behind the five tools.
//
// Why full-stack rather than oneshot: the rmcp client transport requires a
// real URL, and the protocol-level guarantees (initialize handshake, parts
// injection into tool handlers, tool-level isError semantics) only exist
// across the real stack.
//
// Cross-realm rejection is structural: no tool accepts a realm argument, so
// a foreign-realm target simply reads as not_found (verified in the
// query_users cross-realm scenario; the same mechanism covers every tool).
//
// Reference: docs/user-stories/integration/mcp-server.md
//
// =============================================================================

use std::collections::HashMap;

use axum::http::{HeaderName, HeaderValue};
use rmcp::model::{CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock};
use rmcp::service::{ClientInitializeError, RoleClient, RunningService, ServiceExt};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use test_context::test_context;

use crate::tests::helpers::client_helpers::{create_test_api_key, disable_api_key};
use crate::tests::scenarios::points::fixtures::{
    create_test_points_wallet, create_test_transaction, create_test_user,
};
use crate::tests::schema_test_context::SchemaTestContext as TestContext;
use herald_core::domain::authorization::permission_service::PermissionService;
use herald_core::domain::authorization::principal_types;

type McpClient = RunningService<RoleClient, ()>;

// =============================================================================
// Helpers
// =============================================================================

/// Spawn the full production router (create_api_routes) on an ephemeral
/// loopback port and return its base URL. The rmcp client transport needs a
/// real URL; oneshot cannot host it.
async fn spawn_mcp_server(ctx: &TestContext) -> String {
    let router = ctx.create_unified_test_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind ephemeral listener");
    let addr = listener.local_addr().expect("Failed to read local addr");
    tokio::spawn(async move {
        // Runs until the test runtime drops; serve errors surface as a
        // panicked (and reported) spawned task.
        axum::serve(listener, router)
            .await
            .expect("MCP test server failed");
    });
    format!("http://{addr}/mcp")
}

/// Connect an rmcp client carrying `X-API-Key`. Performs the initialize
/// handshake; a rejected credential fails here (HTTP 401 surfaces as a
/// transport error before the handshake completes).
async fn connect_mcp(url: &str, api_key: &str) -> Result<McpClient, ClientInitializeError> {
    let mut headers = HashMap::new();
    headers.insert(
        HeaderName::from_static("x-api-key"),
        HeaderValue::from_str(api_key).expect("API key is valid header text"),
    );
    let transport = StreamableHttpClientTransport::with_client(
        rmcp_test_reqwest::Client::new(),
        StreamableHttpClientTransportConfig::with_uri(url.to_string()).custom_headers(headers),
    );
    ().serve(transport).await
}

/// Connect an rmcp client sending the API key as `Authorization: Bearer`.
async fn connect_mcp_bearer(url: &str, api_key: &str) -> Result<McpClient, ClientInitializeError> {
    let transport = StreamableHttpClientTransport::with_client(
        rmcp_test_reqwest::Client::new(),
        StreamableHttpClientTransportConfig::with_uri(url.to_string())
            .auth_header(api_key.to_string()),
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

/// Assert a tool-level business error: isError + "<code>: ..." prefix, and
/// (for denial cases) no data payload in the content.
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

/// Grant one permission to an API key principal via a dedicated role
/// (mirrors the single-permission seeding used by the RBAC scenario suite,
/// bound to the api_key principal instead of a user) and drop the principal's
/// cached bindings so the grant is visible immediately.
async fn grant_api_key_permission(
    ctx: &TestContext,
    api_key_id: &str,
    resource: &str,
    action: &str,
) {
    let role_uuid = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO roles (id, name, description, realm_id, client_id, is_builtin)
         VALUES ($1, $2, $3, $4, $5, false)",
    )
    .bind(role_uuid)
    .bind(format!("mcp-test-role-{}-{}", resource, action))
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
         VALUES ($1, NULL, $2, $3, $4, $5, $6)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(role_uuid)
    .bind(&ctx._realm_id)
    .bind(&ctx._client_id)
    .bind(principal_types::API_KEY)
    .bind(api_key_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to bind MCP test role to API key");

    let _ = ctx
        ._app_state
        .permission_checker
        .invalidate_principal_role_cache(&ctx._realm_id, principal_types::API_KEY, api_key_id)
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

// =============================================================================
// Scenario 1: connect + tools/list (US-MCP-001 scenario 1)
// =============================================================================

// Given an API key with the four query permissions,
// When an agent client connects and lists tools,
// Then the handshake succeeds and exactly the five read-only tools are listed.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_connect_and_list_tools_succeeds(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-connect", true, None).await;
    for (resource, action) in [
        ("users", "view"),
        ("points", "view"),
        ("audit", "view"),
        ("settings", "view"),
    ] {
        grant_api_key_permission(ctx, &entity.id, resource, action).await;
    }

    let mut client = connect_mcp(&url, &api_key)
        .await
        .expect("connect with a valid, permissioned key must succeed");

    let tools = client
        .list_tools(None)
        .await
        .expect("tools/list must succeed after initialize");

    let mut names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "get_points_balance",
            "get_realm_config_status",
            "list_audit_logs",
            "list_points_transactions",
            "query_users",
        ],
        "the tool list must be exactly the five read-only tools"
    );

    let _ = client.close().await;
}

// =============================================================================
// Scenario 2: invalid key rejected at connect (US-MCP-001 scenario 2)
// =============================================================================

// Given a forged API key,
// When an agent client connects,
// Then the connection is rejected at the HTTP layer (401) before any MCP
// handshake completes — no protocol surface for unauthenticated callers.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_connect_rejected_with_invalid_key(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;

    let err = connect_mcp(&url, "sk-not-a-real-key")
        .await
        .expect_err("connect with a forged key must fail");
    let message = format!("{err:#}");
    assert!(
        message.contains("401"),
        "expected the transport to surface HTTP 401, got: {message}"
    );
}

// =============================================================================
// Scenario 3: disabled key rejected at connect (US-MCP-001 scenario 3)
// =============================================================================

// Given a real API key that is disabled after creation,
// When an agent client connects,
// Then the connection is rejected (401 api_key_disabled) — disabling a key
// immediately evicts the agent, including on the cached path.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_connect_rejected_with_disabled_key(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-disabled", true, None).await;
    disable_api_key(ctx, &entity.id, &api_key).await;

    let err = connect_mcp(&url, &api_key)
        .await
        .expect_err("connect with a disabled key must fail");
    let message = format!("{err:#}");
    assert!(
        message.contains("401"),
        "expected the transport to surface HTTP 401, got: {message}"
    );
}

// =============================================================================
// Scenario 4: connectivity self-check via config status (US-MCP-001
// scenario 4 + US-MCP-006 scenario 1)
// =============================================================================

// Given a key with settings.view,
// When the zero-argument get_realm_config_status tool is called,
// Then it succeeds and returns per-entry status WITHOUT any configValue —
// the documented connectivity self-check and the strictest data-minimization
// point (values never leave, not even non-sensitive ones).
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_config_status_selfcheck_succeeds(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-selfcheck", true, None).await;
    grant_api_key_permission(ctx, &entity.id, "settings", "view").await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(&client, "get_realm_config_status", serde_json::json!({})).await;
    assert_eq!(result.is_error, Some(false), "self-check must succeed");

    let body = result_json(&result);
    assert_eq!(
        body["realmId"].as_str(),
        Some(ctx._realm_id.as_str()),
        "realm id must be echoed back so the agent can confirm context"
    );
    assert!(
        body["configs"].is_array(),
        "configs must be an array, got: {body}"
    );
    for config in body["configs"].as_array().expect("configs array") {
        assert!(
            config.get("configValue").is_none(),
            "configValue must never appear in config status output: {config}"
        );
    }

    let _ = client.close().await;
}

// =============================================================================
// Scenario 5: query_users list + detail (US-MCP-002 scenarios 1/2)
// =============================================================================

// Given a key with users.view and a seeded user,
// When listing users and then fetching one by userId,
// Then the list contains the user with the minimal fields and the detail
// returns the full minimal set (id/email/nickname/status/createdAt).
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_query_users_list_and_detail(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-users", true, None).await;
    grant_api_key_permission(ctx, &entity.id, "users", "view").await;

    let user_id = create_test_user(&ctx._app_state.pool, &ctx._realm_id, "mcp-list@test.com").await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let list = call_tool(
        &client,
        "query_users",
        serde_json::json!({ "page": 1, "pageSize": 100 }),
    )
    .await;
    assert_eq!(list.is_error, Some(false), "list must succeed");
    let list_body = result_json(&list);
    let users = list_body["users"].as_array().expect("users array");
    assert!(
        users
            .iter()
            .any(|u| u["id"].as_str() == Some(user_id.to_string().as_str())),
        "seeded user must appear in the list: {list_body}"
    );

    let detail = call_tool(
        &client,
        "query_users",
        serde_json::json!({ "userId": user_id.to_string() }),
    )
    .await;
    assert_eq!(detail.is_error, Some(false), "detail must succeed");
    let detail_body = result_json(&detail);
    let user = &detail_body["users"][0];
    assert_eq!(user["id"].as_str(), Some(user_id.to_string().as_str()));
    assert_eq!(user["email"].as_str(), Some("mcp-list@test.com"));
    assert!(
        user.get("nickname").is_some()
            && user.get("status").is_some()
            && user.get("createdAt").is_some(),
        "detail must include nickname/status/createdAt: {user}"
    );

    let _ = client.close().await;
}

// =============================================================================
// Scenario 6: query_users permission denied (US-MCP-002 scenario 3)
// =============================================================================

// The user service applies NO RBAC gate to API-key identities — the tool
// layer is the only defense. This test pins that contract: a key with no
// users.view gets an agent-readable permission_denied and zero user data.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_query_users_permission_denied(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, _entity) = create_test_api_key(ctx, "mcp-users-denied", true, None).await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(&client, "query_users", serde_json::json!({})).await;
    assert_tool_error(&result, "permission_denied");
    let text = result_text(&result);
    assert!(
        text.contains("users.view"),
        "the denial must name the missing permission: {text}"
    );
    assert!(
        !text.contains("\"users\""),
        "a denial must not carry a users payload: {text}"
    );

    let _ = client.close().await;
}

// =============================================================================
// Scenario 7: cross-realm target reads as not_found (US-MCP-002 scenario 4)
// =============================================================================

// Given a user that exists only in another realm,
// When the key's realm queries that userId,
// Then the result is not_found — cross-realm reads are structurally
// inexpressible (no realm argument on any tool; realm comes from the
// credential), and the failure mode leaks neither existence nor data.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_query_users_cross_realm_target_not_found(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-cross-realm", true, None).await;
    grant_api_key_permission(ctx, &entity.id, "users", "view").await;

    let other_realm_id = uuid::Uuid::now_v7().to_string();
    let foreign_user_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account (id, realm_id, email, password, status)
         VALUES ($1, $2, 'foreign@other-realm.test', NULL, 1)",
    )
    .bind(foreign_user_id)
    .bind(&other_realm_id)
    .execute(&ctx._app_state.pool)
    .await
    .expect("Failed to seed foreign-realm user");

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(
        &client,
        "query_users",
        serde_json::json!({ "userId": foreign_user_id.to_string() }),
    )
    .await;
    assert_tool_error(&result, "not_found");
    let text = result_text(&result);
    assert!(
        text.contains("was not found in this realm"),
        "cross-realm target must read as realm-local not_found: {text}"
    );

    let _ = client.close().await;
}

// =============================================================================
// Scenario 8: query_users user not found (US-MCP-002 scenario 5)
// =============================================================================

// Given a random UUID that matches no user,
// When fetching it by userId,
// Then the tool returns not_found (an agent can self-correct the id).
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_query_users_not_found(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-users-404", true, None).await;
    grant_api_key_permission(ctx, &entity.id, "users", "view").await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(
        &client,
        "query_users",
        serde_json::json!({ "userId": uuid::Uuid::now_v7().to_string() }),
    )
    .await;
    assert_tool_error(&result, "not_found");

    let _ = client.close().await;
}

// =============================================================================
// Scenario 9: points balance (US-MCP-003 scenario 1)
// =============================================================================

// Given a seeded wallet with a known balance,
// When querying the balance,
// Then the amount is correct and the scope field is present ("realm" for an
// unbound key) so the agent knows what the number covers.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_points_balance_returns_balance(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-balance", true, None).await;
    grant_api_key_permission(ctx, &entity.id, "points", "view").await;

    let user_id =
        create_test_user(&ctx._app_state.pool, &ctx._realm_id, "mcp-balance@test.com").await;
    let balance = 500i64;
    create_test_points_wallet(&ctx._app_state.pool, user_id, balance).await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(
        &client,
        "get_points_balance",
        serde_json::json!({ "userId": user_id.to_string() }),
    )
    .await;
    assert_eq!(result.is_error, Some(false), "balance query must succeed");
    let body = result_json(&result);
    assert_eq!(body["balance"].as_i64(), Some(balance), "body: {body}");
    assert_eq!(body["scope"].as_str(), Some("realm"), "body: {body}");

    let _ = client.close().await;
}

// =============================================================================
// Scenario 10: points balance permission denied (US-MCP-003 scenario 2)
// =============================================================================

// The points service lets API keys view anything (can_view_points is
// unconditionally true for ThirdParty) — the tool gate is the only defense.
// A key without points.view gets a denial and zero balance data.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_points_balance_permission_denied(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, _entity) = create_test_api_key(ctx, "mcp-balance-denied", true, None).await;

    let user_id =
        create_test_user(&ctx._app_state.pool, &ctx._realm_id, "mcp-bal-d@test.com").await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(
        &client,
        "get_points_balance",
        serde_json::json!({ "userId": user_id.to_string() }),
    )
    .await;
    assert_tool_error(&result, "permission_denied");
    assert!(
        result_text(&result).contains("points.view"),
        "denial must name points.view"
    );

    let _ = client.close().await;
}

// =============================================================================
// Scenario 11: points balance user not found ≠ zero balance (US-MCP-003
// scenario 3)
// =============================================================================

// get_balance synthesizes a zero balance for wallet-less users; without the
// existence pre-check a nonexistent user would misreport as "0 points".
// This test pins the corrected contract: not_found, never a fake zero.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_points_balance_user_not_found(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-balance-404", true, None).await;
    grant_api_key_permission(ctx, &entity.id, "points", "view").await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(
        &client,
        "get_points_balance",
        serde_json::json!({ "userId": uuid::Uuid::now_v7().to_string() }),
    )
    .await;
    assert_tool_error(&result, "not_found");

    let _ = client.close().await;
}

// =============================================================================
// Scenario 12: points transactions + filters (US-MCP-004 scenario 1)
// =============================================================================

// Given a wallet with a recharge and a consume transaction,
// When listing with transactionType=consume,
// Then only the consume row returns, amounts are signed, and the payload
// omits the wallet/bucket attribution fields (data minimization: agents may
// carry tool output into third-party models).
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_points_transactions_list_and_filters(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-txs", true, None).await;
    grant_api_key_permission(ctx, &entity.id, "points", "view").await;

    let user_id = create_test_user(&ctx._app_state.pool, &ctx._realm_id, "mcp-txs@test.com").await;
    let wallet_id = create_test_points_wallet(&ctx._app_state.pool, user_id, 1000).await;
    create_test_transaction(
        &ctx._app_state.pool,
        wallet_id,
        user_id,
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
        user_id,
        "consume",
        -200,
        800,
        Some("spend"),
        None,
    )
    .await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(
        &client,
        "list_points_transactions",
        serde_json::json!({ "userId": user_id.to_string(), "transactionType": "consume" }),
    )
    .await;
    assert_eq!(result.is_error, Some(false), "filtered list must succeed");
    let body = result_json(&result);
    let txs = body["transactions"].as_array().expect("transactions array");
    assert_eq!(
        txs.len(),
        1,
        "only the consume row matches the filter: {body}"
    );
    assert_eq!(
        txs[0]["amount"].as_i64(),
        Some(-200),
        "amount is signed: {txs:?}"
    );

    // Data minimization: no ledger attribution fields on the tool surface.
    for tx in txs {
        for field in [
            "walletId",
            "bucketId",
            "correlationId",
            "externalRefId",
            "subscriptionId",
            "clientAppId",
        ] {
            assert!(
                tx.get(field).is_none(),
                "minimized field '{field}' must not appear: {tx}"
            );
        }
    }

    let _ = client.close().await;
}

// =============================================================================
// Scenario 13: points transactions permission denied (US-MCP-004 scenario 2)
// =============================================================================

#[test_context(TestContext)]
#[tokio::test]
async fn mcp_points_transactions_permission_denied(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, _entity) = create_test_api_key(ctx, "mcp-txs-denied", true, None).await;

    let user_id =
        create_test_user(&ctx._app_state.pool, &ctx._realm_id, "mcp-txs-d@test.com").await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(
        &client,
        "list_points_transactions",
        serde_json::json!({ "userId": user_id.to_string() }),
    )
    .await;
    assert_tool_error(&result, "permission_denied");

    let _ = client.close().await;
}

// =============================================================================
// Scenario 14: points transactions user not found (US-MCP-004 scenario 3)
// =============================================================================

#[test_context(TestContext)]
#[tokio::test]
async fn mcp_points_transactions_user_not_found(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-txs-404", true, None).await;
    grant_api_key_permission(ctx, &entity.id, "points", "view").await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(
        &client,
        "list_points_transactions",
        serde_json::json!({ "userId": uuid::Uuid::now_v7().to_string() }),
    )
    .await;
    assert_tool_error(&result, "not_found");

    let _ = client.close().await;
}

// =============================================================================
// Scenario 15: audit logs + filters (US-MCP-005 scenario 1)
// =============================================================================

// Given audit events in two categories,
// When listing with category=user_management,
// Then only matching events return and the payload omits ip/userAgent/
// traceId/details — audit detail is the most sensitive read surface and is
// deliberately absent from the agent-facing contract.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_audit_logs_list_and_filters(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-audit", true, None).await;
    grant_api_key_permission(ctx, &entity.id, "audit", "view").await;

    let actor_id = uuid::Uuid::now_v7().to_string();
    seed_audit_event(ctx, "user_management", "user.create", &actor_id).await;
    seed_audit_event(ctx, "auth", "auth.login", &actor_id).await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(
        &client,
        "list_audit_logs",
        serde_json::json!({ "category": "user_management" }),
    )
    .await;
    assert_eq!(
        result.is_error,
        Some(false),
        "filtered audit list must succeed"
    );
    let body = result_json(&result);
    let events = body["events"].as_array().expect("events array");
    assert!(
        events
            .iter()
            .all(|e| e["category"].as_str() == Some("user_management")),
        "category filter must hold: {body}"
    );
    assert!(
        !events.is_empty(),
        "the seeded user_management event must be returned: {body}"
    );

    for event in events {
        for field in ["ipAddress", "userAgent", "traceId", "details"] {
            assert!(
                event.get(field).is_none(),
                "sensitive audit field '{field}' must not appear: {event}"
            );
        }
    }

    let _ = client.close().await;
}

// =============================================================================
// Scenario 16: audit logs permission denied (US-MCP-005 scenario 2)
// =============================================================================

#[test_context(TestContext)]
#[tokio::test]
async fn mcp_audit_logs_permission_denied(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, _entity) = create_test_api_key(ctx, "mcp-audit-denied", true, None).await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(&client, "list_audit_logs", serde_json::json!({})).await;
    assert_tool_error(&result, "permission_denied");
    assert!(
        result_text(&result).contains("audit.view"),
        "denial must name audit.view"
    );

    let _ = client.close().await;
}

// =============================================================================
// Scenario 17: config status permission denied (US-MCP-006 scenario 2)
// =============================================================================

// Completes per-tool denial coverage: every one of the five tools has a
// no-permission test proving the tool layer (the only RBAC defense) gates it.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_config_status_permission_denied(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, _entity) = create_test_api_key(ctx, "mcp-cfg-denied", true, None).await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(&client, "get_realm_config_status", serde_json::json!({})).await;
    assert_tool_error(&result, "permission_denied");
    assert!(
        result_text(&result).contains("settings.view"),
        "denial must name settings.view"
    );

    let _ = client.close().await;
}

// =============================================================================
// Scenario 18: invalid argument on malformed UUID
// =============================================================================

// Given a non-UUID userId,
// When calling query_users,
// Then the tool returns invalid_argument naming the field — the agent can
// self-correct without a round-trip through opaque protocol errors.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_invalid_argument_on_bad_uuid(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-badarg", true, None).await;
    grant_api_key_permission(ctx, &entity.id, "users", "view").await;

    let mut client = connect_mcp(&url, &api_key).await.expect("connect");

    let result = call_tool(
        &client,
        "query_users",
        serde_json::json!({ "userId": "not-a-uuid" }),
    )
    .await;
    assert_tool_error(&result, "invalid_argument");
    assert!(
        result_text(&result).contains("'userId'"),
        "the message must name the offending field: {}",
        result_text(&result)
    );

    let _ = client.close().await;
}

// =============================================================================
// Scenario 19: Bearer transport form accepted
// =============================================================================

// Given the same valid key sent as Authorization: Bearer (the only header
// some MCP clients can customize),
// When connecting and calling a tool,
// Then it authenticates and succeeds — Bearer is the same Client API Key in
// a different transport form, not a browser token.
#[test_context(TestContext)]
#[tokio::test]
async fn mcp_bearer_header_accepted(ctx: &mut TestContext) {
    let url = spawn_mcp_server(ctx).await;
    let (api_key, entity) = create_test_api_key(ctx, "mcp-bearer", true, None).await;
    grant_api_key_permission(ctx, &entity.id, "users", "view").await;

    let user_id =
        create_test_user(&ctx._app_state.pool, &ctx._realm_id, "mcp-bearer@test.com").await;

    let mut client = connect_mcp_bearer(&url, &api_key)
        .await
        .expect("Bearer-form API key must authenticate");

    let result = call_tool(
        &client,
        "query_users",
        serde_json::json!({ "userId": user_id.to_string() }),
    )
    .await;
    assert_eq!(
        result.is_error,
        Some(false),
        "tool call via Bearer must succeed"
    );

    let _ = client.close().await;
}
