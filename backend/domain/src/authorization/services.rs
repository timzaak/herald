use std::sync::Arc;
use uuid::Uuid;

use crate::authentication::Identity;
use crate::authorization::{
    AssignPermissionRequest, AuthorizationRepository, AuthorizationService, CheckPermissionRequest,
    CreatePermissionRequest, CreateRoleRequest, Permission, PermissionCrudService,
    PermissionRepository, Role, RolePermissionRepository, RoleRepository, RoleService,
};
use crate::common::entities::app_errors::CoreError;

// ============================================================================
// Role Service Implementation
// ============================================================================

pub struct RoleServiceImpl<R>
where
    R: RoleRepository,
{
    role_repository: Arc<R>,
}

impl<R> RoleServiceImpl<R>
where
    R: RoleRepository,
{
    pub fn new(role_repository: Arc<R>) -> Self {
        Self { role_repository }
    }

    /// Internal method: batch query roles scoped to the realm. The
    /// repository filters by `realm_id`, so a foreign-realm ID in the input
    /// is simply absent from the output.
    pub async fn list_roles_internal(
        &self,
        realm_id: &str,
        role_ids: &[Uuid],
    ) -> Result<Vec<Role>, CoreError> {
        self.role_repository
            .find_by_ids(realm_id, role_ids.to_vec())
            .await
    }
}

impl<R> RoleService for RoleServiceImpl<R>
where
    R: RoleRepository,
{
    async fn create_role(
        &self,
        _identity: Identity,
        request: CreateRoleRequest,
    ) -> Result<Role, CoreError> {
        self.role_repository.create_role(request).await
    }

    async fn list_roles(
        &self,
        _identity: Identity,
        realm_id: String,
        client_id: String,
    ) -> Result<Vec<Role>, CoreError> {
        self.role_repository.list_roles(&realm_id, &client_id).await
    }

    async fn delete_role(&self, identity: Identity, id: Uuid) -> Result<(), CoreError> {
        // The repository scopes the delete to the caller's realm: an id from
        // another realm matches zero rows instead of deleting it.
        self.role_repository
            .delete_role(&identity.realm_id(), id)
            .await
    }
}

// ============================================================================
// Permission Service Implementation
// ============================================================================

pub struct PermissionServiceImpl<P>
where
    P: PermissionRepository,
{
    permission_repository: Arc<P>,
}

impl<P> PermissionServiceImpl<P>
where
    P: PermissionRepository,
{
    pub fn new(permission_repository: Arc<P>) -> Self {
        Self {
            permission_repository,
        }
    }
}

impl<P> PermissionCrudService for PermissionServiceImpl<P>
where
    P: PermissionRepository,
{
    async fn create_permission(
        &self,
        _identity: Identity,
        request: CreatePermissionRequest,
    ) -> Result<Permission, CoreError> {
        self.permission_repository.create_permission(request).await
    }

    async fn list_permissions(
        &self,
        _identity: Identity,
        realm_id: String,
    ) -> Result<Vec<Permission>, CoreError> {
        self.permission_repository.list_permissions(&realm_id).await
    }

    async fn delete_permission(&self, identity: Identity, id: Uuid) -> Result<(), CoreError> {
        // Realm-scoped like delete_role: a foreign-realm id is a no-op.
        self.permission_repository
            .delete_permission(&identity.realm_id(), id)
            .await
    }
}

// ============================================================================
// Authorization Service Implementation
// ============================================================================

pub struct AuthorizationServiceImpl<RP, A>
where
    RP: RolePermissionRepository,
    A: AuthorizationRepository,
{
    role_permission_repository: Arc<RP>,
    authorization_repository: Arc<A>,
}

impl<RP, A> AuthorizationServiceImpl<RP, A>
where
    RP: RolePermissionRepository,
    A: AuthorizationRepository,
{
    pub fn new(role_permission_repository: Arc<RP>, authorization_repository: Arc<A>) -> Self {
        Self {
            role_permission_repository,
            authorization_repository,
        }
    }
}

impl<RP, A> AuthorizationService for AuthorizationServiceImpl<RP, A>
where
    RP: RolePermissionRepository,
    A: AuthorizationRepository,
{
    async fn check_permission(
        &self,
        identity: Identity,
        request: CheckPermissionRequest,
    ) -> Result<bool, CoreError> {
        let policy_check = crate::authorization::PolicyCheck {
            realm_id: identity.realm_id(),
            client_id: identity.client_id(),
            user_id: identity.user_id(),
            resource: request.resource,
            action: request.action,
        };

        self.authorization_repository
            .check_permission(policy_check)
            .await
    }

    async fn assign_permission_to_role(
        &self,
        _identity: Identity,
        request: AssignPermissionRequest,
    ) -> Result<(), CoreError> {
        self.role_permission_repository
            .assign_permission_to_role(request.role_id, request.permission_id)
            .await
    }

    async fn remove_permission_from_role(
        &self,
        _identity: Identity,
        role_id: Uuid,
        permission_id: Uuid,
    ) -> Result<(), CoreError> {
        self.role_permission_repository
            .remove_permission_from_role(role_id, permission_id)
            .await
    }
}

impl<R> std::fmt::Debug for RoleServiceImpl<R>
where
    R: RoleRepository,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoleServiceImpl").finish()
    }
}

impl<P> std::fmt::Debug for PermissionServiceImpl<P>
where
    P: PermissionRepository,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PermissionServiceImpl").finish()
    }
}

impl<RP, A> std::fmt::Debug for AuthorizationServiceImpl<RP, A>
where
    RP: RolePermissionRepository,
    A: AuthorizationRepository,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthorizationServiceImpl").finish()
    }
}
