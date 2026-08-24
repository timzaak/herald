// User Admin Service Implementations
//
// These services contain the business logic for user admin operations

use std::sync::Arc;
use uuid::Uuid;

use crate::audit::{
    AuditAction, AuditCategory, AuditContext, AuditEventRepository, AuditResult, AuditTargetType,
    NewAuditEvent,
};
use crate::authentication::ports::BrowserTokenService;
use crate::{
    authentication::Identity,
    authorization::permission_service::PermissionService,
    common::{entities::app_errors::CoreError, policies::ensure_policy},
    security_constants::DEFAULT_BCRYPT_COST,
    user::entities::UserStatus,
};

use super::super::{
    AdminUser, AdminUserEntity, AdminUserRepository, AdminUserService, CreateUserWithRolesRequest,
    PermissionDetail, PermissionListData, PermissionManagementService, PermissionSource,
    RoleAssignmentService, RoleDetail, RoleEntity, RolePolicyRepository, UpdateUserAdminRequest,
    UserAdminError, UserAdminResult, UserPermissionService, UserRoleRepository,
};

// ============================================================================
// Shared permission check helper
// ============================================================================

async fn require_permission<P: PermissionService>(
    permission_checker: &P,
    identity: &Identity,
    realm_id: &str,
    resource: &str,
    action: &str,
    policy_msg: &str,
) -> UserAdminResult<()> {
    let principal = identity.principal_ref();
    let allowed = permission_checker
        .check_principal_permission(
            realm_id,
            principal.principal_type,
            &principal.principal_id,
            resource,
            action,
        )
        .await
        .map_err(|e| UserAdminError::InternalError(format!("Permission check failed: {}", e)))?;

    ensure_policy(allowed, policy_msg).map_err(|e| match e {
        CoreError::Forbidden(msg) => UserAdminError::PermissionDenied(msg),
        _ => UserAdminError::InternalError(format!("Policy check error: {:?}", e)),
    })
}

// ============================================================================
// Target realm-boundary checks
// ============================================================================

/// Load the target user of a path-scoped admin operation and enforce the
/// realm boundary: the user acted on must belong to `realm_id`.
///
/// The caller-vs-path check inside each service does not constrain the target
/// id, so without this a realm admin could act on another realm's user id
/// (cross-tenant update/delete/password-reset). A wrong-realm target returns
/// `UserNotFound` so it is indistinguishable from a missing one (no id oracle).
async fn require_target_user_in_realm(
    user_repository: &impl AdminUserRepository,
    realm_id: &str,
    user_id: Uuid,
) -> UserAdminResult<AdminUserEntity> {
    let user = user_repository
        .get_user_with_profile(user_id)
        .await?
        .ok_or_else(|| UserAdminError::UserNotFound(user_id.to_string()))?;
    if user.realm_id != realm_id {
        tracing::warn!(
            realm_id = realm_id,
            user_id = %user_id,
            target_realm_id = %user.realm_id,
            "Blocked admin operation on a user from another realm"
        );
        return Err(UserAdminError::UserNotFound(user_id.to_string()));
    }
    Ok(user)
}

/// Same target realm-boundary check for services that only hold a
/// [`UserRoleRepository`]: verifies the target user exists and belongs to
/// `realm_id`, returning `UserNotFound` otherwise.
async fn require_user_in_realm(
    user_role_repository: &impl UserRoleRepository,
    realm_id: &str,
    user_id: Uuid,
) -> UserAdminResult<()> {
    let user_realm = user_role_repository
        .get_user_realm(user_id)
        .await?
        .ok_or_else(|| UserAdminError::UserNotFound(user_id.to_string()))?;
    if user_realm != realm_id {
        tracing::warn!(
            realm_id = realm_id,
            user_id = %user_id,
            target_realm_id = %user_realm,
            "Blocked admin operation on a user from another realm"
        );
        return Err(UserAdminError::UserNotFound(user_id.to_string()));
    }
    Ok(())
}

/// Validate that every role id exists and belongs to `realm_id`.
///
/// The `user_roles` schema only foreign-keys `role_id -> roles(id)`, so a
/// foreign realm's role id would otherwise insert cleanly into this realm's
/// namespace. Mirrors the validation already performed by
/// `assign_api_key_roles`.
async fn require_roles_in_realm(
    role_policy_repository: &impl RolePolicyRepository,
    realm_id: &str,
    role_ids: &[Uuid],
) -> UserAdminResult<()> {
    if role_ids.is_empty() {
        return Ok(());
    }

    let roles = role_policy_repository.get_roles_by_ids(role_ids).await?;

    // `IN (...)` returns one row per distinct id, so compare against the
    // deduplicated count to reject unknown ids (and tolerate duplicates).
    let unique_ids: std::collections::HashSet<Uuid> = role_ids.iter().copied().collect();
    if roles.len() != unique_ids.len() {
        let found: std::collections::HashSet<Uuid> = roles.iter().map(|r| r.id).collect();
        let missing: Vec<String> = unique_ids
            .difference(&found)
            .map(|id| id.to_string())
            .collect();
        return Err(UserAdminError::RoleNotFound(missing.join(", ")));
    }

    if let Some(wrong_realm) = roles.iter().find(|r| r.realm_id != realm_id) {
        return Err(UserAdminError::RoleNotFound(wrong_realm.id.to_string()));
    }
    Ok(())
}

/// Hierarchy guard for role grants: assigning a privileged builtin role
/// (any builtin role except the plain end-user "user" role) requires the
/// caller to hold every permission that role grants. Without this, a
/// delegated admin holding only roles.manage/policies.manage could reach
/// primary-admin level by assigning e.g. the builtin realm-admin role to
/// themselves.
async fn require_role_grant_hierarchy<RP, P>(
    role_policy_repository: &RP,
    permission_checker: &P,
    identity: &Identity,
    realm_id: &str,
    role_ids: &[Uuid],
) -> UserAdminResult<()>
where
    RP: RolePolicyRepository,
    P: PermissionService,
{
    let requested_roles = role_policy_repository.get_roles_by_ids(role_ids).await?;
    for role in requested_roles
        .iter()
        .filter(|r| r.is_builtin && r.name != "user")
    {
        let policies = role_policy_repository
            .get_role_policies_for_user(realm_id, &[role.id])
            .await?;
        for policy in &policies {
            require_permission(
                permission_checker,
                identity,
                realm_id,
                &policy.resource,
                &policy.action,
                "Cannot assign a role granting a permission you do not hold",
            )
            .await?;
        }
    }
    Ok(())
}

// ============================================================================
// Admin User Service Implementation
// ============================================================================

pub struct AdminUserServiceImpl<R, UR, RP, P, AE, B>
where
    R: AdminUserRepository,
    UR: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
    AE: AuditEventRepository + 'static,
    B: BrowserTokenService,
{
    user_repository: Arc<R>,
    user_role_repository: Arc<UR>,
    role_policy_repository: Arc<RP>,
    permission_checker: Arc<P>,
    pub(crate) audit_event_repository: Arc<AE>,
    token_service: Arc<B>,
}

impl<R, UR, RP, P, AE, B> AdminUserServiceImpl<R, UR, RP, P, AE, B>
where
    R: AdminUserRepository,
    UR: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
    AE: AuditEventRepository + 'static,
    B: BrowserTokenService,
{
    pub fn new(
        user_repository: Arc<R>,
        user_role_repository: Arc<UR>,
        role_policy_repository: Arc<RP>,
        permission_checker: Arc<P>,
        audit_event_repository: Arc<AE>,
        token_service: Arc<B>,
    ) -> Self {
        Self {
            user_repository,
            user_role_repository,
            role_policy_repository,
            permission_checker,
            audit_event_repository,
            token_service,
        }
    }

    async fn hash_password(&self, password: &str) -> UserAdminResult<String> {
        bcrypt::hash(password, DEFAULT_BCRYPT_COST)
            .map_err(|e| UserAdminError::InternalError(format!("Password hashing failed: {}", e)))
    }

    async fn record_user_audit(
        &self,
        ctx: &AuditContext,
        realm_id: &str,
        action: AuditAction,
        target_id: String,
        target_name: Option<String>,
        result: AuditResult,
        reason: Option<&str>,
    ) {
        if let Err(e) = self
            .audit_event_repository
            .create(NewAuditEvent {
                realm_id: realm_id.to_string(),
                category: AuditCategory::UserManagement,
                action,
                actor_id: ctx.actor_id.clone(),
                actor_type: ctx.actor_type,
                actor_name: ctx.actor_name.clone(),
                target_type: AuditTargetType::User,
                target_id,
                target_name,
                result,
                details: reason.map(|reason| serde_json::json!({"reason": reason})),
                ip_address: ctx.ip_address.clone(),
                user_agent: ctx.user_agent.clone(),
                trace_id: ctx.trace_id.clone(),
            })
            .await
        {
            tracing::warn!(error = %e, "Failed to record audit event");
        }
    }
}

impl<R, UR, RP, P, AE, B> std::fmt::Debug for AdminUserServiceImpl<R, UR, RP, P, AE, B>
where
    R: AdminUserRepository,
    UR: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
    AE: AuditEventRepository + 'static,
    B: BrowserTokenService,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminUserServiceImpl").finish()
    }
}

impl<R, UR, RP, P, AE, B> AdminUserService for AdminUserServiceImpl<R, UR, RP, P, AE, B>
where
    R: AdminUserRepository,
    UR: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
    AE: AuditEventRepository + 'static,
    B: BrowserTokenService,
{
    async fn create_user_with_roles(
        &self,
        identity: Identity,
        ctx: AuditContext,
        realm_id: &str,
        request: CreateUserWithRolesRequest,
    ) -> UserAdminResult<AdminUser> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "users",
            "manage",
            "Insufficient permissions to create users",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            if let Err(e) = self
                .audit_event_repository
                .create(NewAuditEvent {
                    realm_id: realm_id.to_string(),
                    category: AuditCategory::UserManagement,
                    action: AuditAction::UserCreate,
                    actor_id: ctx.actor_id.clone(),
                    actor_type: ctx.actor_type,
                    actor_name: ctx.actor_name.clone(),
                    target_type: AuditTargetType::User,
                    target_id: "".to_string(),
                    target_name: Some(request.email.clone()),
                    result: AuditResult::Failure,
                    details: Some(serde_json::json!({"reason": "realm_boundary"})),
                    ip_address: ctx.ip_address.clone(),
                    user_agent: ctx.user_agent.clone(),
                    trace_id: ctx.trace_id.clone(),
                })
                .await
            {
                tracing::warn!(error = %e, "Failed to record audit event");
            }
            return Err(UserAdminError::PermissionDenied(
                "Cannot create users in a different realm".to_string(),
            ));
        }

        // Role validation must mirror assign_user_roles: without it, a caller
        // holding only users.manage could create an account carrying
        // realm-admin (privilege escalation) or a foreign realm's role id.
        require_roles_in_realm(&*self.role_policy_repository, realm_id, &request.role_ids).await?;
        // Hierarchy check (same rule as RoleAssignmentService::can_assign_roles):
        // without roles.manage a caller may grant no roles (the SDK ext API
        // always creates role-less users) or only the plain "user" role.
        let principal = identity.principal_ref();
        let can_manage_roles = self
            .permission_checker
            .check_principal_permission(
                realm_id,
                principal.principal_type,
                &principal.principal_id,
                "roles",
                "manage",
            )
            .await
            .unwrap_or(false);
        if !can_manage_roles {
            let no_or_plain_user_role = request.role_ids.is_empty()
                || (request.role_ids.len() == 1
                    && self
                        .role_policy_repository
                        .get_roles_by_ids(&request.role_ids)
                        .await?
                        .first()
                        .is_some_and(|role| role.name == "user"));
            if !no_or_plain_user_role {
                return Err(UserAdminError::PermissionDenied(
                    "Cannot assign these roles without roles.manage permission".to_string(),
                ));
            }
        }

        // Check if email already exists
        if self
            .user_repository
            .email_exists(realm_id, &request.email)
            .await?
        {
            if let Err(e) = self
                .audit_event_repository
                .create(NewAuditEvent {
                    realm_id: realm_id.to_string(),
                    category: AuditCategory::UserManagement,
                    action: AuditAction::UserCreate,
                    actor_id: ctx.actor_id.clone(),
                    actor_type: ctx.actor_type,
                    actor_name: ctx.actor_name.clone(),
                    target_type: AuditTargetType::User,
                    target_id: "".to_string(),
                    target_name: Some(request.email.clone()),
                    result: AuditResult::Failure,
                    details: Some(serde_json::json!({"reason": "duplicate_email"})),
                    ip_address: ctx.ip_address.clone(),
                    user_agent: ctx.user_agent.clone(),
                    trace_id: ctx.trace_id.clone(),
                })
                .await
            {
                tracing::warn!(error = %e, "Failed to record audit event");
            }
            return Err(UserAdminError::DuplicateEmail(request.email));
        }

        // Hash password
        let password_hash = match self.hash_password(&request.password).await {
            Ok(hash) => hash,
            Err(e) => {
                self.record_user_audit(
                    &ctx,
                    realm_id,
                    AuditAction::UserCreate,
                    "".to_string(),
                    Some(request.email.clone()),
                    AuditResult::Failure,
                    Some("password_hash_failed"),
                )
                .await;
                return Err(e);
            }
        };

        // Create user with profile
        let user_id = match self
            .user_repository
            .create_user_with_profile(
                realm_id,
                &request.email,
                &password_hash,
                request.nickname.as_deref(),
                request.status.unwrap_or(1),
            )
            .await
        {
            Ok(user_id) => user_id,
            Err(e) => {
                self.record_user_audit(
                    &ctx,
                    realm_id,
                    AuditAction::UserCreate,
                    "".to_string(),
                    Some(request.email.clone()),
                    AuditResult::Failure,
                    Some("create_user_failed"),
                )
                .await;
                return Err(e);
            }
        };

        if let Err(e) = self
            .user_role_repository
            .replace_user_roles(user_id, realm_id, "admin-web-console", &request.role_ids)
            .await
        {
            self.record_user_audit(
                &ctx,
                realm_id,
                AuditAction::UserCreate,
                user_id.to_string(),
                Some(request.email.clone()),
                AuditResult::Failure,
                Some("assign_roles_failed"),
            )
            .await;
            return Err(e);
        }

        // Fetch created user for response
        let user_entity = match self.user_repository.get_user_with_profile(user_id).await {
            Ok(Some(user)) => user,
            Ok(None) => {
                self.record_user_audit(
                    &ctx,
                    realm_id,
                    AuditAction::UserCreate,
                    user_id.to_string(),
                    Some(request.email.clone()),
                    AuditResult::Failure,
                    Some("fetch_created_user_failed"),
                )
                .await;
                return Err(UserAdminError::InternalError(
                    "Failed to fetch created user".to_string(),
                ));
            }
            Err(e) => {
                self.record_user_audit(
                    &ctx,
                    realm_id,
                    AuditAction::UserCreate,
                    user_id.to_string(),
                    Some(request.email.clone()),
                    AuditResult::Failure,
                    Some("fetch_created_user_failed"),
                )
                .await;
                return Err(e);
            }
        };

        let admin_user = AdminUser {
            id: user_entity.id,
            realm_id: user_entity.realm_id.clone(),
            email: user_entity.email.clone(),
            nickname: user_entity.nickname,
            status: user_entity.status,
            created_at: user_entity.created_at.to_rfc3339(),
        };

        // Record audit event (failure does not fail the operation)
        if let Err(e) = self
            .audit_event_repository
            .create(NewAuditEvent {
                realm_id: realm_id.to_string(),
                category: AuditCategory::UserManagement,
                action: AuditAction::UserCreate,
                actor_id: ctx.actor_id.clone(),
                actor_type: ctx.actor_type,
                actor_name: ctx.actor_name.clone(),
                target_type: AuditTargetType::User,
                target_id: admin_user.id.to_string(),
                target_name: Some(admin_user.email.clone()),
                result: AuditResult::Success,
                details: Some(serde_json::json!({"email": admin_user.email})),
                ip_address: ctx.ip_address.clone(),
                user_agent: ctx.user_agent.clone(),
                trace_id: ctx.trace_id.clone(),
            })
            .await
        {
            tracing::warn!(error = %e, "Failed to record audit event");
        }

        Ok(admin_user)
    }

    async fn update_user_admin(
        &self,
        identity: Identity,
        ctx: AuditContext,
        realm_id: &str,
        user_id: Uuid,
        request: UpdateUserAdminRequest,
    ) -> UserAdminResult<AdminUser> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "users",
            "manage",
            "Insufficient permissions to update users",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            if let Err(e) = self
                .audit_event_repository
                .create(NewAuditEvent {
                    realm_id: realm_id.to_string(),
                    category: AuditCategory::UserManagement,
                    action: AuditAction::UserUpdate,
                    actor_id: ctx.actor_id.clone(),
                    actor_type: ctx.actor_type,
                    actor_name: ctx.actor_name.clone(),
                    target_type: AuditTargetType::User,
                    target_id: user_id.to_string(),
                    target_name: None,
                    result: AuditResult::Failure,
                    details: Some(serde_json::json!({"reason": "realm_boundary"})),
                    ip_address: ctx.ip_address.clone(),
                    user_agent: ctx.user_agent.clone(),
                    trace_id: ctx.trace_id.clone(),
                })
                .await
            {
                tracing::warn!(error = %e, "Failed to record audit event");
            }
            return Err(UserAdminError::PermissionDenied(
                "Cannot update users in a different realm".to_string(),
            ));
        }

        // Load the target user before any write: this enforces the target
        // realm boundary (a cross-realm id must fail here, not mid-mutation)
        // and captures the old status for the Forbidden linkage.
        let target_user =
            match require_target_user_in_realm(&*self.user_repository, realm_id, user_id).await {
                Ok(user) => user,
                Err(e) => {
                    self.record_user_audit(
                        &ctx,
                        realm_id,
                        AuditAction::UserUpdate,
                        user_id.to_string(),
                        None,
                        AuditResult::Failure,
                        Some(if matches!(e, UserAdminError::UserNotFound(_)) {
                            "user_not_found"
                        } else {
                            "fetch_existing_user_failed"
                        }),
                    )
                    .await;
                    return Err(e);
                }
            };
        let old_status = Some(UserStatus::from(
            i16::try_from(target_user.status).unwrap_or(i16::from(UserStatus::Forbidden)),
        ));

        // Update user fields (email is read-only after creation)
        if let Err(e) = self
            .user_repository
            .update_user_fields(user_id, None, request.nickname.as_deref(), request.status)
            .await
        {
            self.record_user_audit(
                &ctx,
                realm_id,
                AuditAction::UserUpdate,
                user_id.to_string(),
                None,
                AuditResult::Failure,
                Some("update_user_failed"),
            )
            .await;
            return Err(e);
        }

        // Forbidden linkage: when the user transitions
        // INTO Forbidden, revoke all active sessions within the same logical
        // boundary. Non-target transitions and idempotent re-forbidding do
        // not revoke.
        let new_status = request
            .status
            .and_then(|s| i16::try_from(s).ok())
            .map(UserStatus::from);
        let mut linkage_triggered = false;
        if matches!(new_status, Some(UserStatus::Forbidden))
            && old_status != Some(UserStatus::Forbidden)
        {
            self.token_service
                .revoke_user_families(&user_id.to_string())
                .await
                .map_err(|e| {
                    tracing::error!(
                        error = %e,
                        user_id = %user_id,
                        "forbidden_linkage: revoke sessions failed"
                    );
                    UserAdminError::InternalError(format!("failed to revoke sessions: {e}"))
                })?;
            linkage_triggered = true;
        }

        // Fetch updated user
        let user_entity = match self.user_repository.get_user_with_profile(user_id).await {
            Ok(Some(user)) => user,
            Ok(None) => {
                self.record_user_audit(
                    &ctx,
                    realm_id,
                    AuditAction::UserUpdate,
                    user_id.to_string(),
                    None,
                    AuditResult::Failure,
                    Some("user_not_found"),
                )
                .await;
                return Err(UserAdminError::UserNotFound(user_id.to_string()));
            }
            Err(e) => {
                self.record_user_audit(
                    &ctx,
                    realm_id,
                    AuditAction::UserUpdate,
                    user_id.to_string(),
                    None,
                    AuditResult::Failure,
                    Some("fetch_updated_user_failed"),
                )
                .await;
                return Err(e);
            }
        };

        let admin_user = AdminUser {
            id: user_entity.id,
            realm_id: user_entity.realm_id.clone(),
            email: user_entity.email.clone(),
            nickname: user_entity.nickname,
            status: user_entity.status,
            created_at: user_entity.created_at.to_rfc3339(),
        };

        // Record audit event (failure does not fail the operation). When the
        // Forbidden linkage fired, annotate the details so the audit trail
        // records that session revocation was triggered.
        let audit_details = if linkage_triggered {
            serde_json::json!({
                "email": admin_user.email,
                "trigger": "forbidden_linkage",
                "scope": "all"
            })
        } else {
            serde_json::json!({ "email": admin_user.email })
        };
        if let Err(e) = self
            .audit_event_repository
            .create(NewAuditEvent {
                realm_id: realm_id.to_string(),
                category: AuditCategory::UserManagement,
                action: AuditAction::UserUpdate,
                actor_id: ctx.actor_id.clone(),
                actor_type: ctx.actor_type,
                actor_name: ctx.actor_name.clone(),
                target_type: AuditTargetType::User,
                target_id: admin_user.id.to_string(),
                target_name: Some(admin_user.email.clone()),
                result: AuditResult::Success,
                details: Some(audit_details),
                ip_address: ctx.ip_address.clone(),
                user_agent: ctx.user_agent.clone(),
                trace_id: ctx.trace_id.clone(),
            })
            .await
        {
            tracing::warn!(error = %e, "Failed to record audit event");
        }

        Ok(admin_user)
    }

    async fn get_user_admin(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
    ) -> UserAdminResult<AdminUser> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "users",
            "view",
            "Insufficient permissions to read users",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            return Err(UserAdminError::PermissionDenied(
                "Cannot read users in a different realm".to_string(),
            ));
        }

        let user_entity =
            require_target_user_in_realm(&*self.user_repository, realm_id, user_id).await?;

        Ok(AdminUser {
            id: user_entity.id,
            realm_id: user_entity.realm_id,
            email: user_entity.email,
            nickname: user_entity.nickname,
            status: user_entity.status,
            created_at: user_entity.created_at.to_rfc3339(),
        })
    }

    async fn delete_user(
        &self,
        identity: Identity,
        ctx: AuditContext,
        realm_id: &str,
        user_id: Uuid,
    ) -> UserAdminResult<()> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "users",
            "manage",
            "Insufficient permissions to delete users",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            if let Err(e) = self
                .audit_event_repository
                .create(NewAuditEvent {
                    realm_id: realm_id.to_string(),
                    category: AuditCategory::UserManagement,
                    action: AuditAction::UserDelete,
                    actor_id: ctx.actor_id.clone(),
                    actor_type: ctx.actor_type,
                    actor_name: ctx.actor_name.clone(),
                    target_type: AuditTargetType::User,
                    target_id: user_id.to_string(),
                    target_name: None,
                    result: AuditResult::Failure,
                    details: Some(serde_json::json!({"reason": "realm_boundary"})),
                    ip_address: ctx.ip_address.clone(),
                    user_agent: ctx.user_agent.clone(),
                    trace_id: ctx.trace_id.clone(),
                })
                .await
            {
                tracing::warn!(error = %e, "Failed to record audit event");
            }
            return Err(UserAdminError::PermissionDenied(
                "Cannot delete users in a different realm".to_string(),
            ));
        }

        // Target realm boundary: a realm admin must not be able to delete
        // another realm's user by id.
        if let Err(e) =
            require_target_user_in_realm(&*self.user_repository, realm_id, user_id).await
        {
            self.record_user_audit(
                &ctx,
                realm_id,
                AuditAction::UserDelete,
                user_id.to_string(),
                None,
                AuditResult::Failure,
                Some(if matches!(e, UserAdminError::UserNotFound(_)) {
                    "user_not_found"
                } else {
                    "fetch_target_user_failed"
                }),
            )
            .await;
            return Err(e);
        }

        // Delete user (transactional - profile and account)
        let deleted = match self.user_repository.delete_user(user_id).await {
            Ok(deleted) => deleted,
            Err(e) => {
                self.record_user_audit(
                    &ctx,
                    realm_id,
                    AuditAction::UserDelete,
                    user_id.to_string(),
                    None,
                    AuditResult::Failure,
                    Some("delete_user_failed"),
                )
                .await;
                return Err(e);
            }
        };

        if !deleted {
            self.record_user_audit(
                &ctx,
                realm_id,
                AuditAction::UserDelete,
                user_id.to_string(),
                None,
                AuditResult::Failure,
                Some("user_not_found"),
            )
            .await;
            return Err(UserAdminError::UserNotFound(user_id.to_string()));
        }

        // The account row is gone, but the user's browser-token families live in
        // Redis and outlive the row. Revoke them so a leaked refresh token
        // cannot keep rotating until its absolute expiry (matches the
        // self-delete and password-reset paths). Best-effort: the delete has
        // already committed; access tokens are also rejected by the identity
        // middleware's user lookup, so a failure here is logged, not fatal.
        if let Err(e) = self
            .token_service
            .revoke_user_families(&user_id.to_string())
            .await
        {
            tracing::error!(
                error = %e,
                user_id = %user_id,
                "delete_user: failed to revoke sessions"
            );
        }

        // Record audit event (failure does not fail the operation)
        if let Err(e) = self
            .audit_event_repository
            .create(NewAuditEvent {
                realm_id: realm_id.to_string(),
                category: AuditCategory::UserManagement,
                action: AuditAction::UserDelete,
                actor_id: ctx.actor_id.clone(),
                actor_type: ctx.actor_type,
                actor_name: ctx.actor_name.clone(),
                target_type: AuditTargetType::User,
                target_id: user_id.to_string(),
                target_name: None,
                result: AuditResult::Success,
                details: None,
                ip_address: ctx.ip_address.clone(),
                user_agent: ctx.user_agent.clone(),
                trace_id: ctx.trace_id.clone(),
            })
            .await
        {
            tracing::warn!(error = %e, "Failed to record audit event");
        }

        Ok(())
    }

    async fn reset_user_password(
        &self,
        identity: Identity,
        ctx: AuditContext,
        realm_id: &str,
        user_id: Uuid,
    ) -> UserAdminResult<String> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "users",
            "manage",
            "Insufficient permissions to manage users",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            if let Err(e) = self
                .audit_event_repository
                .create(NewAuditEvent {
                    realm_id: realm_id.to_string(),
                    category: AuditCategory::UserManagement,
                    action: AuditAction::UserUpdate,
                    actor_id: ctx.actor_id.clone(),
                    actor_type: ctx.actor_type,
                    actor_name: ctx.actor_name.clone(),
                    target_type: AuditTargetType::User,
                    target_id: user_id.to_string(),
                    target_name: None,
                    result: AuditResult::Failure,
                    details: Some(serde_json::json!({"reason": "realm_boundary"})),
                    ip_address: ctx.ip_address.clone(),
                    user_agent: ctx.user_agent.clone(),
                    trace_id: ctx.trace_id.clone(),
                })
                .await
            {
                tracing::warn!(error = %e, "Failed to record audit event");
            }
            return Err(UserAdminError::PermissionDenied(
                "Cannot reset passwords for users in a different realm".to_string(),
            ));
        }

        // Target realm boundary: a realm admin must not be able to reset
        // another realm's user password by id.
        if let Err(e) =
            require_target_user_in_realm(&*self.user_repository, realm_id, user_id).await
        {
            self.record_user_audit(
                &ctx,
                realm_id,
                AuditAction::UserUpdate,
                user_id.to_string(),
                None,
                AuditResult::Failure,
                Some(if matches!(e, UserAdminError::UserNotFound(_)) {
                    "user_not_found"
                } else {
                    "fetch_target_user_failed"
                }),
            )
            .await;
            return Err(e);
        }

        // Generate 16-character random password
        let new_password = generate_random_password();

        // Hash the new password
        let password_hash = match self.hash_password(&new_password).await {
            Ok(hash) => hash,
            Err(e) => {
                self.record_user_audit(
                    &ctx,
                    realm_id,
                    AuditAction::UserUpdate,
                    user_id.to_string(),
                    None,
                    AuditResult::Failure,
                    Some("password_hash_failed"),
                )
                .await;
                return Err(e);
            }
        };

        // Update password in database
        let updated = match self
            .user_repository
            .update_user_password(user_id, &password_hash)
            .await
        {
            Ok(updated) => updated,
            Err(e) => {
                self.record_user_audit(
                    &ctx,
                    realm_id,
                    AuditAction::UserUpdate,
                    user_id.to_string(),
                    None,
                    AuditResult::Failure,
                    Some("update_password_failed"),
                )
                .await;
                return Err(e);
            }
        };

        if !updated {
            if let Err(e) = self
                .audit_event_repository
                .create(NewAuditEvent {
                    realm_id: realm_id.to_string(),
                    category: AuditCategory::UserManagement,
                    action: AuditAction::UserUpdate,
                    actor_id: ctx.actor_id.clone(),
                    actor_type: ctx.actor_type,
                    actor_name: ctx.actor_name.clone(),
                    target_type: AuditTargetType::User,
                    target_id: user_id.to_string(),
                    target_name: None,
                    result: AuditResult::Failure,
                    details: Some(serde_json::json!({"reason": "user_not_found"})),
                    ip_address: ctx.ip_address.clone(),
                    user_agent: ctx.user_agent.clone(),
                    trace_id: ctx.trace_id.clone(),
                })
                .await
            {
                tracing::warn!(error = %e, "Failed to record audit event");
            }
            return Err(UserAdminError::UserNotFound(user_id.to_string()));
        }

        // Revoke all active sessions for the target user after an admin-initiated
        // password reset, so any existing (potentially compromised) session is
        // invalidated. Best-effort: the password has already been changed, so a
        // revocation failure is logged but does not prevent the new password from
        // being returned to the admin.
        if let Err(e) = self
            .token_service
            .revoke_user_families(&user_id.to_string())
            .await
        {
            tracing::error!(
                error = %e,
                user_id = %user_id,
                "reset_user_password: failed to revoke sessions"
            );
        }

        // Record success audit event (failure does not fail the operation)
        if let Err(e) = self
            .audit_event_repository
            .create(NewAuditEvent {
                realm_id: realm_id.to_string(),
                category: AuditCategory::UserManagement,
                action: AuditAction::UserUpdate,
                actor_id: ctx.actor_id.clone(),
                actor_type: ctx.actor_type,
                actor_name: ctx.actor_name.clone(),
                target_type: AuditTargetType::User,
                target_id: user_id.to_string(),
                target_name: None,
                result: AuditResult::Success,
                details: Some(serde_json::json!({
                    "action": "reset_password",
                    "user_id": user_id.to_string(),
                    "sessions_revoked": "all",
                })),
                ip_address: ctx.ip_address.clone(),
                user_agent: ctx.user_agent.clone(),
                trace_id: ctx.trace_id.clone(),
            })
            .await
        {
            tracing::warn!(error = %e, "Failed to record audit event");
        }

        Ok(new_password)
    }
}

/// Password character categories used by `generate_random_password`.
const PW_CATEGORIES: &[&[u8]] = &[
    b"abcdefghijklmnopqrstuvwxyz",
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZ",
    b"0123456789",
    b"!@#$%^&*",
];

/// Generate random password (alphanumeric + special characters, 16 characters)
/// Guarantees at least one character from each category: lowercase, uppercase, digit, special.
fn generate_random_password() -> String {
    use rand::Rng;
    use rand::seq::SliceRandom;

    let all_chars: Vec<u8> = PW_CATEGORIES
        .iter()
        .flat_map(|c| c.iter().copied())
        .collect();
    let mut rng = rand::thread_rng();

    // Guarantee one from each category
    let mut chars: Vec<char> = Vec::with_capacity(16);
    for &cat in PW_CATEGORIES {
        chars.push(cat[rng.gen_range(0..cat.len())] as char);
    }

    // Fill remaining slots from full charset
    for _ in 4..16 {
        chars.push(all_chars[rng.gen_range(0..all_chars.len())] as char);
    }

    chars.shuffle(&mut rng);
    chars.into_iter().collect()
}

// ============================================================================
// Role Assignment Service Implementation
// ============================================================================

pub struct RoleAssignmentServiceImpl<R, RP, P>
where
    R: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
{
    user_role_repository: Arc<R>,
    role_policy_repository: Arc<RP>,
    permission_checker: Arc<P>,
}

impl<R, RP, P> RoleAssignmentServiceImpl<R, RP, P>
where
    R: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
{
    pub fn new(
        user_role_repository: Arc<R>,
        role_policy_repository: Arc<RP>,
        permission_checker: Arc<P>,
    ) -> Self {
        Self {
            user_role_repository,
            role_policy_repository,
            permission_checker,
        }
    }
}

impl<R, RP, P> RoleAssignmentService for RoleAssignmentServiceImpl<R, RP, P>
where
    R: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
{
    async fn assign_user_roles(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
        role_ids: Vec<Uuid>,
    ) -> UserAdminResult<()> {
        // Target realm boundary: the user being modified must belong to this
        // realm, and every role id must exist in it.
        require_user_in_realm(&*self.user_role_repository, realm_id, user_id).await?;
        require_roles_in_realm(&*self.role_policy_repository, realm_id, &role_ids).await?;

        // Check if can assign roles
        if !self
            .can_assign_roles(&identity, realm_id, &role_ids)
            .await?
        {
            return Err(UserAdminError::PermissionDenied(
                "Cannot assign these roles without roles.manage permission".to_string(),
            ));
        }

        // Hierarchy guard: a delegated roles.manage holder must not reach
        // primary-admin level by assigning a privileged builtin role (e.g.
        // realm-admin) to themselves. Assigning such a role requires holding
        // every permission it grants. The plain builtin "user" role is exempt
        // (it is the default end-user role).
        require_role_grant_hierarchy(
            &*self.role_policy_repository,
            &*self.permission_checker,
            &identity,
            realm_id,
            &role_ids,
        )
        .await?;

        // Get client ID (hardcoded for now, should come from state)
        let client_id = "admin-web-console";

        // Replace user roles
        self.user_role_repository
            .replace_user_roles(user_id, realm_id, client_id, &role_ids)
            .await?;

        // Invalidate cache
        let _ = self
            .permission_checker
            .invalidate_user_role_cache(realm_id, &user_id.to_string())
            .await;

        Ok(())
    }

    async fn get_user_roles(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
    ) -> UserAdminResult<Vec<RoleDetail>> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "users",
            "view",
            "Insufficient permissions to read user roles",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            return Err(UserAdminError::PermissionDenied(
                "Cannot read roles in a different realm".to_string(),
            ));
        }

        // Target realm boundary: must not enumerate another realm's user roles.
        require_user_in_realm(&*self.user_role_repository, realm_id, user_id).await?;

        let roles = self.user_role_repository.get_user_roles(user_id).await?;

        Ok(roles
            .into_iter()
            .map(|r| RoleDetail {
                id: r.id,
                name: r.name,
                description: r.description,
                source: r.source,
                source_id: r.source_id,
                expires_at: r.expires_at.map(|dt| dt.to_rfc3339()),
            })
            .collect())
    }

    async fn can_assign_roles(
        &self,
        identity: &Identity,
        realm_id: &str,
        role_ids: &[Uuid],
    ) -> UserAdminResult<bool> {
        // Check if principal has roles.manage permission
        let principal = identity.principal_ref();
        let can_manage_roles = self
            .permission_checker
            .check_principal_permission(
                realm_id,
                principal.principal_type,
                &principal.principal_id,
                "roles",
                "manage",
            )
            .await
            .unwrap_or(false);

        if can_manage_roles {
            return Ok(true);
        }

        // If no roles.manage permission, can only assign "user" role
        if role_ids.len() != 1 {
            return Ok(false);
        }

        let roles = self
            .role_policy_repository
            .get_roles_by_ids(role_ids)
            .await?;

        if roles.len() != 1 {
            return Ok(false);
        }

        Ok(roles[0].name == "user")
    }

    async fn assign_api_key_roles(
        &self,
        identity: Identity,
        realm_id: &str,
        api_key_id: &str,
        role_ids: Vec<Uuid>,
    ) -> UserAdminResult<()> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "roles",
            "manage",
            "Insufficient permissions to manage roles",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            return Err(UserAdminError::PermissionDenied(
                "Cannot manage roles in a different realm".to_string(),
            ));
        }

        // Validate roles: all must exist in realm, none can be builtin
        if !role_ids.is_empty() {
            let roles = self
                .role_policy_repository
                .get_roles_by_ids(&role_ids)
                .await?;

            // Check all roles were found
            if roles.len() != role_ids.len() {
                let found_ids: std::collections::HashSet<Uuid> =
                    roles.iter().map(|r| r.id).collect();
                let missing: Vec<String> = role_ids
                    .iter()
                    .filter(|id| !found_ids.contains(id))
                    .map(|id| id.to_string())
                    .collect();
                return Err(UserAdminError::RoleNotFound(missing.join(", ")));
            }

            // Check none are builtin
            if let Some(builtin) = roles.iter().find(|r| r.is_builtin) {
                return Err(UserAdminError::InvalidRoleAssignment(format!(
                    "Cannot assign builtin role '{}' to API Key",
                    builtin.name
                )));
            }

            // Check all belong to realm
            if let Some(wrong_realm) = roles.iter().find(|r| r.realm_id != realm_id) {
                return Err(UserAdminError::RoleNotFound(wrong_realm.id.to_string()));
            }
        }

        // Use the built-in API Key client ID
        let client_id = crate::client_api_keys::constants::ADMIN_API_CLIENT_ID;

        self.user_role_repository
            .replace_api_key_roles(api_key_id, realm_id, client_id, &role_ids)
            .await?;

        // Invalidate principal cache
        if let Err(e) = self
            .permission_checker
            .invalidate_principal_role_cache(realm_id, "api_key", api_key_id)
            .await
        {
            tracing::error!(
                error = %e,
                api_key_id = %api_key_id,
                "Failed to invalidate API key role cache"
            );
        }

        Ok(())
    }

    async fn get_api_key_roles(
        &self,
        identity: Identity,
        realm_id: &str,
        api_key_id: &str,
    ) -> UserAdminResult<Vec<RoleEntity>> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "api_keys",
            "view",
            "Insufficient permissions to view API keys",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            return Err(UserAdminError::PermissionDenied(
                "Cannot view API key roles in a different realm".to_string(),
            ));
        }

        self.user_role_repository
            .get_api_key_roles(api_key_id)
            .await
    }
}

// ============================================================================
// User Permission Service Implementation
// ============================================================================

pub struct UserPermissionServiceImpl<R, RP, P>
where
    R: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
{
    user_role_repository: Arc<R>,
    role_policy_repository: Arc<RP>,
    permission_checker: Arc<P>,
}

impl<R, RP, P> UserPermissionServiceImpl<R, RP, P>
where
    R: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
{
    pub fn new(
        user_role_repository: Arc<R>,
        role_policy_repository: Arc<RP>,
        permission_checker: Arc<P>,
    ) -> Self {
        Self {
            user_role_repository,
            role_policy_repository,
            permission_checker,
        }
    }
}

impl<R, RP, P> UserPermissionService for UserPermissionServiceImpl<R, RP, P>
where
    R: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
{
    async fn get_effective_permissions(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
    ) -> UserAdminResult<Vec<PermissionDetail>> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "users",
            "view",
            "Insufficient permissions to read user permissions",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            return Err(UserAdminError::PermissionDenied(
                "Cannot read permissions in a different realm".to_string(),
            ));
        }

        // Target realm boundary: must not enumerate another realm's user
        // permissions (role ids and names leak through the details below).
        require_user_in_realm(&*self.user_role_repository, realm_id, user_id).await?;

        // 1. Get user's role IDs
        let role_ids = self.user_role_repository.get_user_role_ids(user_id).await?;

        // 2. Get role policies (from assigned roles)
        let mut permissions = Vec::new();

        if !role_ids.is_empty() {
            // Get role policies
            let role_policies = self
                .role_policy_repository
                .get_role_policies_for_user(realm_id, &role_ids)
                .await?;

            // Get role details for mapping
            let roles = self
                .role_policy_repository
                .get_roles_by_ids(&role_ids)
                .await?;

            // Create a role ID -> role name map
            let role_map: std::collections::HashMap<Uuid, String> =
                roles.into_iter().map(|r| (r.id, r.name)).collect();

            // Add role-based permissions
            // Note: Currently get_role_policies_for_user doesn't return which role they came from
            // We'll mark all as coming from the first role for now
            // TODO: Enhance get_role_policies_for_user to return role information
            for policy in role_policies {
                // For now, assign to first role (this is a limitation of current implementation)
                let first_role_id = role_ids.first().copied().unwrap_or_default();
                let role_name = role_map
                    .get(&first_role_id)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());

                permissions.push(PermissionDetail {
                    resource: policy.resource,
                    action: policy.action,
                    source: PermissionSource::Role {
                        role_id: first_role_id,
                        role_name,
                    },
                });
            }
        }

        // 3. Get direct user policies (not via roles)
        let direct_policies = self
            .role_policy_repository
            .get_direct_user_policies(user_id)
            .await?;

        // Add direct permissions
        for policy in direct_policies {
            permissions.push(PermissionDetail {
                resource: policy.resource,
                action: policy.action,
                source: PermissionSource::Direct,
            });
        }

        // 4. Deduplicate (direct permissions take precedence over role permissions)
        let mut seen = std::collections::HashSet::new();
        let mut unique_permissions = Vec::new();

        for perm in permissions {
            let key = (perm.resource.clone(), perm.action.clone());
            if !seen.contains(&key) {
                seen.insert(key);
                unique_permissions.push(perm);
            }
        }

        Ok(unique_permissions)
    }

    async fn assign_direct_permission(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
        policy_id: Uuid,
    ) -> UserAdminResult<()> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "permissions",
            "manage",
            "Insufficient permissions to assign direct permissions",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            return Err(UserAdminError::PermissionDenied(
                "Cannot assign permissions in a different realm".to_string(),
            ));
        }

        self.role_policy_repository
            .assign_direct_permission(user_id, realm_id, policy_id)
            .await?;

        // Invalidate cache
        let _ = self
            .permission_checker
            .invalidate_user_role_cache(realm_id, &user_id.to_string())
            .await;

        Ok(())
    }

    async fn remove_direct_permission(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
        policy_id: Uuid,
    ) -> UserAdminResult<()> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "permissions",
            "manage",
            "Insufficient permissions to remove direct permissions",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            return Err(UserAdminError::PermissionDenied(
                "Cannot remove permissions in a different realm".to_string(),
            ));
        }

        self.role_policy_repository
            .remove_direct_permission(user_id, policy_id)
            .await?;

        // Invalidate cache
        let _ = self
            .permission_checker
            .invalidate_user_role_cache(realm_id, &user_id.to_string())
            .await;

        Ok(())
    }
}

// ============================================================================
// Permission Management Service Implementation
// ============================================================================

pub struct PermissionManagementServiceImpl<UR, RP, P, AE>
where
    UR: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
    AE: AuditEventRepository + 'static,
{
    user_role_repository: Arc<UR>,
    role_policy_repository: Arc<RP>,
    permission_checker: Arc<P>,
    audit_event_repository: Arc<AE>,
}

impl<UR, RP, P, AE> PermissionManagementServiceImpl<UR, RP, P, AE>
where
    UR: UserRoleRepository,
    RP: RolePolicyRepository,
    P: PermissionService,
    AE: AuditEventRepository + 'static,
{
    pub fn new(
        user_role_repository: Arc<UR>,
        role_policy_repository: Arc<RP>,
        permission_checker: Arc<P>,
        audit_event_repository: Arc<AE>,
    ) -> Self {
        Self {
            user_role_repository,
            role_policy_repository,
            permission_checker,
            audit_event_repository,
        }
    }

    async fn record_rbac_audit(
        &self,
        ctx: &AuditContext,
        realm_id: &str,
        action: AuditAction,
        target_type: AuditTargetType,
        target_id: String,
        target_name: Option<String>,
        details: serde_json::Value,
    ) {
        if let Err(e) = self
            .audit_event_repository
            .create(NewAuditEvent {
                realm_id: realm_id.to_string(),
                category: AuditCategory::Rbac,
                action,
                actor_id: ctx.actor_id.clone(),
                actor_type: ctx.actor_type,
                actor_name: ctx.actor_name.clone(),
                target_type,
                target_id,
                target_name,
                result: AuditResult::Success,
                details: Some(details),
                ip_address: ctx.ip_address.clone(),
                user_agent: ctx.user_agent.clone(),
                trace_id: ctx.trace_id.clone(),
            })
            .await
        {
            tracing::warn!(error = %e, "Failed to record audit event");
        }
    }
}

impl<UR, RP, P, AE> PermissionManagementService for PermissionManagementServiceImpl<UR, RP, P, AE>
where
    UR: UserRoleRepository + 'static,
    RP: RolePolicyRepository + 'static,
    P: PermissionService + 'static,
    AE: AuditEventRepository + 'static,
{
    async fn create_permission(
        &self,
        identity: Identity,
        ctx: AuditContext,
        realm_id: &str,
        client_id: &str,
        role_id: Option<Uuid>,
        user_id: Option<Uuid>,
        role: Option<Uuid>,
        resource: Option<String>,
        action: Option<String>,
    ) -> UserAdminResult<()> {
        // Principal-based permission check: policies:manage
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "policies",
            "manage",
            "Insufficient permissions to manage policies",
        )
        .await?;

        // Realm boundary check
        if identity.realm_id() != realm_id {
            return Err(UserAdminError::PermissionDenied(
                "Cannot manage policies in a different realm".to_string(),
            ));
        }

        // Target realm boundaries: the user receiving the role and the role
        // being granted or policy-attached must all belong to this realm.
        if let Some(uid) = user_id {
            require_user_in_realm(&*self.user_role_repository, realm_id, uid).await?;
        }
        let mut involved_roles = Vec::new();
        if let Some(rid) = role_id {
            involved_roles.push(rid);
        }
        if let Some(r) = role {
            involved_roles.push(r);
        }
        require_roles_in_realm(&*self.role_policy_repository, realm_id, &involved_roles).await?;

        // Hierarchy guard for the RoleWrap (user ← role) grant: this path
        // assigns roles with only policies.manage, so it must enforce the
        // same builtin-role escalation guard as assign_user_roles —
        // otherwise a delegated policies.manage holder could self-assign the
        // builtin realm-admin role and take over the realm.
        if let Some(r) = role {
            require_role_grant_hierarchy(
                &*self.role_policy_repository,
                &*self.permission_checker,
                &identity,
                realm_id,
                &[r],
            )
            .await?;
        }

        // Self-holds guard for the PoliceWrap (role ← policy) grant: this
        // path attaches an arbitrary (resource, action) to a role, so the
        // caller must hold that permission themselves — otherwise a delegated
        // policies.manage holder could grant a role a permission they lack
        // and benefit from it via the role. Mirrors add_policy_to_role and
        // direct user-permission assignment.
        if let (Some(res), Some(act)) = (&resource, &action) {
            require_permission(
                &*self.permission_checker,
                &identity,
                realm_id,
                res,
                act,
                "Cannot grant a permission you do not hold",
            )
            .await?;
        }

        // Create role policy
        if let (Some(rid), Some(res), Some(act)) = (role_id, resource, action) {
            self.role_policy_repository
                .create_role_policy(rid, realm_id, &res, &act)
                .await?;
            self.record_rbac_audit(
                &ctx,
                realm_id,
                AuditAction::PermissionGrant,
                AuditTargetType::Role,
                rid.to_string(),
                None,
                serde_json::json!({"resource": res, "action": act}),
            )
            .await;

            // Invalidate realm cache
            let _ = self
                .permission_checker
                .invalidate_realm_cache(realm_id)
                .await;
        }

        // Add user role
        if let (Some(uid), Some(r)) = (user_id, role) {
            self.user_role_repository
                .add_user_role(uid, r, realm_id, client_id)
                .await?;
            self.record_rbac_audit(
                &ctx,
                realm_id,
                AuditAction::RoleAssign,
                AuditTargetType::User,
                uid.to_string(),
                None,
                serde_json::json!({"role_id": r, "client_id": client_id}),
            )
            .await;

            // Invalidate user cache
            let _ = self
                .permission_checker
                .invalidate_user_role_cache(realm_id, &uid.to_string())
                .await;
        }

        Ok(())
    }

    async fn delete_permission(
        &self,
        identity: Identity,
        ctx: AuditContext,
        realm_id: &str,
        client_id: &str,
        role_id: Option<Uuid>,
        user_id: Option<Uuid>,
        role: Option<Uuid>,
        resource: Option<String>,
        action: Option<String>,
    ) -> UserAdminResult<()> {
        require_permission(
            &*self.permission_checker,
            &identity,
            realm_id,
            "policies",
            "manage",
            "Insufficient permissions to manage policies",
        )
        .await?;

        if identity.realm_id() != realm_id {
            return Err(UserAdminError::PermissionDenied(
                "Cannot manage policies in a different realm".to_string(),
            ));
        }

        // Target realm boundaries (see create_permission): the affected user,
        // the unassigned role, and the policy's role must belong to this realm.
        if let Some(uid) = user_id {
            require_user_in_realm(&*self.user_role_repository, realm_id, uid).await?;
        }
        let mut involved_roles = Vec::new();
        if let Some(rid) = role_id {
            involved_roles.push(rid);
        }
        if let Some(r) = role {
            involved_roles.push(r);
        }
        require_roles_in_realm(&*self.role_policy_repository, realm_id, &involved_roles).await?;

        // Delete role policy
        if let (Some(rid), Some(res), Some(act)) = (role_id, resource, action) {
            self.role_policy_repository
                .delete_role_policy(rid, &res, &act)
                .await?;
            self.record_rbac_audit(
                &ctx,
                realm_id,
                AuditAction::PermissionRevoke,
                AuditTargetType::Role,
                rid.to_string(),
                None,
                serde_json::json!({"resource": res, "action": act}),
            )
            .await;

            // Invalidate realm cache
            let _ = self
                .permission_checker
                .invalidate_realm_cache(realm_id)
                .await;
        }

        // Remove user role
        if let (Some(uid), Some(r)) = (user_id, role) {
            self.user_role_repository
                .remove_user_role(uid, r, client_id)
                .await?;
            self.record_rbac_audit(
                &ctx,
                realm_id,
                AuditAction::RoleUnassign,
                AuditTargetType::User,
                uid.to_string(),
                None,
                serde_json::json!({"role_id": r, "client_id": client_id}),
            )
            .await;

            // Invalidate user cache
            let _ = self
                .permission_checker
                .invalidate_user_role_cache(realm_id, &uid.to_string())
                .await;
        }

        Ok(())
    }

    async fn list_permissions(
        &self,
        realm_id: &str,
        client_id: &str,
    ) -> UserAdminResult<PermissionListData> {
        let role_policies = self
            .role_policy_repository
            .list_role_policies_by_realm(realm_id)
            .await?;

        let user_roles = self
            .user_role_repository
            .list_user_roles_by_realm_client(realm_id, client_id)
            .await?;

        Ok(PermissionListData {
            role_policies,
            user_roles,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{AuditEvent, AuditEventFilters, PaginatedAuditEvents};
    use crate::authentication::entities::{BrowserAccessTokenData, BrowserTokenSet};
    use crate::user::admin_entities::{AdminUserEntity, PolicyEntity, RoleEntity};
    use crate::user::{GrantRoleOutcome, RevokeRoleOutcome};
    use chrono::DateTime;
    use chrono::Utc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn has_lowercase(s: &str) -> bool {
        s.chars().any(|c| c.is_ascii_lowercase())
    }

    fn has_uppercase(s: &str) -> bool {
        s.chars().any(|c| c.is_ascii_uppercase())
    }

    fn has_digit(s: &str) -> bool {
        s.chars().any(|c| c.is_ascii_digit())
    }

    fn has_special(s: &str) -> bool {
        let special_chars = PW_CATEGORIES[3]
            .iter()
            .map(|&b| b as char)
            .collect::<Vec<_>>();
        s.chars().any(|c| special_chars.contains(&c))
    }

    #[test]
    fn generate_random_password_has_length_16() {
        let password = generate_random_password();
        assert_eq!(password.len(), 16);
    }

    #[test]
    fn generate_random_password_guarantees_category_diversity() {
        // Every generated password must contain at least one character from each category.
        // This is the core security invariant: uniform sampling over a flat charset
        // would leave ~7.6% of passwords missing digits and ~43% missing specials.
        for _ in 0..100 {
            let password = generate_random_password();
            assert!(has_lowercase(&password), "missing lowercase: {password}");
            assert!(has_uppercase(&password), "missing uppercase: {password}");
            assert!(has_digit(&password), "missing digit: {password}");
            assert!(has_special(&password), "missing special: {password}");
        }
    }

    #[test]
    fn generate_random_password_only_uses_charset_characters() {
        let all_chars: std::collections::HashSet<char> = PW_CATEGORIES
            .iter()
            .flat_map(|cat| cat.iter().map(|&b| b as char))
            .collect();
        for _ in 0..50 {
            for c in generate_random_password().chars() {
                assert!(all_chars.contains(&c), "unexpected character: {c}");
            }
        }
    }

    // ========================================================================
    // Forbidden linkage — hand-rolled mock ports.
    // The port traits return `impl Future` (non-object-safe), so mockall is
    // unusable. Only the methods exercised by `update_user_admin` return
    // meaningful values; the rest panic to surface accidental coupling.
    // ========================================================================

    /// Permission checker that always grants the requested permission.
    struct AlwaysAllowPermission;
    impl PermissionService for AlwaysAllowPermission {
        async fn check_permission(
            &self,
            _realm_id: &str,
            _user_id: &str,
            _resource: &str,
            _action: &str,
        ) -> Result<bool, CoreError> {
            Ok(true)
        }
        async fn get_user_roles(
            &self,
            _realm_id: &str,
            _user_id: &str,
        ) -> Result<Vec<String>, CoreError> {
            Ok(vec![])
        }
        async fn get_role_policies(
            &self,
            _realm_id: &str,
            _role_id: &str,
        ) -> Result<Vec<crate::authorization::Policy>, CoreError> {
            Ok(vec![])
        }
        async fn invalidate_user_role_cache(
            &self,
            _realm_id: &str,
            _user_id: &str,
        ) -> Result<(), CoreError> {
            Ok(())
        }
        async fn invalidate_role_policy_cache(
            &self,
            _realm_id: &str,
            _role_id: &str,
        ) -> Result<(), CoreError> {
            Ok(())
        }
        async fn invalidate_realm_cache(&self, _realm_id: &str) -> Result<(), CoreError> {
            Ok(())
        }
        async fn get_user_permissions(
            &self,
            _realm_id: &str,
            _user_id: &str,
        ) -> Result<Vec<String>, CoreError> {
            Ok(vec![])
        }
        async fn check_principal_permission(
            &self,
            _realm_id: &str,
            _principal_type: &str,
            _principal_id: &str,
            _resource: &str,
            _action: &str,
        ) -> Result<bool, CoreError> {
            Ok(true)
        }
        async fn invalidate_principal_role_cache(
            &self,
            _realm_id: &str,
            _principal_type: &str,
            _principal_id: &str,
        ) -> Result<(), CoreError> {
            Ok(())
        }
    }

    /// Permission checker that only allows an explicit (resource, action)
    /// allowlist — models a delegated sub-admin for the assign-hierarchy guard
    /// tests (e.g. holds roles.manage but NOT users.manage).
    struct SelectivePermission {
        allowed: Vec<(&'static str, &'static str)>,
    }
    impl PermissionService for SelectivePermission {
        async fn check_permission(
            &self,
            _realm_id: &str,
            _user_id: &str,
            resource: &str,
            action: &str,
        ) -> Result<bool, CoreError> {
            Ok(self.allowed.contains(&(resource, action)))
        }
        async fn get_user_roles(
            &self,
            _realm_id: &str,
            _user_id: &str,
        ) -> Result<Vec<String>, CoreError> {
            Ok(vec![])
        }
        async fn get_role_policies(
            &self,
            _realm_id: &str,
            _role_id: &str,
        ) -> Result<Vec<crate::authorization::Policy>, CoreError> {
            Ok(vec![])
        }
        async fn invalidate_user_role_cache(
            &self,
            _realm_id: &str,
            _user_id: &str,
        ) -> Result<(), CoreError> {
            Ok(())
        }
        async fn invalidate_role_policy_cache(
            &self,
            _realm_id: &str,
            _role_id: &str,
        ) -> Result<(), CoreError> {
            Ok(())
        }
        async fn invalidate_realm_cache(&self, _realm_id: &str) -> Result<(), CoreError> {
            Ok(())
        }
        async fn get_user_permissions(
            &self,
            _realm_id: &str,
            _user_id: &str,
        ) -> Result<Vec<String>, CoreError> {
            Ok(vec![])
        }
        async fn check_principal_permission(
            &self,
            _realm_id: &str,
            _principal_type: &str,
            _principal_id: &str,
            resource: &str,
            action: &str,
        ) -> Result<bool, CoreError> {
            Ok(self.allowed.contains(&(resource, action)))
        }
        async fn invalidate_principal_role_cache(
            &self,
            _realm_id: &str,
            _principal_type: &str,
            _principal_id: &str,
        ) -> Result<(), CoreError> {
            Ok(())
        }
    }

    /// In-memory admin user repository. `get_user_with_profile` and
    /// `update_user_fields` are exercised by `update_user_admin`; the call
    /// counters back the cross-realm regression assertions (no write may
    /// happen for a foreign target).
    struct MockAdminUserRepo {
        row: Arc<Mutex<Option<AdminUserEntity>>>,
        update_calls: Arc<AtomicUsize>,
        delete_calls: Arc<AtomicUsize>,
        password_calls: Arc<AtomicUsize>,
    }
    impl AdminUserRepository for MockAdminUserRepo {
        async fn create_user_with_profile(
            &self,
            _realm_id: &str,
            _email: &str,
            _password_hash: &str,
            _nickname: Option<&str>,
            _status: i32,
        ) -> UserAdminResult<Uuid> {
            Ok(Uuid::nil())
        }
        async fn update_user_fields(
            &self,
            user_id: Uuid,
            _email: Option<&str>,
            nickname: Option<&str>,
            status: Option<i32>,
        ) -> UserAdminResult<()> {
            let row = self.row.clone();
            let calls = self.update_calls.clone();
            calls.fetch_add(1, Ordering::SeqCst);
            let mut guard = row.lock().unwrap();
            if let Some(e) = guard.as_mut() {
                if let Some(nick) = nickname {
                    e.nickname = Some(nick.to_string());
                }
                if let Some(s) = status {
                    e.status = s;
                }
                e.updated_at = Utc::now();
            }
            let _ = user_id;
            Ok(())
        }
        async fn get_user_with_profile(
            &self,
            _user_id: Uuid,
        ) -> UserAdminResult<Option<AdminUserEntity>> {
            let row = self.row.clone();
            Ok(row.lock().unwrap().clone())
        }
        async fn email_exists(&self, _realm_id: &str, _email: &str) -> UserAdminResult<bool> {
            Ok(false)
        }
        async fn get_user_by_email(
            &self,
            _realm_id: &str,
            _email: &str,
        ) -> UserAdminResult<Option<AdminUserEntity>> {
            Ok(None)
        }
        async fn delete_user(&self, _user_id: Uuid) -> UserAdminResult<bool> {
            self.delete_calls.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }
        async fn update_user_password(
            &self,
            _user_id: Uuid,
            _password_hash: &str,
        ) -> UserAdminResult<bool> {
            self.password_calls.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }
    }

    /// Minimal user-role repository. `get_user_realm` (target-realm checks)
    /// and `replace_user_roles`/`add_user_role` (call counting) are exercised
    /// by the realm-boundary and hierarchy-guard tests; the rest are inert
    /// defaults.
    struct MockUserRoleRepo {
        user_realm: Option<String>,
        replace_calls: Arc<AtomicUsize>,
        add_calls: Arc<AtomicUsize>,
    }
    impl MockUserRoleRepo {
        fn for_user_realm(realm: &str) -> Self {
            Self {
                user_realm: Some(realm.to_string()),
                replace_calls: Arc::new(AtomicUsize::new(0)),
                add_calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }
    impl UserRoleRepository for MockUserRoleRepo {
        async fn get_user_realm(&self, _user_id: Uuid) -> UserAdminResult<Option<String>> {
            Ok(self.user_realm.clone())
        }
        async fn replace_user_roles(
            &self,
            _user_id: Uuid,
            _realm_id: &str,
            _client_id: &str,
            _role_ids: &[Uuid],
        ) -> UserAdminResult<()> {
            self.replace_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn get_user_role_ids(&self, _user_id: Uuid) -> UserAdminResult<Vec<Uuid>> {
            Ok(vec![])
        }
        async fn get_user_roles(
            &self,
            _user_id: Uuid,
        ) -> UserAdminResult<Vec<crate::user::admin_entities::RoleEntity>> {
            Ok(vec![])
        }
        async fn add_user_role(
            &self,
            _user_id: Uuid,
            _role_id: Uuid,
            _realm_id: &str,
            _client_id: &str,
        ) -> UserAdminResult<()> {
            self.add_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn remove_user_role(
            &self,
            _user_id: Uuid,
            _role_id: Uuid,
            _client_id: &str,
        ) -> UserAdminResult<bool> {
            Ok(true)
        }
        async fn grant_role_by_payment(
            &self,
            _realm_id: &str,
            _user_id: Uuid,
            _role_id: Uuid,
            _client_id: Option<&str>,
            _source_id: &str,
            _expires_at: Option<DateTime<Utc>>,
        ) -> UserAdminResult<GrantRoleOutcome> {
            Ok(GrantRoleOutcome::Granted)
        }
        async fn revoke_roles_by_payment_source(
            &self,
            _realm_id: &str,
            _user_id: Uuid,
            _source_id: &str,
        ) -> UserAdminResult<RevokeRoleOutcome> {
            Ok(RevokeRoleOutcome::NotFound)
        }
        async fn user_has_any_role(
            &self,
            _realm_id: &str,
            _user_id: Uuid,
            _role_ids: &[Uuid],
        ) -> UserAdminResult<bool> {
            Ok(false)
        }
        async fn list_user_roles_by_realm_client(
            &self,
            _realm_id: &str,
            _client_id: &str,
        ) -> UserAdminResult<Vec<(Uuid, Uuid)>> {
            Ok(vec![])
        }
        async fn replace_api_key_roles(
            &self,
            _api_key_id: &str,
            _realm_id: &str,
            _client_id: &str,
            _role_ids: &[Uuid],
        ) -> UserAdminResult<()> {
            Ok(())
        }
        async fn get_api_key_roles(
            &self,
            _api_key_id: &str,
        ) -> UserAdminResult<Vec<crate::user::admin_entities::RoleEntity>> {
            Ok(vec![])
        }
        async fn get_api_key_role_summaries_batch(
            &self,
            _api_key_ids: &[String],
        ) -> UserAdminResult<Vec<(String, Vec<(Uuid, String)>)>> {
            Ok(vec![])
        }
    }

    /// Minimal role-policy repository: `get_roles_by_ids` returns the
    /// configured roles (filtered by id, mirroring the SQL `IN` semantics);
    /// everything else is an inert default.
    struct MockRolePolicyRepo {
        roles: Vec<RoleEntity>,
        /// (role_id, resource, action) rows served by
        /// `get_role_policies_for_user` — lets tests model a role's granted
        /// permission set for the assign-hierarchy guard.
        policies: Vec<(Uuid, String, String)>,
    }
    impl RolePolicyRepository for MockRolePolicyRepo {
        async fn get_role_policies_for_user(
            &self,
            _realm_id: &str,
            role_ids: &[Uuid],
        ) -> UserAdminResult<Vec<PolicyEntity>> {
            Ok(self
                .policies
                .iter()
                .filter(|(role_id, _, _)| role_ids.contains(role_id))
                .map(|(role_id, resource, action)| PolicyEntity {
                    id: *role_id,
                    realm_id: _realm_id.to_string(),
                    resource: resource.clone(),
                    action: action.clone(),
                    policy_json: None,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                })
                .collect())
        }
        async fn get_direct_user_policies(
            &self,
            _user_id: Uuid,
        ) -> UserAdminResult<Vec<PolicyEntity>> {
            Ok(vec![])
        }
        async fn get_roles_by_ids(&self, role_ids: &[Uuid]) -> UserAdminResult<Vec<RoleEntity>> {
            Ok(self
                .roles
                .iter()
                .filter(|r| role_ids.contains(&r.id))
                .cloned()
                .collect())
        }
        async fn assign_direct_permission(
            &self,
            _user_id: Uuid,
            _realm_id: &str,
            _policy_id: Uuid,
        ) -> UserAdminResult<()> {
            Ok(())
        }
        async fn remove_direct_permission(
            &self,
            _user_id: Uuid,
            _policy_id: Uuid,
        ) -> UserAdminResult<()> {
            Ok(())
        }
        async fn create_role_policy(
            &self,
            _role_id: Uuid,
            _realm_id: &str,
            _resource: &str,
            _action: &str,
        ) -> UserAdminResult<()> {
            Ok(())
        }
        async fn delete_role_policy(
            &self,
            _role_id: Uuid,
            _resource: &str,
            _action: &str,
        ) -> UserAdminResult<bool> {
            Ok(true)
        }
        async fn list_role_policies_by_realm(
            &self,
            _realm_id: &str,
        ) -> UserAdminResult<Vec<(Uuid, String, String)>> {
            Ok(vec![])
        }
    }

    /// Best-effort audit sink that always succeeds. The audit details are not
    /// asserted here (the Forbidden-linkage details are covered by the
    /// integration-level test in the test slot); this only needs to not fail.
    struct MockAuditRepo;
    impl AuditEventRepository for MockAuditRepo {
        async fn create(&self, event: NewAuditEvent) -> Result<AuditEvent, CoreError> {
            Ok(AuditEvent {
                id: Uuid::nil(),
                realm_id: event.realm_id,
                category: event.category,
                action: event.action,
                actor_id: event.actor_id,
                actor_type: event.actor_type,
                actor_name: event.actor_name,
                target_type: event.target_type,
                target_id: event.target_id,
                target_name: event.target_name,
                result: event.result,
                details: event.details,
                ip_address: event.ip_address,
                user_agent: event.user_agent,
                trace_id: event.trace_id,
                created_at: Utc::now(),
            })
        }
        async fn list_paginated(
            &self,
            _realm_id: &str,
            _filters: AuditEventFilters,
        ) -> Result<PaginatedAuditEvents, CoreError> {
            Ok(PaginatedAuditEvents {
                items: vec![],
                page: 0,
                page_size: 0,
                total: 0,
            })
        }
        async fn find_by_id(
            &self,
            _realm_id: &str,
            _event_id: Uuid,
        ) -> Result<Option<AuditEvent>, CoreError> {
            Ok(None)
        }
    }

    /// Hand-rolled `BrowserTokenService` mock. Counts `revoke_user_families`
    /// calls; the other trait methods are not exercised by the admin update
    /// path and return minimal values.
    struct MockBrowserTokenService {
        revoke_calls: Arc<AtomicUsize>,
        revoke_err: Option<CoreError>,
    }
    impl BrowserTokenService for MockBrowserTokenService {
        async fn lookup_access_token(
            &self,
            _access_token: &str,
        ) -> Result<Option<BrowserAccessTokenData>, CoreError> {
            Ok(None)
        }
        async fn create_token_family(
            &self,
            _user: &crate::user::entities::User,
            _client_app: &crate::client::entities::ClientApp,
            _user_agent: Option<String>,
            _client_ip: Option<String>,
        ) -> Result<BrowserTokenSet, CoreError> {
            Ok(BrowserTokenSet {
                access_token: String::new(),
                refresh_token: String::new(),
                expires_in: 0,
                refresh_expires_in: 0,
                token_type: "Bearer".to_string(),
            })
        }
        async fn create_first_party_token_family(
            &self,
            _user: &crate::user::entities::User,
            _client_app: &crate::client::entities::ClientApp,
            _user_agent: Option<String>,
            _client_ip: Option<String>,
        ) -> Result<BrowserTokenSet, CoreError> {
            Ok(BrowserTokenSet {
                access_token: String::new(),
                refresh_token: String::new(),
                expires_in: 0,
                refresh_expires_in: 0,
                token_type: "Bearer".to_string(),
            })
        }
        async fn refresh(
            &self,
            _refresh_token: &str,
        ) -> Result<BrowserTokenSet, crate::authentication::entities::RefreshError> {
            Ok(BrowserTokenSet {
                access_token: String::new(),
                refresh_token: String::new(),
                expires_in: 0,
                refresh_expires_in: 0,
                token_type: "Bearer".to_string(),
            })
        }
        async fn revoke_family(&self, _family_id: Uuid) -> Result<(), CoreError> {
            Ok(())
        }
        async fn revoke_client_families(&self, _client_app_id: Uuid) -> Result<(), CoreError> {
            Ok(())
        }
        async fn revoke_user_families(&self, _user_id: &str) -> Result<(), CoreError> {
            let calls = self.revoke_calls.clone();
            let err = self.revoke_err.clone();
            calls.fetch_add(1, Ordering::SeqCst);
            if let Some(e) = err {
                return Err(e);
            }
            Ok(())
        }
        async fn list_user_sessions(
            &self,
            _user_id: &str,
        ) -> Result<Vec<crate::authentication::entities::UserSessionSummary>, CoreError> {
            Ok(vec![])
        }
        async fn get_family_lifecycle(
            &self,
            _family_id: Uuid,
        ) -> Result<Option<crate::authentication::entities::FamilyLifecycle>, CoreError> {
            Ok(None)
        }
    }

    /// Minimal audit context for unit tests — the `update_user_admin` tests
    /// assert on session-revocation behavior, not on IP/UA capture, so a
    /// default (empty) context is sufficient.
    fn audit_ctx() -> AuditContext {
        AuditContext {
            actor_id: "admin".to_string(),
            actor_type: Some(crate::audit::ActorType::Admin),
            actor_name: None,
            ip_address: None,
            user_agent: None,
            trace_id: None,
        }
    }

    /// Build a test `Identity::User` whose realm matches `realm_id`, so the
    /// realm-boundary check in `update_user_admin` passes.
    fn admin_identity(realm_id: &str) -> Identity {
        Identity::User(crate::user::entities::User {
            id: Uuid::nil(),
            realm_id: realm_id.to_string(),
            email: "admin@example.com".to_string(),
            nickname: None,
            password_hash: None,
            provider_ids: vec![],
            status: UserStatus::Normal,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    }

    /// Write-path call counters exposed by [`make_service_with_row_realm`] so
    /// the cross-realm regression tests can assert that a rejected operation
    /// never reached the repository.
    struct MockWriteCounts {
        update_calls: Arc<AtomicUsize>,
        delete_calls: Arc<AtomicUsize>,
        password_calls: Arc<AtomicUsize>,
    }

    fn make_service(
        revoke_calls: Arc<AtomicUsize>,
        revoke_err: Option<CoreError>,
        initial_status: i32,
    ) -> AdminUserServiceImpl<
        MockAdminUserRepo,
        MockUserRoleRepo,
        MockRolePolicyRepo,
        AlwaysAllowPermission,
        MockAuditRepo,
        MockBrowserTokenService,
    > {
        make_service_with_row_realm(revoke_calls, revoke_err, initial_status, "r").0
    }

    /// Variant of [`make_service`] that places the mock target user in
    /// `row_realm` (so cross-realm targets can be simulated) and returns the
    /// write-path call counters.
    #[allow(clippy::type_complexity)]
    fn make_service_with_row_realm(
        revoke_calls: Arc<AtomicUsize>,
        revoke_err: Option<CoreError>,
        initial_status: i32,
        row_realm: &str,
    ) -> (
        AdminUserServiceImpl<
            MockAdminUserRepo,
            MockUserRoleRepo,
            MockRolePolicyRepo,
            AlwaysAllowPermission,
            MockAuditRepo,
            MockBrowserTokenService,
        >,
        MockWriteCounts,
    ) {
        let row = AdminUserEntity {
            id: Uuid::nil(),
            realm_id: row_realm.to_string(),
            email: "target@example.com".to_string(),
            nickname: None,
            status: initial_status,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let user_role_repo = MockUserRoleRepo::for_user_realm(row_realm);
        let counts = MockWriteCounts {
            update_calls: Arc::new(AtomicUsize::new(0)),
            delete_calls: Arc::new(AtomicUsize::new(0)),
            password_calls: Arc::new(AtomicUsize::new(0)),
        };
        let repo = MockAdminUserRepo {
            row: Arc::new(Mutex::new(Some(row))),
            update_calls: counts.update_calls.clone(),
            delete_calls: counts.delete_calls.clone(),
            password_calls: counts.password_calls.clone(),
        };
        let token = MockBrowserTokenService {
            revoke_calls,
            revoke_err,
        };
        (
            AdminUserServiceImpl::new(
                Arc::new(repo),
                Arc::new(user_role_repo),
                Arc::new(MockRolePolicyRepo {
                    roles: vec![],
                    policies: vec![],
                }),
                Arc::new(AlwaysAllowPermission),
                Arc::new(MockAuditRepo),
                Arc::new(token),
            ),
            counts,
        )
    }

    #[tokio::test]
    async fn update_user_admin_revokes_sessions_when_forbidden() {
        // old=Normal(1), request.status=Some(2) -> transition INTO Forbidden
        // must revoke exactly once.
        let revoke_calls = Arc::new(AtomicUsize::new(0));
        let svc = make_service(
            revoke_calls.clone(),
            None,
            i16::from(UserStatus::Normal) as i32,
        );
        let res = svc
            .update_user_admin(
                admin_identity("r"),
                audit_ctx(),
                "r",
                Uuid::nil(),
                UpdateUserAdminRequest {
                    nickname: None,
                    status: Some(2),
                },
            )
            .await;
        assert!(res.is_ok(), "expected Ok, got {:?}", res.err());
        assert_eq!(
            revoke_calls.load(Ordering::SeqCst),
            1,
            "revoke_user_families must be called once on Forbidden transition"
        );
    }

    #[tokio::test]
    async fn update_user_admin_no_revoke_when_status_unchanged_or_non_forbidden() {
        // status=None (no change) -> no revoke.
        let revoke_calls = Arc::new(AtomicUsize::new(0));
        let svc = make_service(
            revoke_calls.clone(),
            None,
            i16::from(UserStatus::Normal) as i32,
        );
        let res = svc
            .update_user_admin(
                admin_identity("r"),
                audit_ctx(),
                "r",
                Uuid::nil(),
                UpdateUserAdminRequest {
                    nickname: Some("new".to_string()),
                    status: None,
                },
            )
            .await;
        assert!(res.is_ok(), "expected Ok, got {:?}", res.err());
        assert_eq!(
            revoke_calls.load(Ordering::SeqCst),
            0,
            "no revoke when status is unchanged"
        );

        // status=Some(1) (NonForbidden) -> no revoke.
        let revoke_calls2 = Arc::new(AtomicUsize::new(0));
        let svc2 = make_service(
            revoke_calls2.clone(),
            None,
            i16::from(UserStatus::Normal) as i32,
        );
        let res2 = svc2
            .update_user_admin(
                admin_identity("r"),
                audit_ctx(),
                "r",
                Uuid::nil(),
                UpdateUserAdminRequest {
                    nickname: None,
                    status: Some(1),
                },
            )
            .await;
        assert!(res2.is_ok(), "expected Ok, got {:?}", res2.err());
        assert_eq!(
            revoke_calls2.load(Ordering::SeqCst),
            0,
            "no revoke when target status is non-Forbidden"
        );

        // old=Forbidden and status=Some(2) (idempotent re-forbid) -> no revoke.
        let revoke_calls3 = Arc::new(AtomicUsize::new(0));
        let svc3 = make_service(
            revoke_calls3.clone(),
            None,
            i16::from(UserStatus::Forbidden) as i32,
        );
        let res3 = svc3
            .update_user_admin(
                admin_identity("r"),
                audit_ctx(),
                "r",
                Uuid::nil(),
                UpdateUserAdminRequest {
                    nickname: None,
                    status: Some(2),
                },
            )
            .await;
        assert!(res3.is_ok(), "expected Ok, got {:?}", res3.err());
        assert_eq!(
            revoke_calls3.load(Ordering::SeqCst),
            0,
            "no revoke when already Forbidden (idempotent)"
        );
    }

    #[tokio::test]
    async fn update_user_admin_revokes_when_forbidden_combined_with_other_fields() {
        // status=Some(2) AND nickname changed together -> still revoke once.
        let revoke_calls = Arc::new(AtomicUsize::new(0));
        let svc = make_service(
            revoke_calls.clone(),
            None,
            i16::from(UserStatus::Normal) as i32,
        );
        let res = svc
            .update_user_admin(
                admin_identity("r"),
                audit_ctx(),
                "r",
                Uuid::nil(),
                UpdateUserAdminRequest {
                    nickname: Some("renamed".to_string()),
                    status: Some(2),
                },
            )
            .await;
        assert!(res.is_ok(), "expected Ok, got {:?}", res.err());
        assert_eq!(
            revoke_calls.load(Ordering::SeqCst),
            1,
            "revoke must still fire when Forbidden is combined with other field changes"
        );
    }

    #[tokio::test]
    async fn update_user_admin_revoke_failure_surfaces_internal_error() {
        // revoke_user_families errors -> operation aborts with InternalError.
        let revoke_calls = Arc::new(AtomicUsize::new(0));
        let svc = make_service(
            revoke_calls.clone(),
            Some(CoreError::InternalServerError("redis down".to_string())),
            i16::from(UserStatus::Normal) as i32,
        );
        let res = svc
            .update_user_admin(
                admin_identity("r"),
                audit_ctx(),
                "r",
                Uuid::nil(),
                UpdateUserAdminRequest {
                    nickname: None,
                    status: Some(2),
                },
            )
            .await;
        match res {
            Err(UserAdminError::InternalError(_)) => {}
            other => panic!("expected InternalError, got {:?}", other),
        }
        assert_eq!(
            revoke_calls.load(Ordering::SeqCst),
            1,
            "revoke must be attempted once even when it errors"
        );
    }

    // ========================================================================
    // Target realm-boundary regressions (cross-tenant IDOR fixes).
    //
    // The caller-vs-path realm check does not constrain the target id: a
    // realm admin must not be able to read, mutate, delete, password-reset,
    // or re-role another realm's user by supplying its uuid.
    // ========================================================================

    fn role_entity(id: Uuid, realm_id: &str) -> RoleEntity {
        RoleEntity {
            id,
            realm_id: realm_id.to_string(),
            name: "user".to_string(),
            description: None,
            is_builtin: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            source: "manual".to_string(),
            source_id: None,
            expires_at: None,
        }
    }

    fn make_role_assignment_service(
        target_user_realm: &str,
        roles: Vec<RoleEntity>,
    ) -> (
        RoleAssignmentServiceImpl<MockUserRoleRepo, MockRolePolicyRepo, AlwaysAllowPermission>,
        Arc<AtomicUsize>,
    ) {
        let repo = MockUserRoleRepo::for_user_realm(target_user_realm);
        let replace_calls = repo.replace_calls.clone();
        (
            RoleAssignmentServiceImpl::new(
                Arc::new(repo),
                Arc::new(MockRolePolicyRepo {
                    roles,
                    policies: vec![],
                }),
                Arc::new(AlwaysAllowPermission),
            ),
            replace_calls,
        )
    }

    #[tokio::test]
    async fn get_user_admin_rejects_cross_realm_target() {
        let (svc, _) = make_service_with_row_realm(
            Arc::new(AtomicUsize::new(0)),
            None,
            i16::from(UserStatus::Normal) as i32,
            "other",
        );
        let res = svc
            .get_user_admin(admin_identity("r"), "r", Uuid::nil())
            .await;
        match res {
            Err(UserAdminError::UserNotFound(_)) => {}
            other => panic!("expected UserNotFound, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn update_user_admin_rejects_cross_realm_target_without_writing() {
        // Path realm matches the caller but the target user belongs to
        // "other": must fail as UserNotFound with zero writes. This is the
        // cross-tenant takeover primitive the check blocks.
        let (svc, counts) = make_service_with_row_realm(
            Arc::new(AtomicUsize::new(0)),
            None,
            i16::from(UserStatus::Normal) as i32,
            "other",
        );
        let res = svc
            .update_user_admin(
                admin_identity("r"),
                audit_ctx(),
                "r",
                Uuid::nil(),
                UpdateUserAdminRequest {
                    nickname: Some("hijack".to_string()),
                    status: None,
                },
            )
            .await;
        match res {
            Err(UserAdminError::UserNotFound(_)) => {}
            other => panic!("expected UserNotFound, got {:?}", other),
        }
        assert_eq!(
            counts.update_calls.load(Ordering::SeqCst),
            0,
            "no field update may run for a cross-realm target"
        );
    }

    #[tokio::test]
    async fn delete_user_rejects_cross_realm_target_without_deleting() {
        let (svc, counts) = make_service_with_row_realm(
            Arc::new(AtomicUsize::new(0)),
            None,
            i16::from(UserStatus::Normal) as i32,
            "other",
        );
        let res = svc
            .delete_user(admin_identity("r"), audit_ctx(), "r", Uuid::nil())
            .await;
        match res {
            Err(UserAdminError::UserNotFound(_)) => {}
            other => panic!("expected UserNotFound, got {:?}", other),
        }
        assert_eq!(
            counts.delete_calls.load(Ordering::SeqCst),
            0,
            "no delete may run for a cross-realm target"
        );
    }

    #[tokio::test]
    async fn reset_user_password_rejects_cross_realm_target_without_resetting() {
        let (svc, counts) = make_service_with_row_realm(
            Arc::new(AtomicUsize::new(0)),
            None,
            i16::from(UserStatus::Normal) as i32,
            "other",
        );
        let res = svc
            .reset_user_password(admin_identity("r"), audit_ctx(), "r", Uuid::nil())
            .await;
        match res {
            Err(UserAdminError::UserNotFound(_)) => {}
            other => panic!("expected UserNotFound, got {:?}", other),
        }
        assert_eq!(
            counts.password_calls.load(Ordering::SeqCst),
            0,
            "no password write may run for a cross-realm target"
        );
    }

    #[tokio::test]
    async fn get_user_roles_rejects_cross_realm_target() {
        let (svc, _) = make_role_assignment_service("other", vec![]);
        let res = svc
            .get_user_roles(admin_identity("r"), "r", Uuid::nil())
            .await;
        match res {
            Err(UserAdminError::UserNotFound(_)) => {}
            other => panic!("expected UserNotFound, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn assign_user_roles_rejects_cross_realm_target() {
        let (svc, replace_calls) =
            make_role_assignment_service("other", vec![role_entity(Uuid::nil(), "r")]);
        let res = svc
            .assign_user_roles(admin_identity("r"), "r", Uuid::nil(), vec![Uuid::nil()])
            .await;
        match res {
            Err(UserAdminError::UserNotFound(_)) => {}
            other => panic!("expected UserNotFound, got {:?}", other),
        }
        assert_eq!(
            replace_calls.load(Ordering::SeqCst),
            0,
            "no role replacement may run for a cross-realm target"
        );
    }

    #[tokio::test]
    async fn assign_user_roles_rejects_cross_realm_role() {
        // The role id belongs to another realm: the user_roles schema only
        // foreign-keys role_id -> roles(id), so this must be rejected here.
        let (svc, replace_calls) =
            make_role_assignment_service("r", vec![role_entity(Uuid::nil(), "other")]);
        let res = svc
            .assign_user_roles(admin_identity("r"), "r", Uuid::nil(), vec![Uuid::nil()])
            .await;
        match res {
            Err(UserAdminError::RoleNotFound(_)) => {}
            other => panic!("expected RoleNotFound, got {:?}", other),
        }
        assert_eq!(
            replace_calls.load(Ordering::SeqCst),
            0,
            "no role replacement may run for a cross-realm role id"
        );
    }

    #[tokio::test]
    async fn assign_user_roles_blocks_privileged_builtin_role_the_caller_cannot_hold() {
        // Hierarchy guard: a delegated sub-admin holding ONLY roles.manage
        // must not reach primary-admin level by assigning the builtin
        // realm-admin role (whose policy set grants users.manage). Without
        // the guard this is a direct self-escalation to full realm admin.
        let role_id = Uuid::now_v7();
        let mut realm_admin = role_entity(role_id, "r");
        realm_admin.name = "realm-admin".to_string();

        let repo = MockUserRoleRepo::for_user_realm("r");
        let replace_calls = repo.replace_calls.clone();
        let svc = RoleAssignmentServiceImpl::new(
            Arc::new(repo),
            Arc::new(MockRolePolicyRepo {
                roles: vec![realm_admin],
                policies: vec![(role_id, "users".to_string(), "manage".to_string())],
            }),
            Arc::new(SelectivePermission {
                allowed: vec![("roles", "manage")],
            }),
        );

        let res = svc
            .assign_user_roles(admin_identity("r"), "r", Uuid::nil(), vec![role_id])
            .await;
        match res {
            Err(UserAdminError::PermissionDenied(msg)) => {
                assert!(
                    msg.contains("permission you do not hold"),
                    "unexpected deny message: {msg}"
                );
            }
            other => panic!("expected PermissionDenied, got {:?}", other),
        }
        assert_eq!(
            replace_calls.load(Ordering::SeqCst),
            0,
            "no role replacement may run when the caller lacks the role's permissions"
        );
    }

    #[tokio::test]
    async fn assign_user_roles_allows_privileged_builtin_role_to_peer_level_caller() {
        // Positive control: a caller who already holds every permission the
        // role grants (a peer realm-admin) may assign it — the guard must not
        // break legitimate admin-to-admin promotion.
        let role_id = Uuid::now_v7();
        let mut realm_admin = role_entity(role_id, "r");
        realm_admin.name = "realm-admin".to_string();

        let repo = MockUserRoleRepo::for_user_realm("r");
        let replace_calls = repo.replace_calls.clone();
        let svc = RoleAssignmentServiceImpl::new(
            Arc::new(repo),
            Arc::new(MockRolePolicyRepo {
                roles: vec![realm_admin],
                policies: vec![(role_id, "users".to_string(), "manage".to_string())],
            }),
            Arc::new(SelectivePermission {
                allowed: vec![("roles", "manage"), ("users", "manage")],
            }),
        );

        let res = svc
            .assign_user_roles(admin_identity("r"), "r", Uuid::nil(), vec![role_id])
            .await;
        assert!(res.is_ok(), "peer-level assignment must succeed: {:?}", res);
        assert_eq!(
            replace_calls.load(Ordering::SeqCst),
            1,
            "the role replacement must run for a peer-level caller"
        );
    }

    #[tokio::test]
    async fn create_permission_blocks_privileged_builtin_role_the_caller_cannot_hold() {
        // Hierarchy guard on the RoleWrap path: a delegated sub-admin holding
        // ONLY policies.manage must not self-assign the builtin realm-admin
        // role through POST /api/permission/{realmId}/permissions — this
        // endpoint grants roles, so it must enforce the same guard as
        // assign_user_roles or it is a direct self-escalation to full admin.
        let role_id = Uuid::now_v7();
        let mut realm_admin = role_entity(role_id, "r");
        realm_admin.name = "realm-admin".to_string();

        let repo = MockUserRoleRepo::for_user_realm("r");
        let add_calls = repo.add_calls.clone();
        let svc = PermissionManagementServiceImpl::new(
            Arc::new(repo),
            Arc::new(MockRolePolicyRepo {
                roles: vec![realm_admin],
                policies: vec![(role_id, "users".to_string(), "manage".to_string())],
            }),
            Arc::new(SelectivePermission {
                allowed: vec![("policies", "manage")],
            }),
            Arc::new(MockAuditRepo),
        );

        let res = svc
            .create_permission(
                admin_identity("r"),
                audit_ctx(),
                "r",
                "admin-web-console",
                None,
                Some(Uuid::nil()),
                Some(role_id),
                None,
                None,
            )
            .await;
        match res {
            Err(UserAdminError::PermissionDenied(msg)) => {
                assert!(
                    msg.contains("permission you do not hold"),
                    "unexpected deny message: {msg}"
                );
            }
            other => panic!("expected PermissionDenied, got {:?}", other),
        }
        assert_eq!(
            add_calls.load(Ordering::SeqCst),
            0,
            "no user-role write may run when the caller lacks the role's permissions"
        );
    }

    #[tokio::test]
    async fn create_permission_allows_privileged_builtin_role_to_peer_level_caller() {
        // Positive control: a caller who holds every permission the role
        // grants may still use the RoleWrap path to grant it.
        let role_id = Uuid::now_v7();
        let mut realm_admin = role_entity(role_id, "r");
        realm_admin.name = "realm-admin".to_string();

        let repo = MockUserRoleRepo::for_user_realm("r");
        let add_calls = repo.add_calls.clone();
        let svc = PermissionManagementServiceImpl::new(
            Arc::new(repo),
            Arc::new(MockRolePolicyRepo {
                roles: vec![realm_admin],
                policies: vec![(role_id, "users".to_string(), "manage".to_string())],
            }),
            Arc::new(SelectivePermission {
                allowed: vec![("policies", "manage"), ("users", "manage")],
            }),
            Arc::new(MockAuditRepo),
        );

        let res = svc
            .create_permission(
                admin_identity("r"),
                audit_ctx(),
                "r",
                "admin-web-console",
                None,
                Some(Uuid::nil()),
                Some(role_id),
                None,
                None,
            )
            .await;
        assert!(res.is_ok(), "peer-level grant must succeed: {:?}", res);
        assert_eq!(
            add_calls.load(Ordering::SeqCst),
            1,
            "the user-role write must run for a peer-level caller"
        );
    }

    #[tokio::test]
    async fn create_permission_blocks_policy_the_caller_cannot_hold() {
        // Self-holds guard on the PoliceWrap path: a delegated sub-admin
        // holding ONLY policies.manage must not attach ("users","manage") to
        // a role — the three sibling grant surfaces (add_policy_to_role,
        // role-definition permissions, direct user permissions) all reject
        // this, so this endpoint must too or it is the odd-one-out
        // escalation hole.
        let rid = Uuid::now_v7();

        let repo = MockUserRoleRepo::for_user_realm("r");
        let svc = PermissionManagementServiceImpl::new(
            Arc::new(repo),
            Arc::new(MockRolePolicyRepo {
                roles: vec![role_entity(rid, "r")],
                policies: vec![],
            }),
            Arc::new(SelectivePermission {
                allowed: vec![("policies", "manage")],
            }),
            Arc::new(MockAuditRepo),
        );

        let res = svc
            .create_permission(
                admin_identity("r"),
                audit_ctx(),
                "r",
                "admin-web-console",
                Some(rid),
                None,
                None,
                Some("users".to_string()),
                Some("manage".to_string()),
            )
            .await;
        match res {
            Err(UserAdminError::PermissionDenied(msg)) => {
                assert!(
                    msg.contains("permission you do not hold"),
                    "unexpected deny message: {msg}"
                );
            }
            other => panic!("expected PermissionDenied, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn create_permission_allows_policy_the_caller_holds() {
        // Positive control: a caller who already holds the granted
        // (resource, action) may attach it to a role — the guard must not
        // break legitimate policy management.
        let rid = Uuid::now_v7();

        let repo = MockUserRoleRepo::for_user_realm("r");
        let svc = PermissionManagementServiceImpl::new(
            Arc::new(repo),
            Arc::new(MockRolePolicyRepo {
                roles: vec![role_entity(rid, "r")],
                policies: vec![],
            }),
            Arc::new(SelectivePermission {
                allowed: vec![("policies", "manage"), ("users", "manage")],
            }),
            Arc::new(MockAuditRepo),
        );

        let res = svc
            .create_permission(
                admin_identity("r"),
                audit_ctx(),
                "r",
                "admin-web-console",
                Some(rid),
                None,
                None,
                Some("users".to_string()),
                Some("manage".to_string()),
            )
            .await;
        assert!(
            res.is_ok(),
            "peer-level policy grant must succeed: {:?}",
            res
        );
    }

    #[tokio::test]
    async fn assign_user_roles_allows_same_realm_target_and_roles() {
        // Sanity: the boundary check must not break the legitimate flow.
        let (svc, replace_calls) =
            make_role_assignment_service("r", vec![role_entity(Uuid::nil(), "r")]);
        svc.assign_user_roles(admin_identity("r"), "r", Uuid::nil(), vec![Uuid::nil()])
            .await
            .expect("same-realm assignment must succeed");
        assert_eq!(replace_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn get_effective_permissions_rejects_cross_realm_target() {
        let svc = UserPermissionServiceImpl::new(
            Arc::new(MockUserRoleRepo::for_user_realm("other")),
            Arc::new(MockRolePolicyRepo {
                roles: vec![],
                policies: vec![],
            }),
            Arc::new(AlwaysAllowPermission),
        );
        let res = svc
            .get_effective_permissions(admin_identity("r"), "r", Uuid::nil())
            .await;
        match res {
            Err(UserAdminError::UserNotFound(_)) => {}
            other => panic!("expected UserNotFound, got {:?}", other),
        }
    }

    fn make_permission_management_service(
        target_user_realm: &str,
        roles: Vec<RoleEntity>,
    ) -> PermissionManagementServiceImpl<
        MockUserRoleRepo,
        MockRolePolicyRepo,
        AlwaysAllowPermission,
        MockAuditRepo,
    > {
        PermissionManagementServiceImpl::new(
            Arc::new(MockUserRoleRepo::for_user_realm(target_user_realm)),
            Arc::new(MockRolePolicyRepo {
                roles,
                policies: vec![],
            }),
            Arc::new(AlwaysAllowPermission),
            Arc::new(MockAuditRepo),
        )
    }

    #[tokio::test]
    async fn create_permission_rejects_cross_realm_user_target() {
        let rid = Uuid::now_v7();
        let svc = make_permission_management_service("other", vec![role_entity(rid, "r")]);
        let res = svc
            .create_permission(
                admin_identity("r"),
                audit_ctx(),
                "r",
                "admin-web-console",
                None,
                Some(Uuid::nil()),
                Some(rid),
                None,
                None,
            )
            .await;
        match res {
            Err(UserAdminError::UserNotFound(_)) => {}
            other => panic!("expected UserNotFound, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn create_permission_rejects_cross_realm_role() {
        // Attaching a policy to another realm's role id must be rejected:
        // the row would insert cleanly and pollute this realm's namespace.
        let rid = Uuid::now_v7();
        let svc = make_permission_management_service("r", vec![role_entity(rid, "other")]);
        let res = svc
            .create_permission(
                admin_identity("r"),
                audit_ctx(),
                "r",
                "admin-web-console",
                Some(rid),
                None,
                None,
                Some("users".to_string()),
                Some("view".to_string()),
            )
            .await;
        match res {
            Err(UserAdminError::RoleNotFound(_)) => {}
            other => panic!("expected RoleNotFound, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn delete_permission_rejects_cross_realm_role_policy() {
        // delete_role_policy has no realm predicate at the repo layer, so the
        // service-level check is what stops cross-tenant policy deletion.
        let rid = Uuid::now_v7();
        let svc = make_permission_management_service("r", vec![role_entity(rid, "other")]);
        let res = svc
            .delete_permission(
                admin_identity("r"),
                audit_ctx(),
                "r",
                "admin-web-console",
                Some(rid),
                None,
                None,
                Some("users".to_string()),
                Some("view".to_string()),
            )
            .await;
        match res {
            Err(UserAdminError::RoleNotFound(_)) => {}
            other => panic!("expected RoleNotFound, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn create_permission_allows_same_realm_targets() {
        // Sanity: the boundary checks must not break the legitimate flow.
        let rid = Uuid::now_v7();
        let uid = Uuid::now_v7();
        let svc = make_permission_management_service("r", vec![role_entity(rid, "r")]);
        svc.create_permission(
            admin_identity("r"),
            audit_ctx(),
            "r",
            "admin-web-console",
            None,
            Some(uid),
            Some(rid),
            None,
            None,
        )
        .await
        .expect("same-realm permission creation must succeed");
    }
}
