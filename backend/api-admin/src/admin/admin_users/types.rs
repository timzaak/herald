use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

// Re-export API error response type for utoipa documentation
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;

// ==================== Request/Response Types ====================

#[derive(Debug, Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UserCreateRequest {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8, max = 100))]
    pub password: String,
    #[validate(length(max = 50))]
    pub nickname: Option<String>,
    #[validate(range(min = 0, max = 3))]
    pub status: Option<i16>,
    // PRD users.md §4.1: the create dialog may submit no role assignment; the
    // domain layer accepts an empty role set. A min=1 here would force every
    // admin-created user into a role.
    pub role_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Validate)]
pub struct UserUpdateRequest {
    #[validate(length(max = 50))]
    pub nickname: Option<String>,
    #[validate(range(min = 0, max = 3))]
    pub status: Option<i16>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
    pub id: Uuid,
    pub realm_id: String,
    pub email: String,
    pub nickname: Option<String>,
    pub status: i16,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserDetailResponse {
    pub id: Uuid,
    pub realm_id: String,
    pub email: String,
    pub nickname: Option<String>,
    pub status: i16,
    pub provider_ids: Vec<Uuid>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListUsersQuery {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
    pub email: Option<String>,
    // Range is enforced in the list handler: query DTOs here are not
    // extracted through Valid<Query<..>>, so validate attributes never run.
    pub status: Option<i16>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordResponse {
    pub new_password: String,
}

// ==================== User Role & Permission Types ====================

#[derive(Debug, Serialize, Deserialize, ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UserRoleDetail {
    pub id: Uuid,     // UUID
    pub name: String, // role name
    pub description: Option<String>,
    /// Grant origin: 'manual' (hand-assigned) or 'payment' (granted on payment success).
    pub source: String,
    /// Payment origin identifier (attempt_id / subscription_id). Null for manual grants.
    pub source_id: Option<String>,
    /// INFORMATIONAL provenance: the billing period end aligned at grant time. Not an authz TTL.
    pub expires_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserRolesResponse {
    pub roles: Vec<UserRoleDetail>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRolesRequest {
    #[validate(length(min = 1))]
    pub role_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserPermission {
    pub resource: String,
    pub action: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserPermissionsResponse {
    pub permissions: Vec<UserPermission>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Validate)]
pub struct AssignPermissionRequest {
    #[validate(length(min = 1))]
    pub resource: String,
    #[validate(length(min = 1))]
    pub action: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EffectivePermission {
    pub name: String,
    pub source: String,              // "role" or "direct"
    pub source_name: Option<String>, // role name if source is role
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EffectivePermissionsResponse {
    pub permissions: Vec<EffectivePermission>,
}

// ==================== User Session Types ====================

/// A single active session for a user, surfaced for Realm Admin session
/// management. Time fields are RFC3339 / ISO8601 strings. Fields derived from
/// the optional session metadata index are `null` for legacy families created
/// before the index existed.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserSessionResponse {
    pub family_id: Uuid,
    pub client_app_id: Uuid,
    pub client_app_name: Option<String>,
    pub credential_class: String,
    pub user_agent: Option<String>,
    pub client_ip: Option<String>,
    /// ISO8601 timestamp. `null` for legacy families without session metadata.
    pub created_at: Option<String>,
    /// ISO8601 timestamp. Always present (derived from the family record).
    pub absolute_expires_at: String,
}

/// Response payload for one-click revoke-all-sessions.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevokeAllSessionsResponse {
    pub revoked_count: i32,
}
