use crate::admin::admin_users::types::{
    ErrorResponse, RevokeAllSessionsResponse, UserSessionResponse,
};
use axum::{
    Extension,
    extract::{Path, State},
    http::HeaderMap,
};
use herald_api_base::application::http::auth::util::{ClientIp, user_agent_from_headers};
use herald_api_base::application::http::common::auth_utils::AdminIdentity;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{
    ActorType, AuditAction, AuditCategory, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use herald_core::domain::authentication::identity::CredentialClass;
use herald_core::domain::authentication::{BrowserTokenService, Identity};
use herald_core::domain::user::AdminUserService;
use herald_core::domain::user::admin_errors::UserAdminError;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use uuid::Uuid;

/// Map a `CredentialClass` enum to its wire string form. Mirrors the enum's
/// `#[serde(rename_all = "snake_case")]` serialization so the DTO `String`
/// field matches the canonical representation.
fn credential_class_to_string(c: CredentialClass) -> &'static str {
    match c {
        CredentialClass::FirstParty => "first_party",
        CredentialClass::CustomUserUi => "custom_user_ui",
    }
}

/// Helper: confirm the target user exists in this realm via the admin user
/// service. Returns `Ok(())` if found, otherwise maps the service error to an
/// `ApiError` (404 / 403 / 500). Reuses the same realm-boundary check that
/// other admin_users handlers rely on.
async fn require_target_user(
    state: &AppState,
    identity: Identity,
    realm_id: &str,
    user_id: Uuid,
) -> Result<(), ApiError> {
    state
        .admin_user_service
        .get_user_admin(identity, realm_id, user_id)
        .await
        .map(|_| ())
        .map_err(|e| match e {
            UserAdminError::UserNotFound(id) => {
                tracing::debug!(
                    realm_id = %realm_id,
                    user_id = %user_id,
                    "Target user not found"
                );
                ApiError::not_found(format!("User not found: {}", id))
            }
            UserAdminError::PermissionDenied(msg) => {
                tracing::warn!(
                    realm_id = %realm_id,
                    user_id = %user_id,
                    error = %msg,
                    "Realm boundary check failed for target user"
                );
                ApiError::forbidden(msg)
            }
            UserAdminError::DatabaseError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %user_id,
                    error = %msg,
                    "Failed to load target user"
                );
                ApiError::internal(format!("Database error: {}", msg))
            }
            UserAdminError::InternalError(msg) => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %user_id,
                    error = %msg,
                    "Failed to load target user"
                );
                ApiError::internal(msg)
            }
            _ => {
                tracing::error!(
                    realm_id = %realm_id,
                    user_id = %user_id,
                    "Unexpected error loading target user"
                );
                ApiError::internal("Unexpected error")
            }
        })
}

/// List active sessions for a user
///
/// Returns the user's currently active browser-token sessions (families that
/// are not revoked and not past their absolute expiry). Requires `users.manage`
/// permission (per kickoff-user PRD §4.1: session view and revoke reuse the
/// existing `users.manage` permission; accounts holding only `users.view` must
/// not see the session list). Read-only: no audit event is recorded.
#[utoipa::path(
    get,
    path = "/api/users/{userId}/sessions",
    tag = "users",
    summary = "List a user's active sessions",
    description = "List active browser-token sessions for a specific user. Requires `users.manage` permission.",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "Active sessions for the user", body = [UserSessionResponse]),
        (status = 403, description = "Forbidden - Insufficient permissions (requires users.manage) or realm boundary violation", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_user_sessions(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(user_id): Path<Uuid>,
    _headers: HeaderMap,
) -> Result<ApiResult<Vec<UserSessionResponse>>, ApiError> {
    let realm_id = identity.realm_id();
    let admin =
        AdminIdentity::require_in_realm(identity.clone(), &realm_id, "user session management")?;
    admin.require_permission(&state, "users", "manage").await?;

    require_target_user(&state, identity, &realm_id, user_id).await?;

    let summaries = RedisBrowserTokenService::new(state.redis_manager.clone())
        .list_user_sessions(&user_id.to_string())
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                user_id = %user_id,
                error = %e,
                "Failed to list user sessions"
            );
            ApiError::internal("Failed to list user sessions")
        })?;

    let response: Vec<UserSessionResponse> = summaries
        .into_iter()
        .map(|s| UserSessionResponse {
            family_id: s.family_id,
            client_app_id: s.client_app_id,
            client_app_name: s.client_app_name,
            credential_class: credential_class_to_string(s.credential_class).to_string(),
            user_agent: s.user_agent,
            client_ip: s.client_ip,
            created_at: s.created_at.map(|dt| dt.to_rfc3339()),
            absolute_expires_at: s.absolute_expires_at.to_rfc3339(),
        })
        .collect();

    Ok(ApiResult::ok(response))
}

/// Revoke a single session family
///
/// Revokes one browser-token family belonging to the target user. The
/// `familyId` must belong to the target `userId` in this realm, otherwise 404
/// is returned without leaking cross-realm data. Requires `users.manage`
/// permission. An audit event is recorded best-effort.
#[utoipa::path(
    delete,
    path = "/api/users/{userId}/sessions/{familyId}",
    tag = "users",
    summary = "Revoke a single user session",
    description = "Revoke a single browser-token session family for a user. Requires `users.manage` permission.",
    params(
        ("userId" = Uuid, Path, description = "User ID"),
        ("familyId" = Uuid, Path, description = "Token family ID")
    ),
    responses(
        (status = 204, description = "Session revoked"),
        (status = 403, description = "Forbidden - Insufficient permissions (requires users.manage) or realm boundary violation", body = ErrorResponse),
        (status = 404, description = "User or session family not found / does not belong to this user", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn revoke_user_session(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((user_id, family_id)): Path<(Uuid, Uuid)>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Result<ApiResult<()>, ApiError> {
    let realm_id = identity.realm_id();
    let admin =
        AdminIdentity::require_in_realm(identity.clone(), &realm_id, "user session management")?;
    admin.require_permission(&state, "users", "manage").await?;

    require_target_user(&state, identity.clone(), &realm_id, user_id).await?;

    // Ownership + lifecycle guard (design kickoff-user §4.2.2). We read the
    // family record directly (`get_family_lifecycle`) instead of going through
    // `list_user_sessions`, because the latter filters out revoked/expired
    // families and could not distinguish:
    //   (A) a family that is absent or belongs to another user / realm, and
    //   (B) a family that belongs to this user/realm but is already revoked or
    //       past its absolute expiry.
    // Case (A) must return 404 to avoid leaking cross-realm data; case (B) is
    // the concurrent idempotent no-op and must return 204.
    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());
    let lifecycle = token_service
        .get_family_lifecycle(family_id)
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                user_id = %user_id,
                family_id = %family_id,
                error = %e,
                "Failed to load session family lifecycle before revoke"
            );
            ApiError::internal("Failed to verify session ownership")
        })?;

    let Some(lifecycle) = lifecycle else {
        tracing::debug!(
            realm_id = %realm_id,
            user_id = %user_id,
            family_id = %family_id,
            "Session family record not found (cross-realm / never existed)"
        );
        return Err(ApiError::not_found("Session not found"));
    };

    // Anti-cross-realm / wrong-owner guard (case A).
    if lifecycle.user_id != user_id.to_string() || lifecycle.realm_id != realm_id {
        tracing::debug!(
            realm_id = %realm_id,
            user_id = %user_id,
            family_id = %family_id,
            family_user_id = %lifecycle.user_id,
            family_realm_id = %lifecycle.realm_id,
            "Session family does not belong to target user/realm"
        );
        return Err(ApiError::not_found("Session not found"));
    }

    // Case B: belongs to this user/realm but already revoked or expired. Treat
    // the concurrent revocation as an idempotent success without re-revoking or
    // recording a (misleading) audit event.
    if lifecycle.revoked || lifecycle.expired {
        tracing::debug!(
            realm_id = %realm_id,
            user_id = %user_id,
            family_id = %family_id,
            revoked = lifecycle.revoked,
            expired = lifecycle.expired,
            "Session family already inactive; returning 204 no-op"
        );
        return Ok(ApiResult::no_content());
    }

    token_service.revoke_family(family_id).await.map_err(|e| {
        tracing::error!(
            realm_id = %realm_id,
            user_id = %user_id,
            family_id = %family_id,
            error = %e,
            "Failed to revoke session family"
        );
        ApiError::internal("Failed to revoke session")
    })?;

    // Best-effort audit event (ip/ua from request headers, mirroring logout.rs).
    let user_agent = user_agent_from_headers(&headers);
    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::UserManagement,
            action: AuditAction::UserUpdate,
            actor_id: identity.user_id(),
            actor_type: Some(ActorType::Admin),
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::Session,
            target_id: family_id.to_string(),
            target_name: Some(user_id.to_string()),
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "trigger": "admin_action",
                "scope": "single",
                "family_id": family_id.to_string(),
            })),
            ip_address: Some(ip),
            user_agent,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %e, "Failed to record session revoke audit event");
    }

    Ok(ApiResult::no_content())
}

/// Revoke all active sessions for a user
///
/// Revokes every currently active browser-token family for the target user.
/// Requires `users.manage` permission. Returns the count of sessions that were
/// active at the moment of the call. An audit event is recorded best-effort.
#[utoipa::path(
    delete,
    path = "/api/users/{userId}/sessions",
    tag = "users",
    summary = "Revoke all sessions for a user",
    description = "Revoke every active browser-token session for a user. Requires `users.manage` permission.",
    params(
        ("userId" = Uuid, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "All sessions revoked", body = RevokeAllSessionsResponse),
        (status = 403, description = "Forbidden - Insufficient permissions (requires users.manage) or realm boundary violation", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn revoke_all_user_sessions(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(user_id): Path<Uuid>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Result<ApiResult<RevokeAllSessionsResponse>, ApiError> {
    let realm_id = identity.realm_id();
    let admin =
        AdminIdentity::require_in_realm(identity.clone(), &realm_id, "user session management")?;
    admin.require_permission(&state, "users", "manage").await?;

    require_target_user(&state, identity.clone(), &realm_id, user_id).await?;

    // Snapshot the currently-active count for the response. Concurrent
    // revocation/expiry between this read and the bulk revoke may cause the
    // effective revoked count to differ; per design we treat already-revoked
    // families as no-ops and report the snapshot count.
    let token_service = RedisBrowserTokenService::new(state.redis_manager.clone());
    let summaries = token_service
        .list_user_sessions(&user_id.to_string())
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                user_id = %user_id,
                error = %e,
                "Failed to list user sessions before bulk revoke"
            );
            ApiError::internal("Failed to list user sessions")
        })?;
    let count = summaries.len();

    token_service
        .revoke_user_families(&user_id.to_string())
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                user_id = %user_id,
                error = %e,
                "Failed to revoke all user sessions"
            );
            ApiError::internal("Failed to revoke all sessions")
        })?;

    // Best-effort audit event.
    let user_agent = user_agent_from_headers(&headers);
    if let Err(e) = state
        .audit_event_repository
        .create(NewAuditEvent {
            realm_id: realm_id.clone(),
            category: AuditCategory::UserManagement,
            action: AuditAction::UserUpdate,
            actor_id: identity.user_id(),
            actor_type: Some(ActorType::Admin),
            actor_name: identity.as_user().map(|u| u.email.clone()),
            target_type: AuditTargetType::Session,
            target_id: user_id.to_string(),
            target_name: Some(user_id.to_string()),
            result: AuditResult::Success,
            details: Some(serde_json::json!({
                "trigger": "admin_action",
                "scope": "all",
                "revoked_count": count,
            })),
            ip_address: Some(ip),
            user_agent,
            trace_id: None,
        })
        .await
    {
        tracing::warn!(error = %e, "Failed to record bulk session revoke audit event");
    }

    Ok(ApiResult::ok(RevokeAllSessionsResponse {
        revoked_count: count as i32,
    }))
}
