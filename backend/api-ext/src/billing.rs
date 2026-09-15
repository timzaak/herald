// Billing API for Third-Party Integration
//
// Allows third-party apps to query billing information using API Key authentication.

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use herald_core::domain::authentication::Identity;
use herald_core::domain::billing::BillingRepository;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::authz::require_principal_permission;
use crate::client_app_scope::ensure_client_app_scope;
use crate::client_helper::ClientAppLookup;
use herald_api_base::application::http::common::error_codes::ErrorCode;
use herald_api_base::application::http::common::error_helpers::json_error;
use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::state::AppState;

/// Subscription detail response (SDK-compatible)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionDetail {
    pub id: String,
    pub client_app_id: Option<String>,
    pub status: String,
    pub entitlement_key: String,
    pub payment_provider: String,
    /// Provider price id bound to this subscription.
    /// `None` for price-less providers (Creem) or when the subscription has no
    /// bound price yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_price_id: Option<String>,
    /// Billing type snapshot (`"recurring"` / `"non_renewing"`). Lets
    /// third-party SDKs distinguish non-renewing subscriptions from recurring
    pub billing_type: String,
    /// Currency of the subscribed price row, resolved from the realm's
    /// entitlement mapping (`provider_product_info.currency`). `None` when no
    /// mapping row matches the subscription's provider/price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub current_period_start: Option<String>,
    pub current_period_end: Option<String>,
    pub cancel_at: Option<String>,
    pub cancel_at_period_end: Option<bool>,
    pub created_at: String,
    pub updated_at: String,
}

/// Get subscription for a client app
///
/// Returns subscription details for the specified client app.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Realm Isolation
/// The API key must belong to the same realm as the client app.
/// Cross-realm requests will return 403 Forbidden.
///
/// # Example
/// ```bash
/// curl -X GET \
///   https://api.example.com/api/ext/bill/realm123/client/client-app-123/subscription \
///   -H "X-API-Key: your-api-key"
/// ```
#[utoipa::path(
    get,
    path = "/api/ext/bill/{realmId}/client/{clientAppId}/subscription",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("clientAppId" = String, Path, description = "Client app ID (client_id or UUID)")
    ),
    responses(
        (status = 200, description = "Subscription details retrieved", body = SubscriptionDetail),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access attempt", body = ErrorResponse),
        (status = 404, description = "Client app not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn get_subscription(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(params): Path<(String, String)>,
) -> Response {
    let api_key_realm_id = identity.realm_id();
    let (realm_id, client_app_id) = params;
    let client_app_id = client_app_id.clone();

    tracing::info!(
        api_key_realm_id = %api_key_realm_id,
        request_realm_id = %realm_id,
        client_app_id = %client_app_id,
        "Subscription detail query requested"
    );

    // 1. Check realm isolation
    if api_key_realm_id != realm_id {
        tracing::warn!(
            api_key_realm_id = %api_key_realm_id,
            request_realm_id = %realm_id,
            "Cross-realm access attempt blocked"
        );
        return json_error(StatusCode::FORBIDDEN, ErrorCode::CrossRealmAccessForbidden);
    }

    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "billing", "view").await
    {
        return resp.into_response();
    }

    // 2. Find client app by identifier to get the UUID
    let client_app_lookup = ClientAppLookup::new(state.pool.clone());
    let client_app_uuid: Uuid = match client_app_lookup
        .find_uuid_by_identifier_required(&client_app_id, &realm_id)
        .await
    {
        Ok(uuid) => uuid,
        Err(e) => return e,
    };

    if let Err(resp) = ensure_client_app_scope(&state, &identity, client_app_uuid).await {
        return resp;
    }

    // 3. Query subscription
    let subscription = match state
        .billing_repository
        .find_subscription_by_client_app_id(client_app_uuid)
        .await
    {
        Ok(Some(sub)) => sub,
        Ok(None) => {
            tracing::info!(
                client_app_id = %client_app_id,
                "No subscription found for client app"
            );
            return json_error(StatusCode::NOT_FOUND, ErrorCode::SubscriptionNotFound);
        }
        Err(e) => {
            tracing::error!("Failed to query subscription: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError);
        }
    };

    // 4. Resolve the subscribed price row's currency from the realm's
    //    entitlement mappings. A lookup failure degrades to `None` (currency
    //    is additive metadata here); the subscription itself was found.
    let currency = match state
        .billing_repository
        .find_entitlement_mapping_by_provider_product_price(
            &realm_id,
            &subscription.payment_provider,
            &subscription.external_product_id,
            subscription.external_price_id.as_deref(),
        )
        .await
    {
        Ok(Some(mapping)) => {
            herald_core::domain::billing::mapping_currency(&mapping).map(|s| s.to_string())
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to resolve subscription currency from entitlement mappings"
            );
            None
        }
    };

    // 5. Build response
    let response = SubscriptionDetail {
        id: subscription.id.to_string(),
        client_app_id: Some(client_app_id.clone()),
        status: subscription.status.as_str().to_string(),
        entitlement_key: subscription.entitlement_key,
        payment_provider: subscription.payment_provider,
        external_price_id: subscription.external_price_id,
        billing_type: subscription.billing_type.as_str().to_string(),
        currency,
        current_period_start: subscription.current_period_start.map(|dt| dt.to_rfc3339()),
        current_period_end: subscription.current_period_end.map(|dt| dt.to_rfc3339()),
        cancel_at: subscription.cancel_at.map(|dt| dt.to_rfc3339()),
        cancel_at_period_end: Some(subscription.cancel_at_period_end),
        created_at: subscription.created_at.to_rfc3339(),
        updated_at: subscription.updated_at.to_rfc3339(),
    };

    tracing::info!(
        client_app_id = %client_app_id,
        subscription_id = %subscription.id,
        status = %response.status,
        "Subscription retrieved successfully"
    );

    Json(response).into_response()
}

// Entitlement mappings are managed via the admin billing API.

/// Single one-time mapping item for external API
///
/// Named `ExtOneTimeMappingItem` (not `OneTimeMappingItem`) because the
/// admin billing API registers a same-named schema in the merged OpenAPI
/// spec; sharing the name shadowed this shape and hid its fields (e.g.
/// `currency`) from generated SDK clients.
///
/// The single-target fields of the pre-rule model (`bucket_id`,
/// `points_per_period`, `validity_days`) are gone: under the multi-wallet
/// distribution-rule model a mapping fans out to 0..N buckets and the grant
/// amounts live on the rules, so no per-mapping scalar can represent them
/// (the view used to placeholder them with a nil UUID / `None`, which read
/// like real values). Rule-based surfacing belongs to the payment-attempt /
/// external-API migration item.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtOneTimeMappingItem {
    pub id: String,
    pub entitlement_key: String,
    pub provider_product_info: Option<serde_json::Value>,
    /// Currency hoisted out of `provider_product_info` for direct SDK
    /// consumption; `None` when the product info carries no currency.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub payment_provider: String,
}

/// Response for one-time mappings external endpoint
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OneTimeMappingExtResponse {
    pub items: Vec<ExtOneTimeMappingItem>,
}

/// Get purchasable one-time entitlement mappings
///
/// Returns enabled one-time entitlement mappings that have provider product info configured.
/// Used by frontend/SDK to display purchasable products.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Realm Isolation
/// The API key must belong to the same realm as the requested realm.
/// Cross-realm requests will return 403 Forbidden.
#[utoipa::path(
    get,
    path = "/api/ext/{realmId}/one-time-mappings",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "One-time mappings retrieved", body = OneTimeMappingExtResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access attempt", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn get_one_time_mappings(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
) -> Response {
    let api_key_realm_id = identity.realm_id();

    tracing::info!(
        api_key_realm_id = %api_key_realm_id,
        request_realm_id = %realm_id,
        "One-time mappings query requested"
    );

    // 1. Check realm isolation
    if !identity.has_access_to_realm(&realm_id) {
        tracing::warn!(
            api_key_realm_id = %api_key_realm_id,
            request_realm_id = %realm_id,
            "Cross-realm access attempt blocked"
        );
        return json_error(StatusCode::FORBIDDEN, ErrorCode::CrossRealmAccessForbidden);
    }

    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "billing", "view").await
    {
        return resp.into_response();
    }

    // 2. Query one-time mappings
    let mappings = match state
        .billing_repository
        .list_one_time_mappings(&realm_id)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("Failed to query one-time mappings: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError);
        }
    };

    // 3. Build response
    let items: Vec<ExtOneTimeMappingItem> = mappings
        .into_iter()
        .map(|m| {
            let currency =
                herald_core::domain::billing::mapping_currency(&m).map(|s| s.to_string());
            ExtOneTimeMappingItem {
                id: m.id.to_string(),
                entitlement_key: m.entitlement_key,
                provider_product_info: m.provider_product_info,
                currency,
                payment_provider: m.payment_provider,
            }
        })
        .collect();

    tracing::info!(
        realm_id = %realm_id,
        count = items.len(),
        "One-time mappings retrieved"
    );

    Json(OneTimeMappingExtResponse { items }).into_response()
}

/// Response for the entitlement currency-set endpoint
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementCurrenciesResponse {
    pub entitlement_key: String,
    /// Deduplicated currencies covered by the entitlement's enabled Stripe
    /// mapping rows. Empty when the entitlement has no priced rows.
    pub currencies: Vec<String>,
}

/// Resolved default price row, usable as an explicit `target_id` purchase
/// target by third-party callers.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementPriceView {
    pub mapping_id: String,
    pub entitlement_key: String,
    pub payment_provider: String,
    pub currency: String,
    /// Price amount in minor units (cents).
    pub amount: i64,
    pub billing_type: Option<String>,
    pub billing_period: Option<String>,
    pub external_price_id: Option<String>,
}

/// Query parameters for the by-currency default-price resolution
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(rename_all = "camelCase", parameter_in = Query)]
pub struct ResolveDefaultPriceParams {
    /// ISO 4217 currency code (e.g. "USD"); reserved codes rejected
    pub currency: String,
    /// Optional narrowing filter: `recurring` | `one_time` | `non_renewing`
    pub billing_type: Option<String>,
    /// Optional narrowing filter: billing period (e.g. `month` / `year`)
    pub billing_period: Option<String>,
}

/// Get the currency set supported by a purchasable entitlement
///
/// Returns the deduplicated currencies covered by the entitlement's enabled
/// Stripe mapping rows (subscription-type and one-time-type rows both count).
/// Creem/IAP rows are excluded: their pricing is provider/store-side and
/// carries no per-currency price rows.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Realm Isolation
/// The API key must belong to the same realm as the requested realm.
#[utoipa::path(
    get,
    path = "/api/ext/{realmId}/entitlements/{entitlementKey}/currencies",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("entitlementKey" = String, Path, description = "Entitlement key")
    ),
    responses(
        (status = 200, description = "Supported currencies retrieved", body = EntitlementCurrenciesResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access attempt", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn get_entitlement_currencies(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, entitlement_key)): Path<(String, String)>,
) -> Response {
    if !identity.has_access_to_realm(&realm_id) {
        tracing::warn!(
            api_key_realm_id = %identity.realm_id(),
            request_realm_id = %realm_id,
            "Cross-realm access attempt blocked"
        );
        return json_error(StatusCode::FORBIDDEN, ErrorCode::CrossRealmAccessForbidden);
    }

    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "billing", "view").await
    {
        return resp.into_response();
    }

    let mappings = match state
        .billing_repository
        .find_enabled_stripe_mappings_by_entitlement(&realm_id, &entitlement_key)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(error = %e, "Failed to query entitlement mappings");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError);
        }
    };

    Json(EntitlementCurrenciesResponse {
        entitlement_key,
        currencies: herald_core::domain::billing::collect_currencies(&mappings),
    })
    .into_response()
}

/// Resolve an entitlement's default price row by currency
///
/// Filters the entitlement's enabled Stripe mapping rows by the requested
/// currency (plus optional billing type/period filters) and returns the unique
/// match. Fail-loud by design: 404 when no row matches the currency (no
/// secondary-currency fallback), 409 when multiple rows remain and the caller
/// must narrow by billing type/period.
///
/// # Authentication
/// Requires valid API Key via X-API-Key header
///
/// # Realm Isolation
/// The API key must belong to the same realm as the requested realm.
#[utoipa::path(
    get,
    path = "/api/ext/{realmId}/entitlements/{entitlementKey}/default-price",
    tag = "ext",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("entitlementKey" = String, Path, description = "Entitlement key"),
        ResolveDefaultPriceParams
    ),
    responses(
        (status = 200, description = "Unique price row resolved", body = EntitlementPriceView),
        (status = 400, description = "Bad request - Missing or invalid currency code / billing type", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid or missing API Key", body = ErrorResponse),
        (status = 403, description = "Forbidden - Cross-realm access attempt", body = ErrorResponse),
        (status = 404, description = "No price row for the requested currency", body = ErrorResponse),
        (status = 409, description = "Multiple price rows; specify billing type/period", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("api_key" = []))
)]
pub async fn resolve_default_price(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, entitlement_key)): Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<ResolveDefaultPriceParams>,
) -> Response {
    use herald_api_base::application::http::server::api_entities::ApiError;

    if !identity.has_access_to_realm(&realm_id) {
        tracing::warn!(
            api_key_realm_id = %identity.realm_id(),
            request_realm_id = %realm_id,
            "Cross-realm access attempt blocked"
        );
        return json_error(StatusCode::FORBIDDEN, ErrorCode::CrossRealmAccessForbidden);
    }

    if let Err(resp) =
        require_principal_permission(&state, &identity, &realm_id, "billing", "view").await
    {
        return resp.into_response();
    }

    if let Err(e) = herald_core::domain::billing::validate_currency_code(&params.currency) {
        return ApiError::bad_request(e.to_string()).into_response();
    }

    let billing_type = match params.billing_type.as_deref() {
        None => None,
        Some(raw) => match raw.parse::<herald_core::domain::billing::BillingType>() {
            Ok(bt) => Some(bt),
            Err(e) => return ApiError::bad_request(e.to_string()).into_response(),
        },
    };

    let mappings = match state
        .billing_repository
        .find_enabled_stripe_mappings_by_entitlement(&realm_id, &entitlement_key)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(error = %e, "Failed to query entitlement mappings");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError);
        }
    };

    let row = match herald_core::domain::billing::resolve_price_row(
        &mappings,
        &entitlement_key,
        &params.currency,
        billing_type.as_ref(),
        params.billing_period.as_deref(),
    ) {
        Ok(row) => row,
        Err(herald_core::domain::billing::CurrencyResolveError::NotFound { .. }) => {
            return ApiError::not_found("No price row for the requested currency".to_string())
                .into_response();
        }
        Err(herald_core::domain::billing::CurrencyResolveError::Ambiguous { count, .. }) => {
            return ApiError::conflict(format!(
                "Multiple price rows ({count}) for the requested currency; specify billing type/period"
            ))
            .into_response();
        }
    };

    Json(EntitlementPriceView {
        mapping_id: row.mapping_id.to_string(),
        entitlement_key: row.entitlement_key,
        payment_provider: row.payment_provider,
        currency: row.currency,
        amount: row.amount,
        billing_type: row.billing_type.map(|t| t.as_str().to_string()),
        billing_period: row.billing_period,
        external_price_id: row.external_price_id,
    })
    .into_response()
}
