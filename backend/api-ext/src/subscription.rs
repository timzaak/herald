// Subscription Status API for Third-Party Integration
//
// Allows third-party apps to query subscription status for client apps.

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use herald_core::domain::authentication::Identity;
use herald_core::domain::billing::{BillingRepository, Subscription};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::authz::require_principal_permission;
use crate::client_app_scope::ensure_client_app_scope;
use crate::client_helper::ClientAppLookup;
use herald_api_base::application::http::common::error_codes::ErrorCode;
use herald_api_base::application::http::common::error_helpers::json_error;
use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::state::AppState;

/// Subscription status response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionResponse {
    /// Client app ID
    pub client_app_id: String,

    /// Whether the app has an active subscription
    pub has_subscription: bool,

    /// Subscription status
    /// - "active": Subscription is active and paid
    /// - "canceled": Subscription was canceled but still active until period end
    /// - "expired": Subscription has expired
    /// - "trialing": Free trial period
    /// - "none": No subscription
    pub status: String,

    /// Entitlement key for the subscription
    pub entitlement_key: String,

    /// Payment provider (stripe, creem, etc.)
    pub payment_provider: String,

    /// Billing type snapshot (`"recurring"` / `"non_renewing"`) when a
    /// subscription exists; empty string when `has_subscription == false`.
    /// Lets third-party SDKs distinguish non-renewing subscriptions from
    /// `billingType`.
    pub billing_type: String,
}

/// Get subscription status for a client app
///
/// Returns subscription information for the specified client app.
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
///   https://api.example.com/api/ext/subscription/client-app-123 \
///   -H "X-API-Key: your-api-key"
/// ```
#[utoipa::path(
    get,
    path = "/api/ext/subscription/{clientAppId}",
    tag = "ext",
    params(
        ("clientAppId" = String, Path, description = "Client app ID")
    ),
    responses(
        (status = 200, description = "Subscription status retrieved", body = SubscriptionResponse),
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
    Path(client_app_id): Path<Uuid>,
) -> Response {
    let api_key_realm_id = identity.realm_id();

    tracing::info!(
        api_key_realm_id = %api_key_realm_id,
        client_app_id = %client_app_id,
        "Subscription query requested"
    );

    // 1. Resolve the target client app scoped to the API key's own realm:
    //    an unknown UUID and another tenant's UUID are both 404. The old
    //    global-lookup-then-realm-check ordering answered 403 for foreign
    //    UUIDs and 404 for unknown ones — a cross-tenant client-app
    //    existence oracle available to any key in any realm.
    let client_app_lookup = ClientAppLookup::new(state.pool.clone());
    let client_app_realm_id: String = match client_app_lookup
        .find_realm_by_uuid_scoped(client_app_id, &api_key_realm_id)
        .await
    {
        Ok(Some(realm_id)) => realm_id,
        Ok(None) => {
            tracing::warn!(
                api_key_realm_id = %api_key_realm_id,
                client_app_id = %client_app_id,
                "Subscription query for a client app outside the key's realm (or unknown)"
            );
            return json_error(StatusCode::NOT_FOUND, ErrorCode::ClientAppNotFound);
        }
        Err(e) => return e,
    };

    if let Err(resp) =
        require_principal_permission(&state, &identity, &client_app_realm_id, "billing", "view")
            .await
    {
        return resp.into_response();
    }

    if let Err(resp) = ensure_client_app_scope(&state, &identity, client_app_id).await {
        return resp;
    }

    // 3. Query subscription status
    let subscription: Option<Subscription> = match state
        .billing_repository
        .find_subscription_by_client_app_id(client_app_id)
        .await
    {
        Ok(sub) => sub,
        Err(e) => {
            tracing::error!("Failed to query subscription: {}", e);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError);
        }
    };

    // 4. Build response
    let response = match subscription {
        Some(sub) => {
            tracing::info!(
                client_app_id = %client_app_id,
                status = %sub.status.as_str(),
                entitlement_key = %sub.entitlement_key,
                "Subscription found"
            );

            SubscriptionResponse {
                client_app_id: client_app_id.to_string(),
                has_subscription: true,
                status: sub.status.as_str().to_string(),
                entitlement_key: sub.entitlement_key.clone(),
                payment_provider: sub.payment_provider.clone(),
                billing_type: sub.billing_type.as_str().to_string(),
            }
        }
        None => {
            tracing::info!(
                client_app_id = %client_app_id,
                "No subscription found (free tier)"
            );

            SubscriptionResponse {
                client_app_id: client_app_id.to_string(),
                has_subscription: false,
                status: "none".to_string(),
                entitlement_key: String::new(),
                payment_provider: String::new(),
                billing_type: String::new(),
            }
        }
    };

    Json(response).into_response()
}
