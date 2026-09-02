// Client App Lookup Helper
//
// Provides efficient client_app lookup functions for external API handlers.
// Extracted to avoid raw SQL queries in handler code.

use sqlx::PgPool;
use uuid::Uuid;

use axum::http::StatusCode;
use axum::response::Response;
use herald_api_base::application::http::common::error_codes::ErrorCode;
use herald_api_base::application::http::common::error_helpers::json_error;

/// Client app lookup helper
pub struct ClientAppLookup {
    pool: PgPool,
}

impl ClientAppLookup {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Find client app realm_id by UUID
    ///
    /// Used to verify realm isolation for external API requests.
    pub async fn find_realm_by_uuid(
        &self,
        client_app_id: Uuid,
    ) -> Result<Option<String>, Response> {
        match sqlx::query_scalar::<_, String>("SELECT realm_id FROM client_app WHERE id = $1")
            .bind(client_app_id)
            .fetch_optional(&self.pool)
            .await
        {
            Ok(realm_id) => Ok(realm_id),
            Err(e) => {
                tracing::error!("Failed to query client_app realm_id: {}", e);
                Err(json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorCode::InternalError,
                ))
            }
        }
    }

    /// Find client app UUID by client_id and realm_id
    ///
    /// Used to convert external client_id strings to internal UUIDs.
    /// Accepts either a client_id string or a UUID string.
    pub async fn find_uuid_by_identifier(
        &self,
        identifier: &str,
        realm_id: &str,
    ) -> Result<Option<Uuid>, Response> {
        // Try parsing as UUID first
        if let Ok(uuid) = Uuid::parse_str(identifier) {
            // Verify the UUID exists in this realm
            match sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM client_app WHERE id = $1 AND realm_id = $2 LIMIT 1",
            )
            .bind(uuid)
            .bind(realm_id)
            .fetch_optional(&self.pool)
            .await
            {
                Ok(Some(id)) => return Ok(Some(id)),
                Ok(None) => {}
                Err(e) => {
                    tracing::error!("Failed to query client_app by UUID: {}", e);
                    return Err(json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ErrorCode::InternalError,
                    ));
                }
            }
        }

        // Try looking up by client_id
        match sqlx::query_scalar::<_, Uuid>(
            "SELECT id::uuid FROM client_app WHERE client_id = $1 AND realm_id = $2 LIMIT 1",
        )
        .bind(identifier)
        .bind(realm_id)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(uuid) => Ok(uuid),
            Err(e) => {
                tracing::error!("Failed to query client_app by client_id: {}", e);
                Err(json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorCode::InternalError,
                ))
            }
        }
    }

    /// Verify client app exists and return its realm_id
    ///
    /// Returns 404 if client app not found.
    pub async fn verify_client_app_exists(&self, client_app_id: Uuid) -> Result<String, Response> {
        match self.find_realm_by_uuid(client_app_id).await? {
            Some(realm_id) => Ok(realm_id),
            None => Err(json_error(
                StatusCode::NOT_FOUND,
                ErrorCode::ClientAppNotFound,
            )),
        }
    }

    /// Find client app UUID by identifier (client_id or UUID)
    ///
    /// Returns 404 if not found.
    pub async fn find_uuid_by_identifier_required(
        &self,
        identifier: &str,
        realm_id: &str,
    ) -> Result<Uuid, Response> {
        match self.find_uuid_by_identifier(identifier, realm_id).await? {
            Some(uuid) => Ok(uuid),
            None => Err(json_error(
                StatusCode::NOT_FOUND,
                ErrorCode::ClientAppNotFound,
            )),
        }
    }
}
