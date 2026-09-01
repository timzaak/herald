// MCP tool input/output DTOs and argument normalization.
//
// Tool-facing pagination is uniformly 1-based across all five tools. The
// backing layers are NOT uniform (points infra is 1-based, audit infra is
// 0-based); the per-tool conversions live in tools.rs and the helpers here
// keep the tool contract itself consistent so agents never have to know
// which legacy layer a tool reads from.

use chrono::{DateTime, NaiveDate, Utc};
use herald_core::domain::audit::{AuditAction, AuditCategory};
use herald_core::domain::points::entities::TransactionType;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::tool_error::ToolError;

pub const DEFAULT_PAGE: u64 = 1;
pub const DEFAULT_PAGE_SIZE: u64 = 20;
pub const MAX_PAGE_SIZE: u64 = 100;

// ============================================================================
// Input DTOs (schemars derives become the MCP tool input schemas)
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryUsersInput {
    /// Optional user ID (UUID). When provided, returns that single user's
    /// detail instead of a list.
    pub user_id: Option<String>,
    /// Optional exact email filter for list mode.
    pub email: Option<String>,
    /// 1-based page number (default 1; values below 1 are rejected).
    pub page: Option<u64>,
    /// Page size (default 20, clamped to 1..100).
    pub page_size: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetPointsBalanceInput {
    /// Target user ID (UUID).
    pub user_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListPointsTransactionsInput {
    /// Target user ID (UUID).
    pub user_id: String,
    /// Optional transaction type filter: recharge, consume, subscription_grant,
    /// subscription_renewal, subscription_upgrade, subscription_downgrade,
    /// registration_grant, free_periodic_grant, refund_revoke, expire_revoke,
    /// cancel_revoke, expiration, refund, grant.
    pub transaction_type: Option<String>,
    /// Optional start time bound (RFC 3339, e.g. 2026-01-01T00:00:00Z).
    pub start_time: Option<String>,
    /// Optional end time bound (RFC 3339).
    pub end_time: Option<String>,
    /// 1-based page number (default 1; values below 1 are rejected).
    pub page: Option<u64>,
    /// Page size (default 20, clamped to 1..100).
    pub page_size: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListAuditLogsInput {
    /// Optional audit category filter: user_management, rbac, realm_management,
    /// auth, billing, oauth, compliance.
    pub category: Option<String>,
    /// Optional audit action filter (e.g. user.create, auth.login).
    pub action: Option<String>,
    /// Optional actor ID filter.
    pub actor_id: Option<String>,
    /// Optional start time bound (RFC 3339 or YYYY-MM-DD).
    pub start_time: Option<String>,
    /// Optional end time bound (RFC 3339 or YYYY-MM-DD).
    pub end_time: Option<String>,
    /// 1-based page number (default 1; values below 1 are rejected).
    pub page: Option<u64>,
    /// Page size (default 20, clamped to 1..100).
    pub page_size: Option<u64>,
}

// ============================================================================
// Output DTOs (minimal field surfaces — see each tool's mapping notes)
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserItem {
    pub id: String,
    pub email: String,
    pub nickname: Option<String>,
    /// Numeric status: 1=normal, 0=disabled (plus legacy values if present).
    pub status: i32,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsersPage {
    pub users: Vec<UserItem>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointsBalanceView {
    pub user_id: String,
    /// "realm" for realm-wide keys, "client_app" when a client-app-bound
    /// (non-admin) key reads only its app's covered buckets.
    pub scope: String,
    pub balance: i64,
    pub topup_balance: i64,
    pub subscription_balance: i64,
    pub granted_balance: i64,
    pub registration_balance: i64,
    pub free_periodic_balance: i64,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionItem {
    pub transaction_id: String,
    pub transaction_type: String,
    /// Signed amount: consumption is negative.
    pub amount: i64,
    pub balance_after: i64,
    pub description: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionsPage {
    pub transactions: Vec<TransactionItem>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventItem {
    pub id: String,
    pub category: String,
    pub action: String,
    pub actor_id: String,
    pub actor_name: Option<String>,
    pub target_type: String,
    pub target_id: String,
    pub result: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventsPage {
    pub events: Vec<AuditEventItem>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigStatusItem {
    pub config_type: String,
    pub config_key: String,
    pub enabled: bool,
    pub is_secret: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmConfigStatus {
    pub realm_id: String,
    pub configs: Vec<ConfigStatusItem>,
}

// ============================================================================
// Argument normalization helpers
// ============================================================================

/// Normalize tool-facing pagination: 1-based, page<1 rejected (agent should
/// self-correct rather than silently read a different page), pageSize clamped
/// to 1..=100 to bound response size.
pub fn normalize_page(page: Option<u64>, page_size: Option<u64>) -> Result<(u64, u64), ToolError> {
    let page = page.unwrap_or(DEFAULT_PAGE);
    if page < 1 {
        return Err(ToolError::invalid_argument(
            "'page' must be 1 or greater (1-based pagination).",
        ));
    }
    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    Ok((page, page_size))
}

pub fn parse_uuid(field: &str, value: &str) -> Result<Uuid, ToolError> {
    Uuid::parse_str(value)
        .map_err(|_| ToolError::invalid_argument(format!("'{field}' must be a valid UUID.")))
}

const TRANSACTION_TYPE_VALUES: &str = "recharge, consume, subscription_grant, \
subscription_renewal, subscription_upgrade, subscription_downgrade, registration_grant, \
free_periodic_grant, refund_revoke, expire_revoke, cancel_revoke, expiration, refund, grant";

pub fn parse_transaction_type(field: &str, value: &str) -> Result<TransactionType, ToolError> {
    value.parse().map_err(|_| {
        ToolError::invalid_argument(format!(
            "'{field}' must be one of: {TRANSACTION_TYPE_VALUES}."
        ))
    })
}

const AUDIT_CATEGORY_VALUES: &str =
    "user_management, rbac, realm_management, auth, billing, oauth, compliance";

pub fn parse_audit_category(field: &str, value: &str) -> Result<AuditCategory, ToolError> {
    serde_json::from_str(&format!("\"{value}\"")).map_err(|_| {
        ToolError::invalid_argument(format!(
            "'{field}' must be one of: {AUDIT_CATEGORY_VALUES}."
        ))
    })
}

pub fn parse_audit_action(field: &str, value: &str) -> Result<AuditAction, ToolError> {
    serde_json::from_str(&format!("\"{value}\"")).map_err(|_| {
        // Enumerate a stable hint instead of the full variant list: the
        // category filter is the primary axis agents use.
        ToolError::invalid_argument(format!(
            "'{field}' is not a known audit action (e.g. user.create, auth.login)."
        ))
    })
}

/// RFC 3339 timestamp (points transactions filter).
pub fn parse_time_rfc3339(field: &str, value: &str) -> Result<DateTime<Utc>, ToolError> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.to_utc())
        .map_err(|_| {
            ToolError::invalid_argument(format!(
                "'{field}' must be an RFC 3339 timestamp (e.g. 2026-01-01T00:00:00Z)."
            ))
        })
}

/// Audit log time filter: RFC 3339, or a bare YYYY-MM-DD interpreted as
/// midnight UTC (the admin audit console additionally tolerates
/// space-separated timezone offsets; this stricter form keeps the tool
/// contract explicit).
pub fn parse_query_time(field: &str, value: &str) -> Result<DateTime<Utc>, ToolError> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Ok(dt.to_utc());
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Ok(date.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc());
    }
    Err(ToolError::invalid_argument(format!(
        "'{field}' must be an RFC 3339 timestamp or a YYYY-MM-DD date."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pagination base handling differs between the legacy backing layers
    // (points 1-based, audit 0-based); the tool surface must stay uniformly
    // 1-based so these conversions cannot silently drift.
    #[test]
    fn normalize_page_applies_defaults_clamp_and_rejects_zero() {
        assert_eq!(normalize_page(None, None).unwrap(), (1, 20));
        assert_eq!(normalize_page(Some(3), Some(5)).unwrap(), (3, 5));
        assert_eq!(normalize_page(Some(2), Some(500)).unwrap(), (2, 100));
        assert_eq!(normalize_page(Some(2), Some(0)).unwrap(), (2, 1));
        assert!(normalize_page(Some(0), None).is_err());
    }

    #[test]
    fn parse_uuid_rejects_non_uuid_with_field_name() {
        assert!(parse_uuid("userId", "not-a-uuid").is_err());
        let err = parse_uuid("userId", "not-a-uuid").unwrap_err();
        assert_eq!(err.code_as_str(), "invalid_argument");
        assert!(err.message().contains("'userId'"));
        let fixed = "01928fa4-1f3c-7cc8-9d3a-3f4f5f6f7f8f";
        assert!(parse_uuid("userId", fixed).is_ok());
    }

    #[test]
    fn parse_transaction_type_accepts_snake_case_and_rejects_unknown() {
        assert!(parse_transaction_type("transactionType", "recharge").is_ok());
        assert!(parse_transaction_type("transactionType", "subscription_grant").is_ok());
        let err = parse_transaction_type("transactionType", "topup").unwrap_err();
        assert_eq!(err.code_as_str(), "invalid_argument");
        assert!(err.message().contains("recharge"));
    }

    #[test]
    fn parse_audit_category_accepts_known_and_rejects_unknown() {
        assert_eq!(
            parse_audit_category("category", "user_management").unwrap(),
            AuditCategory::UserManagement
        );
        let err = parse_audit_category("category", "nonsense").unwrap_err();
        assert_eq!(err.code_as_str(), "invalid_argument");
        assert!(err.message().contains("user_management"));
    }

    #[test]
    fn parse_audit_action_accepts_dotted_names() {
        assert_eq!(
            parse_audit_action("action", "user.create").unwrap(),
            AuditAction::UserCreate
        );
        assert!(parse_audit_action("action", "not.an.action").is_err());
    }

    #[test]
    fn parse_time_rfc3339_rejects_bare_dates() {
        assert!(parse_time_rfc3339("startTime", "2026-01-01T00:00:00Z").is_ok());
        // Bare dates are audit-only tolerance; the points filter rejects them.
        assert!(parse_time_rfc3339("startTime", "2026-01-01").is_err());
        assert!(parse_time_rfc3339("startTime", "yesterday").is_err());
    }

    #[test]
    fn parse_query_time_accepts_rfc3339_and_bare_dates() {
        assert_eq!(
            parse_query_time("startTime", "2026-01-01T00:00:00Z").unwrap(),
            DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .to_utc()
        );
        let bare = parse_query_time("endTime", "2026-01-01").unwrap();
        assert_eq!(
            bare,
            parse_query_time("endTime", "2026-01-01T00:00:00Z").unwrap()
        );
        assert!(parse_query_time("endTime", "01/01/2026").is_err());
    }
}
