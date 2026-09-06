use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use herald_api_base::application::http::common::auth_utils::{
    require_authenticated_user_in_realm_with_token, require_token_scope,
};
use herald_api_base::application::http::common::error_helpers::core_error_to_api_error;
use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::{CredentialScope, Identity, TokenCredentialContext};
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::payment_attempt::PaymentAttemptRepository;
use herald_core::domain::purchase::{
    ALREADY_OWNED_MARKER, CompletePaymentAttemptInput, FulfillmentResult, PaymentCompletionSource,
    PaymentFlow, PreparePaymentAttemptInput,
};

use crate::handlers::require_billing_permission;
use crate::payment_email::formal_payment_email;
use crate::provider_common_types::validate_payment_provider_value;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreatePaymentAttemptRequest {
    #[validate(custom(function = "validate_purchasable_target"))]
    pub target_type: String,
    pub target_id: Uuid,
    #[validate(custom(function = "validate_payment_provider_value"))]
    pub payment_provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// WeChat-only checkout scene: `"native"` (default) or `"jsapi"`. Ignored
    /// by other providers (DEC-wechat-support-009).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_scene: Option<String>,
    /// WeChat JSAPI payer openid; required when `paymentScene = "jsapi"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openid: Option<String>,
    /// Checkout flow: `"hosted"` (default) redirects to the provider's hosted
    /// checkout page; `"payment_intent"` returns a raw Stripe PaymentIntent
    /// `clientSecret` for mobile wallet SDK confirmation (Apple Pay / Google
    /// Pay). `payment_intent` is only valid for `stripe` + one-time purchases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[validate(custom(function = "validate_payment_flow"))]
    pub flow: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePaymentAttemptResponse {
    pub id: Uuid,
    pub realm_id: String,
    pub user_id: Uuid,
    pub payment_provider: String,
    pub target_type: String,
    pub target_id: Uuid,
    pub amount: i64,
    pub currency: String,
    pub status: String,
    pub expires_at: String,
    pub payment_context: PaymentContextResponse,
    pub created_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaymentContextResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stripe_checkout_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creem_checkout_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// WeChat Native (PC scan) `code_url` rendered as a QR code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wechat_code_url: Option<String>,
    /// WeChat JSAPI invocation params (DEC-wechat-support-011).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wechat_jsapi_params:
        Option<herald_core::domain::payment_attempt::entities::WechatJsapiParams>,
}

/// Structured 409 body emitted when a one-time+role entitlement is already
/// owned by the buyer (anti-repeat purchase rule). Surfaced instead
/// of the generic conflict message so the frontend can distinguish "already
/// owned" from other 409s.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlreadyOwnedErrorResponse {
    pub code: String,
    pub entitlement_key: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaymentAttemptStatusResponse {
    pub id: Uuid,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_status: Option<String>,
    pub target_type: String,
    pub target_id: Uuid,
    pub amount: i64,
    pub currency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fulfillment: Option<FulfillmentResultResponse>,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FulfillmentResultResponse {
    #[serde(rename = "type")]
    pub fulfillment_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<Uuid>,
    pub point_grants: Vec<PointGrantResponse>,
    pub granted_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PointGrantResponse {
    pub rule_id: Uuid,
    pub bucket_id: Uuid,
    pub result_id: Uuid,
    pub points_type: String,
    pub points: Option<i64>,
    pub description: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FulfillPaymentResponse {
    #[serde(rename = "type")]
    pub fulfillment_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<Uuid>,
    pub point_grants: Vec<PointGrantResponse>,
    pub granted_at: String,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct PurchaseHistoryQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub payment_provider: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseHistoryResponse {
    pub items: Vec<PurchaseHistoryItem>,
    pub total: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseHistoryItem {
    pub user_id: Uuid,
    pub attempt_id: Uuid,
    pub target_mapping_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<i64>,
    pub amount: i64,
    pub currency: String,
    pub payment_provider: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct FulfillPaymentRequest {
    /// Realm the caller intends to fulfill in. The attempt's own realm must
    /// match or the request is rejected — the internal key is global, so this
    /// binding is what stops a (leaked or mistyped) call from completing a
    /// payment attempt in an arbitrary other realm.
    #[validate(length(min = 1))]
    pub realm_id: String,
    pub provider_status: String,
    pub provider_transaction_id: String,
    pub completed_at: String,
}

// ============================================================================
// Validation Functions
// ============================================================================

fn validate_purchasable_target(target_type: &str) -> Result<(), validator::ValidationError> {
    if matches!(target_type, "entitlement_mapping") {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid target_type"))
    }
}

/// Checkout-flow whitelist. Kept in sync with `PaymentFlow::parse` on the
/// domain side; the domain parser supplies the detailed error message, this
/// gate only needs to 400 early.
fn validate_payment_flow(flow: &str) -> Result<(), validator::ValidationError> {
    if matches!(flow, "hosted" | "payment_intent") {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid flow"))
    }
}

// `validate_payment_provider_value` (provider_common_types) covers the
// payment_provider whitelist; the IAP-query-path rationale is documented there.

// ============================================================================
// Conversion Helpers
// ============================================================================

/// Map a purchase-path `CoreError` to an `ApiError`.
///
/// Intercepts the already-owned conflict (carried as
/// `CoreError::Conflict("<ALREADY_OWNED_MARKER><entitlement_key>")` — see
/// `PurchaseErrorExt::already_owned`) and emits a structured 409 body
/// `{ "code": "already_owned", "entitlementKey": <key> }`.
/// All other errors fall through to the generic `core_error_to_api_error`
/// mapping.
fn map_purchase_error_to_api_error(e: CoreError, operation: &str) -> ApiError {
    if let CoreError::Conflict(msg) = &e
        && let Some(entitlement_key) = msg.strip_prefix(ALREADY_OWNED_MARKER)
    {
        return ApiError::conflict_json(AlreadyOwnedErrorResponse {
            code: "already_owned".to_string(),
            entitlement_key: entitlement_key.to_string(),
        });
    }
    core_error_to_api_error(e, operation)
}

fn payment_context_to_response(
    context: herald_core::domain::payment_attempt::PaymentContext,
) -> PaymentContextResponse {
    PaymentContextResponse {
        stripe_checkout_url: context.stripe_checkout_url,
        creem_checkout_url: context.creem_checkout_url,
        client_secret: context.client_secret,
        wechat_code_url: context.wechat_code_url,
        wechat_jsapi_params: context.wechat_jsapi_params,
    }
}

fn fulfillment_result_to_response(result: FulfillmentResult) -> FulfillPaymentResponse {
    FulfillPaymentResponse {
        fulfillment_type: match result.fulfillment_type {
            herald_core::domain::purchase::FulfillmentType::SubscriptionCreated => {
                "subscription_created".to_string()
            }
            herald_core::domain::purchase::FulfillmentType::SubscriptionUpdated => {
                "subscription_updated".to_string()
            }
            herald_core::domain::purchase::FulfillmentType::PointsGranted => {
                "point_grants".to_string()
            }
        },
        subscription_id: result.subscription_id,
        point_grants: result
            .point_grants
            .into_iter()
            .map(|grant| PointGrantResponse {
                rule_id: grant.rule_id,
                bucket_id: grant.bucket_id,
                result_id: grant.result_id,
                points_type: grant.points_type,
                points: grant.points,
                description: grant.description,
            })
            .collect(),
        granted_at: result.granted_at.to_rfc3339(),
    }
}

// ============================================================================
// Handlers
// ============================================================================

#[utoipa::path(
    post,
    path = "/api/bill/{realmId}/purchase/payment-attempts",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body(
        content = CreatePaymentAttemptRequest,
        description = "Creates a payment attempt. Optional `flow`: \"hosted\" (default) returns the provider's hosted checkout URL; \"payment_intent\" (stripe + one-time purchases only) returns a raw PaymentIntent clientSecret for mobile wallet SDK confirmation (Apple Pay / Google Pay)."
    ),
    responses(
        (status = 201, description = "Payment attempt created successfully", body = CreatePaymentAttemptResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Target not found or not enabled"),
        (status = 409, description = "Payment provider not configured for target")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_payment_attempt(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Path(realm_id): Path<String>,
    Json(input): Json<CreatePaymentAttemptRequest>,
) -> Result<(StatusCode, Json<CreatePaymentAttemptResponse>), ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PurchaseInitiate)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "purchase APIs",
    )?;
    input
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;
    let flow = PaymentFlow::parse(input.flow.as_deref())
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {e}")))?;

    let created = state
        .purchase_service
        .create_payment_attempt(PreparePaymentAttemptInput {
            realm_id: realm_id.clone(),
            user_id,
            user_email: formal_payment_email(&identity),
            payment_provider: input.payment_provider.clone(),
            target_type: input.target_type.clone(),
            target_id: input.target_id,
            metadata: input.metadata,
            flow,
            payment_scene: input.payment_scene,
            openid: input.openid,
        })
        .await
        .map_err(|e| map_purchase_error_to_api_error(e, "Create payment attempt"))?;

    let response = CreatePaymentAttemptResponse {
        id: created.attempt.id,
        realm_id: created.attempt.realm_id,
        user_id: created.attempt.user_id,
        payment_provider: created.attempt.payment_provider,
        target_type: created.attempt.target_type.to_string(),
        target_id: created.attempt.target_id,
        amount: created.attempt.amount,
        currency: created.attempt.currency,
        status: created.attempt.status.to_string(),
        expires_at: created.attempt.expires_at.to_rfc3339(),
        payment_context: payment_context_to_response(created.context),
        created_at: created.attempt.created_at.to_rfc3339(),
    };

    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/purchase/payment-attempts/{attemptId}",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("attemptId" = Uuid, Path, description = "Payment Attempt ID")
    ),
    responses(
        (status = 200, description = "Payment attempt status retrieved", body = PaymentAttemptStatusResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - not your attempt"),
        (status = 404, description = "Payment attempt not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_payment_attempt_status(
    State(state): State<AppState>,
    Path((realm_id, attempt_id)): Path<(String, Uuid)>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
) -> Result<Json<PaymentAttemptStatusResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PurchaseStatusRead)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "purchase APIs",
    )?;

    let service = &state.payment_attempt_service;

    let attempt = service
        .get_payment_attempt_status(&realm_id, attempt_id, user_id)
        .await
        .map_err(|e| core_error_to_api_error(e, "Get payment attempt status"))?;

    let response = PaymentAttemptStatusResponse {
        id: attempt.id,
        status: attempt.status.to_string(),
        provider_status: attempt.provider_status,
        target_type: attempt.target_type.to_string(),
        target_id: attempt.target_id,
        amount: attempt.amount,
        currency: attempt.currency,
        completed_at: attempt.completed_at.map(|dt| dt.to_rfc3339()),
        fulfillment: None,
        created_at: attempt.created_at.to_rfc3339(),
        expires_at: attempt.expires_at.to_rfc3339(),
    };

    Ok(Json(response))
}

#[utoipa::path(
    post,
    path = "/api/bill/{realmId}/purchase/payment-attempts/{attemptId}/cancel",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("attemptId" = Uuid, Path, description = "Payment Attempt ID")
    ),
    responses(
        (status = 200, description = "Payment attempt cancelled", body = PaymentAttemptStatusResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - not your attempt"),
        (status = 404, description = "Payment attempt not found"),
        (status = 409, description = "Cannot cancel attempt in current state")
    ),
    security(("bearer_auth" = []))
)]
pub async fn cancel_payment_attempt(
    State(state): State<AppState>,
    Path((realm_id, attempt_id)): Path<(String, Uuid)>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
) -> Result<Json<PaymentAttemptStatusResponse>, ApiError> {
    require_token_scope(&identity, &context, CredentialScope::PurchaseInitiate)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "purchase APIs",
    )?;

    let service = &state.payment_attempt_service;

    let attempt = service
        .cancel_payment_attempt(&realm_id, attempt_id, user_id)
        .await
        .map_err(|e| core_error_to_api_error(e, "Cancel payment attempt"))?;

    let response = PaymentAttemptStatusResponse {
        id: attempt.id,
        status: attempt.status.to_string(),
        provider_status: attempt.provider_status,
        target_type: attempt.target_type.to_string(),
        target_id: attempt.target_id,
        amount: attempt.amount,
        currency: attempt.currency,
        completed_at: attempt.completed_at.map(|dt| dt.to_rfc3339()),
        fulfillment: None,
        created_at: attempt.created_at.to_rfc3339(),
        expires_at: attempt.expires_at.to_rfc3339(),
    };

    Ok(Json(response))
}

#[utoipa::path(
    post,
    path = "/api/internal/bill/purchase/payment-attempts/{attemptId}/fulfill",
    tag = "billing",
    params(
        ("attemptId" = Uuid, Path, description = "Payment Attempt ID")
    ),
    request_body = FulfillPaymentRequest,
    responses(
        (status = 200, description = "Fulfillment completed", body = FulfillPaymentResponse),
        (status = 400, description = "Fulfillment failed"),
        (status = 404, description = "Payment attempt not found"),
        (status = 409, description = "Already fulfilled")
    )
)]
pub async fn fulfill_payment(
    State(state): State<AppState>,
    Path(attempt_id): Path<Uuid>,
    Json(input): Json<FulfillPaymentRequest>,
) -> Result<Json<FulfillPaymentResponse>, ApiError> {
    let completed_at = chrono::DateTime::parse_from_rfc3339(&input.completed_at)
        .map_err(|_| ApiError::bad_request("Invalid completed_at format"))?
        .with_timezone(&chrono::Utc);

    input
        .validate()
        .map_err(|e| ApiError::bad_request(format!("Invalid fulfill request: {}", e)))?;

    let result = state
        .purchase_service
        .complete_succeeded_payment_attempt(CompletePaymentAttemptInput {
            attempt_id,
            provider_status: input.provider_status,
            provider_transaction_id: input.provider_transaction_id,
            completed_at,
            source: PaymentCompletionSource::InternalApi,
            billing_type_override: None,
            // Demo/test payment simulation behind the INTERNAL_API_KEY gate.
            // The endpoint has no path realm, so the caller must state the
            // realm in the body and the domain layer rejects an attempt from
            // any other realm.
            expected_realm_id: Some(input.realm_id),
        })
        .await
        .map_err(|e| core_error_to_api_error(e, "Fulfill payment"))?;

    Ok(Json(fulfillment_result_to_response(result)))
}

#[utoipa::path(
    get,
    path = "/api/user/bill/purchase/history",
    tag = "billing",
    params(
        PurchaseHistoryQuery
    ),
    responses(
        (status = 200, description = "Purchase history retrieved", body = PurchaseHistoryResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_purchase_history(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Extension(context): Extension<TokenCredentialContext>,
    Query(filters): Query<PurchaseHistoryQuery>,
) -> Result<Json<PurchaseHistoryResponse>, ApiError> {
    let realm_id = identity.realm_id();
    require_token_scope(&identity, &context, CredentialScope::PurchaseRead)?;
    let user_id = require_authenticated_user_in_realm_with_token(
        &identity,
        &context,
        &realm_id,
        "purchase history",
    )?;

    purchase_history_response(&state, &realm_id, Some(user_id), filters).await
}

#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/purchase/history",
    tag = "billing",
    params(("realmId" = String, Path, description = "Realm ID"), PurchaseHistoryQuery),
    responses(
        (status = 200, description = "Realm purchase history", body = PurchaseHistoryResponse),
        (status = 403, description = "Forbidden")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_realm_purchase_history(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Query(filters): Query<PurchaseHistoryQuery>,
) -> Result<Json<PurchaseHistoryResponse>, ApiError> {
    require_billing_permission(&state, &identity, &realm_id, "view").await?;
    purchase_history_response(&state, &realm_id, None, filters).await
}

async fn purchase_history_response(
    state: &AppState,
    realm_id: &str,
    user_id: Option<Uuid>,
    filters: PurchaseHistoryQuery,
) -> Result<Json<PurchaseHistoryResponse>, ApiError> {
    let page = filters.page.unwrap_or(1).max(1);
    let page_size = filters.page_size.unwrap_or(20).clamp(1, 100);

    let (rows, total) = state
        .payment_attempt_repository
        .list_purchase_history(
            realm_id,
            user_id,
            filters.payment_provider.as_deref(),
            filters.start_date.as_deref(),
            filters.end_date.as_deref(),
            page,
            page_size,
        )
        .await
        .map_err(|e| {
            tracing::error!(realm_id = %realm_id, error = %e, "Failed to fetch purchase history");
            ApiError::internal("Failed to fetch purchase history")
        })?;

    let items: Vec<PurchaseHistoryItem> = rows
        .into_iter()
        .map(|row| PurchaseHistoryItem {
            attempt_id: row.attempt_id,
            user_id: row.user_id,
            target_mapping_id: row.target_mapping_id,
            product_name: row.product_name,
            points: row.points,
            amount: row.amount,
            currency: row.currency,
            payment_provider: row.payment_provider,
            status: row.status,
            completed_at: row.completed_at.map(|dt| dt.to_rfc3339()),
            created_at: row.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(PurchaseHistoryResponse { items, total }))
}
