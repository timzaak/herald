// Herald API Ext Module
// External API handlers (API Key authentication)

pub mod api_key_auth;
pub mod authz;
pub mod billing;
pub mod client_app;
mod client_app_scope;
pub mod client_helper;
pub mod permission;
pub mod points;
pub mod realm;
pub mod subscription;
pub mod user;

#[cfg(test)]
mod api_key_auth_test;
#[cfg(test)]
mod authz_test;

use axum::Router;
use herald_api_base::application::http::state::AppState;

/// OpenAPI specification for external API module
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::permission::check_permission,
        crate::subscription::get_subscription,
        crate::billing::get_subscription,
        crate::billing::get_one_time_mappings,
        crate::billing::get_entitlement_currencies,
        crate::billing::resolve_default_price,
        crate::points::get_balance_ext,
        crate::points::consume_points_ext,
        crate::points::grant_points_ext,
        crate::points::get_transaction_ext,
        crate::points::get_transaction_by_external_ref_ext,
        crate::realm::create_realm,
        crate::realm::list_realms,
        crate::realm::get_realm,
        crate::user::create_user,
        crate::user::list_users,
        crate::user::get_user,
        crate::client_app::create_client_app,
        crate::client_app::list_client_apps,
        crate::client_app::get_client_app,
    ),
    components(schemas(
        crate::permission::PermissionCheckRequest,
        crate::permission::PermissionCheckResponse,
        crate::subscription::SubscriptionResponse,
        crate::billing::SubscriptionDetail,
        crate::billing::OneTimeMappingExtResponse,
        crate::billing::ExtOneTimeMappingItem,
        crate::billing::EntitlementCurrenciesResponse,
        crate::billing::EntitlementPriceView,
        crate::points::ExtPointsBalanceResponse,
        crate::points::ExtConsumePointsRequest,
        crate::points::ExtConsumePointsResponse,
        crate::points::BucketTransaction,
        crate::points::AllocationDetail,
        crate::points::ExtGrantPointsRequest,
        crate::points::ExtGrantPointsResponse,
        crate::points::ExtTransactionResponse,
        crate::realm::CreateRealmExtRequest,
        crate::realm::AdminUserInput,
        crate::realm::RealmInfoResponse,
        crate::realm::AdminUserOutput,
        crate::realm::RealmListItem,
        crate::realm::RealmListResponse,
        crate::user::CreateUserExtRequest,
        crate::user::UserInfoResponse,
        crate::user::UserListResponse,
        crate::client_app::CreateClientAppExtRequest,
        crate::client_app::ClientAppInfoResponse,
        crate::client_app::ClientAppListItem,
        crate::client_app::ClientAppListResponse,
    ))
)]
pub struct ApiDoc;

/// Create third-party API router
///
/// All routes in this router require API Key authentication.
pub fn create_router(state: AppState) -> Router<AppState> {
    let api_key_middleware =
        axum::middleware::from_fn_with_state(state.clone(), api_key_auth::api_key_auth_middleware);

    Router::new()
        .route(
            "/permission/check",
            axum::routing::post(permission::check_permission),
        )
        .route(
            "/subscription/{clientAppId}",
            axum::routing::get(subscription::get_subscription),
        )
        .route(
            "/bill/{realmId}/client/{clientAppId}/subscription",
            axum::routing::get(billing::get_subscription),
        )
        .route(
            "/{realmId}/one-time-mappings",
            axum::routing::get(billing::get_one_time_mappings),
        )
        .route(
            "/{realmId}/entitlements/{entitlementKey}/currencies",
            axum::routing::get(billing::get_entitlement_currencies),
        )
        .route(
            "/{realmId}/entitlements/{entitlementKey}/default-price",
            axum::routing::get(billing::resolve_default_price),
        )
        .route(
            "/points/{realmId}/balance",
            axum::routing::get(points::get_balance_ext),
        )
        .route(
            "/points/{realmId}/consume",
            // Request-lifetime bound (audit run-2:
            // consume-idempotency-fingerprint-horizon-gap), scoped to the
            // consume route ONLY: the consume idempotency contract assumes a
            // first request COMPLETES well inside the 1h slack between the
            // fingerprint horizon (24h+1h from request start) and the
            // cached-record TTL (24h from completion). Without a deadline, a
            // stalled consume completing >1h after its start leaves the
            // fingerprint expired beside a still-live cached record, and a
            // different-payload replay answers 200 with a fabricated amount.
            // 60s is far inside the slack and generous for DB-bound handlers.
            // A router-wide layer would also cut off unrelated routes
            // (create_realm etc.) mid-side-effect (review 20260920).
            axum::routing::post(points::consume_points_ext).layer(
                tower_http::timeout::TimeoutLayer::with_status_code(
                    axum::http::StatusCode::REQUEST_TIMEOUT,
                    std::time::Duration::from_secs(60),
                ),
            ),
        )
        .route(
            "/points/{realmId}/grant",
            axum::routing::post(points::grant_points_ext),
        )
        .route(
            "/points/{realmId}/transactions/by-external-ref/{externalRefId}",
            axum::routing::get(points::get_transaction_by_external_ref_ext),
        )
        .route(
            "/points/{realmId}/transactions/{transactionId}",
            axum::routing::get(points::get_transaction_ext),
        )
        .route(
            "/realms",
            axum::routing::post(realm::create_realm).get(realm::list_realms),
        )
        .route("/realms/{realmId}", axum::routing::get(realm::get_realm))
        .route(
            "/realms/{realmId}/users",
            axum::routing::post(user::create_user).get(user::list_users),
        )
        .route(
            "/realms/{realmId}/users/{userId}",
            axum::routing::get(user::get_user),
        )
        .route(
            "/realms/{realmId}/client-apps",
            axum::routing::post(client_app::create_client_app).get(client_app::list_client_apps),
        )
        .route(
            "/realms/{realmId}/client-apps/{clientAppId}",
            axum::routing::get(client_app::get_client_app),
        )
        .layer(api_key_middleware)
}
