use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::realm_config::ConfigType;
use herald_domain::user_passkey::entities::UserPasskeyCredential;
use herald_domain::user_passkey::ports::{
    PasskeyRealmConfigReader, PasskeyRealmPolicy, UserPasskeyRepository, UserVerificationPolicy,
};
use herald_entity::user_passkey_credential;

pub struct PostgresUserPasskeyRepository {
    db: Arc<DatabaseConnection>,
}

impl PostgresUserPasskeyRepository {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }

    fn to_domain(
        model: user_passkey_credential::Model,
    ) -> Result<UserPasskeyCredential, CoreError> {
        let counter = u64::try_from(model.counter).map_err(|_| {
            CoreError::DatabaseError("passkey counter is negative in storage".to_string())
        })?;
        let transports = serde_json::from_value::<Vec<String>>(model.transports)?;

        Ok(UserPasskeyCredential {
            id: model.id,
            user_id: model.user_id,
            realm_id: model.realm_id,
            rp_id: model.rp_id,
            credential_id: model.credential_id,
            credential_public_key: model.credential_public_key,
            counter,
            transports,
            aaguid: model.aaguid,
            backup_eligible: model.backup_eligible,
            backup_state: model.backup_state,
            user_verified: model.user_verified,
            nickname: model.nickname,
            last_used_at: model.last_used_at.map(|dt| dt.into()),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        })
    }

    fn counter_to_i64(counter: u64) -> Result<i64, CoreError> {
        i64::try_from(counter).map_err(|_| {
            CoreError::DatabaseError("passkey counter exceeds BIGINT range".to_string())
        })
    }

    fn list_by_user_and_rp_query(
        realm_id: &str,
        user_id: Uuid,
        rp_id: &str,
    ) -> sea_orm::Select<user_passkey_credential::Entity> {
        user_passkey_credential::Entity::find()
            .filter(user_passkey_credential::Column::RealmId.eq(realm_id))
            .filter(user_passkey_credential::Column::UserId.eq(user_id))
            .filter(user_passkey_credential::Column::RpId.eq(rp_id))
    }

    fn find_by_credential_id_query(
        realm_id: &str,
        rp_id: &str,
        credential_id: &[u8],
    ) -> sea_orm::Select<user_passkey_credential::Entity> {
        user_passkey_credential::Entity::find()
            .filter(user_passkey_credential::Column::RealmId.eq(realm_id))
            .filter(user_passkey_credential::Column::RpId.eq(rp_id))
            .filter(user_passkey_credential::Column::CredentialId.eq(credential_id.to_vec()))
    }

    fn to_active_model(
        credential: UserPasskeyCredential,
    ) -> Result<user_passkey_credential::ActiveModel, CoreError> {
        Ok(user_passkey_credential::ActiveModel {
            id: Set(credential.id),
            user_id: Set(credential.user_id),
            realm_id: Set(credential.realm_id),
            rp_id: Set(credential.rp_id),
            credential_id: Set(credential.credential_id),
            credential_public_key: Set(credential.credential_public_key),
            counter: Set(Self::counter_to_i64(credential.counter)?),
            transports: Set(serde_json::to_value(credential.transports)?),
            aaguid: Set(credential.aaguid),
            backup_eligible: Set(credential.backup_eligible),
            backup_state: Set(credential.backup_state),
            user_verified: Set(credential.user_verified),
            nickname: Set(credential.nickname),
            last_used_at: Set(credential.last_used_at.map(|dt| dt.into())),
            created_at: Set(credential.created_at.into()),
            updated_at: Set(credential.updated_at.into()),
        })
    }
}

impl UserPasskeyRepository for PostgresUserPasskeyRepository {
    async fn list_by_user_and_rp(
        &self,
        realm_id: &str,
        user_id: Uuid,
        rp_id: &str,
    ) -> Result<Vec<UserPasskeyCredential>, CoreError> {
        let results = Self::list_by_user_and_rp_query(realm_id, user_id, rp_id)
            .all(&*self.db)
            .await?;

        results.into_iter().map(Self::to_domain).collect()
    }

    async fn has_any_for_user(&self, realm_id: &str, user_id: Uuid) -> Result<bool, CoreError> {
        let row = user_passkey_credential::Entity::find()
            .filter(user_passkey_credential::Column::RealmId.eq(realm_id))
            .filter(user_passkey_credential::Column::UserId.eq(user_id))
            .one(&*self.db)
            .await?;
        Ok(row.is_some())
    }

    async fn find_by_credential_id(
        &self,
        realm_id: &str,
        rp_id: &str,
        credential_id: &[u8],
    ) -> Result<Option<UserPasskeyCredential>, CoreError> {
        let result = Self::find_by_credential_id_query(realm_id, rp_id, credential_id)
            .one(&*self.db)
            .await?;

        result.map(Self::to_domain).transpose()
    }

    async fn insert(
        &self,
        credential: UserPasskeyCredential,
    ) -> Result<UserPasskeyCredential, CoreError> {
        let active_model = Self::to_active_model(credential)?;

        let result = active_model.insert(&*self.db).await?;
        Self::to_domain(result)
    }

    async fn rename(
        &self,
        realm_id: &str,
        user_id: Uuid,
        rp_id: &str,
        id: Uuid,
        nickname: &str,
    ) -> Result<(), CoreError> {
        let credential = user_passkey_credential::Entity::find_by_id(id)
            .filter(user_passkey_credential::Column::RealmId.eq(realm_id))
            .filter(user_passkey_credential::Column::UserId.eq(user_id))
            .filter(user_passkey_credential::Column::RpId.eq(rp_id))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        let mut active_model: user_passkey_credential::ActiveModel = credential.into();
        active_model.nickname = Set(Some(nickname.to_string()));
        active_model.updated_at = Set(chrono::Utc::now().into());
        active_model.update(&*self.db).await?;

        Ok(())
    }

    async fn delete(
        &self,
        realm_id: &str,
        user_id: Uuid,
        rp_id: &str,
        id: Uuid,
    ) -> Result<(), CoreError> {
        let _credential = user_passkey_credential::Entity::find_by_id(id)
            .filter(user_passkey_credential::Column::RealmId.eq(realm_id))
            .filter(user_passkey_credential::Column::UserId.eq(user_id))
            .filter(user_passkey_credential::Column::RpId.eq(rp_id))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        user_passkey_credential::Entity::delete_by_id(id)
            .exec(&*self.db)
            .await?;

        Ok(())
    }

    async fn update_counter_and_used(
        &self,
        id: Uuid,
        realm_id: &str,
        rp_id: &str,
        counter: u64,
        user_verified: bool,
        used_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), CoreError> {
        let credential = user_passkey_credential::Entity::find_by_id(id)
            .filter(user_passkey_credential::Column::RealmId.eq(realm_id))
            .filter(user_passkey_credential::Column::RpId.eq(rp_id))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        let mut active_model: user_passkey_credential::ActiveModel = credential.into();
        active_model.counter = Set(Self::counter_to_i64(counter)?);
        active_model.user_verified = Set(user_verified);
        active_model.last_used_at = Set(Some(used_at.into()));
        active_model.updated_at = Set(chrono::Utc::now().into());
        active_model.update(&*self.db).await?;

        Ok(())
    }
}

/// Reads the realm passkey policy from the `realm_config` table for the
/// `PasskeyRealmConfigReader` port. Used by `UserPasskeyService` to tailor the
/// ceremony (user-verification requirement, authenticator attachment) per realm.
pub struct PostgresPasskeyRealmConfigReader {
    pool: PgPool,
}

impl PostgresPasskeyRealmConfigReader {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PasskeyRealmConfigReader for PostgresPasskeyRealmConfigReader {
    async fn get_policy(&self, realm_id: &str) -> Result<PasskeyRealmPolicy, CoreError> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT config_value FROM realm_config
             WHERE realm_id = $1 AND config_type = $2 AND config_key = 'settings' AND enabled = true",
        )
        .bind(realm_id)
        .bind(ConfigType::Passkey.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            CoreError::DatabaseError(format!("Failed to query passkey realm config: {e}"))
        })?;

        let config =
            row.and_then(|(value,)| serde_json::from_str::<serde_json::Value>(&value).ok());

        Ok(PasskeyRealmPolicy {
            user_verification: config
                .as_ref()
                .and_then(|v| v.get("user_verification"))
                .and_then(|v| v.as_str())
                .map(UserVerificationPolicy::parse)
                .unwrap_or_default(),
            cross_platform_authenticator: config
                .as_ref()
                .and_then(|v| v.get("cross_platform_authenticator"))
                .and_then(|v| v.as_bool())
                // Default mirrors passkey_config.rs DEFAULT_CROSS_PLATFORM_AUTHENTICATOR.
                .unwrap_or(true),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use sea_orm::{ActiveValue, DatabaseBackend, QueryTrait};

    fn model(rp_id: &str) -> user_passkey_credential::Model {
        let now = Utc::now().into();
        user_passkey_credential::Model {
            id: Uuid::now_v7(),
            user_id: Uuid::now_v7(),
            realm_id: "realm-a".to_string(),
            rp_id: rp_id.to_string(),
            credential_id: vec![1, 2, 3],
            credential_public_key: vec![4, 5, 6],
            counter: 0,
            transports: serde_json::json!(["internal"]),
            aaguid: None,
            backup_eligible: false,
            backup_state: false,
            user_verified: true,
            nickname: None,
            last_used_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn passkey_rp_list_query_filters_by_rp_id() {
        let stored = model("app.example.com");
        let statement = PostgresUserPasskeyRepository::list_by_user_and_rp_query(
            &stored.realm_id,
            stored.user_id,
            &stored.rp_id,
        )
        .build(DatabaseBackend::Postgres);

        assert!(statement.sql.contains("\"rp_id\" = $3"));
        assert!(format!("{:?}", statement.values).contains("app.example.com"));
    }

    #[test]
    fn passkey_rp_insert_persists_rp_id() {
        let stored = model("app.example.com");
        let credential = PostgresUserPasskeyRepository::to_domain(stored).unwrap();

        let active = PostgresUserPasskeyRepository::to_active_model(credential).unwrap();

        assert_eq!(
            active.rp_id,
            ActiveValue::Set("app.example.com".to_string())
        );
    }

    #[test]
    fn passkey_rp_credential_lookup_filters_by_rp_id() {
        let statement = PostgresUserPasskeyRepository::find_by_credential_id_query(
            "realm-a",
            "app.example.com",
            &[1, 2, 3],
        )
        .build(DatabaseBackend::Postgres);

        assert!(statement.sql.contains("\"rp_id\" = $2"));
        assert!(format!("{:?}", statement.values).contains("app.example.com"));
    }
}
