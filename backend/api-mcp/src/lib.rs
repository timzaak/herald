// Herald MCP server crate.
//
// Exposes the Model Context Protocol endpoint at `/mcp` (Streamable HTTP,
// MCP 2026-07-28) with five read-only tools backed by the existing domain
// services. The router is mounted inside `create_api_routes`, so the outer
// request-id / RED metrics / trace / CORS stack applies automatically; the
// admin-console token middleware does not (it only layers `/api/*` nests —
// correct for a protocol surface that carries its own API-key auth).

pub mod dto;
pub mod mcp_api_key_auth;
pub mod tool_error;
pub mod tools;

use axum::Router;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;

use herald_api_base::application::http::state::AppState;

/// Host values accepted by the Streamable HTTP `Host` validation.
///
/// rmcp defaults to loopback-only to protect local servers from DNS
/// rebinding; Herald's endpoint is public, so the deployment's public host
/// (from `public_base_url`) joins the loopback set. Tests bind 127.0.0.1
/// and keep working either way.
fn allowed_hosts_for(public_base_url: &str) -> Vec<String> {
    let mut hosts = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];
    if let Ok(url) = url::Url::parse(public_base_url)
        && let Some(host) = url.host_str()
    {
        hosts.push(host.to_string());
        if let Some(port) = url.port() {
            hosts.push(format!("{host}:{port}"));
        }
    }
    hosts
}

/// Create the `/mcp` router. Mount with `.nest("/mcp", ...)` so the service
/// sees the stripped root path.
pub fn create_mcp_router(state: AppState) -> Router<AppState> {
    // Stateful-mode factory semantics: for 2026-07-28 clients every request
    // is served statelessly (SEP-2567), so the factory runs per request and
    // must only capture cheap clones — AppState is an Arc-field struct.
    let factory_state = state.clone();
    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(allowed_hosts_for(&state.public_base_url));

    let mcp_service = StreamableHttpService::new(
        move || Ok(tools::HeraldMcpService::new(factory_state.clone())),
        std::sync::Arc::new(LocalSessionManager::default()),
        config,
    );

    Router::new()
        .route_service("/", mcp_service)
        .layer(axum::middleware::from_fn_with_state(
            state,
            mcp_api_key_auth::mcp_api_key_auth_middleware,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_hosts_keeps_loopback_and_adds_public_host() {
        let hosts = allowed_hosts_for("https://api.example.com");
        assert!(hosts.contains(&"127.0.0.1".to_string()));
        assert!(hosts.contains(&"api.example.com".to_string()));

        let with_port = allowed_hosts_for("http://demo.local:8080");
        assert!(with_port.contains(&"127.0.0.1".to_string()));
        assert!(with_port.contains(&"demo.local:8080".to_string()));

        // Unparseable base URL must not wipe the loopback defaults.
        assert!(allowed_hosts_for("not a url").contains(&"localhost".to_string()));
    }
}
