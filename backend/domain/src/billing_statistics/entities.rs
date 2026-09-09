use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Fixed statistics window covering the last N calendar days including today.
///
/// The only supported windows are 7 and 30 days; anything else is rejected by
/// `from_days` so callers can never widen the window beyond the two published
/// dashboard presets. The calendar-day boundaries themselves live in the
/// repository SQL (`CURRENT_DATE` in the database session timezone, including
/// the trend's `generate_series` axis); this type only carries the day count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsWindow {
    Last7Days,
    Last30Days,
}

impl StatsWindow {
    /// Map the `days` query parameter to a window. `None` defaults to 7 days;
    /// any other value maps to `None` (invalid).
    pub fn from_days(days: Option<i32>) -> Option<Self> {
        match days {
            None | Some(7) => Some(Self::Last7Days),
            Some(30) => Some(Self::Last30Days),
            _ => None,
        }
    }

    /// Number of calendar days covered by the window (including today).
    pub fn days(self) -> i32 {
        match self {
            Self::Last7Days => 7,
            Self::Last30Days => 30,
        }
    }
}

/// An amount total for a single currency (smallest currency unit, e.g. cents).
///
/// Amounts are never summed across currencies anywhere in the statistics
/// contract, so there is deliberately no cross-currency total field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyAmount {
    pub currency: String,
    pub amount: i64,
}

/// One day of the payment attempt trend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentTrendPoint {
    pub date: String,
    pub succeeded_count: i64,
    pub failed_count: i64,
}

/// Per-provider payment totals. `amounts_by_currency` only includes succeeded
/// attempts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPaymentStats {
    pub payment_provider: String,
    pub succeeded_count: i64,
    pub failed_count: i64,
    pub amounts_by_currency: Vec<CurrencyAmount>,
}

/// Payment statistics for a realm over a fixed window.
///
/// Only finalized attempts (Succeeded/Failed) are counted. The success rate is
/// derived by the frontend from `succeeded_count / (succeeded_count +
/// failed_count)`; the backend intentionally does not return a ratio.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentStats {
    pub window_days: i32,
    pub succeeded_count: i64,
    pub failed_count: i64,
    pub amounts_by_currency: Vec<CurrencyAmount>,
    pub providers: Vec<ProviderPaymentStats>,
    pub payment_trend: Vec<PaymentTrendPoint>,
}

/// Consumption total for one credit bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketConsumption {
    pub bucket_id: Uuid,
    pub bucket_key: String,
    pub bucket_name: String,
    pub consumed_points: i64,
}

/// One day of the points consumption trend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumptionTrendPoint {
    pub date: String,
    pub consumed_points: i64,
}

/// Points consumption statistics for a realm over a fixed window.
///
/// Consumption is measured from `type = 'consume'` transactions only; revocation
/// types (refund/expire/cancel revoke) are not part of the filter set, so
/// reclaimed points never reduce or negatively offset any figure here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointsConsumptionStats {
    pub window_days: i32,
    pub total_consumed_points: i64,
    pub consuming_users: i64,
    pub buckets: Vec<BucketConsumption>,
    pub consumption_trend: Vec<ConsumptionTrendPoint>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_days_accepts_only_7_30_and_default() {
        assert_eq!(StatsWindow::from_days(None), Some(StatsWindow::Last7Days));
        assert_eq!(
            StatsWindow::from_days(Some(7)),
            Some(StatsWindow::Last7Days)
        );
        assert_eq!(
            StatsWindow::from_days(Some(30)),
            Some(StatsWindow::Last30Days)
        );
        for invalid in [0, -7, 1, 8, 29, 31, 45, 365] {
            assert!(StatsWindow::from_days(Some(invalid)).is_none());
        }
    }

    #[test]
    fn window_days_are_7_and_30() {
        assert_eq!(StatsWindow::Last7Days.days(), 7);
        assert_eq!(StatsWindow::Last30Days.days(), 30);
    }
}
