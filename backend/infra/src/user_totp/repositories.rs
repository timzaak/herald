use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, NotSet, PaginatorTrait,
    QueryFilter, Set,
};
use std::sync::Arc;
use uuid::Uuid;

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::user_totp::entities::{
    BackupCodeStats, RealmTotpConfig, RealmTotpStatistics, UserTotpBackupCode, UserTotpConfig,
};
use herald_domain::user_totp::ports::{RealmTotpConfigRepository, UserTotpRepository};
use herald_entity::{account, realm_config, user_totp_backup_codes, user_totp_config};

pub struct PostgresUserTotpRepository {
    db: Arc<DatabaseConnection>,
}

impl PostgresUserTotpRepository {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }

    fn to_domain_config(model: user_totp_config::Model) -> UserTotpConfig {
        UserTotpConfig {
            id: model.id,
            user_id: model.user_id,
            realm_id: model.realm_id.clone(),
            secret_hash: model.secret_hash.clone(),
            key_version: model.key_version.unwrap_or(1), // Default to 1 for existing records
            enabled: model.enabled,
            verified_at: model.verified_at.map(|dt| dt.into()),
            last_used_at: model.last_used_at.map(|dt| dt.into()),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        }
    }

    fn to_domain_backup_code(model: user_totp_backup_codes::Model) -> UserTotpBackupCode {
        UserTotpBackupCode {
            id: model.id,
            user_totp_config_id: model.user_totp_config_id,
            code_hash: model.code_hash.clone(),
            used: model.used,
            used_at: model.used_at.map(|dt| dt.into()),
            created_at: model.created_at.into(),
        }
    }
}

impl UserTotpRepository for PostgresUserTotpRepository {
    async fn create_config(&self, config: UserTotpConfig) -> Result<UserTotpConfig, CoreError> {
        let active_model = user_totp_config::ActiveModel {
            id: Set(config.id),
            user_id: Set(config.user_id),
            realm_id: Set(config.realm_id),
            secret_hash: Set(config.secret_hash),
            key_version: Set(Some(config.key_version)),
            enabled: Set(config.enabled),
            verified_at: Set(config.verified_at.map(|dt| dt.into())),
            last_used_at: Set(config.last_used_at.map(|dt| dt.into())),
            created_at: Set(config.created_at.into()),
            updated_at: Set(config.updated_at.into()),
        };

        let result = active_model.insert(&*self.db).await?;
        Ok(Self::to_domain_config(result))
    }

    async fn get_config_by_user_id(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserTotpConfig>, CoreError> {
        let result = user_totp_config::Entity::find()
            .filter(user_totp_config::Column::UserId.eq(user_id))
            .one(&*self.db)
            .await?;

        Ok(result.map(Self::to_domain_config))
    }

    async fn get_config_by_id(&self, config_id: Uuid) -> Result<UserTotpConfig, CoreError> {
        let result = user_totp_config::Entity::find_by_id(config_id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        Ok(Self::to_domain_config(result))
    }

    async fn update_config(&self, config: UserTotpConfig) -> Result<UserTotpConfig, CoreError> {
        let active_model = user_totp_config::ActiveModel {
            id: Set(config.id),
            user_id: Set(config.user_id),
            realm_id: Set(config.realm_id),
            secret_hash: Set(config.secret_hash),
            key_version: Set(Some(config.key_version)),
            enabled: Set(config.enabled),
            verified_at: Set(config.verified_at.map(|dt| dt.into())),
            last_used_at: Set(config.last_used_at.map(|dt| dt.into())),
            created_at: Set(config.created_at.into()),
            updated_at: Set(config.updated_at.into()),
        };

        let result = active_model.update(&*self.db).await?;
        Ok(Self::to_domain_config(result))
    }

    async fn delete_config(&self, user_id: Uuid) -> Result<(), CoreError> {
        let config = user_totp_config::Entity::find()
            .filter(user_totp_config::Column::UserId.eq(user_id))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        // Explicitly delete backup codes first
        user_totp_backup_codes::Entity::delete_many()
            .filter(user_totp_backup_codes::Column::UserTotpConfigId.eq(config.id))
            .exec(&*self.db)
            .await?;

        // Then delete the config
        user_totp_config::Entity::delete_by_id(config.id)
            .exec(&*self.db)
            .await?;

        Ok(())
    }

    async fn create_backup_codes(
        &self,
        codes: Vec<UserTotpBackupCode>,
    ) -> Result<Vec<UserTotpBackupCode>, CoreError> {
        let mut result_codes = Vec::new();

        for code in codes {
            // Let the database auto-generate the ID via bigserial
            let active_model = user_totp_backup_codes::ActiveModel {
                id: NotSet,
                user_totp_config_id: Set(code.user_totp_config_id),
                code_hash: Set(code.code_hash),
                used: Set(code.used),
                used_at: Set(code.used_at.map(|dt| dt.into())),
                created_at: Set(code.created_at.into()),
            };

            let result = active_model.insert(&*self.db).await?;
            result_codes.push(Self::to_domain_backup_code(result));
        }

        Ok(result_codes)
    }

    async fn get_backup_codes(
        &self,
        config_id: Uuid,
    ) -> Result<Vec<UserTotpBackupCode>, CoreError> {
        let results = user_totp_backup_codes::Entity::find()
            .filter(user_totp_backup_codes::Column::UserTotpConfigId.eq(config_id))
            .all(&*self.db)
            .await?;

        Ok(results
            .into_iter()
            .map(Self::to_domain_backup_code)
            .collect())
    }

    async fn find_unused_backup_code(
        &self,
        config_id: Uuid,
        code_hash: &str,
    ) -> Result<Option<UserTotpBackupCode>, CoreError> {
        let result = user_totp_backup_codes::Entity::find()
            .filter(user_totp_backup_codes::Column::UserTotpConfigId.eq(config_id))
            .filter(user_totp_backup_codes::Column::CodeHash.eq(code_hash))
            .filter(user_totp_backup_codes::Column::Used.eq(false))
            .one(&*self.db)
            .await?;

        Ok(result.map(Self::to_domain_backup_code))
    }

    async fn mark_backup_code_used(&self, code_id: i64) -> Result<UserTotpBackupCode, CoreError> {
        let code_model = user_totp_backup_codes::Entity::find_by_id(code_id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        let mut active_model: user_totp_backup_codes::ActiveModel = code_model.into();
        active_model.used = Set(true);
        active_model.used_at = Set(Some(chrono::Utc::now().into()));

        let result = active_model.update(&*self.db).await?;
        Ok(Self::to_domain_backup_code(result))
    }

    async fn delete_backup_codes(&self, config_id: Uuid) -> Result<(), CoreError> {
        user_totp_backup_codes::Entity::delete_many()
            .filter(user_totp_backup_codes::Column::UserTotpConfigId.eq(config_id))
            .exec(&*self.db)
            .await?;

        Ok(())
    }

    async fn get_backup_code_stats(&self, config_id: Uuid) -> Result<BackupCodeStats, CoreError> {
        let codes = user_totp_backup_codes::Entity::find()
            .filter(user_totp_backup_codes::Column::UserTotpConfigId.eq(config_id))
            .all(&*self.db)
            .await?;

        let total = codes.len() as i32;
        let used = codes.iter().filter(|c| c.used).count() as i32;
        let remaining = total - used;

        Ok(BackupCodeStats {
            total,
            remaining,
            used,
        })
    }
}

pub struct PostgresRealmTotpConfigRepository {
    db: Arc<DatabaseConnection>,
}

impl PostgresRealmTotpConfigRepository {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }

    fn parse_realm_config(config: realm_config::Model) -> Option<RealmTotpConfig> {
        if config.config_type != "totp" {
            return None;
        }

        // Parse config_value as JSON object containing enabled and force_enabled
        let parsed: serde_json::Value = serde_json::from_str(&config.config_value).ok()?;

        let (enabled, force_enabled) = match &parsed {
            serde_json::Value::Object(obj) => (
                obj.get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                obj.get("force_enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            ),
            _ => (false, false),
        };

        Some(RealmTotpConfig {
            enabled,
            force_enabled,
        })
    }
}

impl RealmTotpConfigRepository for PostgresRealmTotpConfigRepository {
    async fn get_realm_totp_config(
        &self,
        realm_id: &str,
    ) -> Result<Option<RealmTotpConfig>, CoreError> {
        let config = realm_config::Entity::find()
            .filter(realm_config::Column::RealmId.eq(realm_id))
            .filter(realm_config::Column::ConfigType.eq("totp"))
            .filter(realm_config::Column::ConfigKey.eq("settings"))
            .one(&*self.db)
            .await?;

        Ok(config.and_then(Self::parse_realm_config))
    }

    async fn upsert_realm_totp_config(
        &self,
        realm_id: &str,
        config: RealmTotpConfig,
    ) -> Result<RealmTotpConfig, CoreError> {
        let config_value = serde_json::json!({
            "enabled": config.enabled,
            "force_enabled": config.force_enabled,
        })
        .to_string();

        // Check if config exists
        let existing = realm_config::Entity::find()
            .filter(realm_config::Column::RealmId.eq(realm_id))
            .filter(realm_config::Column::ConfigType.eq("totp"))
            .filter(realm_config::Column::ConfigKey.eq("settings"))
            .one(&*self.db)
            .await?;

        if let Some(existing_config) = existing {
            // Update existing
            let mut active_model: realm_config::ActiveModel = existing_config.into();
            active_model.enabled = Set(config.enabled);
            active_model.config_value = Set(config_value);
            active_model.updated_at = Set(chrono::Utc::now().into());

            let result = active_model.update(&*self.db).await?;
            Self::parse_realm_config(result).ok_or_else(|| {
                tracing::error!("Failed to parse realm config after update");
                CoreError::InternalServerError("Failed to parse realm config".to_string())
            })
        } else {
            // Create new
            let now = chrono::Utc::now();
            let active_model = realm_config::ActiveModel {
                id: Set(herald_domain::common::generate_uuid_v7()),
                realm_id: Set(realm_id.to_string()),
                config_type: Set("totp".to_string()),
                config_key: Set("settings".to_string()),
                config_value: Set(config_value),
                is_secret: Set(false),
                enabled: Set(config.enabled),
                metadata: Set(serde_json::json!({})),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
            };

            let result = active_model.insert(&*self.db).await?;
            Self::parse_realm_config(result).ok_or_else(|| {
                tracing::error!("Failed to parse realm config after insert");
                CoreError::InternalServerError("Failed to parse realm config".to_string())
            })
        }
    }

    async fn get_realm_totp_statistics(
        &self,
        realm_id: &str,
    ) -> Result<RealmTotpStatistics, CoreError> {
        // Get total users in realm
        let total_users = account::Entity::find()
            .filter(account::Column::RealmId.eq(realm_id))
            .count(&*self.db)
            .await? as i64;

        // Get users with TOTP enabled
        let totp_enabled_users = user_totp_config::Entity::find()
            .filter(user_totp_config::Column::RealmId.eq(realm_id))
            .filter(user_totp_config::Column::Enabled.eq(true))
            .count(&*self.db)
            .await? as i64;

        let totp_disabled_users = total_users - totp_enabled_users;
        let enablement_rate = if total_users > 0 {
            totp_enabled_users as f64 / total_users as f64
        } else {
            0.0
        };

        Ok(RealmTotpStatistics {
            total_users,
            totp_enabled_users,
            totp_disabled_users,
            enablement_rate,
        })
    }
}
