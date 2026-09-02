pub mod api_entities;
pub mod app_state;

use axum::routing::{get, post};
use axum::{
    Extension, Json, Router,
    extract::State,
    http::{
        HeaderName, Method, Request,
        header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    },
};
use opentelemetry::global;
use serde::Serialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tower::ServiceBuilder;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tower_otel_http_metrics::HTTPMetricsLayerBuilder;

use super::points::routes;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::application::http::auth::identity_middleware::{
    inject_token_identity, require_admin_console_token,
};
use crate::application::http::state::AppState;

fn origin_is_allowed(origin: &str, frontend_url: &str, rows: &[serde_json::Value]) -> bool {
    origin == frontend_url
        || rows.iter().any(|origins| {
            origins.as_array().is_some_and(|origins| {
                origins
                    .iter()
                    .any(|allowed| allowed.as_str() == Some(origin))
            })
        })
}

fn snapshot_origin_is_allowed(
    origin: &str,
    frontend_url: &str,
    realm_id: Option<&str>,
    rows: &[(String, serde_json::Value)],
) -> bool {
    rows.iter().any(|(row_realm, origins)| {
        realm_id.is_none_or(|realm| realm == row_realm)
            && origin_is_allowed(origin, frontend_url, std::slice::from_ref(origins))
    })
}

/// Extract the realm id from request paths that encode it as the first route
/// parameter. Returns `None` for realm-less routes whose realm is carried by
/// the Bearer token (or that have no realm at all): `/api/permission/check`,
/// `/api/auth` (browser-token routes), and the personal-center `/api/user/*`
/// routes. CORS runs before auth, so for these routes the realm cannot be
/// recovered from the path; the predicate then falls back to a realm-agnostic
/// scan of every enabled Client App's `allowed_origins`.
fn extract_realm_id_from_path(path: &str) -> Option<&str> {
    let parts: Vec<&str> = path.split('/').collect();
    // Realm-less top-level routes (e.g. /api/permission/check, /api/auth)
    if parts.len() < 4 {
        return None;
    }
    // /api/legal/admin/{realmId} -> realm is the 4th segment
    if parts.get(2) == Some(&"legal") && parts.get(3) == Some(&"admin") {
        return parts.get(4).copied();
    }
    // /api/permission/check is realm-less (no {realmId} segment)
    if parts.get(2) == Some(&"permission") && parts.get(3) == Some(&"check") {
        return None;
    }
    // Personal-center routes (/api/user/*) carry the realm inside the Bearer
    // token, not in the URL. Treat them as realm-less for CORS so a registered
    // Client App origin is matched across all realms.
    if parts.get(2) == Some(&"user") {
        return None;
    }
    // /api/<prefix>/{realmId}/... -> realm is the 3rd segment
    parts.get(3).copied()
}

#[cfg(test)]
mod cors_origin_tests {
    use super::{extract_realm_id_from_path, origin_is_allowed, snapshot_origin_is_allowed};

    #[test]
    fn cors_origin_maps_frontend_and_enabled_client_origin_candidates() {
        let rows = vec![serde_json::json!(["https://app.example.com"])];
        assert!(origin_is_allowed(
            "https://console.example.com",
            "https://console.example.com",
            &[]
        ));
        assert!(origin_is_allowed(
            "https://app.example.com",
            "https://console.example.com",
            &rows
        ));
        assert!(!origin_is_allowed(
            "https://evil.example.com",
            "https://console.example.com",
            &rows
        ));
    }

    #[test]
    fn cors_extracts_realm_id_from_api_paths() {
        assert_eq!(
            extract_realm_id_from_path("/api/oauth/acme/authorize"),
            Some("acme")
        );
        assert_eq!(
            extract_realm_id_from_path("/api/legal/admin/acme"),
            Some("acme")
        );
        assert_eq!(extract_realm_id_from_path("/api/permission/check"), None);
        assert_eq!(extract_realm_id_from_path("/api/auth"), None);
        // Personal-center routes carry the realm in the Bearer token, not the
        // URL, so they must be realm-less for CORS — otherwise the third path
        // segment ("profile", "change-password", …) would be misread as a
        // realm id and the registered-origin lookup would always miss.
        assert_eq!(extract_realm_id_from_path("/api/user/profile"), None);
        assert_eq!(
            extract_realm_id_from_path("/api/user/change-password"),
            None
        );
        assert_eq!(extract_realm_id_from_path("/api/user/reauth"), None);
        assert_eq!(extract_realm_id_from_path("/api/user"), None);
    }

    #[test]
    fn cors_snapshot_keeps_realm_scoping_without_per_request_queries() {
        // WHY: the process-wide snapshot removes a DB amplification path, but
        // realm-scoped routes must still ignore another tenant's origins.
        let rows = vec![
            (
                "acme".to_string(),
                serde_json::json!(["https://acme.example"]),
            ),
            (
                "other".to_string(),
                serde_json::json!(["https://other.example"]),
            ),
        ];
        assert!(snapshot_origin_is_allowed(
            "https://acme.example",
            "https://console.example",
            Some("acme"),
            &rows,
        ));
        assert!(!snapshot_origin_is_allowed(
            "https://other.example",
            "https://console.example",
            Some("acme"),
            &rows,
        ));
        assert!(snapshot_origin_is_allowed(
            "https://other.example",
            "https://console.example",
            None,
            &rows,
        ));
    }
}
use crate::application::http::{
    admin, api_keys, audit, auth, billing, client_apps, dashboard, legal, oauth, permission,
    points, public_config, realm, realm_config, user, users,
};

/// Health check response schema
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct HealthCheckResponse {
    /// Overall service status: "healthy" or "unhealthy"
    pub status: String,
    /// PostgreSQL connection status
    pub database: bool,
    /// Redis connection status
    pub redis: bool,
    /// Service version from Cargo.toml
    pub version: String,
    /// Service uptime in seconds since startup
    pub uptime: u64,
    /// Current timestamp in RFC3339 format
    pub timestamp: String,
}

/// Local OpenAPI spec for modules remaining in the api crate
/// Sub-crate specs are merged at runtime via build_openapi_spec()
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Herald API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Herald Authentication & Authorization Platform API",
        license(name = "MIT")
    ),
    paths(
        realm_config::list_realm_configs,
        realm_config::list_realm_configs_by_type,
        realm_config::get_realm_config,
        realm_config::upsert_realm_config,
        realm_config::batch_upsert_realm_configs,
        realm_config::delete_realm_config,
        realm_config::email_status,
        realm_config::email_test,
        realm::crud::list_realms,
        realm::crud::list_realms_paginated,
        realm::crud::get_realm,
        realm::crud::create_realm,
        realm::crud::update_realm,
        user::permissions::get_user_permissions,
        user::roles::get_user_roles,
        user::get_profile,
        user::update_profile,
        user::change_password,
        user::delete_account::delete_account,
        realm::totp_config::handle_update_realm_totp_config,
        realm::totp_config::handle_get_realm_totp_config,
        realm::passkey_config::handle_update_realm_passkey_config,
        realm::passkey_config::handle_get_realm_passkey_config,
        realm::email_otp_config::handle_update_realm_email_otp_config,
        realm::email_otp_config::handle_get_realm_email_otp_config,
        realm::white_label_config::handle_get_white_label_config,
        realm::white_label_config::handle_save_white_label_draft,
        realm::white_label_config::handle_discard_white_label_draft,
        realm::white_label_config::handle_publish_white_label_config,
        realm::white_label_config::handle_restore_white_label_config,
        realm::custom_domain_config::handle_get_custom_domain_config,
        realm::custom_domain_config::handle_update_custom_domain_config,
        realm::custom_domain_config::handle_custom_domain_authorize,
        public_config::get_public_config,
        public_config::resolve_custom_domain,
        legal::list_agreements,
        legal::get_agreement,
        legal::get_consent_status,
        legal::record_consent,
        legal::admin_list_agreements,
        legal::admin_get_version,
        legal::admin_publish_custom,
        legal::admin_revert_to_default,
        legal::admin_get_draft,
        legal::admin_save_draft,
        legal::admin_publish_from_draft,
        legal::admin_discard_draft,
        health_check,
    ),
    components(
        schemas(
            api_entities::ErrorResponse,
            api_entities::PageResponse<points::types::PointsWalletResponse>,
            api_entities::PageResponse<points::types::PointsTransactionResponse>,
            api_entities::PageResponse<realm::RealmResponse>,
            user::profile::ChangePasswordRequest,
            user::profile::UserProfile,
            user::profile::UpdateProfileRequest,
            user::delete_account::DeleteAccountRequest,
            user::permissions::UserPermissionsResponse,
            user::roles::UserProfileRolesResponse,
            realm_config::UpsertRealmConfigValidator,
            realm_config::BatchUpsertRealmConfigValidator,
            realm_config::RealmConfigResponse,
            realm_config::EmailStatusResponse,
            realm_config::EmailTestRequest,
            realm_config::EmailTestResponse,
            realm::ListRealmsQuery,
            realm::ListRealmsResponse,
            realm::ListRealmsPaginatedQuery,
            realm::CreateRealmValidator,
            realm::UpdateRealmValidator,
            realm::RealmResponse,
            herald_api_base::application::http::common::pagination::PaginationMeta,
            realm::totp_config::UpdateRealmTotpConfigRequest,
            realm::totp_config::UpdateRealmTotpConfigResponse,
            realm::totp_config::GetRealmTotpConfigResponse,
            realm::totp_config::RealmTotpStatisticsResponse,
            realm::passkey_config::UpdateRealmPasskeyConfigRequest,
            realm::passkey_config::UpdateRealmPasskeyConfigResponse,
            realm::passkey_config::GetRealmPasskeyConfigResponse,
            realm::email_otp_config::UpdateRealmEmailOtpConfigRequest,
            realm::email_otp_config::UpdateRealmEmailOtpConfigResponse,
            realm::email_otp_config::GetRealmEmailOtpConfigResponse,
            realm::white_label_config::WhiteLabelBackground,
            realm::white_label_config::WhiteLabelBackgroundType,
            realm::white_label_config::WhiteLabelConfig,
            realm::white_label_config::UpdateWhiteLabelConfigRequest,
            realm::white_label_config::WhiteLabelConfigStateResponse,
            realm::white_label_config::SaveWhiteLabelDraftResponse,
            realm::white_label_config::WhiteLabelLifecycleResponse,
            realm::custom_domain_config::CustomDomainConfigStateResponse,
            realm::custom_domain_config::UpdateCustomDomainConfigRequest,
            realm::custom_domain_config::CustomDomainUpdateResponse,
            realm::custom_domain_config::CustomDomainAuthorizeResponse,
            realm::custom_domain_config::CustomDomainHostQuery,
            herald_core::domain::realm_config::CustomDomainConfig,
            herald_core::domain::realm_config::CustomDomainStatus,
            public_config::PublicConfigResponse,
            public_config::ResolveCustomDomainQuery,
            public_config::ResolveCustomDomainResponse,
            public_config::PublicWhiteLabelConfig,
            public_config::RegistrationConfig,
            public_config::OAuthProviderInfo,
            legal::LegalAgreementDetail,
            legal::AgreementsResponse,
            legal::ConsentStatusResponse,
            legal::RecordConsentRequest,
            legal::RecordConsentItem,
            legal::LegalAgreementVersionSummary,
            legal::LegalAgreementVersionDetailResponse,
            legal::AdminAgreementView,
            legal::AdminAgreementsResponse,
            legal::PublishCustomRequest,
            legal::PublishVersionResponse,
            legal::LegalAgreementDraftResponse,
            legal::SaveDraftRequest,
            legal::PublishFromDraftRequest,
            herald_core::domain::legal::entities::AgreementType,
            herald_core::domain::legal::entities::AgreementSource,
            HealthCheckResponse,
        )
    ),
    tags(
        (name = "auth", description = "Authentication & authorization APIs"),
        (name = "user", description = "User personal center APIs"),
        (name = "users", description = "User management APIs (admin)"),
        (name = "oauth", description = "OAuth provider authentication APIs"),
        (name = "realm_config", description = "Realm configuration management APIs"),
        (name = "realms", description = "Realm management APIs"),
        (name = "api-keys", description = "Realm API key management APIs"),
        (name = "client", description = "OAuth client application management APIs"),
        (name = "permission", description = "Permission policy and role assignment APIs"),
        (name = "permission-definitions", description = "Permission definition management APIs"),
        (name = "role-definitions", description = "Role definition management APIs"),
        (name = "billing", description = "Billing and subscription management APIs"),
        (name = "billing.payment-providers", description = "Payment provider configuration APIs"),
        (name = "billing-invoice", description = "Invoice and credit note management APIs"),
        (name = "points", description = "Points and virtual currency APIs"),
        (name = "InternalPoints", description = "Internal quota entitlement APIs (service-to-service, demo/test only)"),
        (name = "ext", description = "External API (API Key authentication)"),
        (name = "system", description = "System health and monitoring APIs"),
        (name = "audit", description = "Audit log query APIs"),
        (name = "dashboard", description = "Dashboard statistics APIs"),
        (name = "device", description = "OAuth Device Authorization Grant APIs"),
        (name = "legal", description = "Legal agreements and user consent APIs")
    )
)]
pub struct ApiDoc;

/// Build the complete OpenAPI spec by merging local paths with sub-crate specs
pub fn build_openapi_spec() -> utoipa::openapi::OpenApi {
    let mut spec = ApiDoc::openapi()
        .merge_from(herald_api_auth::ApiDoc::openapi())
        .merge_from(herald_api_admin::ApiDoc::openapi())
        .merge_from(herald_api_billing::ApiDoc::openapi())
        .merge_from(herald_api_oauth::ApiDoc::openapi())
        .merge_from(herald_api_points::ApiDoc::openapi())
        .merge_from(herald_api_ext::ApiDoc::openapi());

    // Operations annotate themselves with `security(("bearer_auth"|"api_key" = []))`,
    // but utoipa's `#[openapi(components(...))]` only registers schemas/responses — not security
    // schemes. Without matching `components.securitySchemes`, the generated spec references scheme
    // ids that never resolve, and consumers like the fumadocs-openapi playground crash reading
    // `.deprecated` on the undefined scheme. Register them here, against the real mechanisms:
    // `bearer_auth` reads the Authorization header; `api_key` reads X-API-Key.
    let components = spec.components.get_or_insert_with(Default::default);
    components.security_schemes.insert(
        "bearer_auth".to_string(),
        utoipa::openapi::security::SecurityScheme::Http(utoipa::openapi::security::Http::new(
            utoipa::openapi::security::HttpAuthScheme::Bearer,
        )),
    );
    components.security_schemes.insert(
        "api_key".to_string(),
        utoipa::openapi::security::SecurityScheme::ApiKey(
            utoipa::openapi::security::ApiKey::Header(
                utoipa::openapi::security::ApiKeyValue::with_description(
                    "X-API-Key",
                    "Client-app API key (third-party external API).",
                ),
            ),
        ),
    );

    spec
}

pub fn create_router(
    state: Arc<AppState>,
    frontend_url: String,
    static_dir: Option<String>,
    real_ip_config: herald_api_base::application::http::real_ip::RealIpConfig,
) -> Router {
    // Build CORS layer
    // Note: frontend_url is validated in main.rs before calling this function
    let cors_state = state.clone();
    let cors_frontend_url = frontend_url.clone();
    // CORS is not an authorization boundary, so a short-lived process-local
    // snapshot is preferable to querying Client Apps for every untrusted
    // request carrying an Origin header. One snapshot contains every realm;
    // attacker-controlled fake realm paths therefore cannot create unbounded
    // cache keys or force one database query per distinct path.
    let cors_origins = Arc::new(tokio::sync::Mutex::new(
        None::<(Instant, Vec<(String, serde_json::Value)>)>,
    ));
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::async_predicate(move |origin, parts| {
            let state = cors_state.clone();
            let frontend_url = cors_frontend_url.clone();
            let cors_origins = cors_origins.clone();
            // Extract the realm id before the async block so we do not borrow `parts`.
            let realm_id = extract_realm_id_from_path(parts.uri.path()).map(String::from);
            async move {
                let Ok(origin) = origin.to_str() else {
                    return false;
                };
                if origin == frontend_url {
                    return true;
                }
                let mut snapshot = cors_origins.lock().await;
                let expired = snapshot
                    .as_ref()
                    .is_none_or(|(loaded_at, _)| loaded_at.elapsed() >= Duration::from_secs(30));
                if expired {
                    match sqlx::query_as::<_, (String, serde_json::Value)>(
                        "SELECT realm_id, allowed_origins FROM client_app WHERE enabled = true",
                    )
                    .fetch_all(&state.pool)
                    .await
                    {
                        Ok(rows) => *snapshot = Some((Instant::now(), rows)),
                        Err(error) => {
                            tracing::warn!(%error, "Dynamic CORS origin snapshot refresh failed");
                            return false;
                        }
                    }
                }
                let rows = &snapshot.as_ref().expect("snapshot populated above").1;
                snapshot_origin_is_allowed(origin, &frontend_url, realm_id.as_deref(), rows)
            }
        }))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            ACCEPT,
            HeaderName::from_static("x-request-id"),
        ])
        .allow_credentials(false)
        .expose_headers([HeaderName::from_static("x-request-id")]);

    // All API routes (OAuth, Realm Config, Auth, Permission, Client, Roles, User, Users, Realms, Billing)
    let api_routes = create_api_routes(state.clone());

    // Define the request ID header name
    let request_id_header_name = HeaderName::from_static("x-request-id");

    // Build the RED metrics layer.
    //
    // The meter comes from the global meter provider set up in
    // `crate::observability::build_meter_provider` (metrics always on).
    // Under `feature="axum"` the library reads the axum `MatchedPath` route
    // template from the request extensions and emits `http.route` itself.
    // `RedAttributeExtractor` adds only the `error.type` label on 5xx; see
    // `crate::observability::metrics_extractor` for the governance argument.
    let red_extractor = crate::observability::metrics_extractor::RedAttributeExtractor::new();
    let red_layer = HTTPMetricsLayerBuilder::builder()
        .with_meter(global::meter("herald-api"))
        .with_request_extractor::<_, axum::body::Body>(red_extractor.clone())
        .with_response_extractor::<_, axum::body::Body>(red_extractor)
        .build()
        // The builder only returns Err when no meter is set, and we just set one.
        // There is no runtime exporter state to fail (exporter is owned by the
        // global meter provider, built once).
        .expect("RED metrics layer build: meter was just provided");

    // Merge all stateful routers and convert to stateless by calling with_state
    // api_routes is Router<AppState>, needs with_state
    // health_route is Router<AppState>, needs with_state
    let router = Router::new()
        .merge(api_routes.with_state((*state).clone()))
        .route("/health", get(health_check).with_state(state.clone()))
        .merge(SwaggerUi::new("/swagger").url("/api-docs/openapi.json", build_openapi_spec()))
        .layer(
            ServiceBuilder::new()
                // 1. If request doesn't have X-Request-ID, generate a new UUID
                .layer(SetRequestIdLayer::new(
                    request_id_header_name.clone(),
                    MakeRequestUuid,
                ))
                // Make the generated id available to structured API error bodies.
                .layer(axum::middleware::from_fn(
                    herald_api_base::application::http::request_context::bind_request_id,
                ))
                // 2. Propagate X-Request-ID to downstream services (if any)
                .layer(PropagateRequestIdLayer::new(request_id_header_name.clone()))
                // 3. RED metrics (mount order: request-id -> RED -> trace -> cors).
                //    Placed before TraceLayer so the library's `http.route` is recorded
                //    on the same request the TraceLayer span describes.
                .layer(red_layer)
                // 4. HTTP request tracing with `request_id` span field.
                //    The span is created at request start; `MatchedPath` may not yet be
                //    populated, so for the span's path field we use the route template if
                //    present, else the fixed `"UNMATCHED"` sentinel — never the raw path
                //    (governance).
                .layer(
                    TraceLayer::new_for_http().make_span_with(move |request: &Request<_>| {
                        let request_id = request
                            .headers()
                            .get(&request_id_header_name)
                            .and_then(|v| v.to_str().ok())
                            .filter(|s| !s.is_empty())
                            .unwrap_or("-")
                            .to_owned();
                        let method = request.method().as_str();
                        let route = request
                            .extensions()
                            .get::<axum::extract::MatchedPath>()
                            .map(|m| m.as_str().to_owned())
                            .unwrap_or_else(|| "UNMATCHED".to_owned());
                        tracing::info_span!(
                            "http.request",
                            method = method,
                            // Route TEMPLATE (or UNMATCHED), never the raw path / params.
                            http.route = %route,
                            // Low-cardinality ops correlation key. Always present
                            // so every request log line carries it regardless of traces on/off.
                            request_id = %request_id,
                        )
                    }),
                )
                .layer(cors),
        )
        // Real-IP config consumed by the `ClientIp` extractor. Applied last so it
        // is present for every route (health, swagger, static fallback included).
        .layer(Extension(real_ip_config));

    // println!("{router:?}");
    if let Some(dir) = static_dir {
        tracing::info!("Serving static files from: {}", dir);
        let index_html = format!("{}/index.html", dir.trim_end_matches('/'));
        router.fallback_service(ServeDir::new(&dir).fallback(ServeFile::new(index_html)))
    } else {
        router
    }
}

/// Create API routes for both production and testing
///
/// This function extracts the core API routing logic so it can be reused
/// in both production (create_router) and test (create_unified_test_router) contexts.
/// This eliminates code duplication and ensures route consistency.
///
/// # Arguments
///
/// * `state` - Application state shared across all routes
///
/// # Returns
///
/// A configured Router with all API routes (no middleware layers)
pub fn create_api_routes(state: Arc<AppState>) -> Router<AppState> {
    use axum::middleware::from_fn_with_state;

    let auth_routes = auth::auth_router();
    let admin_routes = admin::admin_router_with_middleware((*state).clone());
    let realm_routes = realm::realm_router();
    let billing_routes = billing::billing_routes();
    let billing_browser_routes = billing::billing_browser_routes();
    let audit_routes = audit::audit_router();

    // Test routes - `billing_test_routes()` currently returns an empty router;
    // kept as the wiring point for any future test-only billing routes.
    let billing_test_routes = billing::billing_test_routes();

    let router = Router::new()
        // Public configuration endpoint (no authentication required) - must come before other nested routes
        .route(
            "/api/public-config/custom-domain/resolve",
            get(super::public_config::resolve_custom_domain),
        )
        .route(
            "/api/public-config/{realmId}",
            get(super::public_config::get_public_config),
        )
        // Internal Caddy On-Demand TLS ask authorization endpoint.
        // Top-level (NOT under /api/realms → no
        // Bearer middleware). Uses the X-Herald-Ask-Key shared secret checked
        // in-handler. The public host→realmId resolve endpoint remains
        // separate: the SPA needs it before a custom-domain visitor has a
        // session, while this endpoint must never disclose realm identity.
        .route(
            "/api/internal/custom-domain/authorize",
            get(realm::custom_domain_config::handle_custom_domain_authorize),
        )
        // Public legal agreement endpoints (no Bearer identity).
        // Grouped separately from the consent nest below so the Bearer middleware
        // layer never covers the public agreements routes.
        .route(
            "/api/legal/{realmId}/agreements",
            get(legal::list_agreements),
        )
        .route(
            "/api/legal/{realmId}/agreements/{agreementType}",
            get(legal::get_agreement),
        )
        // OAuth routes
        .route(
            "/api/oauth/{realmId}/authorize",
            get(oauth::oauth_authorize),
        )
        .route("/api/oauth/{realmId}/token", post(oauth::oauth_token))
        .route(
            "/api/oauth/{realmId}/{provider}/login",
            get(oauth::oauth_login),
        )
        .route(
            "/api/oauth/{realmId}/{provider}/callback",
            get(oauth::oauth_callback),
        )
        // WeChat specific routes
        .route(
            "/api/oauth/{realmId}/wechat/login",
            get(oauth::wechat_login),
        )
        .route(
            "/api/oauth/{realmId}/wechat/callback",
            get(oauth::wechat_callback),
        )
        .route(
            "/api/oauth/{realmId}/wechat-miniprogram/login",
            post(oauth::wechat_miniprogram_login),
        )
        // Google One Tap (GIS ID Token) login — no redirect, direct POST.
        .route(
            "/api/oauth/{realmId}/google/one-tap",
            post(oauth::google_one_tap),
        )
        // Apple native (Sign in with Apple) login — no redirect, direct POST.
        // The iOS app obtains the identityToken via ASAuthorizationAppleIDProvider
        // and submits it here for server-side verification.
        .route(
            "/api/oauth/{realmId}/apple/native-login",
            post(oauth::apple_native_login),
        )
        // Device code authorization
        .route(
            "/api/device/{realmId}/authorize",
            post(oauth::device_authorize),
        )
        .route("/api/device/{realmId}/token", post(oauth::device_token))
        .nest(
            "/api/device/{realmId}",
            Router::new()
                .route("/verify", post(oauth::device_verify))
                .route("/confirm", post(oauth::device_confirm))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        .nest(
            "/api/oauth/{realmId}/configs",
            Router::new()
                .route(
                    "/",
                    get(oauth::list_oauth_configs).post(oauth::create_oauth_config),
                )
                .route(
                    "/{providerType}",
                    get(oauth::get_oauth_config)
                        .put(oauth::update_oauth_config)
                        .delete(oauth::delete_oauth_config),
                )
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        // Realm Config routes
        .nest(
            "/api/configs",
            Router::new()
                .route(
                    "/{realmId}",
                    get(realm_config::list_realm_configs).put(realm_config::upsert_realm_config),
                )
                .route(
                    "/{realmId}/batch",
                    post(realm_config::batch_upsert_realm_configs),
                )
                // Email status and test routes (must be before parameterized {configType} routes)
                .route("/{realmId}/email/status", get(realm_config::email_status))
                .route("/{realmId}/email/test", post(realm_config::email_test))
                .route(
                    "/{realmId}/{configType}",
                    get(realm_config::list_realm_configs_by_type),
                )
                .route(
                    "/{realmId}/{configType}/{configKey}",
                    get(realm_config::get_realm_config).delete(realm_config::delete_realm_config),
                )
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        // Auth routes
        .nest("/api/auth/{realmId}", auth_routes)
        .nest("/api/auth", herald_api_auth::browser_token_router())
        .nest(
            "/api/auth",
            herald_api_auth::token_router()
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        // Permission routes: /check endpoint (authenticated self-introspection;
        // the handler additionally requires the probed token to belong to the
        // caller) + others (WITH middleware)
        .route(
            "/api/permission/check",
            axum::routing::post(crate::application::http::admin::permission::check_permission)
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        .nest(
            "/api/permission",
            permission::permission_router()
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        .nest(
            "/api/client/{realmId}",
            client_apps::router()
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        .nest(
            "/api/api-keys/{realmId}",
            api_keys::router()
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        .nest("/api/roles", admin_routes)
        // Personal center routes (tag = "user") - no realmId in prefix
        .nest(
            "/api/user",
            user::router()
                .merge(users::router())
                .merge(herald_api_points::routes::user_points_router())
                .merge(herald_api_billing::routes::billing_user_routes())
                .merge(herald_api_auth::user_passkey::router())
                .merge(herald_api_auth::reauth_router())
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        // Admin user management (tag = "users") - realm_id required
        .nest(
            "/api/users/{realmId}",
            admin::admin_users::router()
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        .nest(
            "/api/realms",
            realm_routes
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        // Self-service consent endpoints (WITH bearer identity).
        // Distinct prefix from the public agreements routes above so the
        // identity layer only covers consent, not the public agreement reads.
        .nest(
            "/api/legal/{realmId}/consent",
            Router::new()
                .route("/status", get(legal::get_consent_status))
                .route("/", post(legal::record_consent))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        // Admin legal agreement management (WITH first-party bearer identity). Distinct
        // `/admin` prefix keeps the public agreements routes above unguarded.
        // Permission enforcement (settings.view / settings.manage) happens
        // inside each handler via `require_permission`.
        .nest(
            "/api/legal/admin/{realmId}",
            Router::new()
                .route("/agreements", get(legal::admin_list_agreements))
                .route(
                    "/agreements/versions/{versionId}",
                    get(legal::admin_get_version),
                )
                .route(
                    "/agreements/{agreementType}",
                    axum::routing::put(legal::admin_publish_custom),
                )
                .route(
                    "/agreements/{agreementType}/custom",
                    axum::routing::delete(legal::admin_revert_to_default),
                )
                // Draft lifecycle: save/get/discard a staged draft, and publish
                // from it. The admin UI publishes only through this path (no
                // "publish without a draft" entry in the UI).
                .route(
                    "/agreements/{agreementType}/draft",
                    get(legal::admin_get_draft)
                        .put(legal::admin_save_draft)
                        .delete(legal::admin_discard_draft),
                )
                .route(
                    "/agreements/{agreementType}/publish",
                    axum::routing::post(legal::admin_publish_from_draft),
                )
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        // Audit log query routes
        .nest(
            "/api/audit/{realmId}",
            audit_routes
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        // Dashboard statistics routes
        .nest(
            "/api/dashboard/{realmId}",
            dashboard::dashboard_router()
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        .merge(billing::billing_public_routes())
        .merge(routes::internal_public_routes())
        .merge(
            billing_browser_routes
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        .merge(
            billing_routes
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state((*state).clone(), inject_token_identity)),
        )
        // Points admin endpoints - the flexible auth layer authenticates
        // Bearer or API-key credentials, then the same admin-console
        // credential gate as every other admin router rejects API-key and
        // CustomUserUi identities: only first-party admin-console Bearer
        // tokens reach these handlers. Third-party API keys use
        // /api/ext/points/*.
        .nest(
            "/api/points/{realmId}",
            routes::points_router()
                .layer(axum::middleware::from_fn(require_admin_console_token))
                .layer(from_fn_with_state(
                    (*state).clone(),
                    crate::application::http::points::auth_middleware::flexible_auth_middleware,
                )),
        )
        // External API routes
        .nest("/api/ext", super::ext::create_router((*state).clone()))
        // MCP protocol endpoint. Top-level path by design: a protocol
        // surface, not a REST resource — no OpenAPI, no admin-console token
        // gate (auth is the crate's own API-key middleware). Mounted here
        // (inside create_api_routes) so request-id / metrics / trace / CORS
        // still apply.
        .nest("/mcp", super::mcp::create_mcp_router((*state).clone()));

    router.merge(billing_test_routes)
}

/// Health check endpoint for monitoring and orchestration
///
/// Used by:
/// - Kubernetes liveness probes (is the service running?)
/// - Kubernetes readiness probes (can the service handle traffic?)
/// - Monitoring systems (Prometheus, Datadog, etc.)
/// - Load balancers (health checks)
///
/// # Health Criteria
///
/// The service is considered **healthy** when:
/// - PostgreSQL database is reachable (`SELECT 1` succeeds)
/// - Redis cache is reachable (PING succeeds)
///
/// # Response Codes
///
/// - **200 OK**: Service is healthy
/// - **503 Service Unavailable**: Service is unhealthy
#[utoipa::path(
    get,
    path = "/health",
    tag = "system",
    responses(
        (status = 200, description = "Service is healthy", body = HealthCheckResponse),
        (status = 503, description = "Service is unhealthy", body = HealthCheckResponse)
    )
)]
async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthCheckResponse> {
    // Check database connection
    let db_healthy = sqlx::query("SELECT 1").fetch_one(&state.pool).await.is_ok();

    // Check Redis connection
    let redis_healthy = state.redis_manager.health_check().await.is_ok();

    let status = if db_healthy && redis_healthy {
        "healthy"
    } else {
        "unhealthy"
    };

    // Calculate uptime in seconds
    let uptime = state.startup_time.elapsed().as_secs();

    // Get version from env var (set by Cargo during build)
    let version = env!("CARGO_PKG_VERSION");

    // Get current timestamp
    let timestamp = chrono::Utc::now().to_rfc3339();

    Json(HealthCheckResponse {
        status: status.to_string(),
        database: db_healthy,
        redis: redis_healthy,
        version: version.to_string(),
        uptime,
        timestamp,
    })
}
