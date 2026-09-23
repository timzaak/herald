// Points Grant Handler (Admin)

use axum::{
    Json,
    extract::{Extension, State},
};
use serde::Serialize;
use uuid::Uuid;

use crate::types::{GrantPointsRequest, GrantPointsResponse};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ErrorResponse};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::points::dtos::GrantPointsInput;
use herald_core::domain::points::entities::CreditSourceType;

/// Structured 400 `grant_bucket_required` body.
/// Mirrors the api-ext grant handler contract so consumers see one shape.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GrantBucketRequiredBody {
    code: &'static str,
    message: &'static str,
}

fn grant_bucket_required_error() -> ApiError {
    ApiError::bad_request_json(GrantBucketRequiredBody {
        code: "grant_bucket_required",
        message: "Points grant requires an explicit target bucket",
    })
}

/// Grant points to a user (admin)
#[utoipa::path(
    post,
    path = "/api/points/grant",
        request_body = GrantPointsRequest,
    responses(
        (status = 200, description = "Points granted successfully", body = GrantPointsResponse),
        (status = 400, description = "Bad request (invalid amount, invalid user ID, empty reason, missing/invalid bucketId → code=grant_bucket_required)", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "points"
)]
#[tracing::instrument(
    // Governance: identity carries user_id/realm_id; request
    // body carries the target user_id; realm_id is conservatively skipped.
    // Only the low-cardinality operation type is recorded.
    skip_all,
    fields(db.operation = "grant_points")
)]
pub async fn grant_points(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Json(request): Json<GrantPointsRequest>,
) -> Result<Json<GrantPointsResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, "points grant")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "points", "manage").await?;

    // Parse user_id as UUID
    let user_id = request
        .user_id
        .parse::<Uuid>()
        .map_err(|_| ApiError::bad_request("Invalid user ID"))?;

    // bucketId is REQUIRED: every grant must target an
    // explicit Credit Bucket. Missing or malformed → 400 grant_bucket_required.
    let bucket_id = match request.bucket_id.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => match s.parse::<Uuid>() {
            Ok(parsed) => parsed,
            Err(_) => return Err(grant_bucket_required_error()),
        },
        _ => return Err(grant_bucket_required_error()),
    };

    // Validate amount > 0
    if request.amount <= 0 {
        return Err(ApiError::bad_request("Amount must be greater than 0"));
    }

    // Validate reason non-empty
    if request.reason.trim().is_empty() {
        return Err(ApiError::bad_request("Reason must not be empty"));
    }

    // Validate validity_days is None or > 0
    if let Some(days) = request.validity_days
        && days <= 0
    {
        return Err(ApiError::bad_request(
            "Validity days must be greater than 0",
        ));
    }

    let input = GrantPointsInput {
        user_id,
        bucket_id,
        amount: request.amount,
        reason: request.reason,
        validity_days: request.validity_days,
        source_type: CreditSourceType::AdminGrant,
        source_id: admin.user_id_string(),
    };

    // Echo the resolved target bucket back to the caller.
    // `GrantPointsOutput` does not carry `bucket_id`, so the input
    // value is reused — it is the authoritative routing target.
    let response_bucket_id = input.bucket_id;

    match state
        .points_service
        .grant_points(admin.identity().clone(), &realm_id, input)
        .await
    {
        Ok(output) => Ok(Json(GrantPointsResponse {
            transaction_id: output.transaction_id,
            user_id: output.user_id,
            bucket_id: response_bucket_id,
            amount: output.amount,
            granted_balance: output.granted_balance,
            total_balance: output.total_balance,
            expires_at: output.expires_at.map(|dt| dt.to_rfc3339()),
        })),
        Err(e) => Err(ApiError::from(e)),
    }
}
