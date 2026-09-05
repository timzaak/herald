use axum::{
    Router,
    routing::{get, post},
};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};

mod create;
mod delete;
mod get;
mod list;
pub mod types;
mod update;

pub use create::*;
pub use delete::*;
pub use get::*;
pub use list::*;
pub use update::*;

// Re-export for utoipa
pub use create::__path_create_permission as __path_create_permission_definition;
pub use delete::__path_delete_permission as __path_delete_permission_definition;
pub use get::__path_get_permission as __path_get_permission_definition;
pub use list::__path_list_permissions as __path_list_permission_definitions;
pub use update::__path_update_permission as __path_update_permission_definition;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_permission).get(list_permissions))
        .route(
            "/{permissionDefinitionId}",
            get(get_permission)
                .put(update_permission)
                .delete(delete_permission),
        )
}

/// Record the permission-definition audit event shared by create / update /
/// delete (permissions.md [US-AU-005] requires permission-definition changes
/// to be audited; the shape mirrors role-definitions). Best-effort: an audit
/// write failure never fails the CRUD operation.
async fn record_permission_audit(
    state: &AppState,
    admin: &AdminIdentity,
    realm_id: &str,
    action: AuditAction,
    target_id: String,
    target_name: Option<String>,
    details: Option<serde_json::Value>,
) {
    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.to_string(),
            category: AuditCategory::Rbac,
            action,
            actor_id: admin.user_id_string(),
            actor_type: Some(ActorType::Admin),
            actor_name: admin.identity().as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::Permission,
            target_id,
            target_name,
            result: AuditResult::Success,
            details,
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %e, "Failed to record audit event");
    }
}
