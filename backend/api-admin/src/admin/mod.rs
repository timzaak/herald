use axum::Router;
use axum::middleware::from_fn_with_state;
use herald_api_base::application::http::auth::identity_middleware::{
    inject_token_identity, require_admin_console_token,
};
use herald_api_base::application::http::state::AppState;

pub mod admin_users;
pub mod middleware;
pub mod permission;
pub mod permission_definitions;

// Re-export for utoipa

/// Admin router with permission middleware applied
/// This is the secure version that should be used in production
/// Routes are mounted at /api/roles/...
///
/// **Architecture Note**: Permission checks are performed in Service layer (HTTP handlers)
/// NOT in HTTP middleware, following six-sided architecture principles.
pub fn admin_router_with_middleware(state: AppState) -> Router<AppState> {
    use crate::role_definitions;

    Router::new()
        // RBAC 元数据管理 - 仅主管理员可访问
        // Permission checks are done in Service layer or HTTP handlers
        // Realm is session-derived: the admin console token pins it.
        .nest(
            "/define",
            role_definitions::role_defs_router()
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state(state.clone(), inject_token_identity)),
        )
}
