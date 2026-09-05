use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter};
use std::sync::Arc;
use uuid::Uuid;

use herald_domain::client::{
    entities::ClientApp,
    is_builtin_first_party_client,
    ports::ClientRepository,
    value_objects::{CreateClientAppRequest, UpdateClientAppRequest},
};
use herald_domain::common::entities::app_errors::CoreError;
use herald_entity::{client_api_key, client_app};

pub struct PostgresClientRepository {
    db: Arc<sea_orm::DatabaseConnection>,
}

impl PostgresClientRepository {
    pub fn new(db: Arc<sea_orm::DatabaseConnection>) -> Self {
        Self { db }
    }

    fn to_domain(model: &client_app::Model) -> Result<ClientApp, CoreError> {
        // Parse redirect_uris from JSON
        let redirect_uris: Vec<String> = serde_json::from_value(model.redirect_uris.clone())
            .map_err(|e| CoreError::DatabaseError(format!("Invalid redirect_uris JSON: {e}")))?;
        let allowed_origins: Vec<String> = serde_json::from_value(model.allowed_origins.clone())
            .map_err(|e| CoreError::DatabaseError(format!("Invalid allowed_origins JSON: {e}")))?;

        Ok(ClientApp {
            id: model.id,
            realm_id: model.realm_id.clone(),
            client_id: model.client_id.clone(),
            name: model.name.clone(),
            description: model.description.clone(),
            redirect_uris,
            allowed_origins,
            email_verify_return_url: model.email_verify_return_url.clone(),
            password_reset_return_url: model.password_reset_return_url.clone(),
            browser_refresh_absolute_ttl_seconds: model.browser_refresh_absolute_ttl_seconds,
            is_first_party: model.is_first_party,
            enabled: model.enabled,
            icon_url: model.icon_url.clone(),
            client_secret: model.client_secret.clone(),
            device_code_grant_enabled: model.device_code_grant_enabled,
            turnstile_enabled: model.turnstile_enabled,
            turnstile_site_key: model.turnstile_site_key.clone(),
            turnstile_secret_key: model.turnstile_secret_key.clone(),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
        })
    }
}

impl ClientRepository for PostgresClientRepository {
    async fn create_client_app(
        &self,
        request: CreateClientAppRequest,
    ) -> Result<ClientApp, CoreError> {
        let now = chrono::Utc::now();
        let id = herald_domain::common::entities::generate_uuid_v7();

        // Convert redirect_uris to JSON (use empty array if not provided)
        let redirect_uris = request.redirect_uris.unwrap_or_default();
        let redirect_uris_json = serde_json::to_value(&redirect_uris)
            .map_err(|e| CoreError::BadRequest(format!("Invalid redirect URIs: {}", e)))?;
        let allowed_origins_json =
            serde_json::to_value(request.allowed_origins.unwrap_or_default())
                .map_err(|e| CoreError::BadRequest(format!("Invalid allowed origins: {e}")))?;

        // Generate client secret
        let client_secret = Some(herald_domain::common::entities::generate_uuid_v7().to_string());

        let enabled = request.enabled.unwrap_or(true);
        let active_model = client_app::ActiveModel {
            id: sea_orm::Set(id),
            realm_id: sea_orm::Set(request.realm_id),
            client_id: sea_orm::Set(request.client_id),
            name: sea_orm::Set(request.name),
            description: sea_orm::Set(request.description),
            redirect_uris: sea_orm::Set(redirect_uris_json),
            allowed_origins: sea_orm::Set(allowed_origins_json),
            email_verify_return_url: sea_orm::Set(request.email_verify_return_url),
            password_reset_return_url: sea_orm::Set(request.password_reset_return_url),
            browser_refresh_absolute_ttl_seconds: sea_orm::Set(
                request
                    .browser_refresh_absolute_ttl_seconds
                    .unwrap_or(2_592_000),
            ),
            is_first_party: sea_orm::Set(false),
            enabled: sea_orm::Set(enabled),
            icon_url: sea_orm::Set(request.icon_url),
            client_secret: sea_orm::Set(client_secret),
            device_code_grant_enabled: sea_orm::Set(
                request.device_code_grant_enabled.unwrap_or(false),
            ),
            turnstile_enabled: sea_orm::Set(request.turnstile_enabled.unwrap_or(false)),
            turnstile_site_key: sea_orm::Set(request.turnstile_site_key),
            turnstile_secret_key: sea_orm::Set(request.turnstile_secret_key),
            created_at: sea_orm::Set(now.into()),
            updated_at: sea_orm::Set(now.into()),
        };

        let result = active_model.insert(&*self.db).await?;
        Self::to_domain(&result)
    }

    async fn get_client_app_by_id(&self, id: Uuid) -> Result<ClientApp, CoreError> {
        let result = client_app::Entity::find_by_id(id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        Self::to_domain(&result)
    }

    async fn get_client_app_by_client_id(
        &self,
        realm_id: &str,
        client_id: &str,
    ) -> Result<ClientApp, CoreError> {
        let result = client_app::Entity::find()
            .filter(client_app::Column::RealmId.eq(realm_id))
            .filter(client_app::Column::ClientId.eq(client_id))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        Self::to_domain(&result)
    }

    async fn list_client_apps(&self, realm_id: &str) -> Result<Vec<ClientApp>, CoreError> {
        let results = client_app::Entity::find()
            .filter(client_app::Column::RealmId.eq(realm_id))
            .all(&*self.db)
            .await?;

        results.iter().map(Self::to_domain).collect()
    }

    async fn list_client_apps_paginated(
        &self,
        realm_id: &str,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<ClientApp>, u64), CoreError> {
        use sea_orm::{
            ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
        };

        // Get total count
        let total_count = client_app::Entity::find()
            .filter(client_app::Column::RealmId.eq(realm_id))
            .count(&*self.db)
            .await?;

        // Calculate offset
        let offset = page * page_size;

        // Get paginated results
        let results: Vec<client_app::Model> = client_app::Entity::find()
            .filter(client_app::Column::RealmId.eq(realm_id))
            .order_by(client_app::Column::CreatedAt, sea_orm::Order::Desc)
            .limit(page_size)
            .offset(offset)
            .all(&*self.db)
            .await?;

        let apps: Vec<ClientApp> = results
            .iter()
            .map(Self::to_domain)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((apps, total_count))
    }

    async fn update_client_app(
        &self,
        id: Uuid,
        request: UpdateClientAppRequest,
    ) -> Result<ClientApp, CoreError> {
        let mut active_model: client_app::ActiveModel = client_app::Entity::find_by_id(id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?
            .into();

        if let Some(name) = request.name {
            active_model.name = sea_orm::Set(name);
        }

        if let Some(description) = request.description {
            active_model.description = sea_orm::Set(Some(description));
        }

        // Update new fields
        if let Some(redirect_uris) = request.redirect_uris {
            let redirect_uris_json = serde_json::to_value(&redirect_uris)
                .map_err(|e| CoreError::BadRequest(format!("Invalid redirect URIs: {}", e)))?;
            active_model.redirect_uris = sea_orm::Set(redirect_uris_json);
        }
        if let Some(allowed_origins) = request.allowed_origins {
            active_model.allowed_origins = sea_orm::Set(
                serde_json::to_value(allowed_origins)
                    .map_err(|e| CoreError::BadRequest(format!("Invalid allowed origins: {e}")))?,
            );
        }
        if let Some(url) = request.email_verify_return_url {
            active_model.email_verify_return_url = sea_orm::Set(Some(url));
        }
        if let Some(url) = request.password_reset_return_url {
            active_model.password_reset_return_url = sea_orm::Set(Some(url));
        }
        if let Some(ttl) = request.browser_refresh_absolute_ttl_seconds {
            active_model.browser_refresh_absolute_ttl_seconds = sea_orm::Set(ttl);
        }

        if let Some(enabled) = request.enabled {
            active_model.enabled = sea_orm::Set(enabled);
        }

        if let Some(icon_url) = request.icon_url {
            active_model.icon_url = sea_orm::Set(Some(icon_url));
        }

        if let Some(v) = request.device_code_grant_enabled {
            active_model.device_code_grant_enabled = sea_orm::Set(v);
        }

        // Turnstile (D-PROTECT-01): update only the fields the caller supplied.
        // An empty string clears the stored key (maps to NULL).
        if let Some(v) = request.turnstile_enabled {
            active_model.turnstile_enabled = sea_orm::Set(v);
        }
        if let Some(v) = request.turnstile_site_key {
            active_model.turnstile_site_key = sea_orm::Set(Some(v).filter(|s| !s.is_empty()));
        }
        if let Some(v) = request.turnstile_secret_key {
            active_model.turnstile_secret_key = sea_orm::Set(Some(v).filter(|s| !s.is_empty()));
        }

        // Regenerate client secret if requested
        if request.regenerate_secret.unwrap_or(false) {
            let new_secret = herald_domain::common::entities::generate_uuid_v7().to_string();
            active_model.client_secret = sea_orm::Set(Some(new_secret));
        }

        active_model.updated_at = sea_orm::Set(chrono::Utc::now().into());

        let result = active_model.update(&*self.db).await?;
        Self::to_domain(&result)
    }

    async fn delete_client_app(&self, id: Uuid) -> Result<(), CoreError> {
        // Query the client app first
        let client = client_app::Entity::find_by_id(id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        if is_builtin_first_party_client(&client.client_id) {
            return Err(CoreError::BadRequest(
                "Cannot delete a built-in first-party client app".to_string(),
            ));
        }

        let has_api_keys = client_api_key::Entity::find()
            .filter(client_api_key::Column::ClientAppId.eq(id))
            .one(&*self.db)
            .await?
            .is_some();
        if has_api_keys {
            return Err(CoreError::Conflict(
                "Cannot delete a client app while API keys are bound to it".to_string(),
            ));
        }

        client_app::Entity::delete_by_id(id).exec(&*self.db).await?;

        Ok(())
    }

    async fn set_first_party(&self, id: Uuid, is_first_party: bool) -> Result<(), CoreError> {
        let model = client_app::Entity::find_by_id(id)
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;
        if is_first_party && !is_builtin_first_party_client(&model.client_id) {
            return Err(CoreError::Forbidden(
                "First-party flag is reserved for built-in Herald clients".to_string(),
            ));
        }
        let mut active_model: client_app::ActiveModel = model.into();
        active_model.is_first_party = sea_orm::Set(is_first_party);
        active_model.updated_at = sea_orm::Set(chrono::Utc::now().into());
        active_model.update(&*self.db).await?;
        Ok(())
    }
}
