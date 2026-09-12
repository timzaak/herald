use axum::{
    Router,
    routing::{get, post},
};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::ApiError;
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

/// A `permissions` row loaded for an admin write path (update/delete).
#[derive(sqlx::FromRow)]
pub(super) struct PermissionDefinitionRow {
    pub(super) is_builtin: bool,
    pub(super) name: String,
    pub(super) resource: String,
    pub(super) action: String,
}

/// Load a permission definition scoped to the realm; `None` means the id is
/// unknown in this realm (callers map it to 404).
async fn fetch_permission_definition(
    state: &AppState,
    id: uuid::Uuid,
    realm_id: &str,
) -> Result<Option<PermissionDefinitionRow>, ApiError> {
    sqlx::query_as(
        "SELECT is_builtin, name, resource, action FROM permissions WHERE id = $1 AND realm_id = $2",
    )
    .bind(id)
    .bind(realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to check permission: {e}");
        ApiError::internal("Failed to check permission")
    })
}

/// Whether any role still references this permission: via a role_permissions
/// assignment (admin display) or a role_policies mirror (runtime
/// authorization — role_policies snapshots resource/action, so both update
/// renames and deletes must refuse while a mirror exists).
async fn permission_in_use(
    state: &AppState,
    id: uuid::Uuid,
    realm_id: &str,
    resource: &str,
    action: &str,
) -> Result<bool, ApiError> {
    sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM role_permissions WHERE permission_id = $1
            UNION ALL
            SELECT 1 FROM role_policies
            WHERE realm_id = $2 AND resource = $3 AND action = $4 AND effect = true
        )
        "#,
    )
    .bind(id)
    .bind(realm_id)
    .bind(resource)
    .bind(action)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to check permission usage: {e}");
        ApiError::internal("Failed to check permission usage")
    })
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
    target: (String, Option<String>),
    result: AuditResult,
    details: Option<serde_json::Value>,
) {
    let (target_id, target_name) = target;
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
            result,
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
