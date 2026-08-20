// Points API routes

use axum::{Router, middleware::from_fn, routing};

use herald_api_base::application::http::internal_auth::internal_api_key_middleware;
use herald_api_base::application::http::state::AppState;

use super::{
    grant::grant_points,
    internal_quota::{grant_quota_entitlement, revoke_quota_entitlement},
    registration_rules::{get_registration_rules, upsert_registration_rules},
    transactions::{list_transactions, list_user_transactions},
    wallets::{get_wallet, list_user_wallets, list_wallets},
};

/// Points admin router for `/api/points/{realmId}`
///
/// Nested in server/mod.rs under BOTH the flexible auth layer (Bearer or API
/// key) and `require_admin_console_token`. The admin-console credential gate
/// rejects API-key identities (`Identity::ThirdParty` gets a synthetic
/// CustomUserUi context), so effective access is a first-party admin-console
/// Bearer credential only. Third-party API-key callers use `/api/ext/points/*`.
///
/// Routes (when nested under /api/points/{realmId}):
/// - GET /api/points/{realmId}/wallets - List wallets (admin console)
/// - GET /api/points/{realmId}/wallets/{userId} - Get wallet (admin console)
/// - GET /api/points/{realmId}/transactions - List transactions (admin console)
/// - GET /api/points/{realmId}/registration-rules - Get Realm registration rules (points.view)
/// - PUT /api/points/{realmId}/registration-rules - Upsert Realm registration rules (points.manage)
/// - POST /api/points/{realmId}/grant - Grant points to user (admin session only)
///
/// Note: Balance and consume endpoints have been moved to /api/ext/points/ for SDK compatibility.
/// Note: The old default-config / user-configs endpoints have been removed;
/// registration/free-periodic routing is now expressed by registration rules.
pub fn points_router() -> Router<AppState> {
    Router::new()
        .route("/wallets", routing::get(list_wallets))
        .route("/wallets/{userId}", routing::get(get_wallet))
        .route("/transactions", routing::get(list_transactions))
        .route(
            "/registration-rules",
            routing::get(get_registration_rules).put(upsert_registration_rules),
        )
        .route("/grant", routing::post(grant_points))
}

pub fn user_points_router() -> Router<AppState> {
    Router::new()
        .route("/wallets", routing::get(list_user_wallets))
        .route("/transactions", routing::get(list_user_transactions))
}

/// Internal (demo/test-only) points routes.
///
/// These routes bypass normal user authentication and are guarded solely by
/// `internal_api_key_middleware` (the shared `X-Internal-API-Key` /
/// `INTERNAL_API_KEY` secret). Mounted without the identity-injection layer, so
/// handlers receive no `Identity` and must not assume one.
///
/// Routes (absolute paths, since this router is `.merge`-d, not nested):
/// - POST /api/internal/points/{realmId}/quota-entitlement/grant
/// - POST /api/internal/points/{realmId}/quota-entitlement/revoke
pub fn internal_public_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/internal/points/{realmId}/quota-entitlement/grant",
            routing::post(grant_quota_entitlement).layer(from_fn(internal_api_key_middleware)),
        )
        .route(
            "/api/internal/points/{realmId}/quota-entitlement/revoke",
            routing::post(revoke_quota_entitlement).layer(from_fn(internal_api_key_middleware)),
        )
}
