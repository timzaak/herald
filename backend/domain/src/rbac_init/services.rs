use std::sync::Arc;

use crate::authorization::{
    CreatePermissionRequest, CreateRoleRequest, PermissionRepository, RolePermissionRepository,
    RoleRepository,
};
use crate::common::entities::app_errors::CoreError;
use crate::rbac_init::{RealmInitializationService, RealmRBACInitRequest, RolePolicyRepository};

/// Parse permission name in format "resource.action" into (resource, action) tuple
fn parse_permission(permission: &str) -> Result<(&str, &str), String> {
    let parts: Vec<&str> = permission.split('.').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid permission format: {}. Expected 'resource.action'",
            permission
        ));
    }
    Ok((parts[0], parts[1]))
}

pub struct RealmInitializationServiceImpl<R, P, RP, RPR>
where
    R: RoleRepository,
    P: PermissionRepository,
    RP: RolePermissionRepository,
    RPR: RolePolicyRepository,
{
    role_repository: Arc<R>,
    permission_repository: Arc<P>,
    role_permission_repository: Arc<RP>,
    role_policy_repository: Arc<RPR>,
}

impl<R, P, RP, RPR> RealmInitializationServiceImpl<R, P, RP, RPR>
where
    R: RoleRepository,
    P: PermissionRepository,
    RP: RolePermissionRepository,
    RPR: RolePolicyRepository,
{
    pub fn new(
        role_repository: Arc<R>,
        permission_repository: Arc<P>,
        role_permission_repository: Arc<RP>,
        role_policy_repository: Arc<RPR>,
    ) -> Self {
        Self {
            role_repository,
            permission_repository,
            role_permission_repository,
            role_policy_repository,
        }
    }
}

impl<R, P, RP, RPR> RealmInitializationService for RealmInitializationServiceImpl<R, P, RP, RPR>
where
    R: RoleRepository,
    P: PermissionRepository,
    RP: RolePermissionRepository,
    RPR: RolePolicyRepository,
{
    async fn init_default_rbac(&self, request: RealmRBACInitRequest) -> Result<(), CoreError> {
        tracing::info!(
            "Starting RBAC initialization for realm {} with client_id '{}'",
            request.realm_id,
            request.admin_web_console_client_id
        );

        // 1. 创建 realm-admin 角色
        let realm_admin_role = self
            .role_repository
            .create_role(CreateRoleRequest {
                realm_id: request.realm_id.clone(),
                client_id: request.admin_web_console_client_id.clone(),
                name: "realm-admin".to_string(),
                description: Some("Realm Administrator with full permissions".to_string()),
                is_builtin: true,
            })
            .await?;

        tracing::info!(
            "Created realm-admin role {} for realm {}",
            realm_admin_role.id,
            request.realm_id
        );

        // 2. 创建 user 角色
        let user_role = self
            .role_repository
            .create_role(CreateRoleRequest {
                realm_id: request.realm_id.clone(),
                client_id: request.admin_web_console_client_id.clone(),
                name: "user".to_string(),
                description: Some("Regular user".to_string()),
                is_builtin: true,
            })
            .await?;

        tracing::info!(
            "Created user role {} for realm {}",
            user_role.id,
            request.realm_id
        );

        // 3. 创建 realm-admin 的权限
        // Note: realm.view and realm.manage only exist in admin realm
        let is_admin_realm = request.realm_id == "admin";
        let mut realm_admin_permissions: Vec<(&str, &str)> = vec![
            ("dashboard.view", "View dashboard"),
            ("audit.view", "View audit logs"),
            ("users.view", "View users"),
            ("users.manage", "User management"),
            ("clients.view", "View client applications"),
            ("clients.manage", "Client application management"),
            ("roles.view", "View roles"),
            ("roles.manage", "Role management"),
            ("permissions.view", "View permissions"),
            ("permissions.manage", "Permission management"),
            ("policies.view", "View policies"),
            ("policies.manage", "Policy management"),
            ("settings.view", "View settings"),
            ("settings.manage", "Settings management"),
            ("api_keys.view", "View API keys"),
            ("api_keys.manage", "API Key management"),
            ("billing.view", "View billing plans"),
            ("billing.manage", "Manage billing plans"),
            ("points.view", "View points accounts and transactions"),
            ("points.manage", "Manage points accounts and transactions"),
            // Organization face gate — new builtin permissions must keep the
            // permissions list and the realm-admin policy list below in sync.
            ("orgs.view", "View organizations"),
            ("orgs.manage", "Manage organizations (grant to app keys)"),
        ];
        if is_admin_realm {
            realm_admin_permissions.insert(0, ("realm.view", "View realm information"));
            realm_admin_permissions.insert(1, ("realm.manage", "Create and manage realms"));
        }

        for (name, description) in realm_admin_permissions {
            let (resource, action) = parse_permission(name).map_err(|e| {
                CoreError::InternalServerError(format!("Failed to parse permission: {}", e))
            })?;

            let permission = self
                .permission_repository
                .create_permission(CreatePermissionRequest {
                    realm_id: request.realm_id.clone(),
                    name: name.to_string(),
                    resource: resource.to_string(),
                    action: action.to_string(),
                    description: Some(description.to_string()),
                    is_builtin: true,
                })
                .await?;

            self.role_permission_repository
                .assign_permission_to_role(realm_admin_role.id, permission.id)
                .await?;
        }

        // 4. user 角色的基本权限（允许查看自己的积分）
        // 用户修改自己的 profile 和 password 不需要权限检查（在业务逻辑层处理）
        let user_permissions = vec![("points.view", "View own points balance")];

        for (name, description) in user_permissions {
            let (resource, action) = parse_permission(name).map_err(|e| {
                CoreError::InternalServerError(format!("Failed to parse permission: {}", e))
            })?;

            let permission = self
                .permission_repository
                .create_permission(CreatePermissionRequest {
                    realm_id: request.realm_id.clone(),
                    name: name.to_string(),
                    resource: resource.to_string(),
                    action: action.to_string(),
                    description: Some(description.to_string()),
                    is_builtin: true,
                })
                .await?;

            self.role_permission_repository
                .assign_permission_to_role(user_role.id, permission.id)
                .await?;
        }

        // 5. 为 realm-admin 角色添加权限策略

        // Realm admin policies
        // Note: realm.view and realm.manage policies only exist in admin realm
        let mut realm_admin_policies: Vec<(&str, &str)> = vec![
            ("dashboard", "view"),
            ("audit", "view"),
            ("users", "view"),
            ("users", "manage"),
            ("clients", "view"),
            ("clients", "manage"),
            ("roles", "view"),
            ("roles", "manage"),
            ("permissions", "view"),
            ("permissions", "manage"),
            ("policies", "view"),
            ("policies", "manage"),
            ("settings", "view"),
            ("settings", "manage"),
            ("api_keys", "view"),
            ("api_keys", "manage"),
            ("billing", "view"),
            ("billing", "manage"),
            ("points", "view"),
            ("points", "manage"),
            // Organization face gate: without these the realm admin cannot
            // self-hold orgs.* and could never grant them to app keys
            // (grantor self-hold on policy grants), leaving the ext orgs
            // endpoints unreachable for every real deployment. Keep this list
            // in sync with the builtin permissions above.
            ("orgs", "view"),
            ("orgs", "manage"),
        ];
        if is_admin_realm {
            realm_admin_policies.insert(0, ("realm", "view"));
            realm_admin_policies.insert(1, ("realm", "manage"));
        }

        for (resource, action) in realm_admin_policies {
            self.role_policy_repository
                .create_policy(crate::rbac_init::CreateRolePolicyRequest {
                    realm_id: request.realm_id.clone(),
                    role_id: realm_admin_role.id,
                    resource: resource.to_string(),
                    action: action.to_string(),
                })
                .await?;

            tracing::info!(
                realm_id = %request.realm_id,
                resource = %resource,
                action = %action,
                "Policy added successfully"
            );
        }

        // 5. 为 user 角色添加基本策略
        // 用户修改自己的 profile 和 password 在业务逻辑层处理，不需要权限检查
        let user_policies: Vec<(&str, &str)> = vec![("points", "view")];

        for (resource, action) in user_policies {
            self.role_policy_repository
                .create_policy(crate::rbac_init::CreateRolePolicyRequest {
                    realm_id: request.realm_id.clone(),
                    role_id: user_role.id,
                    resource: resource.to_string(),
                    action: action.to_string(),
                })
                .await?;

            tracing::info!(
                realm_id = %request.realm_id,
                resource = %resource,
                action = %action,
                "User role policy added successfully"
            );
        }

        // Invalidate cache for this realm to ensure new policies are loaded
        self.role_policy_repository
            .invalidate_realm_cache(&request.realm_id)
            .await?;

        tracing::info!(
            realm_id = %request.realm_id,
            "Successfully initialized default RBAC configuration"
        );

        Ok(())
    }
}
