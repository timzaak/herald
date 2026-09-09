use axum::extract::{Extension, Path, Query, State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::billing_statistics::{
    BillingStatisticsRepository, PointsConsumptionStats, StatsWindow,
};
use herald_core::infrastructure::billing_statistics::PostgresBillingStatisticsRepository;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct PointsConsumptionStatsQuery {
    /// Statistics window in days; only 7 or 30 are accepted. Defaults to 7.
    pub days: Option<i32>,
}

/// Consumption total for one credit bucket.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BucketConsumptionResponse {
    pub bucket_id: uuid::Uuid,
    pub bucket_key: String,
    pub bucket_name: String,
    pub consumed_points: i64,
}

/// One day of the consumption trend (zero-filled, ascending).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsumptionTrendPointResponse {
    pub date: String,
    pub consumed_points: i64,
}

/// Points consumption statistics over a fixed window, measured from
/// `consume` transactions only — revoked/reclaimed points never offset
/// these figures.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PointsConsumptionStatsResponse {
    pub window_days: i32,
    pub total_consumed_points: i64,
    pub consuming_users: i64,
    pub buckets: Vec<BucketConsumptionResponse>,
    pub consumption_trend: Vec<ConsumptionTrendPointResponse>,
}

impl From<PointsConsumptionStats> for PointsConsumptionStatsResponse {
    fn from(stats: PointsConsumptionStats) -> Self {
        PointsConsumptionStatsResponse {
            window_days: stats.window_days,
            total_consumed_points: stats.total_consumed_points,
            consuming_users: stats.consuming_users,
            buckets: stats
                .buckets
                .into_iter()
                .map(|b| BucketConsumptionResponse {
                    bucket_id: b.bucket_id,
                    bucket_key: b.bucket_key,
                    bucket_name: b.bucket_name,
                    consumed_points: b.consumed_points,
                })
                .collect(),
            consumption_trend: stats
                .consumption_trend
                .into_iter()
                .map(|p| ConsumptionTrendPointResponse {
                    date: p.date,
                    consumed_points: p.consumed_points,
                })
                .collect(),
        }
    }
}

/// Points consumption statistics for the admin statistics page.
///
/// Requires the `points.view` permission. Consumption is attributed to the
/// transaction's creation day.
#[utoipa::path(
    get,
    path = "/api/points/{realmId}/stats/consumption",
    tag = "points",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        PointsConsumptionStatsQuery,
    ),
    responses(
        (status = 200, description = "Points consumption statistics for the requested window", body = PointsConsumptionStatsResponse),
        (status = 400, description = "days must be 7 or 30", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_points_consumption_stats(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Query(query): Query<PointsConsumptionStatsQuery>,
) -> Result<ApiResult<PointsConsumptionStatsResponse>, ApiError> {
    let window = StatsWindow::from_days(query.days)
        .ok_or_else(|| ApiError::bad_request("days must be 7 or 30"))?;

    let admin = AdminIdentity::require(identity, &realm_id, "statistics")?;
    admin.require_permission(&state, "points", "view").await?;

    let repo = PostgresBillingStatisticsRepository::new(state.pool.clone());
    let stats = repo
        .get_points_consumption_stats(&realm_id, window)
        .await
        .map_err(|e| {
            tracing::error!(realm_id = %realm_id, error = %e, "Failed to fetch points consumption statistics");
            ApiError::internal("Failed to fetch points consumption statistics")
        })?;

    Ok(ApiResult::ok(PointsConsumptionStatsResponse::from(stats)))
}
