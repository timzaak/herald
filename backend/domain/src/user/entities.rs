use crate::common::entities::Entity;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct User {
    pub id: Uuid,
    pub realm_id: String,
    pub email: String,
    pub nickname: Option<String>,
    pub password_hash: Option<String>,
    pub provider_ids: Vec<Uuid>,
    pub status: UserStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Entity for User {
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

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    WaitVerified = 0,
    Normal = 1,
    Forbidden = 2,
    Deleted = 3,
}

impl From<i16> for UserStatus {
    fn from(value: i16) -> Self {
        match value {
            0 => UserStatus::WaitVerified,
            1 => UserStatus::Normal,
            2 => UserStatus::Forbidden,
            3 => UserStatus::Deleted,
            // Unknown / legacy values collapse to Forbidden (inactive, login
            // blocked) rather than a distinct state.
            _ => UserStatus::Forbidden,
        }
    }
}

impl UserStatus {
    /// Blocks authentication while allowing pending users to verify their email.
    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::Forbidden | Self::Deleted)
    }
}

impl From<UserStatus> for i16 {
    fn from(status: UserStatus) -> Self {
        status as i16
    }
}

impl User {
    pub fn new(config: CreateUserConfig) -> Self {
        let now = Utc::now();
        Self {
            id: crate::common::entities::generate_uuid_v7(),
            realm_id: config.realm_id,
            email: config.email,
            nickname: config.nickname,
            password_hash: config.password_hash,
            provider_ids: config.provider_ids.unwrap_or_default(),
            status: UserStatus::Normal,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == UserStatus::Normal
    }

    pub fn verify(&mut self) {
        self.status = UserStatus::Normal;
        self.updated_at = Utc::now();
    }

    pub fn verify_password(&self, password: &str) -> Result<bool, bcrypt::BcryptError> {
        match &self.password_hash {
            Some(hash) => bcrypt::verify(password, hash),
            None => Ok(false),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct Profile {
    pub id: Uuid,
    pub realm_id: String,
    pub nickname: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Entity for Profile {
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

impl Profile {
    pub fn new(user_id: Uuid, realm_id: String, nickname: Option<String>) -> Self {
        let now = Utc::now();
        Self {
            id: user_id,
            realm_id,
            nickname,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateUserConfig {
    pub realm_id: String,
    pub email: String,
    pub nickname: Option<String>,
    pub password_hash: Option<String>,
    pub provider_ids: Option<Vec<Uuid>>,
}

#[cfg(test)]
mod tests {
    use super::UserStatus;

    #[test]
    fn disabled_status_blocks_access_without_blocking_email_verification() {
        assert!(!UserStatus::WaitVerified.is_disabled());
        assert!(!UserStatus::Normal.is_disabled());
        assert!(UserStatus::Forbidden.is_disabled());
        assert!(UserStatus::Deleted.is_disabled());
        assert!(UserStatus::from(-1).is_disabled());
    }
}
