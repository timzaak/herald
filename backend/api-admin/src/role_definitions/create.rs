use crate::role_definitions::types::{ErrorResponse, RoleCreateRequest, RoleResponse};
use axum::{
    Extension, Json,
    extract::{Path, State},
};
use axum_valid::Valid;
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::Identity;

/// Create a new role
#[utoipa::path(
    post,
    path = "/api/roles/{realmId}/define",
    tag = "role-definitions",
    summary = "Create a new role",
    description = "Create a new role definition. Requires `roles.manage` permission.",
    params(
        ("realmId" = String, Path, description = "Realm ID")
    ),
    request_body = RoleCreateRequest,
    responses(
        (status = 201, description = "Role created", body = RoleResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (requires roles.manage)", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_role(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Valid(Json(payload)): Valid<Json<RoleCreateRequest>>,
) -> Result<ApiResult<RoleResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, &realm_id, "role definitions")?;
    admin.require_permission(&state, "roles", "manage").await?;
    let insert = sqlx::query_as::<_, RoleResponse>(
        r#"
        INSERT INTO roles (name, description, realm_id, client_id, is_builtin)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, name, description, realm_id, client_id, is_builtin
        "#,
    )
    .bind(&payload.name)
    .bind(&payload.description)
    .bind(&realm_id)
    .bind(&payload.client_id)
    .bind(false) // is_builtin = false for user-created roles
    .fetch_one(&state.pool)
    .await;

    let row = match insert {
        Ok(row) => row,
        Err(e) => {
            tracing::error!("Failed to create role: {e}");
            let is_duplicate = matches!(
                &e,
                sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23505")
            );
            if is_duplicate {
                record_role_create_failure(
                    &state,
                    &admin,
                    &realm_id,
                    &payload.name,
                    "duplicate_name",
                )
                .await;
                return Err(ApiError::bad_request(
                    "Role name already exists in this realm",
                ));
            }
            return Err(ApiError::internal("Failed to create role"));
        }
    };

    // Record audit event (failure does not fail the operation)
    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::Rbac,
            action: AuditAction::RoleCreate,
            actor_id: admin.user_id_string(),
            actor_type: Some(ActorType::Admin),
            actor_name: admin.identity().as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::Role,
            target_id: row.id.to_string(),
            target_name: Some(row.name.clone()),
            result: AuditResult::Success,
            details: Some(serde_json::json!({"name": row.name})),
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %e, "Failed to record audit event");
    }

    Ok(ApiResult::created(row))
}

/// Record a failed role-creation attempt (audit.md requires failed RBAC
/// writes to be audited alongside successes). Best-effort, like the success
/// path above.
async fn record_role_create_failure(
    state: &AppState,
    admin: &AdminIdentity,
    realm_id: &str,
    name: &str,
    reason: &str,
) {
    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.to_string(),
            category: AuditCategory::Rbac,
            action: AuditAction::RoleCreate,
            actor_id: admin.user_id_string(),
            actor_type: Some(ActorType::Admin),
            actor_name: admin.identity().as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::Role,
            target_id: name.to_string(),
            target_name: Some(name.to_string()),
            result: AuditResult::Failure,
            details: Some(serde_json::json!({"reason": reason})),
            ip_address: None,
            user_agent: None,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %e, "Failed to record audit event");
    }
}
