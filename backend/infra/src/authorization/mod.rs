use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter};
use std::sync::Arc;
use uuid::Uuid;

use herald_domain::authorization::{
    AssignRoleToUserRequest, AuthorizationRepository, CreatePermissionRequest, CreateRoleRequest,
    Permission, PermissionRepository, PermissionService, PolicyCheck, Role,
    RolePermissionRepository, RoleRepository, UserRoleRepository, principal_types,
};
use herald_domain::common::entities::app_errors::CoreError;
use herald_entity::{permissions, role_permissions, roles, user_roles};

// Infrastructure implementations
pub mod cache;
pub mod policies;
pub mod redis_permission_checker;
pub mod role_policy_repository;

pub use cache::RedisCache;
pub use policies::{
    PermissionBasedBillingPolicy, PermissionBasedClientPolicy, PermissionBasedOAuthConfigPolicy,
    PermissionBasedPointsPolicy, PermissionBasedRealmConfigPolicy, PermissionBasedRealmPolicy,
};
pub use redis_permission_checker::RedisPermissionChecker;
pub use role_policy_repository::PostgresRolePolicyRepository;

pub struct PermissionCheckerAuthorizationRepository {
    permission_checker: Arc<RedisPermissionChecker>,
    db: Arc<sea_orm::DatabaseConnection>,
}

impl PermissionCheckerAuthorizationRepository {
    pub fn new(
        permission_checker: Arc<RedisPermissionChecker>,
        db: Arc<sea_orm::DatabaseConnection>,
    ) -> Self {
        Self {
            permission_checker,
            db,
        }
    }
}

impl AuthorizationRepository for PermissionCheckerAuthorizationRepository {
    async fn check_permission(&self, check: PolicyCheck) -> Result<bool, CoreError> {
        self.permission_checker
            .check_permission(
                &check.realm_id,
                &check.user_id,
                &check.resource,
                &check.action,
            )
            .await
            .map_err(|e| CoreError::InternalServerError(format!("Permission check failed: {}", e)))
    }

    async fn get_user_roles(&self, user_id: &str, realm_id: &str) -> Result<Vec<Role>, CoreError> {
        // Query user's roles through the permission checker
        let role_ids = self
            .permission_checker
            .get_user_roles(realm_id, user_id)
            .await?;

        // Convert role IDs to Role domain objects
        if role_ids.is_empty() {
            return Ok(vec![]);
        }

        // Parse role IDs as UUIDs
        let role_uuids: Vec<Uuid> = role_ids
            .into_iter()
            .filter_map(|id| Uuid::parse_str(&id).ok())
            .collect();

        // Query role details from database
        let repo = PostgresRoleRepository::new(self.db.clone());
        let roles = repo.find_by_ids(realm_id, role_uuids).await?;

        Ok(roles)
    }
}

pub struct PostgresRoleRepository {
    db: Arc<sea_orm::DatabaseConnection>,
}

impl PostgresRoleRepository {
    pub fn new(db: Arc<sea_orm::DatabaseConnection>) -> Self {
        Self { db }
    }

    fn to_domain(model: &roles::Model) -> Role {
        Role {
            id: model.id,
            name: model.name.clone(),
            description: model.description.clone(),
            realm_id: model.realm_id.clone(),
            client_id: model.client_id.clone(),
            is_builtin: model.is_builtin,
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        }
    }
}

impl RoleRepository for PostgresRoleRepository {
    async fn create_role(&self, request: CreateRoleRequest) -> Result<Role, CoreError> {
        if let Ok(existing_role) = self
            .find_by_name(&request.name, &request.realm_id, &request.client_id)
            .await
        {
            tracing::info!(
                "Role already exists: id={}, name={}, realm_id={}, client_id={}",
                existing_role.id,
                existing_role.name,
                existing_role.realm_id,
                existing_role.client_id
            );
            return Ok(existing_role);
        }

        let now = chrono::Utc::now();
        // 使用 UUID v7 生成角色 ID
        let id = herald_domain::common::entities::generate_uuid_v7();

        tracing::debug!(
            "Creating role: id={}, name={}, realm_id={}, client_id={}",
            id,
            request.name,
            request.realm_id,
            request.client_id
        );

        let active_model = roles::ActiveModel {
            id: sea_orm::Set(id),
            name: sea_orm::Set(request.name),
            description: sea_orm::Set(request.description),
            realm_id: sea_orm::Set(request.realm_id),
            client_id: sea_orm::Set(request.client_id),
            is_builtin: sea_orm::Set(request.is_builtin),
            created_at: sea_orm::Set(now.into()),
            updated_at: sea_orm::Set(now.into()),
        };

        let result = active_model.insert(&*self.db).await?;
        let role = Self::to_domain(&result);

        tracing::info!(
            "Successfully created role: id={}, name={}, realm_id={}, client_id={}",
            role.id,
            role.name,
            role.realm_id,
            role.client_id
        );

        Ok(role)
    }

    async fn get_role_by_id(&self, realm_id: &str, id: Uuid) -> Result<Role, CoreError> {
        let result = roles::Entity::find_by_id(id)
            .filter(roles::Column::RealmId.eq(realm_id))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        Ok(Self::to_domain(&result))
    }

    async fn find_by_name(
        &self,
        name: &str,
        realm_id: &str,
        client_id: &str,
    ) -> Result<Role, CoreError> {
        let result = roles::Entity::find()
            .filter(roles::Column::Name.eq(name))
            .filter(roles::Column::RealmId.eq(realm_id))
            .filter(roles::Column::ClientId.eq(client_id))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        Ok(Self::to_domain(&result))
    }

    async fn list_roles(&self, realm_id: &str, client_id: &str) -> Result<Vec<Role>, CoreError> {
        let results = roles::Entity::find()
            .filter(roles::Column::RealmId.eq(realm_id))
            .filter(roles::Column::ClientId.eq(client_id))
            .all(&*self.db)
            .await?;

        Ok(results.iter().map(Self::to_domain).collect())
    }

    async fn delete_role(&self, realm_id: &str, id: Uuid) -> Result<(), CoreError> {
        roles::Entity::delete_many()
            .filter(roles::Column::Id.eq(id))
            .filter(roles::Column::RealmId.eq(realm_id))
            .exec(&*self.db)
            .await?;

        Ok(())
    }

    async fn find_by_ids(&self, realm_id: &str, ids: Vec<Uuid>) -> Result<Vec<Role>, CoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let results = roles::Entity::find()
            .filter(roles::Column::Id.is_in(ids))
            .filter(roles::Column::RealmId.eq(realm_id))
            .all(&*self.db)
            .await?;

        Ok(results.iter().map(Self::to_domain).collect())
    }
}

pub struct PostgresPermissionRepository {
    db: Arc<sea_orm::DatabaseConnection>,
}

impl PostgresPermissionRepository {
    pub fn new(db: Arc<sea_orm::DatabaseConnection>) -> Self {
        Self { db }
    }

    fn to_domain(model: &permissions::Model) -> Permission {
        Permission {
            id: model.id,
            name: model.name.clone(),
            resource: model.resource.clone(),
            action: model.action.clone(),
            description: model.description.clone(),
            realm_id: model.realm_id.clone(),
            is_builtin: model.is_builtin,
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        }
    }
}

impl PermissionRepository for PostgresPermissionRepository {
    async fn create_permission(
        &self,
        request: CreatePermissionRequest,
    ) -> Result<Permission, CoreError> {
        if let Ok(existing_permission) = self.find_by_name(&request.name, &request.realm_id).await {
            tracing::info!(
                "Permission already exists: id={}, name={}, realm_id={}",
                existing_permission.id,
                existing_permission.name,
                existing_permission.realm_id
            );
            return Ok(existing_permission);
        }

        let now = chrono::Utc::now();
        // 使用 UUID v7 生成权限 ID
        let id = herald_domain::common::entities::generate_uuid_v7();

        let active_model = permissions::ActiveModel {
            id: sea_orm::Set(id),
            name: sea_orm::Set(request.name),
            resource: sea_orm::Set(request.resource),
            action: sea_orm::Set(request.action),
            description: sea_orm::Set(request.description),
            realm_id: sea_orm::Set(request.realm_id),
            is_builtin: sea_orm::Set(request.is_builtin),
            created_at: sea_orm::Set(now.into()),
            updated_at: sea_orm::Set(now.into()),
        };

        let result = active_model.insert(&*self.db).await?;
        Ok(Self::to_domain(&result))
    }

    async fn get_permission_by_id(
        &self,
        realm_id: &str,
        id: Uuid,
    ) -> Result<Permission, CoreError> {
        let result = permissions::Entity::find_by_id(id)
            .filter(permissions::Column::RealmId.eq(realm_id))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        Ok(Self::to_domain(&result))
    }

    async fn list_permissions(&self, realm_id: &str) -> Result<Vec<Permission>, CoreError> {
        let results = permissions::Entity::find()
            .filter(permissions::Column::RealmId.eq(realm_id))
            .all(&*self.db)
            .await?;

        Ok(results.iter().map(Self::to_domain).collect())
    }

    async fn delete_permission(&self, realm_id: &str, id: Uuid) -> Result<(), CoreError> {
        permissions::Entity::delete_many()
            .filter(permissions::Column::Id.eq(id))
            .filter(permissions::Column::RealmId.eq(realm_id))
            .exec(&*self.db)
            .await?;

        Ok(())
    }
}

impl PostgresPermissionRepository {
    /// Find permission by name and realm_id (for idempotency check)
    async fn find_by_name(&self, name: &str, realm_id: &str) -> Result<Permission, CoreError> {
        let result = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(name))
            .filter(permissions::Column::RealmId.eq(realm_id))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        Ok(Self::to_domain(&result))
    }
}

pub struct PostgresRolePermissionRepository {
    db: Arc<sea_orm::DatabaseConnection>,
    permission_checker: Arc<RedisPermissionChecker>,
}

impl PostgresRolePermissionRepository {
    pub fn new(
        db: Arc<sea_orm::DatabaseConnection>,
        permission_checker: Arc<RedisPermissionChecker>,
    ) -> Self {
        Self {
            db,
            permission_checker,
        }
    }

    async fn invalidate_cache(&self, realm_id: &str) -> Result<(), CoreError> {
        self.permission_checker
            .invalidate_realm_cache(realm_id)
            .await?;
        Ok(())
    }
}

pub struct PostgresUserRoleRepository {
    db: Arc<sea_orm::DatabaseConnection>,
    permission_checker: Arc<RedisPermissionChecker>,
}

impl PostgresUserRoleRepository {
    pub fn new(
        db: Arc<sea_orm::DatabaseConnection>,
        permission_checker: Arc<RedisPermissionChecker>,
    ) -> Self {
        Self {
            db,
            permission_checker,
        }
    }
}

impl UserRoleRepository for PostgresUserRoleRepository {
    fn assign_role_to_user(
        &self,
        request: AssignRoleToUserRequest,
    ) -> impl Future<Output = Result<(), CoreError>> + Send {
        let db = self.db.clone();
        let permission_checker = self.permission_checker.clone();
        let realm_id = request.realm_id.clone();
        let user_id = request.user_id;

        async move {
            let id = herald_domain::common::entities::generate_uuid_v7();

            let user_role = user_roles::ActiveModel {
                id: sea_orm::Set(id),
                realm_id: sea_orm::Set(request.realm_id.clone()),
                user_id: sea_orm::Set(Some(request.user_id)),
                role_id: sea_orm::Set(request.role_id),
                client_id: sea_orm::Set(Some(request.client_id)),
                principal_type: sea_orm::Set(principal_types::USER.to_string()),
                principal_id: sea_orm::Set(request.user_id.to_string()),
                // BE-D01 columns: this is the manual admin-assign path — origin
                // is `manual` (no payment source, no subscription expiry).
                source: sea_orm::Set("manual".to_string()),
                source_id: sea_orm::Set(None),
                expires_at: sea_orm::Set(None),
                created_at: sea_orm::Set(chrono::Utc::now().into()),
            };

            match user_role.insert(&*db).await {
                Ok(_) => {
                    permission_checker
                        .invalidate_user_role_cache(&realm_id, &user_id.to_string())
                        .await?;
                    Ok(())
                }
                Err(e) if is_duplicate_key_error(&e) => {
                    tracing::debug!(
                        user_id = %user_id,
                        role_id = %request.role_id,
                        "Role already assigned to user (idempotent)"
                    );
                    Ok(())
                }
                Err(e) => Err(CoreError::InternalServerError(format!(
                    "Failed to assign role: {}",
                    e
                ))),
            }
        }
    }

    fn invalidate_cache(
        &self,
        realm_id: &str,
        user_id: &str,
    ) -> impl Future<Output = Result<(), CoreError>> + Send {
        let permission_checker = self.permission_checker.clone();
        let realm_id = realm_id.to_string();
        let user_id = user_id.to_string();

        async move {
            permission_checker
                .invalidate_user_role_cache(&realm_id, &user_id)
                .await
                .map_err(|e| {
                    CoreError::InternalServerError(format!("Cache invalidation failed: {}", e))
                })
        }
    }
}

/// Helper function to detect duplicate key errors
pub fn is_duplicate_key_error(err: &sea_orm::DbErr) -> bool {
    matches!(err, sea_orm::DbErr::Exec(_) if err.to_string().contains("duplicate key"))
}

impl RolePermissionRepository for PostgresRolePermissionRepository {
    async fn assign_permission_to_role(
        &self,
        role_id: Uuid,
        permission_id: Uuid,
    ) -> Result<(), CoreError> {
        let now = chrono::Utc::now();
        // 使用 UUID v7 生成角色-权限关联 ID
        let id = herald_domain::common::entities::generate_uuid_v7();

        let existing = role_permissions::Entity::find()
            .filter(role_permissions::Column::RoleId.eq(role_id))
            .filter(role_permissions::Column::PermissionId.eq(permission_id))
            .one(&*self.db)
            .await?;

        if existing.is_some() {
            tracing::debug!(
                "Role-permission assignment already exists: role_id={}, permission_id={}",
                role_id,
                permission_id
            );
            return Ok(()); // Idempotent: no-op if already assigned
        }

        let active_model = role_permissions::ActiveModel {
            id: sea_orm::Set(id),
            role_id: sea_orm::Set(role_id),
            permission_id: sea_orm::Set(permission_id),
            created_at: sea_orm::Set(now.into()),
        };

        active_model.insert(&*self.db).await?;

        // Invalidate cache for the realm
        // Get realm_id from role
        let role = roles::Entity::find_by_id(role_id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;
        self.invalidate_cache(&role.realm_id).await?;

        Ok(())
    }

    async fn remove_permission_from_role(
        &self,
        role_id: Uuid,
        permission_id: Uuid,
    ) -> Result<(), CoreError> {
        // Get realm_id before deleting
        let role = roles::Entity::find_by_id(role_id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        role_permissions::Entity::delete_many()
            .filter(role_permissions::Column::RoleId.eq(role_id))
            .filter(role_permissions::Column::PermissionId.eq(permission_id))
            .exec(&*self.db)
            .await?;

        // Invalidate cache for the realm
        self.invalidate_cache(&role.realm_id).await?;

        Ok(())
    }

    async fn get_role_permissions(&self, role_id: Uuid) -> Result<Vec<Permission>, CoreError> {
        let results = role_permissions::Entity::find()
            .filter(role_permissions::Column::RoleId.eq(role_id))
            .find_also_related(permissions::Entity)
            .all(&*self.db)
            .await?;

        let permissions = results
            .into_iter()
            .filter_map(|(_, perm)| perm)
            .map(|p| Permission {
                id: p.id,
                name: p.name,
                resource: p.resource,
                action: p.action,
                description: p.description,
                realm_id: p.realm_id,
                is_builtin: p.is_builtin,
                created_at: p.created_at.into(),
                updated_at: p.updated_at.into(),
            })
            .collect();

        Ok(permissions)
    }
}
