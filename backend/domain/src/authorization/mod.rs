use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::future::Future;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::authentication::Identity;
use crate::common::entities::{Entity, app_errors::CoreError};

// ============================================================================
// Entities
// ============================================================================

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct Role {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub realm_id: String,
    pub client_id: String,
    pub is_builtin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Entity for Role {
    fn id(&self) -> Uuid {
        self.id
    }

    fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct Permission {
    pub id: Uuid,
    pub name: String,
    pub resource: String,
    pub action: String,
    pub description: Option<String>,
    pub realm_id: String,
    pub is_builtin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Entity for Permission {
    fn id(&self) -> Uuid {
        self.id
    }

    fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PolicyCheck {
    pub realm_id: String,
    pub client_id: String,
    pub user_id: String,
    pub resource: String,
    pub action: String,
}

// ============================================================================
// Value Objects (DTOs)
// ============================================================================

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct CreateRoleRequest {
    #[validate(length(min = 1))]
    pub realm_id: String,
    #[validate(length(min = 1))]
    pub client_id: String,
    #[validate(length(min = 1))]
    pub name: String,
    pub description: Option<String>,
    pub is_builtin: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct CreatePermissionRequest {
    #[validate(length(min = 1))]
    pub realm_id: String,
    #[validate(length(min = 1))]
    pub name: String,
    #[validate(length(min = 1))]
    pub resource: String,
    #[validate(length(min = 1))]
    pub action: String,
    pub description: Option<String>,
    pub is_builtin: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct AssignPermissionRequest {
    pub role_id: Uuid,
    pub permission_id: Uuid,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct CheckPermissionRequest {
    #[validate(length(min = 1))]
    pub user_id: String,
    #[validate(length(min = 1))]
    pub resource: String,
    #[validate(length(min = 1))]
    pub action: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CheckPermissionResponse {
    pub allowed: bool,
}

// ============================================================================
// Repository Ports
// ============================================================================

#[cfg_attr(test, mockall::automock)]
pub trait RoleRepository: Send + Sync {
    fn create_role(
        &self,
        request: CreateRoleRequest,
    ) -> impl Future<Output = Result<Role, CoreError>> + Send;

    /// Fetch a role by ID, scoped to `realm_id`. A role outside the realm
    /// resolves to `NotFound` — the repository enforces the tenant boundary
    /// so callers cannot forget it.
    fn get_role_by_id(
        &self,
        realm_id: &str,
        id: Uuid,
    ) -> impl Future<Output = Result<Role, CoreError>> + Send;

    fn find_by_name(
        &self,
        name: &str,
        realm_id: &str,
        client_id: &str,
    ) -> impl Future<Output = Result<Role, CoreError>> + Send;

    fn list_roles(
        &self,
        realm_id: &str,
        client_id: &str,
    ) -> impl Future<Output = Result<Vec<Role>, CoreError>> + Send;

    /// Delete a role scoped to `realm_id`. Deleting a role outside the
    /// realm affects zero rows (idempotent no-op), never another tenant's row.
    fn delete_role(
        &self,
        realm_id: &str,
        id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// Batch query roles by IDs (for validating role assignments), scoped to
    /// `realm_id`: IDs resolving to roles in other realms are silently absent
    /// from the result.
    fn find_by_ids(
        &self,
        realm_id: &str,
        ids: Vec<Uuid>,
    ) -> impl Future<Output = Result<Vec<Role>, CoreError>> + Send;
}

#[cfg_attr(test, mockall::automock)]
pub trait PermissionRepository: Send + Sync {
    fn create_permission(
        &self,
        request: CreatePermissionRequest,
    ) -> impl Future<Output = Result<Permission, CoreError>> + Send;

    /// Fetch a permission by ID, scoped to `realm_id`. A permission outside
    /// the realm resolves to `NotFound`.
    fn get_permission_by_id(
        &self,
        realm_id: &str,
        id: Uuid,
    ) -> impl Future<Output = Result<Permission, CoreError>> + Send;

    fn list_permissions(
        &self,
        realm_id: &str,
    ) -> impl Future<Output = Result<Vec<Permission>, CoreError>> + Send;

    /// Delete a permission scoped to `realm_id`. Deleting a permission
    /// outside the realm affects zero rows, never another tenant's row.
    fn delete_permission(
        &self,
        realm_id: &str,
        id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}

#[cfg_attr(test, mockall::automock)]
pub trait RolePermissionRepository: Send + Sync {
    fn assign_permission_to_role(
        &self,
        role_id: Uuid,
        permission_id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    fn remove_permission_from_role(
        &self,
        role_id: Uuid,
        permission_id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    fn get_role_permissions(
        &self,
        role_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Permission>, CoreError>> + Send;
}

#[cfg_attr(test, mockall::automock)]
pub trait AuthorizationRepository: Send + Sync {
    fn check_permission(
        &self,
        check: PolicyCheck,
    ) -> impl Future<Output = Result<bool, CoreError>> + Send;

    fn get_user_roles(
        &self,
        user_id: &str,
        realm_id: &str,
    ) -> impl Future<Output = Result<Vec<Role>, CoreError>> + Send;
}

// ============================================================================
// User Role Management
// ============================================================================

/// Request to assign a role to a user
#[derive(Debug, Clone)]
pub struct AssignRoleToUserRequest {
    pub realm_id: String,
    pub user_id: Uuid,
    pub role_id: Uuid,
    pub client_id: String,
}

/// UserRoleRepository - Domain Port
#[cfg_attr(test, mockall::automock)]
pub trait UserRoleRepository: Send + Sync {
    /// Assign a role to a user
    fn assign_role_to_user(
        &self,
        request: AssignRoleToUserRequest,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// Invalidate user role cache
    fn invalidate_cache(
        &self,
        realm_id: &str,
        user_id: &str,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}

// ============================================================================
// Service Ports
// ============================================================================

#[cfg_attr(test, mockall::automock)]
pub trait RoleService: Send + Sync {
    fn create_role(
        &self,
        identity: Identity,
        request: CreateRoleRequest,
    ) -> impl Future<Output = Result<Role, CoreError>> + Send;

    fn list_roles(
        &self,
        identity: Identity,
        realm_id: String,
        client_id: String,
    ) -> impl Future<Output = Result<Vec<Role>, CoreError>> + Send;

    fn delete_role(
        &self,
        identity: Identity,
        id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}

#[cfg_attr(test, mockall::automock)]
pub trait PermissionCrudService: Send + Sync {
    fn create_permission(
        &self,
        identity: Identity,
        request: CreatePermissionRequest,
    ) -> impl Future<Output = Result<Permission, CoreError>> + Send;

    fn list_permissions(
        &self,
        identity: Identity,
        realm_id: String,
    ) -> impl Future<Output = Result<Vec<Permission>, CoreError>> + Send;

    fn delete_permission(
        &self,
        identity: Identity,
        id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}

#[cfg_attr(test, mockall::automock)]
pub trait AuthorizationService: Send + Sync {
    fn check_permission(
        &self,
        identity: Identity,
        request: CheckPermissionRequest,
    ) -> impl Future<Output = Result<bool, CoreError>> + Send;

    fn assign_permission_to_role(
        &self,
        identity: Identity,
        request: AssignPermissionRequest,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    fn remove_permission_from_role(
        &self,
        identity: Identity,
        role_id: Uuid,
        permission_id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}

pub mod principal;

// Service implementations
pub mod permission_service;
pub mod services;

// Re-exports
pub use permission_service::{PermissionService, Policy};
pub use principal::PrincipalRef;
pub use principal::principal_types;
pub use services::PermissionServiceImpl as PermissionCrudServiceImpl;
