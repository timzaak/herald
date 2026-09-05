// OAuth provider configuration repository implementation

use sea_orm::DatabaseConnection;
use std::sync::Arc;

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::oauth::entities::{OAuthProviderConfig, UpdateOAuthProviderConfigRequest};
use herald_domain::oauth::ports::OAuthConfigRepository;

// Type alias to reduce complexity
type OAuthConfigRow = (
    uuid::Uuid,
    String,
    String,
    String,
    String,
    Vec<String>,
    bool,
    chrono::DateTime<chrono::Utc>,
    chrono::DateTime<chrono::Utc>,
);

pub struct PostgresOAuthConfigRepository {
    db: Arc<DatabaseConnection>,
}

impl PostgresOAuthConfigRepository {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }

    fn model_to_entity(
        id: uuid::Uuid,
        realm_id: String,
        provider_type: String,
        client_id: String,
        client_secret: String,
        scopes: Vec<String>,
        enabled: bool,
        created_at: chrono::DateTime<chrono::Utc>,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<OAuthProviderConfig, CoreError> {
        use herald_domain::oauth::entities::ProviderType;
        use std::str::FromStr;

        let provider = ProviderType::from_str(&provider_type).map_err(|_| {
            CoreError::BadRequest(format!("Invalid provider type: {}", provider_type))
        })?;

        Ok(OAuthProviderConfig {
            id,
            realm_id,
            provider_type: provider,
            client_id,
            client_secret,
            scopes,
            enabled,
            created_at,
            updated_at,
        })
    }
}

impl OAuthConfigRepository for PostgresOAuthConfigRepository {
    async fn get_config(
        &self,
        realm_id: &str,
        provider_type: &str,
    ) -> Result<OAuthProviderConfig, CoreError> {
        let (id, realm, provider, client_id, client_secret, scopes, enabled, created_at, updated_at): OAuthConfigRow =
            sqlx::query_as(
                "SELECT id, realm_id, provider_type, client_id, client_secret, scopes, enabled, created_at, updated_at
                 FROM oauth_provider_config
                 WHERE realm_id = $1 AND provider_type = $2",
            )
            .bind(realm_id)
            .bind(provider_type)
            .fetch_one(self.db.get_postgres_connection_pool())
            .await
            .map_err(|e| {
                tracing::error!("Failed to get oauth config: {e}");
                if e.to_string().contains("not found") || e.to_string().contains("no rows") {
                    CoreError::NotFound
                } else {
                    CoreError::InternalServerError(e.to_string())
                }
            })?;

        Self::model_to_entity(
            id,
            realm,
            provider,
            client_id,
            client_secret,
            scopes,
            enabled,
            created_at,
            updated_at,
        )
    }

    async fn get_config_by_id(&self, id: uuid::Uuid) -> Result<OAuthProviderConfig, CoreError> {
        let (id, realm, provider, client_id, client_secret, scopes, enabled, created_at, updated_at): OAuthConfigRow =
            sqlx::query_as(
                "SELECT id, realm_id, provider_type, client_id, client_secret, scopes, enabled, created_at, updated_at
                 FROM oauth_provider_config
                 WHERE id = $1",
            )
            .bind(id)
            .fetch_one(self.db.get_postgres_connection_pool())
            .await
            .map_err(|e| {
                tracing::error!("Failed to get oauth config by id: {e}");
                if e.to_string().contains("not found") || e.to_string().contains("no rows") {
                    CoreError::NotFound
                } else {
                    CoreError::InternalServerError(e.to_string())
                }
            })?;

        Self::model_to_entity(
            id,
            realm,
            provider,
            client_id,
            client_secret,
            scopes,
            enabled,
            created_at,
            updated_at,
        )
    }

    async fn list_configs(&self, realm_id: &str) -> Result<Vec<OAuthProviderConfig>, CoreError> {
        let rows = sqlx::query_as(
            "SELECT id, realm_id, provider_type, client_id, client_secret, scopes, enabled, created_at, updated_at
             FROM oauth_provider_config
             WHERE realm_id = $1
             ORDER BY provider_type",
        )
        .bind(realm_id)
        .fetch_all(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| {
            tracing::error!("Failed to list oauth configs: {e}");
            CoreError::InternalServerError(e.to_string())
        })?;

        rows.into_iter()
            .map(
                |(
                    id,
                    realm,
                    provider,
                    client_id,
                    client_secret,
                    scopes,
                    enabled,
                    created_at,
                    updated_at,
                )| {
                    Self::model_to_entity(
                        id,
                        realm,
                        provider,
                        client_id,
                        client_secret,
                        scopes,
                        enabled,
                        created_at,
                        updated_at,
                    )
                },
            )
            .collect()
    }

    async fn list_enabled_configs(
        &self,
        realm_id: &str,
    ) -> Result<Vec<OAuthProviderConfig>, CoreError> {
        let rows = sqlx::query_as(
            "SELECT id, realm_id, provider_type, client_id, client_secret, scopes, enabled, created_at, updated_at
             FROM oauth_provider_config
             WHERE realm_id = $1 AND enabled = true
             ORDER BY provider_type",
        )
        .bind(realm_id)
        .fetch_all(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| {
            tracing::error!("Failed to list enabled oauth configs: {e}");
            CoreError::InternalServerError(e.to_string())
        })?;

        rows.into_iter()
            .map(
                |(
                    id,
                    realm,
                    provider,
                    client_id,
                    client_secret,
                    scopes,
                    enabled,
                    created_at,
                    updated_at,
                )| {
                    Self::model_to_entity(
                        id,
                        realm,
                        provider,
                        client_id,
                        client_secret,
                        scopes,
                        enabled,
                        created_at,
                        updated_at,
                    )
                },
            )
            .collect()
    }

    async fn create_config(
        &self,
        config: OAuthProviderConfig,
    ) -> Result<OAuthProviderConfig, CoreError> {
        let rec: OAuthConfigRow = sqlx::query_as(
                "INSERT INTO oauth_provider_config (id, realm_id, provider_type, client_id, client_secret, scopes, enabled, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 RETURNING id, realm_id, provider_type, client_id, client_secret, scopes, enabled, created_at, updated_at",
            )
            .bind(config.id)
            .bind(&config.realm_id)
            .bind(config.provider_type.as_str())
            .bind(&config.client_id)
            .bind(&config.client_secret)
            .bind(&config.scopes)
            .bind(config.enabled)
            .bind(config.created_at)
            .bind(config.updated_at)
            .fetch_one(self.db.get_postgres_connection_pool())
            .await
            .map_err(|e| {
                tracing::error!("Failed to create oauth config: {e}");
                if e.to_string().contains("duplicate key") || e.to_string().contains("unique constraint") {
                    CoreError::Conflict("OAuth provider config already exists".to_string())
                } else {
                    CoreError::InternalServerError(e.to_string())
                }
            })?;

        Self::model_to_entity(
            rec.0, rec.1, rec.2, rec.3, rec.4, rec.5, rec.6, rec.7, rec.8,
        )
    }

    async fn update_config(
        &self,
        id: uuid::Uuid,
        request: UpdateOAuthProviderConfigRequest,
    ) -> Result<OAuthProviderConfig, CoreError> {
        let now = chrono::Utc::now();

        // A scope change is validated against the stored provider — the
        // create path enforces the same contract (e.g. WeChat = snsapi_login
        // only, mini-program = none). Updates that don't touch scopes skip
        // the lookup entirely.
        if let Some(ref new_scopes) = request.scopes {
            let stored = self.get_config_by_id(id).await?;
            herald_domain::oauth::entities::validate_scopes(&stored.provider_type, new_scopes)?;
        }

        sqlx::query(
            "UPDATE oauth_provider_config
             SET client_id = COALESCE($2, client_id),
                 client_secret = COALESCE($3, client_secret),
                 scopes = COALESCE($4, scopes),
                 enabled = COALESCE($5, enabled),
                 updated_at = $6
             WHERE id = $1",
        )
        .bind(id)
        .bind(&request.client_id)
        .bind(&request.client_secret)
        .bind(&request.scopes)
        .bind(request.enabled)
        .bind(now)
        .execute(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| {
            tracing::error!("Failed to update oauth config: {e}");
            CoreError::InternalServerError(e.to_string())
        })?;

        // Re-read so the returned entity reflects the merged (COALESCE) state.
        let (id, realm_id, provider_type, client_id, client_secret, scopes, enabled, created_at, updated_at): OAuthConfigRow =
            sqlx::query_as(
            "SELECT id, realm_id, provider_type, client_id, client_secret, scopes, enabled, created_at, updated_at
             FROM oauth_provider_config
             WHERE id = $1",
        )
        .bind(id)
        .fetch_one(self.db.get_postgres_connection_pool())
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch updated oauth config: {e}");
            CoreError::InternalServerError(e.to_string())
        })?;

        Self::model_to_entity(
            id,
            realm_id,
            provider_type,
            client_id,
            client_secret,
            scopes,
            enabled,
            created_at,
            updated_at,
        )
    }

    async fn delete_config(&self, id: uuid::Uuid) -> Result<(), CoreError> {
        let result = sqlx::query("DELETE FROM oauth_provider_config WHERE id = $1")
            .bind(id)
            .execute(self.db.get_postgres_connection_pool())
            .await
            .map_err(|e| {
                tracing::error!("Failed to delete oauth config: {e}");
                CoreError::InternalServerError(e.to_string())
            })?;

        if result.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }

        Ok(())
    }
}

impl std::fmt::Debug for PostgresOAuthConfigRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresOAuthConfigRepository").finish()
    }
}
