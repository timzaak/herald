//! Permission router
//!
//! Provides endpoints for managing permissions, policies, roles, and user assignments
//! Mounted at /api/permission

use axum::Router;
use herald_api_base::application::http::state::AppState;

pub mod role_policies;
pub mod user_roles;

pub fn permission_router() -> Router<AppState> {
    Router::new()
        // Note: /check endpoint is defined at server level without auth middleware
        // Permission definitions (moved from /api/system)
        .nest(
            "/{realmId}/define",
            crate::admin::permission_definitions::router(),
        )
        // Role policies (moved from /api/system)
        .nest("/roles", role_policies::router())
        // User roles (moved from /api/system)
        .nest("/users", user_roles::router())
}
