use std::future::Future;

use crate::common::entities::app_errors::CoreError;

use super::entities::{PaymentStats, PointsConsumptionStats, StatsWindow};

/// Read-only aggregation port for the admin billing statistics page.
///
/// Deliberately separate from `PaymentAttemptRepository` and the points
/// repository: statistics span payment attempts, points transactions and
/// credit buckets without belonging to either write-heavy aggregate. Handlers
/// construct the postgres implementation per request.
#[cfg_attr(test, mockall::automock)]
pub trait BillingStatisticsRepository: Send + Sync {
    fn get_payment_stats(
        &self,
        realm_id: &str,
        window: StatsWindow,
    ) -> impl Future<Output = Result<PaymentStats, CoreError>> + Send;

    fn get_points_consumption_stats(
        &self,
        realm_id: &str,
        window: StatsWindow,
    ) -> impl Future<Output = Result<PointsConsumptionStats, CoreError>> + Send;
}
