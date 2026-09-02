use chrono::{DateTime, Duration, Months, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::common::entities::app_errors::CoreError;

/// Grant period type enum (积分发放周期类型)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum GrantPeriodType {
    Once,
    Daily,
    Weekly,
    Monthly,
}

impl GrantPeriodType {
    pub fn as_str(&self) -> &'static str {
        match self {
            GrantPeriodType::Once => "once",
            GrantPeriodType::Daily => "daily",
            GrantPeriodType::Weekly => "weekly",
            GrantPeriodType::Monthly => "monthly",
        }
    }

    /// Calculate the next grant time based on base time and periods granted
    ///
    /// # Arguments
    /// * `base_time` - The base time (subscription time or registration time)
    /// * `periods_granted` - Number of periods already granted (0 for first grant)
    ///
    /// # Returns
    /// The next grant time
    ///
    /// # Examples
    /// ```
    /// let base = Utc::now();
    /// assert_eq!(GrantPeriodType::Once.next_grant_time(base, 0), base);
    /// assert_eq!(GrantPeriodType::Daily.next_grant_time(base, 1), base + Duration::days(1));
    /// ```
    pub fn next_grant_time(&self, base_time: DateTime<Utc>, periods_granted: i64) -> DateTime<Utc> {
        match self {
            GrantPeriodType::Once => base_time,
            GrantPeriodType::Daily => base_time + Duration::days(periods_granted),
            GrantPeriodType::Weekly => base_time + Duration::weeks(periods_granted),
            GrantPeriodType::Monthly => {
                // Handle month calculation carefully
                let months = periods_granted as u32;
                base_time + Months::new(months)
            }
        }
    }

    /// Calculate expiration time based on grant time and validity days
    ///
    /// # Arguments
    /// * `grant_time` - The time when points are granted
    /// * `validity_days` - Number of days the points are valid (0 = permanent)
    ///
    /// # Returns
    /// None if permanent, Some(expiration_time) otherwise
    pub fn calculate_expiration(
        &self,
        grant_time: DateTime<Utc>,
        validity_days: i64,
    ) -> Option<DateTime<Utc>> {
        if validity_days == 0 {
            None // Permanent
        } else {
            Some(grant_time + Duration::days(validity_days))
        }
    }

    /// Check if this grant type should stop after reaching max periods
    pub fn should_stop(&self, periods_granted: i64, max_periods: Option<i64>) -> bool {
        if matches!(self, GrantPeriodType::Once) {
            return periods_granted >= 1;
        }
        if let Some(max) = max_periods {
            periods_granted >= max
        } else {
            false // No limit
        }
    }
}

impl std::str::FromStr for GrantPeriodType {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase();
        match s.as_str() {
            "once" => Ok(GrantPeriodType::Once),
            "daily" => Ok(GrantPeriodType::Daily),
            "weekly" => Ok(GrantPeriodType::Weekly),
            "monthly" => Ok(GrantPeriodType::Monthly),
            _ => Err(CoreError::BadRequest(format!(
                "Invalid grant_period_type: {}",
                s
            ))),
        }
    }
}

impl std::fmt::Display for GrantPeriodType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Points grant schedule domain model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsGrantSchedule {
    pub id: Uuid,
    pub user_id: Uuid,
    pub realm_id: String,
    pub bucket_id: Uuid,
    pub subscription_id: Option<Uuid>,
    pub entitlement_key: Option<String>,
    pub grant_period_type: GrantPeriodType,
    pub base_time: DateTime<Utc>,
    pub next_grant_time: DateTime<Utc>,
    pub points_per_period: i64,
    pub validity_days: i64,
    pub granted_periods: i64,
    pub max_periods: Option<i64>,
    pub active: bool,
    /// Distribution attribution. A schedule is always created by a free-periodic
    /// fixed distribution rule, so both references are always present.
    pub distribution_event_id: Uuid,
    pub distribution_rule_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PointsGrantSchedule {
    /// Check if this schedule is due for granting
    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        self.active && self.next_grant_time <= now
    }

    /// Check if this schedule should stop (reached max periods)
    pub fn should_stop(&self) -> bool {
        self.grant_period_type
            .should_stop(self.granted_periods, self.max_periods)
    }

    /// Calculate the next grant time after incrementing periods
    pub fn calculate_next_grant_time(&self) -> DateTime<Utc> {
        self.grant_period_type
            .next_grant_time(self.base_time, self.granted_periods + 1)
    }

    /// Calculate expiration time for the next grant
    pub fn calculate_next_expiration(&self) -> Option<DateTime<Utc>> {
        let next_grant_time = self.calculate_next_grant_time();
        self.grant_period_type
            .calculate_expiration(next_grant_time, self.validity_days)
    }
}

/// Points grant record domain model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsGrantRecord {
    pub id: Uuid,
    pub schedule_id: Uuid,
    pub user_id: Uuid,
    pub realm_id: String,
    pub period_number: i64,
    pub granted_amount: i64,
    pub grant_time: DateTime<Utc>,
    /// FK bridge to the single ledger row this grant record deduplicates.
    /// `pregrant_next_period_atomic` and the
    /// subscription current-period grant both populate this in the same
    /// transaction that inserts the ledger row, so reclaim can resolve
    /// `(schedule_id, period_number) → ledger_id → ledger row` without a
    /// `schedule_id` column on `points_credit_ledger`. NOT NULL at the SQL
    /// layer; pre-launch, no backfill needed.
    pub ledger_id: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Grant summary - Results of processing grant schedules
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GrantSummary {
    pub processed: u64,
    pub skipped: u64,
    pub failed: u64,
    pub total_granted: i64,
}

/// Process result - Result of processing a single grant schedule
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessResult {
    Granted,
    Skipped,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grant_period_type_next_grant_time() {
        let base = Utc::now();

        // Once: always returns base time
        assert_eq!(GrantPeriodType::Once.next_grant_time(base, 0), base);
        assert_eq!(GrantPeriodType::Once.next_grant_time(base, 10), base);

        // Daily: adds days
        let next = GrantPeriodType::Daily.next_grant_time(base, 1);
        assert_eq!(next, base + Duration::days(1));

        // Weekly: adds weeks
        let next = GrantPeriodType::Weekly.next_grant_time(base, 1);
        assert_eq!(next, base + Duration::weeks(1));

        // Monthly: adds months
        let next = GrantPeriodType::Monthly.next_grant_time(base, 1);
        assert_eq!(next, base + Months::new(1));
    }

    #[test]
    fn test_grant_period_type_expiration() {
        let now = Utc::now();

        // Permanent (validity_days = 0)
        assert!(
            GrantPeriodType::Daily
                .calculate_expiration(now, 0)
                .is_none()
        );

        // 1 day validity
        let exp = GrantPeriodType::Daily.calculate_expiration(now, 1).unwrap();
        assert_eq!(exp, now + Duration::days(1));
    }

    #[test]
    fn test_grant_period_type_should_stop() {
        // No max periods - never stops
        assert!(!GrantPeriodType::Daily.should_stop(5, None));

        // With max periods
        assert!(!GrantPeriodType::Daily.should_stop(2, Some(5)));
        assert!(GrantPeriodType::Daily.should_stop(5, Some(5)));
        assert!(GrantPeriodType::Daily.should_stop(6, Some(5)));
    }
}
