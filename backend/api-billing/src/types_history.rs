use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use herald_api_base::application::http::common::pagination::PaginationMeta;

/// Subscription history event for API responses
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionHistoryEventResponse {
    pub id: String,
    pub subscription_id: Uuid,
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub actor: Option<String>,
    pub changes: Option<serde_json::Value>,
    pub previous_state: Option<serde_json::Value>,
    pub new_state: Option<serde_json::Value>,
}

/// Subscription history response for a single subscription
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionHistoryResponse {
    pub subscription_id: Uuid,
    pub events: Vec<SubscriptionHistoryEventResponse>,
    pub total: usize,
}

/// User information in subscription history
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub id: Uuid,
    pub email: String,
}

/// Subscription summary in history. `status` is absent when the referenced
/// subscription row no longer resolves (history outlives its subscription).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionSummary {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<herald_core::domain::billing::entities::SubscriptionStatus>,
    pub entitlement_key: Option<String>,
}

/// Subscription history list response with user and subscription details
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionHistoryListResponse {
    pub events: Vec<SubscriptionHistoryEventWithUser>,
    pub pagination: PaginationMeta,
}

/// Subscription history event with user and subscription details
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionHistoryEventWithUser {
    pub id: String,
    pub subscription_id: Uuid,
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub actor: Option<String>,
    pub changes: Option<serde_json::Value>,
    pub previous_state: Option<serde_json::Value>,
    pub new_state: Option<serde_json::Value>,
    pub user: Option<UserInfo>,
    pub subscription: SubscriptionSummary,
}

/// Query parameters for subscription history list
#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionHistoryListQuery {
    /// Filter by user ID
    pub user_id: Option<Uuid>,
    /// Filter by entitlement key
    pub entitlement_key: Option<String>,
    /// Filter by event type
    pub event_type: Option<String>,
    /// Filter by subscription status
    pub subscription_status: Option<String>,
    /// Filter by date range start (ISO 8601 format)
    pub from_date: Option<DateTime<Utc>>,
    /// Filter by date range end (ISO 8601 format)
    pub to_date: Option<DateTime<Utc>>,
    /// Page number (1-based)
    #[serde(default = "default_page")]
    pub page: u64,
    /// Number of items per page
    #[serde(default = "default_page_size")]
    pub page_size: u64,
    /// Sort field
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    /// Sort order
    #[serde(default = "default_sort_order")]
    pub sort_order: String,
}

fn default_page() -> u64 {
    1
}

fn default_page_size() -> u64 {
    20
}

fn default_sort_by() -> String {
    "timestamp".to_string()
}

fn default_sort_order() -> String {
    "desc".to_string()
}

impl From<herald_core::domain::billing::SubscriptionHistoryEvent>
    for SubscriptionHistoryEventResponse
{
    fn from(event: herald_core::domain::billing::SubscriptionHistoryEvent) -> Self {
        SubscriptionHistoryEventResponse {
            id: event.id,
            subscription_id: event.subscription_id,
            event_type: event.event_type.as_str().to_string(),
            timestamp: event.timestamp,
            actor: event.actor,
            changes: event.changes,
            previous_state: event.previous_state,
            new_state: event.new_state,
        }
    }
}
