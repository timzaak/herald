use herald_api_base::application::http::server::api_entities::ApiError;

const SENSITIVE_PERMISSIONS: &[&str] = &["realm.manage"];

/// Validates that sensitive permissions can only be created in the admin realm
///
/// # Arguments
/// * `permission_name` - The name of the permission being created
/// * `caller_realm_id` - The realm ID of the caller creating the permission
///
/// # Returns
/// * `Ok(())` if the permission can be created
/// * `Err(ApiError::Forbidden)` if the permission is sensitive and caller is not in admin realm
pub fn validate_sensitive_permission_creation(
    permission_name: &str,
    caller_realm_id: &str,
) -> Result<(), ApiError> {
    if SENSITIVE_PERMISSIONS.contains(&permission_name) && caller_realm_id != "admin" {
        return Err(ApiError::forbidden(format!(
            "Permission '{}' can only be created in admin realm",
            permission_name
        )));
    }
    Ok(())
}
