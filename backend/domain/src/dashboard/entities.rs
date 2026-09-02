use serde::{Deserialize, Serialize};

/// User statistics for a realm.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserStats {
    pub total_users: i64,
    pub new_users: i64,
    pub active_users: i64,
}

/// A single data point in the authentication trend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthTrendPoint {
    pub date: String,
    pub success_count: i64,
    pub failure_count: i64,
}

/// Aggregated dashboard statistics for a realm.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    pub user_stats: UserStats,
    pub auth_trend: Vec<AuthTrendPoint>,
}
