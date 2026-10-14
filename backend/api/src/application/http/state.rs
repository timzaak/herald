// Re-export AppState from api-base crate to ensure type compatibility
// across all api sub-crates
pub use herald_api_base::application::http::state::AppState;
pub use herald_api_base::application::http::state::build_org_service;
