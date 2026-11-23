use crate::admin::admin_users::types::{
    ErrorResponse, UpdateUserRolesRequest, UserRoleDetail, UserRolesResponse,
};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::HeaderMap,
};
use axum_valid::Valid;
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::authentication::Identity;
use herald_core::domain::user::UserRoleRepository;
use herald_core::domain::user::admin_errors::UserAdminError;
use herald_core::domain::user::admin_ports::RoleAssignmentService;
use herald_core::domain::webhook_outbound::{
    OutboundEventData, OutboundEventInput, OutboundEventPublisher, OutboundEventType,
    RoleEventData, RoleEventSource,
};
use uuid::Uuid;

/// Shared by the write-path handlers below so their error shapes stay
/// consistent.
fn map_user_admin_error(e: UserAdminError) -> ApiError {
    match e {
        UserAdminError::PermissionDenied(msg) => ApiError::forbidden(msg),
        UserAdminError::UserNotFound(id) => ApiError::not_found(format!("User not found: {}", id)),
        UserAdminError::RoleNotFound(id) => {
            ApiError::bad_request(format!("Role not found: {}", id))
        }
        UserAdminError::InvalidRoleAssignment(msg) => {
            ApiError::bad_request(format!("Invalid role assignment: {}", msg))
        }
        UserAdminError::DatabaseError(msg) => {
            ApiError::internal(format!("Database error: {}", msg))
        }
        UserAdminError::InternalError(msg) => ApiError::internal(msg),
        _ => ApiError::internal("Unexpected error"),
    }
}

/// Get user roles
///
/// Returns the list of roles assigned to a specific user.
#[utoipa::path(
    get,
    path = "/api/users/{userId}/roles",
    tag = "users",
    operation_id = "adminGetUserRoles",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User roles", body = UserRolesResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_user_roles(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(user_id): Path<Uuid>,
    _headers: HeaderMap,
) -> Result<ApiResult<UserRolesResponse>, ApiError> {
    let admin = AdminIdentity::require(identity, "user role management")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "users", "view").await?;

    let role_assignment_service = &state.role_assignment_service;

    let roles = role_assignment_service
        .get_user_roles(admin.identity().clone(), &realm_id, user_id)
        .await
        .map_err(|e| match e {
            UserAdminError::PermissionDenied(msg) => ApiError::forbidden(msg),
            UserAdminError::UserNotFound(id) => {
                ApiError::not_found(format!("User not found: {}", id))
            }
            UserAdminError::DatabaseError(msg) => {
                ApiError::internal(format!("Database error: {}", msg))
            }
            UserAdminError::InternalError(msg) => ApiError::internal(msg),
            _ => ApiError::internal("Unexpected error"),
        })?;

    // Convert RoleDetail to UserRoleDetail
    let role_details: Vec<UserRoleDetail> = roles
        .into_iter()
        .map(|r| UserRoleDetail {
            id: r.id,
            name: r.name,
            description: r.description,
            source: r.source,
            source_id: r.source_id,
            expires_at: r.expires_at,
        })
        .collect();

    Ok(ApiResult::ok(UserRolesResponse {
        roles: role_details,
    }))
}

/// Update user roles
///
/// Updates the roles assigned to a user. Replaces all existing roles with the new list.
///
/// **Security constraints**:
/// - Realm-admin can only assign "user" role, not "realm-admin"
#[utoipa::path(
    put,
    path = "/api/users/{userId}/roles",
    tag = "users",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    request_body = UpdateUserRolesRequest,
    responses(
        (status = 200, description = "Roles updated"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden - cannot assign admin roles", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 409, description = "Conflict - User is deleted (anonymized terminal state)", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_user_roles(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(target_user_id): Path<Uuid>,
    _headers: HeaderMap,
    Valid(Json(payload)): Valid<Json<UpdateUserRolesRequest>>,
) -> Result<ApiResult<()>, ApiError> {
    let admin = AdminIdentity::require(identity, "user role management")?;
    let realm_id = admin.realm_id().to_string();
    admin.require_permission(&state, "roles", "manage").await?;

    // Deleted(3) terminal-state guard (users.md §4.2): role writes must not
    // mutate the anonymized tombstone. Mirrors the domain-level guard on the
    // admin update path; the presence check stays with the role service's
    // own realm-boundary lookup below.
    crate::admin::middleware::ensure_target_writable(
        &state.pool,
        &realm_id,
        target_user_id,
        "role update",
        false,
    )
    .await?;

    let role_assignment_service = &state.role_assignment_service;

    // Pre-read the current role set (with provenance) for the diff-based
    // outbound events: the replace below deletes rows whose source would
    // otherwise be unrecoverable for the role.revoked payloads. Read via the
    // repository (realm-filtered below) — the service getter would demand
    // `users.view`, which this roles.manage-only PUT path must not require.
    let before_roles = state
        .user_role_repository
        .get_user_roles(target_user_id)
        .await
        .map_err(map_user_admin_error)?
        .into_iter()
        .filter(|r| r.realm_id == realm_id)
        .collect::<Vec<_>>();

    role_assignment_service
        .assign_user_roles(
            admin.identity().clone(),
            &realm_id,
            target_user_id,
            payload.role_ids.clone(),
        )
        .await
        .map_err(map_user_admin_error)?;

    // Diff-based outbound events: added ids are manual grants written by the
    // replace; removed ids carry their pre-read original source.
    let before_ids: std::collections::HashMap<Uuid, &str> = before_roles
        .iter()
        .map(|r| (r.id, r.source.as_str()))
        .collect();
    let after_set: std::collections::HashSet<Uuid> = payload.role_ids.iter().copied().collect();
    // replace_user_roles only deletes manual rows and preserves payment
    // grants: a role the user still holds via a payment row must NOT get a
    // role.revoked — the entitlement survives the reassignment.
    let payment_held: std::collections::HashSet<Uuid> = before_roles
        .iter()
        .filter(|r| r.source == "payment")
        .map(|r| r.id)
        .collect();

    let mut outbound_events = Vec::new();
    let now = chrono::Utc::now();
    for role_id in &after_set {
        if !before_ids.contains_key(role_id) {
            outbound_events.push(OutboundEventInput {
                realm_id: realm_id.clone(),
                event_type: OutboundEventType::RoleGranted,
                occurred_at: now,
                data: OutboundEventData::Role(RoleEventData {
                    user_id: target_user_id,
                    role_id: *role_id,
                    // Newly granted ids are by definition absent from the
                    // pre-read set — the fan-out service resolves the name.
                    role_name: None,
                    source: RoleEventSource::Manual,
                    expires_at: None,
                }),
            });
        }
    }
    for (role_id, source) in &before_ids {
        if !after_set.contains(role_id) && !payment_held.contains(role_id) {
            outbound_events.push(OutboundEventInput {
                realm_id: realm_id.clone(),
                event_type: OutboundEventType::RoleRevoked,
                occurred_at: now,
                data: OutboundEventData::Role(RoleEventData {
                    user_id: target_user_id,
                    role_id: *role_id,
                    role_name: None,
                    source: RoleEventSource::from_db_str(source),
                    expires_at: None,
                }),
            });
        }
    }
    if !outbound_events.is_empty() {
        state.webhook_outbound.emit(outbound_events).await;
    }

    Ok(ApiResult::ok(()))
}
