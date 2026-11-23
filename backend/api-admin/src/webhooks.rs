use axum::{
    Json,
    extract::{Extension, Path, Query, State},
};
use axum_valid::Valid;
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult, PageResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::Identity;
use herald_core::domain::webhook_outbound::{
    self, OutboundEventCategory, WebhookOutboundError, WebhookStatusTarget, WebhookSubscription,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookSubscriptionRequest {
    /// Publicly reachable http(s) callback URL; private/loopback/link-local
    /// addresses are rejected.
    #[validate(length(min = 1, max = 2048))]
    pub endpoint_url: String,

    /// Non-empty subset of `user_lifecycle`, `role_events`,
    /// `subscription_lifecycle`. Emptiness and unknown values are judged in
    /// the handler so the contract's `invalid_event_categories` code (not a
    /// generic 422) is returned.
    pub event_categories: Vec<String>,
}

#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWebhookSubscriptionRequest {
    #[validate(length(min = 1, max = 2048))]
    pub endpoint_url: String,
    pub event_categories: Vec<String>,
}

#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SetWebhookSubscriptionStatusRequest {
    /// Target status: `active` or `disabled`. `paused` is system-only and
    /// rejected with 400.
    #[validate(length(min = 1))]
    pub status: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebhookSubscriptionItem {
    pub id: Uuid,
    pub realm_id: String,
    pub endpoint_url: String,
    pub event_categories: Vec<String>,
    /// `active` / `paused` (system) / `disabled`
    pub status: String,
    pub consecutive_failures: i32,
    /// Truncated (500 chars) summary of the latest delivery failure — the
    /// data source behind the paused-reason surface.
    pub last_error: Option<String>,
    pub paused_at: Option<String>,
    /// Last 4 secret characters (`…wxyz`); the secret itself is only ever
    /// returned by create/rotate.
    pub secret_hint: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Create/rotate response: the item plus the full secret, shown exactly
/// once. Any subsequent read returns only `secretHint`.
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebhookSubscriptionItemWithSecret {
    pub id: Uuid,
    pub realm_id: String,
    pub endpoint_url: String,
    pub event_categories: Vec<String>,
    pub status: String,
    pub consecutive_failures: i32,
    pub last_error: Option<String>,
    pub paused_at: Option<String>,
    pub secret_hint: String,
    pub secret: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_page_size")]
    pub page_size: i64,
}

fn default_page() -> i64 {
    0
}

fn default_page_size() -> i64 {
    20
}

fn to_item(subscription: &WebhookSubscription) -> WebhookSubscriptionItem {
    WebhookSubscriptionItem {
        id: subscription.id,
        realm_id: subscription.realm_id.clone(),
        endpoint_url: subscription.endpoint_url.clone(),
        event_categories: subscription
            .event_categories
            .iter()
            .map(|c| c.as_str().to_string())
            .collect(),
        status: subscription.status.as_str().to_string(),
        consecutive_failures: subscription.consecutive_failures,
        last_error: subscription.last_error.clone(),
        paused_at: subscription.paused_at.map(|dt| dt.to_rfc3339()),
        secret_hint: subscription.secret_hint(),
        created_at: subscription.created_at.to_rfc3339(),
        updated_at: subscription.updated_at.to_rfc3339(),
    }
}

fn to_item_with_secret(
    subscription: &WebhookSubscription,
    secret: &str,
) -> WebhookSubscriptionItemWithSecret {
    WebhookSubscriptionItemWithSecret {
        id: subscription.id,
        realm_id: subscription.realm_id.clone(),
        endpoint_url: subscription.endpoint_url.clone(),
        event_categories: subscription
            .event_categories
            .iter()
            .map(|c| c.as_str().to_string())
            .collect(),
        status: subscription.status.as_str().to_string(),
        consecutive_failures: subscription.consecutive_failures,
        last_error: subscription.last_error.clone(),
        paused_at: subscription.paused_at.map(|dt| dt.to_rfc3339()),
        secret_hint: subscription.secret_hint(),
        secret: secret.to_string(),
        created_at: subscription.created_at.to_rfc3339(),
        updated_at: subscription.updated_at.to_rfc3339(),
    }
}

fn map_error(e: WebhookOutboundError) -> ApiError {
    use axum::http::StatusCode;
    match e {
        WebhookOutboundError::InvalidEndpointUrl => ApiError::with_error_code(
            StatusCode::BAD_REQUEST,
            "invalid_endpoint_url",
            "callback endpoint must be a publicly reachable http(s) URL",
        ),
        WebhookOutboundError::InvalidEventCategories => ApiError::with_error_code(
            StatusCode::BAD_REQUEST,
            "invalid_event_categories",
            WebhookOutboundError::InvalidEventCategories.to_string(),
        ),
        WebhookOutboundError::DuplicateEndpointUrl => ApiError::with_error_code(
            StatusCode::CONFLICT,
            "duplicate_endpoint_url",
            WebhookOutboundError::DuplicateEndpointUrl.to_string(),
        ),
        WebhookOutboundError::SubscriptionLimitExceeded => ApiError::with_error_code(
            StatusCode::BAD_REQUEST,
            "subscription_limit_exceeded",
            WebhookOutboundError::SubscriptionLimitExceeded.to_string(),
        ),
        WebhookOutboundError::NotFound => ApiError::not_found("Webhook subscription not found"),
        WebhookOutboundError::Database(msg) => {
            tracing::error!(error = %msg, "webhook subscription database error");
            ApiError::internal("Internal server error")
        }
    }
}

/// Registration-time public-endpoint validation; the single 400 message
/// does not distinguish private ranges to avoid a probing surface.
async fn validated_endpoint_url(raw: &str) -> Result<url::Url, ApiError> {
    webhook_outbound::validate_public_endpoint(raw)
        .await
        .map_err(|_| {
            ApiError::with_error_code(
                axum::http::StatusCode::BAD_REQUEST,
                "invalid_endpoint_url",
                "callback endpoint must be a publicly reachable http(s) URL",
            )
        })
}

fn validated_categories(raw: &[String]) -> Result<Vec<OutboundEventCategory>, ApiError> {
    webhook_outbound::parse_categories(raw).map_err(|_| {
        ApiError::with_error_code(
            axum::http::StatusCode::BAD_REQUEST,
            "invalid_event_categories",
            WebhookOutboundError::InvalidEventCategories.to_string(),
        )
    })
}

/// Best-effort management audit (same pattern as the permission user-role
/// write audit).
async fn record_webhook_audit(
    state: &AppState,
    identity: &Identity,
    realm_id: &str,
    subscription_id: Uuid,
    action: AuditAction,
    result: AuditResult,
    details: serde_json::Value,
) {
    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.to_string(),
            category: AuditCategory::RealmManagement,
            action,
            actor_id: identity.user_id().to_string(),
            actor_type: Some(ActorType::Admin),
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::Realm,
            target_id: subscription_id.to_string(),
            target_name: None,
            result,
            details: Some(details),
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %e, "Failed to record webhook subscription audit event");
    }
}

/// List webhook subscriptions for the admin's realm
#[utoipa::path(
    get,
    path = "/api/webhooks",
    tag = "webhooks",
    operation_id = "list_webhook_subscriptions",
    params(
        ("page" = Option<i64>, Query, description = "Page number (0-based, default 0)"),
        ("pageSize" = Option<i64>, Query, description = "Page size (default 20, max 100)"),
    ),
    responses(
        (status = 200, description = "Subscription list", body = PageResponse<WebhookSubscriptionItem>),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn list_webhook_subscriptions(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Query(query): Query<ListQuery>,
) -> Result<ApiResult<PageResponse<WebhookSubscriptionItem>>, ApiError> {
    let admin = AdminIdentity::require(identity, "webhook subscriptions")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "webhooks", "view").await?;

    let page_size = query.page_size.clamp(1, 100);
    // Negative or oversized pages resolve like the shared pagination
    // normalizer instead of producing a negative/overflowed SQL OFFSET.
    let offset = query.page.max(0).saturating_mul(page_size);
    let (subscriptions, total) = state
        .webhook_outbound
        .list_subscriptions(&realm_id, offset, page_size)
        .await
        .map_err(map_error)?;

    Ok(ApiResult::ok(PageResponse {
        items: subscriptions.iter().map(to_item).collect(),
        page: query.page,
        page_size,
        total,
    }))
}

/// Create a webhook subscription
///
/// Generates the signing secret and echoes it back exactly once.
#[utoipa::path(
    post,
    path = "/api/webhooks",
    tag = "webhooks",
    operation_id = "create_webhook_subscription",
    request_body = CreateWebhookSubscriptionRequest,
    responses(
        (status = 201, description = "Subscription created (secret shown once)", body = WebhookSubscriptionItemWithSecret),
        (status = 400, description = "Invalid endpoint URL or categories", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 409, description = "Duplicate endpoint URL in realm", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn create_webhook_subscription(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Valid(Json(payload)): Valid<Json<CreateWebhookSubscriptionRequest>>,
) -> Result<ApiResult<WebhookSubscriptionItemWithSecret>, ApiError> {
    let admin = AdminIdentity::require(identity.clone(), "webhook subscriptions")?;
    let realm_id = admin.realm_id().to_string();
    admin
        .require_permission(&state, "webhooks", "manage")
        .await?;

    let url = validated_endpoint_url(&payload.endpoint_url).await?;
    let categories = validated_categories(&payload.event_categories)?;

    let (subscription, secret) = state
        .webhook_outbound
        .create_subscription(&realm_id, url, categories)
        .await
        .map_err(map_error)?;

    record_webhook_audit(
        &state,
        &identity,
        &realm_id,
        subscription.id,
        AuditAction::WebhookSubscriptionCreate,
        AuditResult::Success,
        serde_json::json!({
            "endpointUrl": subscription.endpoint_url,
            "eventCategories": payload.event_categories,
        }),
    )
    .await;

    Ok(ApiResult::created(to_item_with_secret(
        &subscription,
        &secret,
    )))
}

/// Get a webhook subscription
#[utoipa::path(
    get,
    path = "/api/webhooks/{subscriptionId}",
    tag = "webhooks",
    operation_id = "get_webhook_subscription",
    params(
        ("subscriptionId" = Uuid, Path, description = "Subscription ID"),
    ),
    responses(
        (status = 200, description = "Subscription detail", body = WebhookSubscriptionItem),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Not found (unknown and cross-realm are indistinguishable)", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn get_webhook_subscription(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(subscription_id): Path<Uuid>,
) -> Result<ApiResult<WebhookSubscriptionItem>, ApiError> {
    let admin = AdminIdentity::require(identity, "webhook subscriptions")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "webhooks", "view").await?;

    let subscription = state
        .webhook_outbound
        .get_subscription(&realm_id, subscription_id)
        .await
        .map_err(map_error)?
        .ok_or_else(|| ApiError::not_found("Webhook subscription not found"))?;

    Ok(ApiResult::ok(to_item(&subscription)))
}

/// Replace a webhook subscription's URL and categories
#[utoipa::path(
    put,
    path = "/api/webhooks/{subscriptionId}",
    tag = "webhooks",
    operation_id = "update_webhook_subscription",
    params(
        ("subscriptionId" = Uuid, Path, description = "Subscription ID"),
    ),
    request_body = UpdateWebhookSubscriptionRequest,
    responses(
        (status = 200, description = "Subscription updated", body = WebhookSubscriptionItem),
        (status = 400, description = "Invalid endpoint URL or categories", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 409, description = "Duplicate endpoint URL in realm", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn update_webhook_subscription(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(subscription_id): Path<Uuid>,
    Valid(Json(payload)): Valid<Json<UpdateWebhookSubscriptionRequest>>,
) -> Result<ApiResult<WebhookSubscriptionItem>, ApiError> {
    let admin = AdminIdentity::require(identity.clone(), "webhook subscriptions")?;
    let realm_id = admin.realm_id().to_string();
    admin
        .require_permission(&state, "webhooks", "manage")
        .await?;

    let url = validated_endpoint_url(&payload.endpoint_url).await?;
    let categories = validated_categories(&payload.event_categories)?;

    let subscription = state
        .webhook_outbound
        .update_subscription(&realm_id, subscription_id, url, categories)
        .await
        .map_err(map_error)?;

    record_webhook_audit(
        &state,
        &identity,
        &realm_id,
        subscription.id,
        AuditAction::WebhookSubscriptionUpdate,
        AuditResult::Success,
        serde_json::json!({
            "endpointUrl": subscription.endpoint_url,
            "eventCategories": payload.event_categories,
        }),
    )
    .await;

    Ok(ApiResult::ok(to_item(&subscription)))
}

/// Delete a webhook subscription
#[utoipa::path(
    delete,
    path = "/api/webhooks/{subscriptionId}",
    tag = "webhooks",
    operation_id = "delete_webhook_subscription",
    params(
        ("subscriptionId" = Uuid, Path, description = "Subscription ID"),
    ),
    responses(
        (status = 204, description = "Subscription deleted (deliveries cascade)"),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn delete_webhook_subscription(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(subscription_id): Path<Uuid>,
) -> Result<ApiResult<()>, ApiError> {
    let admin = AdminIdentity::require(identity.clone(), "webhook subscriptions")?;
    let realm_id = admin.realm_id().to_string();
    admin
        .require_permission(&state, "webhooks", "manage")
        .await?;

    state
        .webhook_outbound
        .delete_subscription(&realm_id, subscription_id)
        .await
        .map_err(map_error)?;

    record_webhook_audit(
        &state,
        &identity,
        &realm_id,
        subscription_id,
        AuditAction::WebhookSubscriptionDelete,
        AuditResult::Success,
        serde_json::json!({"subscriptionId": subscription_id}),
    )
    .await;

    Ok(ApiResult::no_content())
}

/// Rotate a webhook subscription's signing secret
///
/// The new secret takes effect immediately for every subsequent delivery
/// attempt (including pending retries) and is echoed back exactly once.
#[utoipa::path(
    post,
    path = "/api/webhooks/{subscriptionId}/rotate-secret",
    tag = "webhooks",
    operation_id = "rotate_webhook_secret",
    params(
        ("subscriptionId" = Uuid, Path, description = "Subscription ID"),
    ),
    responses(
        (status = 200, description = "Secret rotated (new secret shown once)", body = WebhookSubscriptionItemWithSecret),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn rotate_webhook_secret(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(subscription_id): Path<Uuid>,
) -> Result<ApiResult<WebhookSubscriptionItemWithSecret>, ApiError> {
    let admin = AdminIdentity::require(identity.clone(), "webhook subscriptions")?;
    let realm_id = admin.realm_id().to_string();
    admin
        .require_permission(&state, "webhooks", "manage")
        .await?;

    let (subscription, secret) = state
        .webhook_outbound
        .rotate_secret(&realm_id, subscription_id)
        .await
        .map_err(map_error)?;

    // The audit payload never carries the secret — audit rows are readable
    // long after the one-time reveal is gone.
    record_webhook_audit(
        &state,
        &identity,
        &realm_id,
        subscription.id,
        AuditAction::WebhookSubscriptionRotateSecret,
        AuditResult::Success,
        serde_json::json!({"secretHint": subscription.secret_hint()}),
    )
    .await;

    Ok(ApiResult::ok(to_item_with_secret(&subscription, &secret)))
}

/// Enable / disable a subscription (and resume from auto-pause)
#[utoipa::path(
    post,
    path = "/api/webhooks/{subscriptionId}/status",
    tag = "webhooks",
    operation_id = "set_webhook_subscription_status",
    params(
        ("subscriptionId" = Uuid, Path, description = "Subscription ID"),
    ),
    request_body = SetWebhookSubscriptionStatusRequest,
    responses(
        (status = 200, description = "Status updated", body = WebhookSubscriptionItem),
        (status = 400, description = "Unknown status or `paused` target (system-only state)", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    )
)]
pub async fn set_webhook_subscription_status(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(subscription_id): Path<Uuid>,
    Valid(Json(payload)): Valid<Json<SetWebhookSubscriptionStatusRequest>>,
) -> Result<ApiResult<WebhookSubscriptionItem>, ApiError> {
    let admin = AdminIdentity::require(identity.clone(), "webhook subscriptions")?;
    let realm_id = admin.realm_id().to_string();
    admin
        .require_permission(&state, "webhooks", "manage")
        .await?;

    let target = WebhookStatusTarget::try_from_str(&payload.status).ok_or_else(|| {
        ApiError::with_error_code(
            axum::http::StatusCode::BAD_REQUEST,
            "invalid_status",
            "status must be 'active' or 'disabled' ('paused' is set by the system only)",
        )
    })?;

    let (previous_status, subscription) = state
        .webhook_outbound
        .set_status(&realm_id, subscription_id, target)
        .await
        .map_err(map_error)?;

    record_webhook_audit(
        &state,
        &identity,
        &realm_id,
        subscription.id,
        AuditAction::WebhookSubscriptionStatus,
        AuditResult::Success,
        serde_json::json!({"from": previous_status.as_str(), "to": subscription.status.as_str()}),
    )
    .await;

    Ok(ApiResult::ok(to_item(&subscription)))
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/",
            axum::routing::get(list_webhook_subscriptions).post(create_webhook_subscription),
        )
        .route(
            "/{subscriptionId}",
            axum::routing::get(get_webhook_subscription)
                .put(update_webhook_subscription)
                .delete(delete_webhook_subscription),
        )
        .route(
            "/{subscriptionId}/rotate-secret",
            axum::routing::post(rotate_webhook_secret),
        )
        .route(
            "/{subscriptionId}/status",
            axum::routing::post(set_webhook_subscription_status),
        )
}
