use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, TransactionTrait,
};
use sha2::Digest;
use std::sync::Arc;
use uuid::Uuid;

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::user::{
    entities::{Profile, User, UserStatus},
    ports::{UserRepository, UserVerificationRepository},
    value_objects::{CreateUserRequest, UpdateUserRequest},
};
use herald_entity::{
    account, profile, user_passkey_credential, user_totp_backup_codes, user_totp_config,
};

pub struct PostgresUserRepository {
    db: Arc<DatabaseConnection>,
}

impl PostgresUserRepository {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }

    fn to_domain_user(model: &account::Model, nickname: Option<String>) -> User {
        // Validate the ID is a proper UUID
        let id_str = model.id.to_string();
        if id_str.len() != 36 {
            tracing::error!(
                id = %id_str,
                length = id_str.len(),
                "Invalid UUID length in to_domain_user conversion"
            );
        }

        // Validate the UUID format
        if uuid::Uuid::parse_str(&id_str).is_err() {
            tracing::error!(
                id = %id_str,
                "Invalid UUID format in to_domain_user conversion"
            );
        }

        User {
            id: model.id,
            realm_id: model.realm_id.clone().unwrap_or_default(),
            email: model.email.clone(),
            nickname,
            password_hash: model.password.clone(),
            provider_ids: model.provider_ids.clone(),
            status: UserStatus::from(model.status),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        }
    }

    fn to_domain_profile(model: &profile::Model) -> Profile {
        Profile {
            id: model.id,
            realm_id: model.realm_id.clone().unwrap_or_default(),
            nickname: model.nickname.clone(),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        }
    }
}

impl UserRepository for PostgresUserRepository {
    async fn create_user(
        &self,
        request: CreateUserRequest,
        password_hash: Option<String>,
    ) -> Result<User, CoreError> {
        let now = chrono::Utc::now();
        // 使用 UUID v7 生成用户 ID
        let user_id = herald_domain::common::entities::generate_uuid_v7();

        let active_model = account::ActiveModel {
            id: sea_orm::Set(user_id),
            realm_id: sea_orm::Set(Some(request.realm_id)),
            email: sea_orm::Set(request.email),
            username: sea_orm::Set(None), // Explicitly set username to None
            password: sea_orm::Set(password_hash),
            provider_ids: sea_orm::Set(request.provider_ids.unwrap_or_default()),
            status: sea_orm::Set(UserStatus::WaitVerified.into()),
            deleted_original_email_hash: sea_orm::Set(None),
            created_at: sea_orm::Set(now.into()),
            updated_at: sea_orm::Set(now.into()),
        };

        let result = active_model.insert(&*self.db).await?;
        Ok(Self::to_domain_user(&result, None))
    }

    async fn get_user_by_id(&self, id: Uuid) -> Result<User, CoreError> {
        tracing::debug!("Querying user by ID: {}", id);

        let result = account::Entity::find_by_id(id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        // Verify the ID matches
        tracing::debug!("Found user: id={}, email={}", result.id, result.email);

        if result.id != id {
            tracing::error!(
                query_id = %id,
                result_id = %result.id,
                "User ID mismatch in database query result"
            );
            return Err(CoreError::NotFound);
        }

        Ok(Self::to_domain_user(&result, None))
    }

    async fn get_user_by_email(&self, realm_id: &str, email: &str) -> Result<User, CoreError> {
        let result = account::Entity::find()
            .filter(account::Column::RealmId.eq(realm_id))
            .filter(account::Column::Email.eq(email))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        Ok(Self::to_domain_user(&result, None))
    }

    async fn get_user_by_email_or_username(
        &self,
        realm_id: &str,
        email: Option<String>,
        username: Option<String>,
    ) -> Result<Option<(Uuid, Option<String>, i16)>, CoreError> {
        let mut query = account::Entity::find().filter(account::Column::RealmId.eq(realm_id));

        // Add email or username filter
        if let Some(email) = email {
            query = query.filter(account::Column::Email.eq(email));
        } else if let Some(username) = username {
            query = query.filter(account::Column::Username.eq(username));
        } else {
            // Neither email nor username provided
            return Ok(None);
        }

        let result = query.one(&*self.db).await?;

        Ok(result.map(|model| (model.id, model.password, model.status)))
    }

    async fn find_deleted_user_by_email_hash(
        &self,
        realm_id: &str,
        email_hash: &str,
    ) -> Result<Option<(Uuid, i16)>, CoreError> {
        let result = account::Entity::find()
            .filter(account::Column::RealmId.eq(realm_id))
            .filter(account::Column::DeletedOriginalEmailHash.eq(email_hash))
            .filter(account::Column::Status.eq(i16::from(UserStatus::Deleted)))
            .one(&*self.db)
            .await?;

        Ok(result.map(|model| (model.id, model.status)))
    }

    async fn change_password(
        &self,
        realm_id: &str,
        user_id: Uuid,
        new_password_hash: String,
    ) -> Result<(), CoreError> {
        let mut active_model: account::ActiveModel = account::Entity::find()
            .filter(account::Column::RealmId.eq(realm_id))
            .filter(account::Column::Id.eq(user_id))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?
            .into();

        active_model.password = sea_orm::Set(Some(new_password_hash));
        active_model.updated_at = sea_orm::Set(chrono::Utc::now().into());

        active_model.update(&*self.db).await?;
        Ok(())
    }

    async fn update_user_status(&self, user_id: Uuid, status: i16) -> Result<(), CoreError> {
        let mut active_model: account::ActiveModel = account::Entity::find_by_id(user_id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?
            .into();

        active_model.status = sea_orm::Set(status);
        active_model.updated_at = sea_orm::Set(chrono::Utc::now().into());

        active_model.update(&*self.db).await?;
        Ok(())
    }

    async fn list_users(
        &self,
        realm_id: &str,
        page: u64,
        page_size: u64,
        email: Option<String>,
        status: Option<i16>,
    ) -> Result<(Vec<User>, i64), CoreError> {
        let page = page.max(1);
        let page_size = page_size.min(100);
        let offset = (page - 1) * page_size;

        let mut query = account::Entity::find().filter(account::Column::RealmId.eq(realm_id));

        // Add email filter if provided
        if let Some(email_filter) = email {
            query = query.filter(account::Column::Email.contains(email_filter));
        }
        if let Some(status) = status {
            query = query.filter(account::Column::Status.eq(status));
        }

        let total = query.clone().count(&*self.db).await?;

        let results = query
            .order_by_desc(account::Column::CreatedAt)
            .limit(page_size)
            .offset(offset)
            .all(&*self.db)
            .await?;

        let users = results
            .iter()
            .map(|model| Self::to_domain_user(model, None))
            .collect();
        Ok((users, total as i64))
    }

    async fn update_user(&self, id: Uuid, request: UpdateUserRequest) -> Result<User, CoreError> {
        let mut active_model: account::ActiveModel = account::Entity::find_by_id(id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?
            .into();

        if let Some(status) = request.status {
            active_model.status = sea_orm::Set(status);
        }

        active_model.updated_at = sea_orm::Set(chrono::Utc::now().into());

        let result = active_model.update(&*self.db).await?;
        Ok(Self::to_domain_user(&result, None))
    }

    async fn delete_user(&self, id: Uuid) -> Result<(), CoreError> {
        account::Entity::delete_by_id(id).exec(&*self.db).await?;

        Ok(())
    }

    async fn create_profile(&self, profile: Profile) -> Result<Profile, CoreError> {
        let active_model = profile::ActiveModel {
            id: sea_orm::Set(profile.id),
            realm_id: sea_orm::Set(Some(profile.realm_id.clone())),
            nickname: sea_orm::Set(profile.nickname.clone()),
            created_at: sea_orm::Set(profile.created_at.into()),
            updated_at: sea_orm::Set(profile.updated_at.into()),
        };

        active_model.insert(&*self.db).await?;
        Ok(profile)
    }

    async fn get_profile(&self, user_id: Uuid) -> Result<Profile, CoreError> {
        let result = profile::Entity::find_by_id(user_id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        Ok(Self::to_domain_profile(&result))
    }

    async fn update_profile(
        &self,
        user_id: Uuid,
        nickname: Option<Option<String>>,
    ) -> Result<Profile, CoreError> {
        let mut active_model: profile::ActiveModel = profile::Entity::find_by_id(user_id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?
            .into();

        if let Some(nickname) = nickname {
            active_model.nickname = sea_orm::Set(nickname);
        }

        active_model.updated_at = sea_orm::Set(chrono::Utc::now().into());

        let result = active_model.update(&*self.db).await?;
        Ok(Self::to_domain_profile(&result))
    }

    async fn anonymize_user_for_deletion(
        &self,
        realm_id: &str,
        user_id: Uuid,
    ) -> Result<(), CoreError> {
        // Single SeaORM transaction. The anonymized email is derived from the
        // account id so it stays unique within `(realm_id, email)`
        // (account_realm_id_email_index). `profile.nickname` and the TOTP
        // config (+ backup codes) are wiped in the same transaction so the
        // PII/credential purge is atomic.
        let txn = self.db.begin().await?;
        let now = chrono::Utc::now();

        // 1. account: status=Deleted, email=anonymized, password/username=NULL,
        //    provider_ids='{}'. Scope by (realm_id, id) to honor the realm
        //    boundary.
        let account_model = account::Entity::find()
            .filter(account::Column::RealmId.eq(realm_id))
            .filter(account::Column::Id.eq(user_id))
            .one(&txn)
            .await?
            .ok_or(CoreError::NotFound)?;
        let original_email = account_model.email.clone();
        let mut acc_model: account::ActiveModel = account_model.into();
        let mut hasher = sha2::Sha256::new();
        hasher.update(original_email.as_bytes());
        let email_hash = format!("{:x}", hasher.finalize());
        let anonymized_email = format!("deleted+{}@anonymized.local", user_id);
        acc_model.status = Set(i16::from(UserStatus::Deleted));
        acc_model.email = Set(anonymized_email);
        acc_model.password = Set(None);
        acc_model.username = Set(None);
        acc_model.provider_ids = Set(Vec::new());
        acc_model.deleted_original_email_hash = Set(Some(email_hash));
        acc_model.updated_at = Set(now.into());
        acc_model.update(&txn).await?;

        // 2. profile.nickname = NULL (optional row; 0 rows affected is fine).
        //    Use update_many so a missing profile row is not an error.
        profile::Entity::update_many()
            .col_expr(
                profile::Column::Nickname,
                sea_orm::sea_query::Expr::value(None::<String>),
            )
            .col_expr(
                profile::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(sea_orm::prelude::DateTimeWithTimeZone::from(now)),
            )
            .filter(profile::Column::Id.eq(user_id))
            .exec(&txn)
            .await?;

        // 3. TOTP config + backup codes wipe (mirrors the standalone
        //    `delete_config` pattern, but on this transaction). First collect
        //    the config ids owned by this user, delete their backup codes, then
        //    delete the configs.
        let config_ids: Vec<Uuid> = user_totp_config::Entity::find()
            .filter(user_totp_config::Column::UserId.eq(user_id))
            .all(&txn)
            .await?
            .into_iter()
            .map(|c| c.id)
            .collect();
        if !config_ids.is_empty() {
            user_totp_backup_codes::Entity::delete_many()
                .filter(user_totp_backup_codes::Column::UserTotpConfigId.is_in(config_ids))
                .exec(&txn)
                .await?;
            user_totp_config::Entity::delete_many()
                .filter(user_totp_config::Column::UserId.eq(user_id))
                .exec(&txn)
                .await?;
        }

        // 4. Passkey credentials are authentication secrets too. Soft-deleting
        //    the account does not trigger an FK cascade, so remove them in the
        //    same transaction as the other authenticators.
        user_passkey_credential::Entity::delete_many()
            .filter(user_passkey_credential::Column::UserId.eq(user_id))
            .filter(user_passkey_credential::Column::RealmId.eq(realm_id))
            .exec(&txn)
            .await?;

        // 5. OAuth provider binding wipe. The `provider` table holds the
        //    user's third-party identity bindings (open_id / union_id / email).
        //    Because account deletion is a soft delete (status=Deleted, the
        //    account row is NOT removed), the `ON DELETE CASCADE` foreign key
        //    never fires — so provider rows must be removed explicitly here,
        //    otherwise the deleted user's PII survives and the OAuth identity
        //    can still be resolved on a future login attempt.
        use sea_orm::ConnectionTrait;
        let delete_stmt = sea_orm::sea_query::Query::delete()
            .from_table(sea_orm::sea_query::Alias::new("provider"))
            .cond_where(
                sea_orm::sea_query::Expr::col(sea_orm::sea_query::Alias::new("user_id"))
                    .eq(user_id),
            )
            .to_owned();
        let backend = txn.get_database_backend();
        txn.execute(backend.build(&delete_stmt)).await?;

        txn.commit().await?;
        Ok(())
    }
}

pub struct PostgresVerificationRepository {
    db: Arc<DatabaseConnection>,
}

impl PostgresVerificationRepository {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

impl UserVerificationRepository for PostgresVerificationRepository {
    async fn create_verification_code(
        &self,
        realm_id: &str,
        email: &str,
        code_type: &str,
        code: &str,
    ) -> Result<(), CoreError> {
        use herald_entity::email_verification_code;

        let now = chrono::Utc::now();
        let id = herald_domain::common::entities::generate_uuid_v7();

        // Newest code wins: every consumer reads the latest row for an
        // (email, type), so older unconsumed codes are dead weight that only
        // widens the window in which a leaked older email stays valid.
        email_verification_code::Entity::delete_many()
            .filter(email_verification_code::Column::RealmId.eq(realm_id))
            .filter(email_verification_code::Column::Email.eq(email))
            .filter(email_verification_code::Column::Type.eq(code_type))
            .exec(&*self.db)
            .await?;

        let active_model = email_verification_code::ActiveModel {
            id: sea_orm::Set(id),
            realm_id: sea_orm::Set(realm_id.to_string()),
            email: sea_orm::Set(email.to_string()),
            r#type: sea_orm::Set(code_type.to_string()),
            verification_code: sea_orm::Set(code.to_string()),
            created_at: sea_orm::Set(now.into()),
        };

        active_model.insert(&*self.db).await?;
        Ok(())
    }

    async fn verify_code(
        &self,
        realm_id: &str,
        email: &str,
        code_type: &str,
        code: &str,
    ) -> Result<bool, CoreError> {
        use herald_domain::security_constants::EMAIL_VERIFICATION_CODE_TTL_SECONDS;
        use herald_entity::email_verification_code;

        let cutoff = (chrono::Utc::now()
            - chrono::Duration::seconds(EMAIL_VERIFICATION_CODE_TTL_SECONDS as i64))
        .fixed_offset();
        let result = email_verification_code::Entity::find()
            .filter(email_verification_code::Column::RealmId.eq(realm_id))
            .filter(email_verification_code::Column::Email.eq(email))
            .filter(email_verification_code::Column::Type.eq(code_type))
            .filter(email_verification_code::Column::VerificationCode.eq(code))
            .filter(email_verification_code::Column::CreatedAt.gte(cutoff))
            .one(&*self.db)
            .await?;

        Ok(result.is_some())
    }

    async fn consume_code(&self, realm_id: &str, code: &str) -> Result<(), CoreError> {
        use herald_entity::email_verification_code;

        email_verification_code::Entity::delete_many()
            .filter(email_verification_code::Column::RealmId.eq(realm_id))
            .filter(email_verification_code::Column::VerificationCode.eq(code))
            .exec(&*self.db)
            .await?;

        Ok(())
    }

    async fn get_email_by_code(
        &self,
        realm_id: &str,
        code: &str,
    ) -> Result<Option<String>, CoreError> {
        use herald_domain::security_constants::EMAIL_VERIFICATION_CODE_TTL_SECONDS;
        use herald_entity::email_verification_code;

        let cutoff = (chrono::Utc::now()
            - chrono::Duration::seconds(EMAIL_VERIFICATION_CODE_TTL_SECONDS as i64))
        .fixed_offset();
        let result = email_verification_code::Entity::find()
            .filter(email_verification_code::Column::RealmId.eq(realm_id))
            .filter(email_verification_code::Column::VerificationCode.eq(code))
            .filter(email_verification_code::Column::CreatedAt.gte(cutoff))
            .one(&*self.db)
            .await?;

        Ok(result.map(|r| r.email))
    }

    async fn delete_code_by_type(
        &self,
        realm_id: &str,
        email: &str,
        code_type: &str,
    ) -> Result<(), CoreError> {
        use herald_entity::email_verification_code;

        email_verification_code::Entity::delete_many()
            .filter(email_verification_code::Column::RealmId.eq(realm_id))
            .filter(email_verification_code::Column::Email.eq(email))
            .filter(email_verification_code::Column::Type.eq(code_type))
            .exec(&*self.db)
            .await?;

        Ok(())
    }
}
