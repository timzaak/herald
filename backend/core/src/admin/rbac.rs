use sqlx::{PgPool, Row};
use std::sync::Arc;
use tracing::info;

use crate::domain::rbac_init::RealmInitializationService;

/// Initialize default RBAC for the admin realm
///
/// This function is idempotent - it can be safely called multiple times.
/// It uses RealmInitializationService to atomically create roles and permissions.
pub async fn init_admin_realm_rbac<R>(
    pool: &PgPool,
    rbac_init_service: Arc<R>,
) -> anyhow::Result<()>
where
    R: RealmInitializationService,
{
    // 1. Get admin realm and client_app info
    let (realm_id, client_id) = get_admin_root_id(pool).await?;

    // 2. Check if permissions already exist
    let permission_count: i64 =
        sqlx::query("SELECT COUNT(*) FROM permissions WHERE realm_id = $1 AND is_builtin = true")
            .bind(&realm_id)
            .fetch_one(pool)
            .await?
            .get("count");

    // Expected: 24 admin-realm built-in permissions. RealmInitializationService
    // creates 23 realm-admin permissions plus admin-only realm.manage; the user
    // role reuses points.view and does not create another permission row.
    // All permissions are created by RealmInitializationService with is_builtin=true
    const EXPECTED_BUILTIN_PERMISSIONS: i64 = 24;

    if permission_count >= EXPECTED_BUILTIN_PERMISSIONS {
        info!(
            "Admin realm RBAC already initialized ({} permissions found)",
            permission_count
        );
        return Ok(());
    }

    // 3. Initialize RBAC using RealmInitializationService
    // This atomically creates realm-admin and user roles with their permissions
    info!(
        "Initializing admin realm RBAC (current: {} permissions, expected: {})",
        permission_count, EXPECTED_BUILTIN_PERMISSIONS
    );

    rbac_init_service
        .init_default_rbac(crate::domain::rbac_init::RealmRBACInitRequest {
            realm_id,
            admin_web_console_client_id: client_id, // Use client_app.client_id (string identifier)
        })
        .await?;

    info!("Admin realm RBAC initialized successfully");
    Ok(())
}

/// Query admin realm ID and client_app.client_id from database
async fn get_admin_root_id(pg: &PgPool) -> anyhow::Result<(String, String)> {
    let realm_id: String = sqlx::query("SELECT id FROM realm LIMIT 1")
        .fetch_one(pg)
        .await
        .map(|x| x.get("id"))?;
    // Use client_id (string identifier) instead of id (UUID)
    // roles.client_id stores the client identifier string (e.g., 'admin-web-console')
    let client_id: String = sqlx::query("SELECT client_id FROM client_app LIMIT 1")
        .fetch_one(pg)
        .await
        .map(|x| x.get("client_id"))?;
    Ok((realm_id, client_id))
}
