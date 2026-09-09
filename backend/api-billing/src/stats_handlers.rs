use axum::extract::{Extension, Path, Query, State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::billing_statistics::{
    BillingStatisticsRepository, PaymentStats, StatsWindow,
};
use herald_core::infrastructure::billing_statistics::PostgresBillingStatisticsRepository;

use crate::handlers::require_billing_permission;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct PaymentStatsQuery {
    /// Statistics window in days; only 7 or 30 are accepted. Defaults to 7.
    pub days: Option<i32>,
}

/// An amount total for one currency (smallest currency unit).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyAmountResponse {
    pub currency: String,
    pub amount: i64,
}

/// Per-provider payment totals within the window.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPaymentStatsResponse {
    pub payment_provider: String,
    pub succeeded_count: i64,
    pub failed_count: i64,
    pub amounts_by_currency: Vec<CurrencyAmountResponse>,
}

/// One day of the payment attempt trend (zero-filled, ascending).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaymentTrendPointResponse {
    pub date: String,
    pub succeeded_count: i64,
    pub failed_count: i64,
}

/// Payment statistics over a fixed window. The success rate is derived on the
/// client from succeeded / (succeeded + failed); amounts are grouped per
/// currency and never summed across currencies.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaymentStatsResponse {
    pub window_days: i32,
    pub succeeded_count: i64,
    pub failed_count: i64,
    pub amounts_by_currency: Vec<CurrencyAmountResponse>,
    pub providers: Vec<ProviderPaymentStatsResponse>,
    pub payment_trend: Vec<PaymentTrendPointResponse>,
}

impl From<PaymentStats> for PaymentStatsResponse {
    fn from(stats: PaymentStats) -> Self {
        PaymentStatsResponse {
            window_days: stats.window_days,
            succeeded_count: stats.succeeded_count,
            failed_count: stats.failed_count,
            amounts_by_currency: stats
                .amounts_by_currency
                .into_iter()
                .map(|c| CurrencyAmountResponse {
                    currency: c.currency,
                    amount: c.amount,
                })
                .collect(),
            providers: stats
                .providers
                .into_iter()
                .map(|p| ProviderPaymentStatsResponse {
                    payment_provider: p.payment_provider,
                    succeeded_count: p.succeeded_count,
                    failed_count: p.failed_count,
                    amounts_by_currency: p
                        .amounts_by_currency
                        .into_iter()
                        .map(|c| CurrencyAmountResponse {
                            currency: c.currency,
                            amount: c.amount,
                        })
                        .collect(),
                })
                .collect(),
            payment_trend: stats
                .payment_trend
                .into_iter()
                .map(|p| PaymentTrendPointResponse {
                    date: p.date,
                    succeeded_count: p.succeeded_count,
                    failed_count: p.failed_count,
                })
                .collect(),
        }
    }
}

/// Payment statistics for the admin statistics page.
///
/// Counts only finalized attempts (Succeeded/Failed) attributed to their
/// initiation day (`created_at`). Requires the `billing.view` permission.
#[utoipa::path(
    get,
    path = "/api/bill/{realmId}/stats/payments",
    tag = "billing",
    params(
        ("realmId" = String, Path, description = "Realm ID"),
        PaymentStatsQuery,
    ),
    responses(
        (status = 200, description = "Payment statistics for the requested window", body = PaymentStatsResponse),
        (status = 400, description = "days must be 7 or 30", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 401, description = "Unauthorized", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 403, description = "Forbidden", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
        (status = 500, description = "Internal server error", body = herald_api_base::application::http::server::api_entities::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_payment_stats(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(realm_id): Path<String>,
    Query(query): Query<PaymentStatsQuery>,
) -> Result<ApiResult<PaymentStatsResponse>, ApiError> {
    let window = StatsWindow::from_days(query.days)
        .ok_or_else(|| ApiError::bad_request("days must be 7 or 30"))?;

    require_billing_permission(&state, &identity, &realm_id, "view").await?;

    let repo = PostgresBillingStatisticsRepository::new(state.pool.clone());
    let stats = repo
        .get_payment_stats(&realm_id, window)
        .await
        .map_err(|e| {
            tracing::error!(realm_id = %realm_id, error = %e, "Failed to fetch payment statistics");
            ApiError::internal("Failed to fetch payment statistics")
        })?;

    Ok(ApiResult::ok(PaymentStatsResponse::from(stats)))
}
