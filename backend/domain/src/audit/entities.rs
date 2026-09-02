use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::event_types::{ActorType, AuditAction, AuditCategory, AuditResult, AuditTargetType};

/// A persisted audit event record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Uuid,
    pub realm_id: String,
    pub category: AuditCategory,
    pub action: AuditAction,
    pub actor_id: String,
    pub actor_type: Option<ActorType>,
    pub actor_name: Option<String>,
    pub target_type: AuditTargetType,
    pub target_id: String,
    pub target_name: Option<String>,
    pub result: AuditResult,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub trace_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Data needed to create a new audit event (without id and created_at).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAuditEvent {
    pub realm_id: String,
    pub category: AuditCategory,
    pub action: AuditAction,
    pub actor_id: String,
    pub actor_type: Option<ActorType>,
    pub actor_name: Option<String>,
    pub target_type: AuditTargetType,
    pub target_id: String,
    pub target_name: Option<String>,
    pub result: AuditResult,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub trace_id: Option<String>,
}

/// Filters for querying audit events with pagination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEventFilters {
    pub category: Option<AuditCategory>,
    pub action: Option<AuditAction>,
    pub actor_id: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub page: u64,
    #[serde(default = "default_page_size")]
    pub page_size: u64,
}

fn default_page_size() -> u64 {
    20
}

impl Default for AuditEventFilters {
    fn default() -> Self {
        Self {
            category: None,
            action: None,
            actor_id: None,
            start_time: None,
            end_time: None,
            page: 0,
            page_size: default_page_size(),
        }
    }
}

/// A paginated list of audit events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedAuditEvents {
    pub items: Vec<AuditEvent>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
}
