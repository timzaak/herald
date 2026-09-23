use axum::{
    Json,
    extract::{Extension, Path, Query, State},
};
use chrono::Utc;
use uuid::Uuid;

use crate::types::{
    CancelSubscriptionRequest,
    CancelSubscriptionResponse,
    PurchaseOptionListResponse,
    PurchaseOptionView,
    // Subscription types
    SubscriptionDetailResponse,
    SubscriptionListItemResponse,
    SubscriptionListQuery,
    SubscriptionListResponse,
};

use herald_api_base::application::http::common::auth_utils::{
    AdminIdentity, require_authenticated_user_in_realm_with_token, require_token_scope,
};
use herald_api_base::application::http::common::error_helpers::core_error_to_api_error;
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
// Import the trait and types from herald_core
use herald_core::domain::authentication::{CredentialScope, Identity, TokenCredentialContext};
use herald_core::domain::billing::entities::BillingType;
use herald_core::domain::billing::{BillingRepository, EntitlementMapping, Subscription};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::payment_attempt::PaymentAttemptRepository;
use herald_core::domain::user::UserRoleRepository;

fn subscription_to_response(sub: &Subscription) -> SubscriptionDetailResponse {
    SubscriptionDetailResponse {
        id: sub.id,
        client_app_id: sub.client_app_id,
        entitlement_key: sub.entitlement_key.clone(),
        external_price_id: sub.external_price_id.clone(),
        payment_provider: sub.payment_provider.clone(),
        status: sub.status.as_str().to_string(),
        billing_type: sub.billing_type.as_str().to_string(),
        current_period_start: sub.current_period_start.map(|dt| dt.to_rfc3339()),
        current_period_end: sub.current_period_end.map(|dt| dt.to_rfc3339()),
        cancel_at: sub.cancel_at.map(|dt| dt.to_rfc3339()),
        cancel_at_period_end: Some(sub.cancel_at_period_end),
        provider_metadata: sub.provider_metadata.clone(),
        synced_at: sub.synced_at.map(|dt| dt.to_rfc3339()),
        created_at: sub.created_at.to_rfc3339(),
        updated_at: sub.updated_at.to_rfc3339(),
    }
}

// ============================================================================
// Permission Check Helper
// ============================================================================

/// Check billing permissions for a realm
///
/// This helper function:
/// 1. Verifies realm boundary (identity's realm must match requested realm)
/// 2. Checks business permissions (billing.view or billing.manage)
pub async fn require_billing_permission(
    state: &AppState,
    identity: &Identity,
    realm_id: &str,
    action: &str,
) -> Result<(), ApiError> {
    let admin = AdminIdentity::require_in_realm(identity.clone(), realm_id, "billing")?;
    admin.require_permission(state, "billing", action).await
}

async fn require_client_app_in_realm(
    state: &AppState,
    realm_id: &str,
    client_app_id: Uuid,
) -> Result<(), ApiError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM client_app WHERE id = $1 AND realm_id = $2)",
    )
    .bind(client_app_id)
    .bind(realm_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(
            realm_id = %realm_id,
            client_app_id = %client_app_id,
            error = %e,
            "Failed to validate client app realm ownership"
        );
        ApiError::internal("Failed to validate client app")
    })?;

    if !exists {
        return Err(ApiError::not_found("Client app not found"));
    }

    Ok(())
}

fn require_bound_client_app(
    context: &TokenCredentialContext,
    client_app_id: Uuid,
) -> Result<(), ApiError> {
    if context.client_app_id != client_app_id {
        return Err(ApiError::forbidden(
            "token is bound to a different Client App",
        ));
    }
    Ok(())
}

fn require_subscription_ownership(
    subscription: &Subscription,
    realm_id: &str,
    user_id: Uuid,
) -> Result<(), ApiError> {
    if subscription.realm_id != realm_id || subscription.user_id != user_id {
        return Err(ApiError::not_found("Subscription not found"));
    }
    Ok(())
}

// ============================================================================
// Subscription Handlers
// ============================================================================

/// List subscriptions for a realm
#[utoipa::path(
    get,
    path = "/api/bill/subscriptions",
    tag = "billing",
        responses(
        (status = 200, description = "Subscriptions listed successfully", body = SubscriptionListResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_subscriptions(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Query(query): Query<SubscriptionListQuery>,
) -> Result<Json<SubscriptionListResponse>, ApiError> {
    let realm_id = identity.realm_id();
    tracing::info!("Listing subscriptions for realm: {}", realm_id);

    require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20);

    let (subs, total) = state
        .billing_repository
        .list_subscriptions(
            &realm_id,
            query.entitlement_key.as_deref(),
            query.status.as_deref(),
            query.payment_provider.as_deref(),
            page,
            page_size,
        )
        .await
        .map_err(|e| {
            tracing::error!(realm_id = %realm_id, error = %e, "Failed to list subscriptions");
            ApiError::internal("Failed to list subscriptions".to_string())
        })?;

    let items: Vec<SubscriptionListItemResponse> = subs
        .iter()
        .map(|sub| SubscriptionListItemResponse {
            id: sub.id,
            client_app_id: sub.client_app_id,
            entitlement_key: sub.entitlement_key.clone(),
            external_price_id: sub.external_price_id.clone(),
            payment_provider: sub.payment_provider.clone(),
            status: sub.status.as_str().to_string(),
            billing_type: sub.billing_type.as_str().to_string(),
            current_period_start: sub.current_period_start.map(|dt| dt.to_rfc3339()),
            current_period_end: sub.current_period_end.map(|dt| dt.to_rfc3339()),
            synced_at: sub.synced_at.map(|dt| dt.to_rfc3339()),
            created_at: sub.created_at.to_rfc3339(),
            updated_at: sub.updated_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(SubscriptionListResponse {
        items,
        total: total as i64,
    }))
}

/// Get a specific subscription
#[utoipa::path(
    get,
    path = "/api/bill/subscriptions/{subscriptionId}",
    tag = "billing",
    params(
        ("subscriptionId" = Uuid, Path, description = "Subscription ID")
    ),
    responses(
        (status = 200, description = "Subscription found", body = SubscriptionDetailResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Subscription not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_subscription(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(subscription_id): Path<Uuid>,
) -> Result<Json<SubscriptionDetailResponse>, ApiError> {
    let realm_id = identity.realm_id();
    tracing::info!(
        "Getting subscription {} for realm: {}",
        subscription_id,
        realm_id
    );

    require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let subscription = state
        .billing_repository
        .find_subscription_by_id(subscription_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Subscription not found"))?;

    if subscription.realm_id != realm_id {
        return Err(ApiError::not_found("Subscription not found"));
    }

    Ok(Json(subscription_to_response(&subscription)))
}

/// Get subscription for a client app
#[utoipa::path(
    get,
    path = "/api/bill/client/{clientAppId}/subscription",
    tag = "billing",
    params(
        ("clientAppId" = Uuid, Path, description = "Client App ID")
    ),
    responses(
        (status = 200, description = "Subscription found", body = SubscriptionDetailResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = ErrorResponse),
        (status = 404, description = "Subscription not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_subscription_for_client_app(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Path(client_app_id): Path<Uuid>,
) -> Result<Json<SubscriptionDetailResponse>, ApiError> {
    let realm_id = identity.realm_id();
    tracing::info!(
        "Getting subscription for client app {} in realm: {}",
        client_app_id,
        realm_id
    );

    require_token_scope(&identity, &context, CredentialScope::SubscriptionRead)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "subscription",
    )?;
    require_bound_client_app(&context, client_app_id)?;

    let subscription = state
        .billing_repository
        .find_subscription_by_client_app_id(client_app_id)
        .await?
        .ok_or_else(|| CoreError::SubscriptionNotFound(client_app_id.to_string()))?;

    require_subscription_ownership(&subscription, &realm_id, user_id)?;

    Ok(Json(subscription_to_response(&subscription)))
}

/// Cancel the caller's own subscription for a client app.
///
/// Unlike the previous admin-only flip, this handler is a **user self-service**
/// endpoint mounted on the browser-token router. It calls the **payment provider
/// cancel API** (Stripe/Creem) and does **NOT** touch the local subscription
/// row — local status is updated exclusively by the subsequent provider webhook
/// (`customer.subscription.deleted` / `subscription.canceled`). If the provider
/// call fails the error is surfaced to the caller and the local state is left
/// untouched.
///
/// Apple / Google in-app purchases cannot be canceled via a developer API
/// (only via the App Store / Play Store by the user); this endpoint rejects
/// them with 400.
#[utoipa::path(
    post,
    path = "/api/bill/client/{clientAppId}/subscription/cancel",
    tag = "billing",
    params(
        ("clientAppId" = Uuid, Path, description = "Client App ID")
    ),
    request_body = CancelSubscriptionRequest,
    responses(
        (status = 200, description = "Cancel request submitted to provider; local status will update via webhook", body = CancelSubscriptionResponse),
        (status = 400, description = "Provider does not support developer-initiated cancel (Apple/Google)", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - token scope denied or not the subscription owner", body = ErrorResponse),
        (status = 404, description = "Subscription not found", body = ErrorResponse),
        (status = 502, description = "Provider cancel API failed; local state unchanged", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn cancel_subscription_for_client_app(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Path(client_app_id): Path<Uuid>,
    Json(request): Json<CancelSubscriptionRequest>,
) -> Result<Json<CancelSubscriptionResponse>, ApiError> {
    let realm_id = identity.realm_id();
    tracing::info!(
        "Canceling subscription for client app {} in realm: {}, cancel_at_period_end: {}",
        client_app_id,
        realm_id,
        request.cancel_at_period_end
    );

    // User self-service auth: scope + realm user + bound client app.
    require_token_scope(&identity, &context, CredentialScope::SubscriptionCancel)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "subscription",
    )?;
    require_bound_client_app(&context, client_app_id)?;

    let subscription = state
        .billing_repository
        .find_subscription_by_client_app_id(client_app_id)
        .await?
        .ok_or_else(|| CoreError::SubscriptionNotFound(client_app_id.to_string()))?;

    require_subscription_ownership(&subscription, &realm_id, user_id)?;

    if subscription.external_subscription_id.is_empty() {
        return Err(ApiError::bad_request(
            "Subscription has no external provider id; cannot cancel via provider API",
        ));
    }

    // Dispatch to the provider cancel API. Local DB state is intentionally NOT
    // mutated here — it is updated by the provider webhook. A provider failure
    // must surface to the user with the local row unchanged.
    let canceled_at = dispatch_provider_cancel(
        &state,
        &realm_id,
        &subscription.payment_provider,
        &subscription.external_subscription_id,
        request.cancel_at_period_end,
    )
    .await?;

    let message = if request.cancel_at_period_end {
        "Cancel request submitted; subscription will end at the next period (status updates via webhook)"
    } else {
        "Cancel request submitted to provider (status will update via webhook)"
    };

    Ok(Json(CancelSubscriptionResponse {
        subscription_id: subscription.id.to_string(),
        canceled_at: canceled_at.to_rfc3339(),
        message: message.to_string(),
    }))
}

/// Provider dispatch for subscription cancellation.
///
/// Routes to the matching provider's cancel API based on
/// `subscription.payment_provider`. Returns the effective cancellation timestamp
/// (provider-reported when available, else now). Errors propagate as
/// `CoreError` which `From<CoreError> for ApiError` maps to a 5xx/502-style
/// response, leaving the local subscription row untouched.
async fn dispatch_provider_cancel(
    state: &AppState,
    realm_id: &str,
    payment_provider: &str,
    external_subscription_id: &str,
    cancel_at_period_end: bool,
) -> Result<chrono::DateTime<Utc>, CoreError> {
    let purchase_service = &state.purchase_service;
    match payment_provider {
        "stripe" => {
            let client = purchase_service
                .get_stripe_client_for_realm(realm_id)
                .await?;
            let resp = client
                .cancel_subscription(&herald_infra_stripe::CancelSubscriptionRequest {
                    subscription_id: external_subscription_id.to_string(),
                    cancel_at_period_end,
                })
                .await?;
            tracing::info!(
                provider = "stripe",
                external_id = external_subscription_id,
                status = ?resp.status,
                "Stripe cancel submitted; awaiting webhook"
            );
            Ok(Utc::now())
        }
        "creem" => {
            let client = purchase_service
                .get_creem_client_for_realm(realm_id)
                .await?;
            let mode = if cancel_at_period_end {
                herald_infra_creem::CreemCancelMode::Scheduled
            } else {
                herald_infra_creem::CreemCancelMode::Immediate
            };
            let resp = client
                .cancel_subscription(external_subscription_id, mode)
                .await?;
            tracing::info!(
                provider = "creem",
                external_id = external_subscription_id,
                status = ?resp.status,
                "Creem cancel submitted; awaiting webhook"
            );
            Ok(Utc::now())
        }
        "apple" | "google" => Err(CoreError::BadRequest(format!(
            "{payment_provider} subscriptions must be canceled in the {} by the user; \
             developer-initiated cancel is not supported",
            if payment_provider == "apple" {
                "App Store"
            } else {
                "Google Play Store"
            }
        ))),
        other => Err(CoreError::BadRequest(format!(
            "Unsupported payment provider for cancel: {other}"
        ))),
    }
}

/// Map a price-level entitlement mapping to the purchase-page view.
///
/// `display_name` / `amount` / `currency` are read from the
/// `provider_product_info` JSONB cache populated by sync (same source the
/// one-time-mappings read model and checkout price_amount use).
fn mapping_to_purchase_option(
    m: EntitlementMapping,
    point_rules: Vec<crate::types::PointDistributionRuleResponse>,
) -> PurchaseOptionView {
    let info = m.provider_product_info.as_ref();
    // Only the one_time+role combo is the gated one-per-user entitlement
    // gated, so `grants_role` is `false` for them even if they carry role
    // grants. `already_owned` is computed per-user in the handler; seeded
    // `false` here and overwritten for gated options.
    let grants_role =
        m.billing_type == Some(BillingType::OneTime) && !m.granted_role_ids.is_empty();
    PurchaseOptionView {
        mapping_id: m.id,
        external_product_id: m.external_product_id,
        external_price_id: m.external_price_id,
        payment_provider: m.payment_provider,
        entitlement_key: m.entitlement_key,
        billing_type: m.billing_type.map(|t| t.as_str().to_string()),
        billing_period: m.billing_period,
        display_name: info
            .and_then(|i| i.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        amount: info.and_then(|i| i.get("price")).and_then(|v| v.as_i64()),
        currency: info
            .and_then(|i| i.get("currency"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        point_rules,
        enabled: m.enabled,
        grants_role,
        already_owned: false,
    }
}

/// List purchasable price-level options for a client app.
///
/// Returns a FLAT list of enabled price-granularity mappings (recurring +
/// one_time) for the purchase page; the frontend groups by
/// `external_product_id` / billing period. Replaces the purchase page's
/// dependency on `list_one_time_mappings` (which only covered one_time).
#[utoipa::path(
    get,
    path = "/api/bill/client/{clientAppId}/purchase-options",
    tag = "billing",
    params(
        ("clientAppId" = Uuid, Path, description = "Client App ID")
    ),
    responses(
        (status = 200, description = "Purchase options listed successfully", body = PurchaseOptionListResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_purchase_options(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Path(client_app_id): Path<Uuid>,
) -> Result<Json<PurchaseOptionListResponse>, ApiError> {
    let realm_id = identity.realm_id();
    tracing::info!(
        "Listing purchase options for client app {} in realm {}",
        client_app_id,
        realm_id
    );

    // Purchase-page read is an authenticated-user action. The user id drives
    require_token_scope(&identity, &context, CredentialScope::PurchaseRead)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "purchase-options",
    )?;
    require_bound_client_app(&context, client_app_id)?;
    require_client_app_in_realm(&state, &realm_id, client_app_id).await?;

    // List ALL enabled price-granularity mappings for the realm (recurring +
    // one_time). Page size is set high to return the full purchasable set in a
    // single page; the purchase page expects a flat list, not pagination.
    let (mappings, _total) = state
        .billing_repository
        .list_entitlement_mappings(&realm_id, None, Some(true), Some(1), Some(200))
        .await
        .map_err(|e| {
            tracing::error!(realm_id = %realm_id, error = %e, "Failed to list purchase options");
            ApiError::internal("Failed to list purchase options".to_string())
        })?;

    // Build the base views (sets `grants_role` from mapping fields), then for
    // the gated combo (one_time + non-empty granted_role_ids) compute
    // `already_owned` for the authenticated user. The options list is per-realm
    // and typically small, so a per-option ownership query is acceptable (the
    // role check is a single indexed lookup; the attempt check is indexed too).
    let mut items: Vec<PurchaseOptionView> = Vec::with_capacity(mappings.len());
    for m in mappings {
        // Capture the gated-combo inputs before `m` is moved into the mapper.
        let grants_role =
            m.billing_type == Some(BillingType::OneTime) && !m.granted_role_ids.is_empty();
        let granted_role_ids: Vec<Uuid> = if grants_role {
            m.granted_role_ids.clone()
        } else {
            Vec::new()
        };
        let mapping_id = m.id;
        let rules = state
            .billing_repository
            .find_mapping_rules(&realm_id, m.id)
            .await
            .map_err(|e| core_error_to_api_error(e, "Purchase options rule load"))?;
        let point_rules: Vec<crate::types::PointDistributionRuleResponse> = rules
            .into_iter()
            .map(crate::entitlement_mapping_handlers::rule_to_response)
            .collect();
        let mut view = mapping_to_purchase_option(m, point_rules);
        if grants_role {
            let has_role = state
                .user_role_repository
                .user_has_any_role(&realm_id, user_id, &granted_role_ids)
                .await
                .map_err(CoreError::from)
                .map_err(|e| core_error_to_api_error(e, "Purchase options ownership check"))?;
            let has_attempt = state
                .payment_attempt_repository
                .has_succeeded_attempt(user_id, mapping_id)
                .await
                .map_err(|e| core_error_to_api_error(e, "Purchase options ownership check"))?;
            view.already_owned = has_role || has_attempt;
        }
        items.push(view);
    }

    Ok(Json(PurchaseOptionListResponse { items }))
}

#[cfg(test)]
mod browser_scope_tests {
    use super::*;
    use chrono::Utc;
    use herald_core::domain::authentication::CredentialClass;
    use herald_core::domain::billing::entities::SubscriptionStatus;
    use std::collections::HashSet;

    fn context(client_app_id: Uuid) -> TokenCredentialContext {
        TokenCredentialContext {
            client_app_id,
            client_id: "custom-user-ui".to_string(),
            family_id: Uuid::now_v7(),
            credential_class: CredentialClass::CustomUserUi,
            allowed_scopes: HashSet::new(),
        }
    }

    fn subscription(realm_id: &str, user_id: Uuid) -> Subscription {
        Subscription {
            id: Uuid::now_v7(),
            realm_id: realm_id.to_string(),
            user_id,
            external_subscription_id: "external".to_string(),
            external_product_id: "product".to_string(),
            payment_provider: "stripe".to_string(),
            status: SubscriptionStatus::Active,
            entitlement_key: "plan".to_string(),
            billing_type: BillingType::Recurring,
            external_price_id: None,
            provider_metadata: None,
            synced_at: None,
            current_period_start: None,
            current_period_end: None,
            cancel_at_period_end: false,
            client_app_id: None,
            cancel_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn browser_scope_rejects_mismatched_client_app_before_lookup() {
        let bound = Uuid::now_v7();
        assert!(require_bound_client_app(&context(bound), Uuid::now_v7()).is_err());
    }

    #[test]
    fn browser_scope_rejects_cross_user_subscription_ownership() {
        let owner = Uuid::now_v7();
        let sub = subscription("realm", owner);
        assert!(require_subscription_ownership(&sub, "realm", Uuid::now_v7()).is_err());
    }
}
