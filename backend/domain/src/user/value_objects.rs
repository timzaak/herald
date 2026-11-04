use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::user::entities::{Profile, User};

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct CreateUserRequest {
    #[validate(length(min = 1))]
    pub realm_id: String,
    #[validate(email)]
    pub email: String,
    pub password: Option<String>,
    #[serde(default)]
    pub provider_ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct UpdateUserRequest {
    pub status: Option<i16>,
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct LoginRequest {
    #[validate(length(min = 1))]
    pub realm_id: String,
    pub email: Option<String>,    // Optional email login
    pub username: Option<String>, // Optional username login
    /// Server-derived E.164 phone parsed from `username` when the realm
    /// phone-login switch is on; `username` stays as fallback so pure-numeric
    /// usernames still match exactly.
    #[serde(default)]
    pub phone: Option<String>,
    #[validate(length(min = 1))]
    pub password: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct RegisterRequest {
    #[validate(length(min = 1))]
    pub realm_id: String,
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8))]
    pub password: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct UserResponse {
    pub id: Uuid,
    pub realm_id: String,
    pub email: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            realm_id: user.realm_id,
            email: user.email,
            status: format!("{:?}", user.status),
            created_at: user.created_at.to_rfc3339(),
            updated_at: user.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ProfileResponse {
    pub id: Uuid,
    pub realm_id: String,
    pub nickname: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Profile> for ProfileResponse {
    fn from(profile: Profile) -> Self {
        Self {
            id: profile.id,
            realm_id: profile.realm_id,
            nickname: profile.nickname,
            created_at: profile.created_at.to_rfc3339(),
            updated_at: profile.updated_at.to_rfc3339(),
        }
    }
}
