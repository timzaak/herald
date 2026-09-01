pub mod api_keys;
pub mod audit;
pub mod client_apps;
pub mod common;
pub mod dashboard;
pub mod legal;
pub mod public_config;
pub mod rate_limit;
pub mod realm;
pub mod realm_config;
pub mod server;
pub mod user;
pub mod users;

pub mod state;

// Modules extracted to sub-crates - re-exported for backward compatibility
// Each sub-crate is exposed under its original module name

// admin module: re-exports from herald_api_admin
pub mod admin {
    pub use herald_api_admin::admin::*;
    pub use herald_api_admin::admin_router_with_middleware;
}

// permission module: re-exported from herald_api_admin
pub use herald_api_admin::permission;

// role_definitions module: re-exported from herald_api_admin
pub use herald_api_admin::role_definitions;

// auth module: re-exports from herald_api_auth
pub mod auth {
    pub use herald_api_auth::*;
}

// billing module: re-exported from herald_api_billing
pub use herald_api_billing as billing;

// ext module: re-exported from herald_api_ext
pub mod ext {
    pub use herald_api_ext::*;
}

// mcp module: re-exported from herald_api_mcp
pub mod mcp {
    pub use herald_api_mcp::*;
}

// oauth module: re-exported from herald_api_oauth
pub mod oauth {
    pub use herald_api_oauth::*;
}

// points module: re-exported from herald_api_points
pub mod points {
    pub use herald_api_points::*;
}
