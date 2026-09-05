use axum::Router;
use axum::extract::{Extension, Path, State};
use axum::routing::get;
use herald_core::domain::authentication::Identity;
use herald_core::domain::dashboard::DashboardRepository;
use herald_core::infrastructure::dashboard::PostgresDashboardRepository;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;

/// User statistics response DTO.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserStatsResponse {
    pub total_users: i64,
    pub new_users: i64,
    pub active_users: i64,
}

/// A single data point in the authentication trend response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthTrendPointResponse {
    pub date: String,
    pub success_count: i64,
    pub failure_count: i64,
}

/// Aggregated dashboard statistics response DTO.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStatsResponse {
    pub user_stats: UserStatsResponse,
    pub auth_trend: Vec<AuthTrendPointResponse>,
}

/// Get dashboard statistics for a realm
#[utoipa::path(
    get,
    path = "/api/dashboard/{realmId}/stats",
    tag = "dashboard",
    params(("realmId" = String, Path, description = "Realm ID")),
    responses(
        (status = 200, description = "Dashboard statistics", body = DashboardStatsResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 404, description = "Realm not found", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_dashboard_stats(
    State(state): State<AppState>,
    Path(realm_id): Path<String>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<DashboardStatsResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "dashboard statistics")?;
    admin
        .require_permission(&state, "dashboard", "view")
        .await?;

    let realm_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM realm WHERE id = $1)")
        .bind(&realm_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, %realm_id, "Failed to verify dashboard realm");
            ApiError::internal("Failed to verify realm")
        })?;
    if !realm_exists {
        return Err(ApiError::not_found("Realm not found"));
    }

    let repo = PostgresDashboardRepository::new(state.db.clone(), state.pool.clone());
    let stats = repo.get_stats(&realm_id).await.map_err(|e| {
        tracing::error!("Failed to fetch dashboard statistics: {e}");
        ApiError::internal("Failed to fetch dashboard statistics")
    })?;

    let response = DashboardStatsResponse {
        user_stats: UserStatsResponse {
            total_users: stats.user_stats.total_users,
            new_users: stats.user_stats.new_users,
            active_users: stats.user_stats.active_users,
        },
        auth_trend: stats
            .auth_trend
            .into_iter()
            .map(|p| AuthTrendPointResponse {
                date: p.date,
                success_count: p.success_count,
                failure_count: p.failure_count,
            })
            .collect(),
    };

    Ok(ApiResult::ok(response))
}

/// Dashboard router with stats endpoint
pub fn dashboard_router() -> Router<AppState> {
    Router::new().route("/stats", get(get_dashboard_stats))
}
