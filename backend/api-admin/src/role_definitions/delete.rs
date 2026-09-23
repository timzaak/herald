use crate::role_definitions::types::ErrorResponse;
use axum::{
    Extension,
    extract::{Path, State},
};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::Identity;
use uuid::Uuid;

/// Delete role
#[utoipa::path(
    delete,
    path = "/api/roles/define/{roleId}",
    tag = "role-definitions",
    summary = "Delete a role",
    description = "Delete a role definition. Built-in roles cannot be deleted. Requires `roles.manage` permission.",
    params(
        ("roleId" = Uuid, Path, description = "Role ID")
    ),
    responses(
        (status = 204, description = "Role deleted"),
        (status = 403, description = "Forbidden - Insufficient permissions (requires roles.manage) or attempting to delete built-in role", body = ErrorResponse),
        (status = 404, description = "Role not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_role(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Extension(identity): Extension<Identity>,
) -> Result<ApiResult<()>, ApiError> {
    let realm_id = identity.realm_id();
    let admin = AdminIdentity::require_in_realm(identity.clone(), &realm_id, "role definitions")?;
    admin.require_permission(&state, "roles", "manage").await?;

    // 3. Check if role is built-in
    let role: Option<(bool, String)> =
        sqlx::query_as("SELECT is_builtin, name FROM roles WHERE id = $1 AND realm_id = $2")
            .bind(id)
            .bind(&realm_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| {
                tracing::error!("Failed to check role: {e}");
                ApiError::internal("Failed to check role")
            })?;

    let role_name = match role {
        Some((is_builtin, role_name)) => {
            if is_builtin {
                tracing::warn!(
                    user_id = %identity.user_id(),
                    role_id = %id,
                    role_name = %role_name,
                    "Attempted to delete built-in role"
                );
                if let Err(e) = state
                    .audit_event_repository
                    .create(NewAuditEvent {
                        realm_id: realm_id.clone(),
                        category: AuditCategory::Rbac,
                        action: AuditAction::RoleDelete,
                        actor_id: identity.user_id().to_string(),
                        actor_type: Some(ActorType::Admin),
                        actor_name: identity.as_user().map(|u| u.email.clone()),
                        target_type: AuditTargetType::Role,
                        target_id: id.to_string(),
                        target_name: Some(role_name.clone()),
                        result: AuditResult::Failure,
                        details: Some(serde_json::json!({"reason": "builtin_role"})),
                        ip_address: None,
                        user_agent: None,
                        trace_id: None,
                    })
                    .await
                {
                    tracing::warn!(error = %e, "Failed to record audit event");
                }
                return Err(ApiError::forbidden("Cannot delete built-in role"));
            }
            role_name
        }
        None => {
            if let Err(e) = state
                .audit_event_repository
                .create(NewAuditEvent {
                    realm_id: realm_id.clone(),
                    category: AuditCategory::Rbac,
                    action: AuditAction::RoleDelete,
                    actor_id: identity.user_id().to_string(),
                    actor_type: Some(ActorType::Admin),
                    actor_name: identity.as_user().map(|u| u.email.clone()),
                    target_type: AuditTargetType::Role,
                    target_id: id.to_string(),
                    target_name: None,
                    result: AuditResult::Failure,
                    details: Some(serde_json::json!({"reason": "role_not_found"})),
                    ip_address: None,
                    user_agent: None,
                    trace_id: None,
                })
                .await
            {
                tracing::warn!(error = %e, "Failed to record audit event");
            }
            return Err(ApiError::not_found("Role not found"));
        }
    };

    let role_in_use: Option<(bool,)> = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM user_roles WHERE role_id = $1 AND realm_id = $2)",
    )
    .bind(id)
    .bind(&realm_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to check role usage: {e}");
        ApiError::internal("Failed to check role usage")
    })?;

    if matches!(role_in_use, Some((true,))) {
        if let Err(e) = state
            .audit_event_repository
            .create(NewAuditEvent {
                realm_id: realm_id.clone(),
                category: AuditCategory::Rbac,
                action: AuditAction::RoleDelete,
                actor_id: identity.user_id().to_string(),
                actor_type: Some(ActorType::Admin),
                actor_name: identity.as_user().map(|u| u.email.clone()),
                target_type: AuditTargetType::Role,
                target_id: id.to_string(),
                target_name: None,
                result: AuditResult::Failure,
                details: Some(serde_json::json!({"reason": "role_in_use"})),
                ip_address: None,
                user_agent: None,
                trace_id: None,
            })
            .await
        {
            tracing::warn!(error = %e, "Failed to record audit event");
        }
        return Err(ApiError::conflict(
            "Cannot delete role that is still assigned to users",
        ));
    }

    // 4. Execute deletion
    let result = sqlx::query("DELETE FROM roles WHERE id = $1 AND realm_id = $2")
        .bind(id)
        .bind(&realm_id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete role: {e}");
            ApiError::internal("Failed to delete role")
        })?;

    if result.rows_affected() == 0 {
        if let Err(e) = state
            .audit_event_repository
            .create(NewAuditEvent {
                realm_id: realm_id.clone(),
                category: AuditCategory::Rbac,
                action: AuditAction::RoleDelete,
                actor_id: identity.user_id().to_string(),
                actor_type: Some(ActorType::Admin),
                actor_name: identity.as_user().map(|u| u.email.clone()),
                target_type: AuditTargetType::Role,
                target_id: id.to_string(),
                target_name: Some(role_name.clone()),
                result: AuditResult::Failure,
                details: Some(serde_json::json!({"reason": "role_not_found_on_delete"})),
                ip_address: None,
                user_agent: None,
                trace_id: None,
            })
            .await
        {
            tracing::warn!(error = %e, "Failed to record audit event");
        }
        return Err(ApiError::not_found("Role not found"));
    }

    // Record audit event (failure does not fail the operation)
    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::Rbac,
            action: AuditAction::RoleDelete,
            actor_id: identity.user_id().to_string(),
            actor_type: Some(ActorType::Admin),
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::Role,
            target_id: id.to_string(),
            target_name: Some(role_name.clone()),
            result: AuditResult::Success,
            details: None,
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %e, "Failed to record audit event");
    }

    Ok(ApiResult::no_content())
}
