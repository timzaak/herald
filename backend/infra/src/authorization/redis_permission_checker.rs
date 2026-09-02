// =============================================================================
// Redis Permission Checker - Infrastructure Layer
// =============================================================================
//
// Implements permission checking logic with Redis caching
/// Concrete implementation of PermissionService trait using:
// - Database for persistent storage
// - Redis for caching layer
//
// =============================================================================
use herald_domain::authorization::permission_service::{PermissionService, Policy};
use herald_domain::authorization::principal_types;
use herald_domain::common::entities::app_errors::CoreError;
use herald_entity::{role_policies, user_roles};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use super::cache::RedisCache;

/// Cache key builder for consistent key formatting
struct CacheKey;

impl CacheKey {
    /// Build a principal permission check result cache key
    fn principal_permission(
        realm_id: &str,
        principal_type: &str,
        principal_id: &str,
        resource: &str,
        action: &str,
    ) -> String {
        format!(
            "principal_perm:{}:{}:{}:{}:{}",
            realm_id, principal_type, principal_id, resource, action
        )
    }

    /// Build a principal role bindings cache key
    fn principal_role_bindings(realm_id: &str, principal_type: &str, principal_id: &str) -> String {
        format!(
            "principal_role_bindings:{}:{}:{}",
            realm_id, principal_type, principal_id
        )
    }

    /// Build a role policies cache key
    fn role_policies(realm_id: &str, role_id: &str) -> String {
        format!("role_policies:{}:{}", realm_id, role_id)
    }

    /// Build a user roles invalidation pattern
    fn user_roles_pattern(realm_id: &str) -> String {
        format!("user_roles:{}:*", realm_id)
    }

    /// Build a principal role bindings invalidation pattern
    fn principal_role_bindings_pattern(realm_id: &str) -> String {
        format!("principal_role_bindings:{}:*", realm_id)
    }

    /// Build a permission cache invalidation pattern
    fn permission_pattern(realm_id: &str, user_id: Option<&str>) -> String {
        match user_id {
            Some(uid) => format!("perm:{}:{}:*", realm_id, uid),
            None => format!("perm:{}:*", realm_id),
        }
    }

    /// Build a principal permission cache invalidation pattern
    fn principal_permission_pattern(
        realm_id: &str,
        principal_type: Option<&str>,
        principal_id: Option<&str>,
    ) -> String {
        match (principal_type, principal_id) {
            (Some(pt), Some(pid)) => {
                format!("principal_perm:{}:{}:{}:*", realm_id, pt, pid)
            }
            _ => format!("principal_perm:{}:*", realm_id),
        }
    }

    /// Build a role policies invalidation pattern
    fn role_policies_pattern(realm_id: &str) -> String {
        format!("role_policies:{}:*", realm_id)
    }
}

/// Cache TTL constants (in seconds)
mod cache_ttl {
    /// Short cache for denials (1 minute)
    pub const DENIAL: u64 = 60;
    /// Cache for user roles (5 minutes)
    pub const USER_ROLES: u64 = 300;
    /// Cache for role policies (10 minutes)
    pub const ROLE_POLICIES: u64 = 600;
}

/// Permission Checker - Infrastructure implementation
#[derive(Debug)]
pub struct RedisPermissionChecker {
    db: Arc<DatabaseConnection>,
    cache: Arc<RwLock<RedisCache>>,
}

impl RedisPermissionChecker {
    /// Create a new RedisPermissionChecker instance
    pub fn new(db: Arc<DatabaseConnection>, cache: Arc<RwLock<RedisCache>>) -> Self {
        Self { db, cache }
    }
}

impl PermissionService for RedisPermissionChecker {
    /// Check if a user has permission to access a resource.
    ///
    /// Delegates to `check_principal_permission` with `principal_type = "user"`.
    async fn check_permission(
        &self,
        realm_id: &str,
        user_id: &str,
        resource: &str,
        action: &str,
    ) -> Result<bool, CoreError> {
        self.check_principal_permission(realm_id, principal_types::USER, user_id, resource, action)
            .await
    }

    /// Get all roles for a user.
    ///
    /// Delegates to `get_principal_roles` with `principal_type = "user"`.
    async fn get_user_roles(
        &self,
        realm_id: &str,
        user_id: &str,
    ) -> Result<Vec<String>, CoreError> {
        self.get_principal_roles(realm_id, principal_types::USER, user_id)
            .await
    }

    /// Get all policies for a role (with caching)
    ///
    /// # Caching
    /// * TTL: 10 minutes (600 seconds)
    /// * Invalidated on: role policy changes
    async fn get_role_policies(
        &self,
        realm_id: &str,
        role_id: &str,
    ) -> Result<Vec<Policy>, CoreError> {
        let cache_key = CacheKey::role_policies(realm_id, role_id);

        // Return cached policies if available
        if let Some(cached) = self.get_cached::<Vec<Policy>>(&cache_key).await {
            debug!("Role policies from cache");
            return Ok(cached);
        }

        // Parse role_id as UUID
        let role_uuid = uuid::Uuid::parse_str(role_id)
            .map_err(|_| CoreError::BadRequest(format!("Invalid role_id UUID: {}", role_id)))?;

        // Query from database
        let db_results = role_policies::Entity::find()
            .filter(role_policies::Column::RealmId.eq(realm_id))
            .filter(role_policies::Column::RoleId.eq(role_uuid))
            .all(&*self.db)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to query role policies");
                CoreError::DatabaseError(format!("Failed to query role policies: {}", e))
            })?;

        let policies: Vec<Policy> = db_results
            .into_iter()
            .map(|p| Policy {
                resource: p.resource,
                action: p.action,
            })
            .collect();

        // Cache for 10 minutes
        self.cache(&cache_key, &policies, cache_ttl::ROLE_POLICIES)
            .await;
        debug!(count = policies.len(), "Role policies from database");

        Ok(policies)
    }

    /// Invalidate user role cache
    ///
    /// Call this when a user's roles are assigned/removed.
    /// Also invalidates the principal-oriented caches for the same user.
    async fn invalidate_user_role_cache(
        &self,
        realm_id: &str,
        user_id: &str,
    ) -> Result<(), CoreError> {
        let patterns = vec![
            CacheKey::user_roles_pattern(realm_id),
            CacheKey::permission_pattern(realm_id, Some(user_id)),
            CacheKey::principal_role_bindings_pattern(realm_id),
            CacheKey::principal_permission_pattern(
                realm_id,
                Some(principal_types::USER),
                Some(user_id),
            ),
        ];
        self.invalidate_patterns(&patterns).await;

        info!(
            realm_id = %realm_id,
            user_id = %user_id,
            "User role cache invalidated"
        );

        Ok(())
    }

    /// Invalidate role policy cache
    ///
    /// Call this when a role's policies are added/removed
    async fn invalidate_role_policy_cache(
        &self,
        realm_id: &str,
        role_id: &str,
    ) -> Result<(), CoreError> {
        let patterns = vec![
            CacheKey::role_policies(realm_id, role_id),
            CacheKey::permission_pattern(realm_id, None),
            CacheKey::principal_permission_pattern(realm_id, None, None),
        ];
        self.invalidate_patterns(&patterns).await;

        info!(
            realm_id = %realm_id,
            role_id = %role_id,
            "Role policy cache invalidated"
        );

        Ok(())
    }

    /// Invalidate all cache for a realm
    ///
    /// Call this when a realm's RBAC configuration is initialized or updated
    /// This invalidates:
    /// - All user role caches for the realm
    /// - All principal role binding caches for the realm
    /// - All role policy caches for the realm
    /// - All permission check result caches for the realm
    async fn invalidate_realm_cache(&self, realm_id: &str) -> Result<(), CoreError> {
        let patterns = vec![
            CacheKey::user_roles_pattern(realm_id),
            CacheKey::principal_role_bindings_pattern(realm_id),
            CacheKey::role_policies_pattern(realm_id),
            CacheKey::permission_pattern(realm_id, None),
            CacheKey::principal_permission_pattern(realm_id, None, None),
        ];
        self.invalidate_patterns(&patterns).await;

        debug!(
            realm_id = %realm_id,
            "Realm cache invalidated"
        );

        Ok(())
    }

    /// Get user's effective permissions
    ///
    /// Returns all permissions a user has, including:
    /// - Permissions inherited from roles
    /// - Direct permissions assigned to the user
    ///
    /// Returns permission strings in dot form "resource.action"
    /// (e.g., "users.view", "roles.manage"), matching the canonical permission
    /// names stored in the database and consumed by the frontend.
    ///
    /// # Arguments
    /// * `realm_id` - Realm ID for multi-tenant isolation
    /// * `user_id` - User ID to get permissions for
    ///
    /// # Returns
    /// * `Ok(Vec<String>)` - List of unique permissions
    /// * `Err(CoreError)` if an error occurs
    async fn get_user_permissions(
        &self,
        realm_id: &str,
        user_id: &str,
    ) -> Result<Vec<String>, CoreError> {
        debug!(
            realm_id = %realm_id,
            user_id = %user_id,
            "Getting user permissions"
        );

        let user_uuid = uuid::Uuid::parse_str(user_id)
            .map_err(|_| CoreError::BadRequest(format!("Invalid user_id UUID: {}", user_id)))?;

        let mut permissions = Vec::new();

        // 1. Get user's roles from user_roles table
        let role_ids: Vec<uuid::Uuid> = user_roles::Entity::find()
            .filter(user_roles::Column::RealmId.eq(realm_id))
            .filter(user_roles::Column::UserId.eq(user_uuid))
            .all(&*self.db)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to fetch user roles");
                CoreError::DatabaseError(format!("Failed to fetch user roles: {}", e))
            })?
            .into_iter()
            .map(|r| r.role_id)
            .collect();

        // 2. Get permissions inherited from roles
        if !role_ids.is_empty() {
            let role_permissions = role_policies::Entity::find()
                .filter(role_policies::Column::RealmId.eq(realm_id))
                .filter(role_policies::Column::RoleId.is_in(role_ids))
                .all(&*self.db)
                .await
                .map_err(|e| {
                    error!(error = %e, "Failed to fetch role permissions");
                    CoreError::DatabaseError(format!("Failed to fetch role permissions: {}", e))
                })?;

            for policy in role_permissions {
                permissions.push(format!("{}.{}", policy.resource, policy.action));
            }
        }

        // 3. Get direct user permissions (stored with user_id as role_id)
        let direct_permissions = role_policies::Entity::find()
            .filter(role_policies::Column::RealmId.eq(realm_id))
            .filter(role_policies::Column::RoleId.eq(user_uuid))
            .all(&*self.db)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to fetch direct user permissions");
                CoreError::DatabaseError(format!("Failed to fetch direct user permissions: {}", e))
            })?;

        for policy in direct_permissions {
            permissions.push(format!("{}.{}", policy.resource, policy.action));
        }

        // 4. Remove duplicates while preserving order
        let mut seen = std::collections::HashSet::new();
        let unique_permissions: Vec<String> = permissions
            .into_iter()
            .filter(|p| seen.insert(p.clone()))
            .collect();

        debug!(
            count = unique_permissions.len(),
            "User permissions retrieved"
        );

        Ok(unique_permissions)
    }

    /// Check if a principal has permission to access a resource
    ///
    /// Generalized permission check for any principal type (user, api_key, client).
    ///
    /// Uses a two-tier caching strategy:
    /// - **Denial cache**: negative results (permission denied) are cached with a
    ///   short TTL to reduce DB load on repeated denied checks.
    /// - **Grant**: positive results are NOT cached as a top-level permission result.
    ///   Instead, they are derived from the cached role bindings and role policies,
    ///   which are invalidated atomically on role changes.  This eliminates the
    ///   TOCTOU race where a concurrent request re-caches a stale "granted" result
    ///   after cache invalidation.
    #[tracing::instrument(
        // Governance: realm_id / principal_id are tenant +
        // user identifiers (principal_id is commonly a user_id) — skipped.
        // resource/action are low-cardinality enums but skipped to stay
        // conservative and keep the span minimal; only db.system + cache.hit
        // (bool, low cardinality) are recorded.
        skip(self, realm_id, principal_type, principal_id, resource, action),
        fields(db.system = "redis")
    )]
    async fn check_principal_permission(
        &self,
        realm_id: &str,
        principal_type: &str,
        principal_id: &str,
        resource: &str,
        action: &str,
    ) -> Result<bool, CoreError> {
        debug!(
            realm_id = %realm_id,
            principal_type = %principal_type,
            principal_id = %principal_id,
            resource = %resource,
            action = %action,
            "Checking principal permission"
        );

        // 1. Check denial cache only (positive results are not cached at this level)
        let cache_key = CacheKey::principal_permission(
            realm_id,
            principal_type,
            principal_id,
            resource,
            action,
        );
        if let Some(cached) = self.get_cached_result(&cache_key).await {
            // Record only the low-cardinality bool hit/miss into the span.
            tracing::Span::current().record("cache.hit", true);
            if !cached {
                debug!("Principal permission denied (cached denial)");
                return Ok(false);
            }
            // Stale cached grant from a previous code version — treat as miss.
            // We do NOT return early for cached=true to avoid the TOCTOU race.
            debug!("Ignoring stale cached grant, re-evaluating from role bindings");
        }

        // 2. Query principal's roles (uses role bindings cache internally)
        let mut roles = self
            .get_principal_roles(realm_id, principal_type, principal_id)
            .await?;

        // Direct user permissions are stored as role_policies rows keyed by
        // the user's own uuid (role_id = user_id — the same convention
        // get_user_permissions reads). Include that key in the policy lookup
        // so a granted direct permission actually authorizes, instead of only
        // appearing in the effective-permissions display.
        if principal_type == principal_types::USER && !roles.iter().any(|r| r == principal_id) {
            roles.push(principal_id.to_string());
        }

        if roles.is_empty() {
            debug!("Principal has no roles, permission denied");
            self.cache_result(&cache_key, false, cache_ttl::DENIAL)
                .await;
            return Ok(false);
        }

        debug!(count = roles.len(), "Found roles for principal");

        // 3. Check role policies for permission match (uses role policies cache internally)
        let has_permission = self
            .check_roles_policies(&roles, realm_id, principal_id, resource, action)
            .await?;

        // 4. Only cache denials; grants are derived from role bindings + policies
        //    which have their own cache layers with proper invalidation.
        if !has_permission {
            self.cache_result(&cache_key, false, cache_ttl::DENIAL)
                .await;
        }

        Ok(has_permission)
    }

    /// Invalidate cached roles and permissions for any principal type.
    async fn invalidate_principal_role_cache(
        &self,
        realm_id: &str,
        principal_type: &str,
        principal_id: &str,
    ) -> Result<(), CoreError> {
        let role_bindings_key =
            CacheKey::principal_role_bindings(realm_id, principal_type, principal_id);

        // Direct DEL for the exact role bindings key — avoids SCAN unreliability.
        if let Err(e) = self.cache.write().await.delete(&role_bindings_key).await {
            warn!(error = %e, key = %role_bindings_key, "Failed to delete role bindings key");
        }

        let patterns = vec![
            role_bindings_key,
            CacheKey::principal_permission_pattern(
                realm_id,
                Some(principal_type),
                Some(principal_id),
            ),
        ];
        self.invalidate_patterns(&patterns).await;

        info!(
            realm_id = %realm_id,
            principal_type = %principal_type,
            principal_id = %principal_id,
            "Principal role cache invalidated"
        );

        Ok(())
    }
}

impl RedisPermissionChecker {
    /// Batch-delete cache entries matching multiple patterns under a single write lock.
    ///
    /// Acquires the write lock once, collects all matching keys, and deletes them
    /// in a single `DEL` call.  This avoids repeated lock acquisition when several
    /// patterns need invalidation (e.g. on role-policy change).
    async fn invalidate_patterns(&self, patterns: &[String]) {
        let cache = self.cache.write().await;
        let mut all_keys = Vec::new();
        for pattern in patterns {
            match cache.find_keys(pattern).await {
                Ok(keys) => all_keys.extend(keys),
                Err(e) => {
                    warn!(error = %e, pattern = %pattern, "Failed to find keys for pattern");
                }
            }
        }
        if !all_keys.is_empty()
            && let Err(e) = cache.delete_keys(&all_keys).await
        {
            warn!(error = %e, count = all_keys.len(), "Failed to delete batch keys");
        }
    }

    /// Check if any role grants the requested permission
    ///
    /// Iterates through all roles and their policies to find a matching permission.
    async fn check_roles_policies(
        &self,
        roles: &[String],
        realm_id: &str,
        user_id: &str,
        resource: &str,
        action: &str,
    ) -> Result<bool, CoreError> {
        for role_id in roles {
            let policies = self.get_role_policies(realm_id, role_id).await?;

            for policy in policies {
                if self.matches_policy(&policy, resource, action) {
                    debug!(
                        realm_id = %realm_id,
                        user_id = %user_id,
                        role_id = %role_id,
                        resource = %resource,
                        action = %action,
                        "Permission granted"
                    );
                    return Ok(true);
                }
            }
        }

        debug!("No matching policy found, permission denied");
        Ok(false)
    }

    /// Get all roles for a principal (with caching)
    ///
    /// Queries by (realm_id, principal_type, principal_id).
    async fn get_principal_roles(
        &self,
        realm_id: &str,
        principal_type: &str,
        principal_id: &str,
    ) -> Result<Vec<String>, CoreError> {
        let cache_key = CacheKey::principal_role_bindings(realm_id, principal_type, principal_id);

        // Return cached roles if available
        if let Some(cached) = self.get_cached::<Vec<String>>(&cache_key).await {
            debug!("Principal roles from cache");
            return Ok(cached);
        }

        // Query from database
        let roles = self
            .query_principal_roles_from_db(realm_id, principal_type, principal_id)
            .await?;

        // Cache for 5 minutes
        self.cache(&cache_key, &roles, cache_ttl::USER_ROLES).await;
        debug!(count = roles.len(), "Principal roles from database");

        Ok(roles)
    }

    /// Query principal roles from database
    ///
    /// Queries by (realm_id, principal_type, principal_id).
    async fn query_principal_roles_from_db(
        &self,
        realm_id: &str,
        principal_type: &str,
        principal_id: &str,
    ) -> Result<Vec<String>, CoreError> {
        Ok(user_roles::Entity::find()
            .filter(user_roles::Column::RealmId.eq(realm_id))
            .filter(user_roles::Column::PrincipalType.eq(principal_type))
            .filter(user_roles::Column::PrincipalId.eq(principal_id))
            .all(&*self.db)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to query principal roles");
                CoreError::DatabaseError(format!("Failed to query principal roles: {}", e))
            })?
            .into_iter()
            .map(|r| r.role_id.to_string())
            .collect::<Vec<_>>())
    }

    /// Check if a policy matches the requested resource and action
    ///
    /// # Matching Rules (with hierarchy)
    /// * Exact resource and action match → Permission granted
    /// * Hierarchical match: higher-level actions grant lower-level access
    ///   - `manage` grants access to: `view`, `manage`, `create`
    ///   - `view` grants access to: `view`
    /// * Resource must always match exactly (no wildcards)
    fn matches_policy(&self, policy: &Policy, resource: &str, action: &str) -> bool {
        // Resource must match exactly
        if policy.resource != resource {
            return false;
        }

        // Check action match (with hierarchy)
        let matches = self.action_matches_hierarchy(&policy.action, action);

        if matches {
            debug!(
                resource = %policy.resource,
                policy_action = %policy.action,
                requested_action = %action,
                "Matched policy (with hierarchy)"
            );
        }

        matches
    }

    /// Check if a granted action covers the requested action (with hierarchy)
    ///
    /// Permission hierarchy: manage > create > view
    fn action_matches_hierarchy(&self, granted_action: &str, requested_action: &str) -> bool {
        // Exact match always works
        if granted_action == requested_action {
            return true;
        }

        // Hierarchical checks
        match granted_action {
            "manage" => matches!(requested_action, "view" | "create"),
            "view" => false,
            _ => false,
        }
    }

    /// Get cached value with error handling (fallback to None on error)
    async fn get_cached<T>(&self, key: &str) -> Option<T>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        self.cache
            .read()
            .await
            .get(key)
            .await
            .map_err(|e| {
                warn!(error = %e, key = %key, "Cache read error, falling back to database");
            })
            .ok()
            .flatten()
    }

    /// Get cached permission check result
    async fn get_cached_result(&self, key: &str) -> Option<bool> {
        self.get_cached::<bool>(key).await
    }

    /// Cache a value with error handling
    async fn cache<T>(&self, key: &str, value: &T, ttl: u64)
    where
        T: serde::Serialize,
    {
        if let Err(e) = self.cache.write().await.set(key, value, ttl).await {
            warn!(error = %e, key = %key, "Cache write error, proceeding without cache");
        }
    }

    /// Cache permission check result
    async fn cache_result(&self, key: &str, value: bool, ttl: u64) {
        self.cache(key, &value, ttl).await;
    }
}

// Governance tests.
//
// Covers: `check_principal_permission` instrument skip correctness.
//
// WHY: `realm_id` / `principal_id` are tenant + user identifiers
// (principal_id is commonly a user_id). If the `#[instrument]` macro ever
// stops skipping them, the identifiers leak into a span field. Source-scan
// baseline, anchored to `fn check_principal_permission` and its
// immediately-preceding `#[tracing::instrument(...)]`.
#[cfg(test)]
mod instrument_skip_tests {
    const SRC: &str = include_str!("redis_permission_checker.rs");

    fn instrument_body_preceding(fn_name: &str) -> String {
        let needle = format!("fn {fn_name}");
        let fn_pos = SRC
            .find(&needle)
            .unwrap_or_else(|| panic!("fn {fn_name} not found in source"));
        let attr_start = SRC[..fn_pos]
            .rfind("#[tracing::instrument(")
            .unwrap_or_else(|| panic!("no #[tracing::instrument( preceding fn {fn_name}"));
        let body_start = attr_start + "#[tracing::instrument(".len();
        // Find the attribute close: the first line at/after body_start whose
        // trimmed content is exactly `)]`. This handles indented closes (e.g.
        // inside an `impl` block) and ignores inline `))]` sequences such as
        // `#[validate(length(...))]` that appear on struct fields.
        let tail = &SRC[body_start..];
        let mut consumed = 0usize;
        for line in tail.lines() {
            let prev = consumed;
            consumed += line.len() + 1; // +1 for the line separator
            if line.trim() == ")]" {
                return tail[..prev].to_string();
            }
        }
        panic!("unterminated #[tracing::instrument( for fn {fn_name}")
    }

    #[test]
    fn instrument_skip_check_principal_permission_excludes_realm_and_principal_id() {
        let body = instrument_body_preceding("check_principal_permission");
        for required in [
            "realm_id",
            "principal_id",
            "principal_type",
            "resource",
            "action",
        ] {
            assert!(
                body.contains(required),
                "check_principal_permission must skip `{required}`; body was:\n{body}"
            );
        }
        for banned in ["token", "password", "email", "secret", "user_id"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "check_principal_permission span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }
}
