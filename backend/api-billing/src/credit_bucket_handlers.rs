//! Credit Bucket directory handlers (reads + writes/overview).
//!
//! Implements the directory endpoints over `PostgresBillingRepository`'s inherent
//! bucket directory methods. Permission gate: Realm Admin `points.manage`.
//! HTTP contracts follow the crate's camelCase convention. Each management
//! response surfaces the distribution rules referencing the bucket
//! (`ruleReferences`); registration routing is configured through those rules.

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::DistributionRuleReferenceResponse;
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::billing::{
    BucketByCreditType, CreateCreditBucketInput, CreditBucket, CreditBucketDetail,
    CreditBucketError, CreditBucketListItem, CreditBucketOverviewRow, UpdateCreditBucketInput,
};
use herald_core::domain::common::entities::app_errors::CoreError;

/// `bucket_key` format: lowercase ASCII letters/digits/hyphens, 1..=64 chars
/// (mirrors DB CHECK constraint `chk_credit_buckets_key`).
const BUCKET_KEY_MAX_LEN: usize = 64;

// ===== Named error bodies =====
//
// Surfaced as typed OpenAPI schemas so `@hey-api/openapi-ts` can generate
// strongly-typed clients.

/// 400 `bucket_key_duplicate` body: the requested `bucketKey`
/// already exists in this realm (`uq_credit_buckets_realm_key`).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct BucketKeyDuplicateErrorBody {
    pub code: &'static str,
    pub message: &'static str,
}

/// 409 `bucket_in_use` body.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BucketInUseErrorBody {
    pub code: &'static str,
    pub active_subscriptions: i64,
    pub holders_with_balance: i64,
}

fn validate_bucket_key(key: &str) -> Result<(), ApiError> {
    if key.is_empty() || key.len() > BUCKET_KEY_MAX_LEN {
        return Err(ApiError::bad_request(
            "bucketKey must be 1-64 characters".to_string(),
        ));
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ApiError::bad_request(
            "bucketKey must match ^[a-z0-9-]{1,64}$".to_string(),
        ));
    }
    Ok(())
}

/// Translate a `CreditBucketError` into the error contract.
///
/// Structured variants produce 400/409 with the exact body shapes; passthrough
/// `Other(CoreError)` keeps the wrapped error's status (404 for NotFound, 500
/// for DatabaseError, etc.). Must NOT flatten structured variants through
/// `From<CreditBucketError> for CoreError`.
fn map_bucket_error(err: CreditBucketError) -> ApiError {
    match err {
        CreditBucketError::BucketKeyDuplicate { realm_id: _ } => {
            ApiError::bad_request_json(BucketKeyDuplicateErrorBody {
                code: "bucket_key_duplicate",
                message: "bucketKey already exists in this realm",
            })
        }
        CreditBucketError::BucketInUse {
            bucket_id: _,
            active_subscriptions,
            holders_with_balance,
        } => ApiError::conflict_json(BucketInUseErrorBody {
            code: "bucket_in_use",
            active_subscriptions,
            holders_with_balance,
        }),
        CreditBucketError::Other(core) => ApiError::from(core),
    }
}

/// Best-effort fallback when a `CoreError` is surfaced directly (overview path
/// does not use `CreditBucketError`). Mirrors `ApiError::from(CoreError)` but
/// inlined here so the overview handler stays explicit about its mapping.
fn map_core_error(err: CoreError) -> ApiError {
    ApiError::from(err)
}

/// Permission check helper for Credit Bucket directory operations and sibling
/// `points.manage`-gated writes (e.g. entitlement-mapping ownership writes).
///
/// Mirrors `handlers::require_billing_permission` but gated on `points.manage`
/// (bucket directory / ownership / grant management requires Realm
/// Admin `points.manage`). Performs realm boundary + business permission check.
pub(crate) async fn require_points_manage_permission(
    state: &AppState,
    identity: &Identity,
    realm_id: &str,
) -> Result<(), ApiError> {
    let admin = AdminIdentity::require(identity.clone(), realm_id, "credit bucket management")?;
    admin.require_permission(state, "points", "manage").await
}

// ===== Response Types =====

/// Reference to a Client App covered by a Credit Bucket (detail view).
///
/// Only carries an id today; the frontend resolves display metadata via the
/// existing client-app directory. Kept as a struct (not a bare `Uuid`) so the
/// SDK contract is forward-compatible with future enrichment.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientAppRef {
    pub id: Uuid,
}

/// List-item shape of a Credit Bucket (`Bucket[]`).
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BucketResponse {
    pub id: Uuid,
    pub bucket_key: String,
    pub name: String,
    pub display_order: i32,
    pub enabled: bool,
    pub covered_client_app_count: i64,
    /// Number of distribution rules currently targeting this bucket (across
    /// both `entitlement_mapping` and `realm_registration` owners).
    pub rule_reference_count: i64,
}

/// Detail shape of a Credit Bucket (`BucketDetail`).
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BucketDetailResponse {
    pub id: Uuid,
    pub bucket_key: String,
    pub name: String,
    /// Optional human-readable description (echoed from the stored bucket;
    /// POST/PUT already accept this field).
    pub description: Option<String>,
    pub display_order: i32,
    pub enabled: bool,
    pub client_apps: Vec<ClientAppRef>,
    /// Distribution rules referencing this bucket. Aggregates
    /// both owners; empty when no rule targets this bucket.
    pub rule_references: Vec<DistributionRuleReferenceResponse>,
}

fn rule_ref_to_response(
    r: herald_core::domain::points::DistributionRuleReference,
) -> DistributionRuleReferenceResponse {
    DistributionRuleReferenceResponse {
        rule_id: r.rule_id,
        owner_type: r.owner_type,
        entitlement_mapping_id: r.entitlement_mapping_id,
        trigger_sources: r.trigger_sources,
        enabled: r.enabled,
    }
}

fn bucket_to_response(item: CreditBucketListItem) -> BucketResponse {
    let b = item.bucket;
    BucketResponse {
        id: b.id,
        bucket_key: b.bucket_key,
        name: b.name,
        display_order: b.display_order,
        enabled: b.enabled,
        covered_client_app_count: item.covered_client_app_count,
        rule_reference_count: item.rule_reference_count,
    }
}

fn bucket_detail_to_response(detail: CreditBucketDetail) -> BucketDetailResponse {
    let CreditBucket {
        id,
        bucket_key,
        name,
        description,
        display_order,
        enabled,
        ..
    } = detail.bucket;
    BucketDetailResponse {
        id,
        bucket_key,
        name,
        description,
        display_order,
        enabled,
        client_apps: detail
            .client_app_ids
            .into_iter()
            .map(|id| ClientAppRef { id })
            .collect(),
        rule_references: detail
            .rule_references
            .into_iter()
            .map(rule_ref_to_response)
            .collect(),
    }
}

// ===== Handlers =====

/// List all Credit Buckets for a realm.
#[utoipa::path(
    get,
    path = "/api/realms/{realmId}/billing/credit-buckets",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Credit buckets listed successfully", body = [BucketResponse]),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_credit_buckets_handler(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
) -> Result<Json<Vec<BucketResponse>>, ApiError> {
    tracing::info!("Listing credit buckets for realm: {}", realm_id);

    require_points_manage_permission(&state, &identity, &realm_id).await?;

    let items = state
        .billing_repository
        .list_credit_buckets(&realm_id)
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                error = %e,
                "Failed to list credit buckets"
            );
            ApiError::internal("Failed to list credit buckets".to_string())
        })?;

    let response: Vec<BucketResponse> = items.into_iter().map(bucket_to_response).collect();
    Ok(Json(response))
}

/// Get a single Credit Bucket with coverage set and attached mappings.
#[utoipa::path(
    get,
    path = "/api/realms/{realmId}/billing/credit-buckets/{bucketId}",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("bucketId" = Uuid, Path, description = "Credit Bucket ID")
    ),
    responses(
        (status = 200, description = "Credit bucket found", body = BucketDetailResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Credit bucket not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_credit_bucket_handler(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, bucket_id)): Path<(String, Uuid)>,
) -> Result<Json<BucketDetailResponse>, ApiError> {
    tracing::info!(
        "Getting credit bucket {} for realm: {}",
        bucket_id,
        realm_id
    );

    require_points_manage_permission(&state, &identity, &realm_id).await?;

    let detail = state
        .billing_repository
        .get_credit_bucket(&realm_id, bucket_id)
        .await
        .map_err(|e| {
            tracing::error!(
                bucket_id = %bucket_id,
                realm_id = %realm_id,
                error = %e,
                "Failed to get credit bucket"
            );
            ApiError::internal("Failed to get credit bucket".to_string())
        })?
        .ok_or_else(|| ApiError::not_found("Credit bucket not found"))?;

    Ok(Json(bucket_detail_to_response(detail)))
}

// ===== Request Types =====

/// Request body for creating a Credit Bucket.
///
/// `client_app_ids` (coverage set) MUST be non-empty — enforced fail-loud at the
/// handler layer (400). Registration routing is configured via distribution
/// rules rather than a bucket-level switch.
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCreditBucketRequest {
    pub bucket_key: String,
    pub name: String,
    pub description: Option<String>,
    pub display_order: Option<i32>,
    pub enabled: Option<bool>,
    /// Coverage set — at least one entry required.
    pub client_app_ids: Vec<Uuid>,
}

/// Request body for updating a Credit Bucket (PUT).
///
/// All provided fields fully replace the stored state (coverage set is
/// replaced, not merged). Clearing the coverage set (`client_app_ids` empty) is
/// rejected with 400. Registration routing remains rule-owned.
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCreditBucketRequest {
    pub name: String,
    pub description: Option<String>,
    pub display_order: Option<i32>,
    pub enabled: Option<bool>,
    /// Replacement coverage set — at least one entry required.
    pub client_app_ids: Vec<Uuid>,
}

// ===== Overview Response Types =====

/// Per-credit-type balance totals surfaced in the overview matrix.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ByCreditTypeResponse {
    pub topup: i64,
    pub subscription: i64,
    pub registration: i64,
    pub free_periodic: i64,
    pub granted: i64,
}

impl From<BucketByCreditType> for ByCreditTypeResponse {
    fn from(b: BucketByCreditType) -> Self {
        Self {
            topup: b.topup,
            subscription: b.subscription,
            registration: b.registration,
            free_periodic: b.free_periodic,
            granted: b.granted,
        }
    }
}

/// One row of the overview matrix (per bucket × credit type).
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OverviewRowResponse {
    pub bucket_id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub by_credit_type: ByCreditTypeResponse,
    pub bucket_total: i64,
}

/// Overview response: rows per bucket + a SEPARATE `grandTotal` field
/// (grandTotal is NOT appended as an extra row).
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BucketOverviewResponse {
    pub rows: Vec<OverviewRowResponse>,
    pub grand_total: ByCreditTypeResponse,
}

fn overview_row_to_response(row: CreditBucketOverviewRow) -> OverviewRowResponse {
    OverviewRowResponse {
        bucket_id: row.bucket_id,
        name: row.name,
        enabled: row.enabled,
        by_credit_type: row.by_credit_type.into(),
        bucket_total: row.bucket_total,
    }
}

// ===== Write handlers =====

/// Create a Credit Bucket.
#[utoipa::path(
    post,
    path = "/api/realms/{realmId}/billing/credit-buckets",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = CreateCreditBucketRequest,
    responses(
        (status = 201, description = "Credit bucket created", body = BucketDetailResponse),
        (status = 400, description = "Bad request - invalid bucketKey / empty coverage set", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - points.manage required", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 409, description = "bucket_in_use - bucket still referenced by balances/subscriptions", body = BucketInUseErrorBody),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_credit_bucket_handler(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Json(request): Json<CreateCreditBucketRequest>,
) -> Result<(StatusCode, Json<BucketDetailResponse>), ApiError> {
    tracing::info!("Creating credit bucket for realm: {}", realm_id);

    require_points_manage_permission(&state, &identity, &realm_id).await?;

    // Fail-loud request validation.
    validate_bucket_key(&request.bucket_key)?;
    if request.client_app_ids.is_empty() {
        return Err(ApiError::bad_request(
            "clientAppIds must contain at least one entry".to_string(),
        ));
    }

    let input = CreateCreditBucketInput {
        realm_id: realm_id.clone(),
        bucket_key: request.bucket_key,
        name: request.name,
        description: request.description,
        display_order: request.display_order.unwrap_or(0),
        enabled: request.enabled.unwrap_or(true),
        client_app_ids: request.client_app_ids,
    };

    let detail = state
        .billing_repository
        .create_credit_bucket(input)
        .await
        .map_err(map_bucket_error)?;

    Ok((StatusCode::CREATED, Json(bucket_detail_to_response(detail))))
}

/// Update a Credit Bucket (PUT). Coverage set is fully replaced.
#[utoipa::path(
    put,
    path = "/api/realms/{realmId}/billing/credit-buckets/{bucketId}",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("bucketId" = Uuid, Path, description = "Credit Bucket ID")
    ),
    request_body = UpdateCreditBucketRequest,
    responses(
        (status = 200, description = "Credit bucket updated", body = BucketDetailResponse),
        (status = 400, description = "Bad request - empty coverage set (clientAppIds must contain at least one entry)", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - points.manage required", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Credit bucket not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 409, description = "bucket_in_use - bucket still referenced by balances/subscriptions", body = BucketInUseErrorBody),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_credit_bucket_handler(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, bucket_id)): Path<(String, Uuid)>,
    Json(request): Json<UpdateCreditBucketRequest>,
) -> Result<Json<BucketDetailResponse>, ApiError> {
    tracing::info!(
        "Updating credit bucket {} for realm: {}",
        bucket_id,
        realm_id
    );

    require_points_manage_permission(&state, &identity, &realm_id).await?;

    // Clearing the coverage set is rejected.
    if request.client_app_ids.is_empty() {
        return Err(ApiError::bad_request(
            "clientAppIds must contain at least one entry".to_string(),
        ));
    }

    let input = UpdateCreditBucketInput {
        realm_id: realm_id.clone(),
        bucket_id,
        name: request.name,
        description: request.description,
        display_order: request.display_order.unwrap_or(0),
        enabled: request.enabled.unwrap_or(true),
        client_app_ids: request.client_app_ids,
    };

    let detail = state
        .billing_repository
        .update_credit_bucket(input)
        .await
        .map_err(map_bucket_error)?;

    Ok(Json(bucket_detail_to_response(detail)))
}

/// Delete a Credit Bucket (DELETE).
///
/// 204 on success; 409 `bucket_in_use` with `{ code, activeSubscriptions,
/// holdersWithBalance }` when in-flight subscriptions or residual balances exist.
#[utoipa::path(
    delete,
    path = "/api/realms/{realmId}/billing/credit-buckets/{bucketId}",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        ("bucketId" = Uuid, Path, description = "Credit Bucket ID")
    ),
    responses(
        (status = 204, description = "Credit bucket deleted"),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - points.manage required", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Credit bucket not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 409, description = "bucket_in_use - in-flight subscriptions or residual balances", body = BucketInUseErrorBody),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_credit_bucket_handler(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((realm_id, bucket_id)): Path<(String, Uuid)>,
) -> Result<StatusCode, ApiError> {
    tracing::info!(
        "Deleting credit bucket {} for realm: {}",
        bucket_id,
        realm_id
    );

    require_points_manage_permission(&state, &identity, &realm_id).await?;

    state
        .billing_repository
        .delete_credit_bucket(&realm_id, bucket_id)
        .await
        .map_err(map_bucket_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// Get the bucket overview matrix.
///
/// Returns `{ rows: OverviewRow[], grandTotal: ByCreditType }` — `grandTotal`
/// is a SEPARATE top-level field, not appended to rows.
#[utoipa::path(
    get,
    path = "/api/realms/{realmId}/billing/credit-buckets/overview",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    responses(
        (status = 200, description = "Overview matrix", body = BucketOverviewResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden - points.manage required", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_bucket_overview_handler(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
) -> Result<Json<BucketOverviewResponse>, ApiError> {
    tracing::info!("Getting bucket overview for realm: {}", realm_id);

    require_points_manage_permission(&state, &identity, &realm_id).await?;

    let overview = state
        .billing_repository
        .list_bucket_overview(&realm_id)
        .await
        .map_err(map_core_error)?;

    let response = BucketOverviewResponse {
        rows: overview
            .rows
            .into_iter()
            .map(overview_row_to_response)
            .collect(),
        grand_total: overview.grand_total.into(),
    };

    Ok(Json(response))
}
