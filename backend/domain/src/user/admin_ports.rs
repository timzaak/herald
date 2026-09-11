// User Admin Domain Ports (Traits)
//
// These traits define the interfaces for repositories and services
// following the Dependency Inversion Principle.

use std::future::Future;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{
    admin_dtos::{
        AdminUser, CreateUserWithRolesRequest, PermissionDetail, RoleDetail, UpdateUserAdminRequest,
    },
    admin_entities::{AdminUserEntity, PolicyEntity, RoleEntity},
    admin_errors::UserAdminResult,
};

type ApiKeyRoleSummaries = Vec<(String, Vec<(Uuid, String)>)>;
use crate::audit::AuditContext;
use crate::authentication::Identity;

// ============================================================================
// Payment-driven role grant outcomes
// ============================================================================

/// Outcome of an idempotent payment-driven role grant.
///
/// Used by `UserRoleRepository::grant_role_by_payment`. The existence check is
/// keyed on `(source='payment', source_id, user_id, role_id)`, so an
/// out-of-order renewal after a cancel re-inserts (returns `Granted`), while a
/// duplicate grant for the same payment origin returns `AlreadyExists`
/// (idempotent skip).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantRoleOutcome {
    /// A new `user_roles` row was inserted with `source='payment'`.
    Granted,
    /// An equivalent payment grant already exists for this origin; no row written.
    AlreadyExists,
}

/// Outcome of revoking payment-driven roles by payment origin (`source_id`).
///
/// Used by `UserRoleRepository::revoke_roles_by_payment_source`. Only deletes
/// rows with `source='payment'`; manual grants are never touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeRoleOutcome {
    /// `n` payment-granted role rows were deleted.
    Revoked(u64),
    /// No matching payment-granted rows existed for this `source_id`.
    NotFound,
}

// ============================================================================
// Repository Ports
// ============================================================================

/// Repository for admin user operations
pub trait AdminUserRepository: Send + Sync {
    /// Create a user with profile in a single transaction
    fn create_user_with_profile(
        &self,
        realm_id: &str,
        email: &str,
        password_hash: &str,
        nickname: Option<&str>,
        status: i32,
    ) -> impl Future<Output = UserAdminResult<Uuid>> + Send;

    /// Update user fields
    fn update_user_fields(
        &self,
        user_id: Uuid,
        email: Option<&str>,
        nickname: Option<&str>,
        status: Option<i32>,
    ) -> impl Future<Output = UserAdminResult<()>> + Send;

    /// Get user with profile
    fn get_user_with_profile(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<Option<AdminUserEntity>>> + Send;

    /// Check if email exists in realm
    fn email_exists(
        &self,
        realm_id: &str,
        email: &str,
    ) -> impl Future<Output = UserAdminResult<bool>> + Send;

    /// Get user by email
    fn get_user_by_email(
        &self,
        realm_id: &str,
        email: &str,
    ) -> impl Future<Output = UserAdminResult<Option<AdminUserEntity>>> + Send;

    /// Delete user (including profile and account) in a transaction
    fn delete_user(&self, user_id: Uuid) -> impl Future<Output = UserAdminResult<bool>> + Send;

    /// Update user password
    fn update_user_password(
        &self,
        user_id: Uuid,
        password_hash: &str,
    ) -> impl Future<Output = UserAdminResult<bool>> + Send;
}

/// Repository for user role operations
pub trait UserRoleRepository: Send + Sync {
    /// Get the realm a user belongs to (None if the user does not exist).
    ///
    /// Lets services that only hold this repository enforce the target-realm
    /// boundary on path-supplied user ids before reading or writing role data.
    fn get_user_realm(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<Option<String>>> + Send;

    /// Replace all roles for a user (transactional)
    fn replace_user_roles(
        &self,
        user_id: Uuid,
        realm_id: &str,
        client_id: &str,
        role_ids: &[Uuid],
    ) -> impl Future<Output = UserAdminResult<()>> + Send;

    /// Get role IDs assigned to a user
    fn get_user_role_ids(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<Vec<Uuid>>> + Send;

    /// Get roles with details for a user
    fn get_user_roles(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<Vec<RoleEntity>>> + Send;

    /// Add a single role to a user
    fn add_user_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        realm_id: &str,
        client_id: &str,
    ) -> impl Future<Output = UserAdminResult<()>> + Send;

    /// Remove a single role from a user
    fn remove_user_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        client_id: &str,
    ) -> impl Future<Output = UserAdminResult<bool>> + Send;

    /// Grant a role to a user as a consequence of a successful payment.
    ///
    /// Writes a `user_roles` row with `source='payment'`, `source_id=<payment
    /// origin>` and (for subscriptions) `expires_at`. Idempotent: if a row with
    /// the same `(source='payment', source_id, user_id, role_id)` already
    /// exists, returns `GrantRoleOutcome::AlreadyExists` without writing.
    /// Out-of-order-renewal safe: a prior cancel that deleted the row allows a
    /// fresh insert (returns `Granted`).
    ///
    /// `source_id` is the payment-attempt id (one-time) or subscription id
    /// (subscription). `expires_at` is `None` for one-time (permanent) grants
    /// and `Some(current_period_end)` for subscription grants — note this is a
    /// provenance/observability snapshot only, NOT an authz TTL (no sweep
    /// removes the row once it passes; revoke is event-driven). `client_id` is
    /// `None` when the triggering payment attempt does not carry one (the
    /// `client_id` column is nullable).
    fn grant_role_by_payment(
        &self,
        realm_id: &str,
        user_id: Uuid,
        role_id: Uuid,
        client_id: Option<&str>,
        source_id: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> impl Future<Output = UserAdminResult<GrantRoleOutcome>> + Send;

    /// Revoke all payment-driven roles for a user that originated from the
    /// given `source_id` (payment-attempt id or subscription id).
    ///
    /// Deletes only rows with `source='payment' AND source_id=$1 AND user_id`;
    /// manual grants (`source='manual'`) are never affected.
    /// Returns `RevokeRoleOutcome::NotFound` when zero rows matched.
    fn revoke_roles_by_payment_source(
        &self,
        realm_id: &str,
        user_id: Uuid,
        source_id: &str,
    ) -> impl Future<Output = UserAdminResult<RevokeRoleOutcome>> + Send;

    /// Check whether a user holds ANY of the given roles, regardless of grant
    /// source. Used to prevent selling an entitlement the user already owns.
    fn user_has_any_role(
        &self,
        realm_id: &str,
        user_id: Uuid,
        role_ids: &[Uuid],
    ) -> impl Future<Output = UserAdminResult<bool>> + Send;

    /// Replace all roles for an API key principal (transactional)
    fn replace_api_key_roles(
        &self,
        api_key_id: &str,
        realm_id: &str,
        client_id: &str,
        role_ids: &[Uuid],
    ) -> impl Future<Output = UserAdminResult<()>> + Send;

    /// Get roles with details for an API key principal
    fn get_api_key_roles(
        &self,
        api_key_id: &str,
    ) -> impl Future<Output = UserAdminResult<Vec<RoleEntity>>> + Send;

    /// Batch get role summaries for multiple API key principals
    /// Returns (principal_id, Vec<(role_id, role_name)>)
    fn get_api_key_role_summaries_batch(
        &self,
        api_key_ids: &[String],
    ) -> impl Future<Output = UserAdminResult<ApiKeyRoleSummaries>> + Send;
}

/// Repository for role policy operations
pub trait RolePolicyRepository: Send + Sync {
    /// Get all role policies for a user (via their assigned roles).
    /// Each entry pairs the policy with the role that grants it so callers
    /// can attribute the permission source correctly.
    fn get_role_policies_for_user(
        &self,
        realm_id: &str,
        role_ids: &[Uuid],
    ) -> impl Future<Output = UserAdminResult<Vec<(Uuid, PolicyEntity)>>> + Send;

    /// Get direct user policies (not via roles)
    fn get_direct_user_policies(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<Vec<PolicyEntity>>> + Send;

    /// Get roles by IDs
    fn get_roles_by_ids(
        &self,
        role_ids: &[Uuid],
    ) -> impl Future<Output = UserAdminResult<Vec<RoleEntity>>> + Send;

    /// Assign direct permission to user
    fn assign_direct_permission(
        &self,
        user_id: Uuid,
        realm_id: &str,
        policy_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<()>> + Send;

    /// Remove direct permission from user
    fn remove_direct_permission(
        &self,
        user_id: Uuid,
        policy_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<()>> + Send;

    /// Create a role policy
    fn create_role_policy(
        &self,
        role_id: Uuid,
        realm_id: &str,
        resource: &str,
        action: &str,
    ) -> impl Future<Output = UserAdminResult<()>> + Send;

    /// Delete a role policy
    fn delete_role_policy(
        &self,
        role_id: Uuid,
        resource: &str,
        action: &str,
    ) -> impl Future<Output = UserAdminResult<bool>> + Send;
}

// ============================================================================
// Service Ports
// ============================================================================

/// Service for admin user management
pub trait AdminUserService: Send + Sync {
    /// Create a new user with roles
    fn create_user_with_roles(
        &self,
        identity: Identity,
        ctx: AuditContext,
        realm_id: &str,
        request: CreateUserWithRolesRequest,
    ) -> impl Future<Output = UserAdminResult<AdminUser>> + Send;

    /// Create a new user with roles for a caller whose ENTRANCE permission
    /// gate already ran (the ext API keys user creation to `users:create`,
    /// its own permission tier, instead of the admin face's `users.manage`).
    /// Skips only that domain permission re-check; realm boundary, role
    /// validation, hierarchy guards, duplicate-email check and audit still
    /// apply exactly as in [`AdminUserService::create_user_with_roles`].
    fn create_user_with_roles_pre_authorized(
        &self,
        identity: Identity,
        ctx: AuditContext,
        realm_id: &str,
        request: CreateUserWithRolesRequest,
    ) -> impl Future<Output = UserAdminResult<AdminUser>> + Send;

    /// Update user admin fields
    fn update_user_admin(
        &self,
        identity: Identity,
        ctx: AuditContext,
        realm_id: &str,
        user_id: Uuid,
        request: UpdateUserAdminRequest,
    ) -> impl Future<Output = UserAdminResult<AdminUser>> + Send;

    /// Get user admin details
    fn get_user_admin(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<AdminUser>> + Send;

    /// Delete a user (including profile and account)
    fn delete_user(
        &self,
        identity: Identity,
        ctx: AuditContext,
        realm_id: &str,
        user_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<()>> + Send;

    /// Reset user password (generate random password)
    fn reset_user_password(
        &self,
        identity: Identity,
        ctx: AuditContext,
        realm_id: &str,
        user_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<String>> + Send;
}

/// Service for role assignment
pub trait RoleAssignmentService: Send + Sync {
    /// Assign roles to a user
    fn assign_user_roles(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
        role_ids: Vec<Uuid>,
    ) -> impl Future<Output = UserAdminResult<()>> + Send;

    /// Get user roles
    fn get_user_roles(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<Vec<RoleDetail>>> + Send;

    /// Check if roles can be assigned
    fn can_assign_roles(
        &self,
        identity: &Identity,
        realm_id: &str,
        role_ids: &[Uuid],
    ) -> impl Future<Output = UserAdminResult<bool>> + Send;

    /// Assign roles to an API Key principal
    fn assign_api_key_roles(
        &self,
        identity: Identity,
        realm_id: &str,
        api_key_id: &str,
        role_ids: Vec<Uuid>,
    ) -> impl Future<Output = UserAdminResult<()>> + Send;

    /// Get roles assigned to an API Key principal
    fn get_api_key_roles(
        &self,
        identity: Identity,
        realm_id: &str,
        api_key_id: &str,
    ) -> impl Future<Output = UserAdminResult<Vec<RoleEntity>>> + Send;
}

/// Service for user permission management
pub trait UserPermissionService: Send + Sync {
    /// Get effective permissions for a user (roles + direct)
    fn get_effective_permissions(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<Vec<PermissionDetail>>> + Send;

    /// Assign direct permission to user
    fn assign_direct_permission(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
        policy_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<()>> + Send;

    /// Remove direct permission from user
    fn remove_direct_permission(
        &self,
        identity: Identity,
        realm_id: &str,
        user_id: Uuid,
        policy_id: Uuid,
    ) -> impl Future<Output = UserAdminResult<()>> + Send;
}
