use std::sync::Arc;
use uuid::Uuid;

use crate::authentication::Identity;
use crate::client::{
    entities::ClientApp,
    is_builtin_first_party_client, normalize_origins,
    ports::{ClientRepository, ClientService},
    validate_icon_url, validate_redirect_uri, validate_redirect_uris,
    value_objects::{CreateClientAppRequest, UpdateClientAppRequest},
};

fn is_development_env() -> bool {
    std::env::var("ENV").unwrap_or_else(|_| "development".to_string()) == "development"
}

fn validate_redirect_uris_if_needed(redirect_uris: &[String]) -> Result<(), CoreError> {
    validate_redirect_uris(redirect_uris, is_development_env())
        .map_err(|e| CoreError::BadRequest(format!("Redirect URI validation failed: {}", e)))
}
use crate::common::entities::app_errors::CoreError;
use crate::common::policies::{ClientPolicy, ensure_policy};
use crate::security_constants::{
    BROWSER_REFRESH_ABSOLUTE_TTL_MAX_SECONDS, BROWSER_REFRESH_ABSOLUTE_TTL_MIN_SECONDS,
};

pub struct ClientServiceImpl<R, P>
where
    R: ClientRepository,
    P: ClientPolicy,
{
    client_repository: Arc<R>,
    policy: Arc<P>,
}

impl<R, P> ClientServiceImpl<R, P>
where
    R: ClientRepository,
    P: ClientPolicy,
{
    pub fn new(client_repository: Arc<R>, policy: Arc<P>) -> Self {
        Self {
            client_repository,
            policy,
        }
    }

    /// Internal method: get client by client_id without permission check
    pub async fn get_client_by_id_internal(
        &self,
        realm_id: &str,
        client_id: &str,
    ) -> Result<ClientApp, CoreError> {
        self.client_repository
            .get_client_app_by_client_id(realm_id, client_id)
            .await
    }
}

impl<R, P> ClientService for ClientServiceImpl<R, P>
where
    R: ClientRepository,
    P: ClientPolicy,
{
    async fn create_client_app(
        &self,
        identity: Identity,
        mut request: CreateClientAppRequest,
    ) -> Result<ClientApp, CoreError> {
        // Policy check（使用具体方法 + ensure_policy）
        ensure_policy(
            self.policy.can_create_client(identity.clone()).await,
            "Insufficient permissions to create client",
        )?;

        // CRITICAL: Realm boundary check - prevent cross-realm client creation
        if !identity.has_access_to_realm(&request.realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot create client in a different realm".to_string(),
            ));
        }

        // Reserved client IDs are not available to realm admins.
        if is_builtin_first_party_client(&request.client_id) {
            return Err(CoreError::Forbidden(
                "Reserved client id cannot be used".to_string(),
            ));
        }

        // Field-level guards mirror the admin request validators so the ext
        // endpoint (which has no Valid<> layer) enforces the same bounds.
        if request.name.trim().is_empty() || request.name.len() > 100 {
            return Err(CoreError::BadRequest(
                "Client app name must be 1-100 characters".to_string(),
            ));
        }
        if request
            .description
            .as_ref()
            .is_some_and(|description| description.len() > 500)
        {
            return Err(CoreError::BadRequest(
                "Client app description must be at most 500 characters".to_string(),
            ));
        }

        // Validate redirect URIs if provided
        if let Some(ref redirect_uris) = request.redirect_uris {
            let skip_empty =
                request.device_code_grant_enabled.unwrap_or(false) && redirect_uris.is_empty();
            if !skip_empty {
                validate_redirect_uris_if_needed(redirect_uris)?;
            }
        }
        if let Some(origins) = request.allowed_origins.take() {
            request.allowed_origins = Some(normalize_origins(&origins)?);
        }
        validate_icon_url(request.icon_url.as_ref())?;
        for return_url in [
            &request.email_verify_return_url,
            &request.password_reset_return_url,
        ]
        .into_iter()
        .flatten()
        {
            validate_redirect_uri(return_url, is_development_env())?;
        }
        let ttl = request
            .browser_refresh_absolute_ttl_seconds
            .unwrap_or(2_592_000);
        if !(BROWSER_REFRESH_ABSOLUTE_TTL_MIN_SECONDS..=BROWSER_REFRESH_ABSOLUTE_TTL_MAX_SECONDS)
            .contains(&ttl)
        {
            return Err(CoreError::BadRequest(format!(
                "Browser refresh absolute TTL must be between {} and {} seconds",
                BROWSER_REFRESH_ABSOLUTE_TTL_MIN_SECONDS, BROWSER_REFRESH_ABSOLUTE_TTL_MAX_SECONDS
            )));
        }

        self.client_repository.create_client_app(request).await
    }

    async fn get_client_app(&self, identity: Identity, id: Uuid) -> Result<ClientApp, CoreError> {
        // Policy check
        ensure_policy(
            self.policy.can_read_client(identity.clone()).await,
            "Insufficient permissions to read client",
        )?;

        // Get client and check realm boundary
        let client = self.client_repository.get_client_app_by_id(id).await?;

        // CRITICAL: Realm boundary check - prevent cross-realm access
        if !identity.has_access_to_realm(&client.realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: client belongs to a different realm".to_string(),
            ));
        }

        Ok(client)
    }

    async fn get_client_app_by_client_id(
        &self,
        realm_id: &str,
        client_id: &str,
    ) -> Result<ClientApp, CoreError> {
        // Direct repository call without permission check (used in auth/login for client validation)
        self.client_repository
            .get_client_app_by_client_id(realm_id, client_id)
            .await
    }

    async fn list_client_apps(
        &self,
        identity: Identity,
        realm_id: String,
    ) -> Result<Vec<ClientApp>, CoreError> {
        // Policy check
        ensure_policy(
            self.policy.can_list_clients(identity.clone()).await,
            "Insufficient permissions to list clients",
        )?;

        // CRITICAL: Realm boundary check - prevent cross-realm access
        if !identity.has_access_to_realm(&realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot list clients from a different realm".to_string(),
            ));
        }

        self.client_repository.list_client_apps(&realm_id).await
    }

    async fn list_client_apps_paginated(
        &self,
        identity: Identity,
        realm_id: String,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<ClientApp>, u64), CoreError> {
        // Policy check
        ensure_policy(
            self.policy.can_list_clients(identity.clone()).await,
            "Insufficient permissions to list clients",
        )?;

        // CRITICAL: Realm boundary check - prevent cross-realm access
        if !identity.has_access_to_realm(&realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot list clients from a different realm".to_string(),
            ));
        }

        self.client_repository
            .list_client_apps_paginated(&realm_id, page, page_size)
            .await
    }

    async fn update_client_app(
        &self,
        identity: Identity,
        id: Uuid,
        mut request: UpdateClientAppRequest,
    ) -> Result<ClientApp, CoreError> {
        // Policy check
        ensure_policy(
            self.policy.can_update_client(identity.clone()).await,
            "Insufficient permissions to update client",
        )?;

        // Get client and check realm boundary
        let client = self
            .client_repository
            .get_client_app_by_id(id)
            .await
            .map_err(|e| {
                // If client not found, we still want to validate realm for new creation
                // But for update, client must exist, so we can check its realm
                if matches!(e, CoreError::NotFound) {
                    return CoreError::NotFound;
                }
                e
            })?;

        // CRITICAL: Realm boundary check - prevent cross-realm access
        if !identity.has_access_to_realm(&client.realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot update client from a different realm".to_string(),
            ));
        }

        // Validate redirect URIs if provided
        if let Some(ref redirect_uris) = request.redirect_uris {
            validate_redirect_uris_if_needed(redirect_uris)?;
        }
        if let Some(origins) = request.allowed_origins.take() {
            request.allowed_origins = Some(normalize_origins(&origins)?);
        }
        validate_icon_url(request.icon_url.as_ref())?;
        for return_url in [
            &request.email_verify_return_url,
            &request.password_reset_return_url,
        ]
        .into_iter()
        .flatten()
        {
            validate_redirect_uri(return_url, is_development_env())?;
        }
        if let Some(ttl) = request.browser_refresh_absolute_ttl_seconds
            && !(BROWSER_REFRESH_ABSOLUTE_TTL_MIN_SECONDS
                ..=BROWSER_REFRESH_ABSOLUTE_TTL_MAX_SECONDS)
                .contains(&ttl)
        {
            return Err(CoreError::BadRequest(format!(
                "Browser refresh absolute TTL must be between {} and {} seconds",
                BROWSER_REFRESH_ABSOLUTE_TTL_MIN_SECONDS, BROWSER_REFRESH_ABSOLUTE_TTL_MAX_SECONDS
            )));
        }

        self.client_repository.update_client_app(id, request).await
    }

    async fn delete_client_app(&self, identity: Identity, id: Uuid) -> Result<(), CoreError> {
        // Policy check
        ensure_policy(
            self.policy.can_delete_client(identity.clone()).await,
            "Insufficient permissions to delete client",
        )?;

        // Get client and check realm boundary
        let client = self.client_repository.get_client_app_by_id(id).await?;

        // CRITICAL: Realm boundary check - prevent cross-realm access
        if !identity.has_access_to_realm(&client.realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot delete client from a different realm".to_string(),
            ));
        }

        self.client_repository.delete_client_app(id).await
    }
}

impl<R, P> std::fmt::Debug for ClientServiceImpl<R, P>
where
    R: ClientRepository,
    P: ClientPolicy,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientServiceImpl").finish()
    }
}
