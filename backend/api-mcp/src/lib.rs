// Herald MCP server crate.
//
// Exposes the Model Context Protocol endpoint at `/mcp/{realmId}` (Streamable
// HTTP, MCP 2026-07-28) with nine read-only tools backed by the existing
// domain services. The tenant is an explicit path segment: the tool realm
// always comes from the verified OAuth credential and must match the URL.
// The router is mounted inside `create_api_routes` (merged, not nested), so
// the outer request-id / RED metrics / trace / CORS stack applies
// automatically and the auth middleware sees the full path it extracts the
// realm from; the admin-console token middleware does not apply (this
// protocol surface carries its own OAuth resource-server middleware).

pub mod dto;
pub mod mcp_oauth_auth;
pub mod tool_error;
pub mod tools;

use axum::Router;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;

use herald_api_base::application::http::state::AppState;

/// Create the `/mcp/{realmId}` router. The routes live at top-level paths so
/// the auth middleware (layered on this router) reads the full request path;
/// rmcp's transport service is path-agnostic and only inspects method,
/// headers and body.
///
/// rmcp's own allowed-hosts list is left EMPTY on purpose: a static list
/// cannot follow per-realm custom domains enabled at runtime, so the
/// DNS-rebinding gate runs in `mcp_oauth_auth` (per request, against the
/// same live canonical origin the audience binding uses) before the
/// transport ever sees the request.
pub fn create_mcp_router(state: AppState) -> Router<AppState> {
    // Stateful-mode factory semantics: for 2026-07-28 clients every request
    // is served statelessly (SEP-2567), so the factory runs per request and
    // must only capture cheap clones — AppState is an Arc-field struct.
    let factory_state = state.clone();
    let config = StreamableHttpServerConfig::default().with_allowed_hosts(Vec::<String>::new());

    let mcp_service = StreamableHttpService::new(
        move || Ok(tools::HeraldMcpService::new(factory_state.clone())),
        std::sync::Arc::new(LocalSessionManager::default()),
        config,
    );

    Router::new()
        .route_service("/mcp/{realmId}", mcp_service)
        .layer(axum::middleware::from_fn_with_state(
            state,
            mcp_oauth_auth::mcp_oauth_auth_middleware,
        ))
}
