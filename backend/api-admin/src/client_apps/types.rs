use herald_core::domain::client::ClientApp;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

/// Custom validator for client_id (alphanumeric, hyphen, underscore only)
fn validate_client_id(client_id: &str) -> Result<(), validator::ValidationError> {
    // Allow alphanumeric characters, hyphens, and underscores (URL-safe)
    if client_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid client_id format"))
    }
}

/// Request to create a new OAuth client application
#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientAppCreateRequest {
    /// OAuth client identifier (3-36 characters)
    ///
    /// Used as the client_id in OAuth flows. Must be unique within the realm.
    /// Can contain alphanumeric characters only.
    #[validate(length(min = 3, max = 36))]
    #[validate(custom(function = "validate_client_id"))]
    #[schema(example = "mywebapp")]
    pub client_id: String,

    /// Human-readable application name (1-100 characters)
    ///
    /// Display name shown to users in authorization screens and admin panels.
    #[validate(length(min = 1, max = 100))]
    #[schema(example = "My Web Application")]
    pub name: String,

    /// Detailed description of the client application (max 500 characters)
    ///
    /// Optional description explaining the purpose and functionality of the application.
    #[validate(length(max = 500))]
    #[schema(example = "Internal admin dashboard for organization management")]
    pub description: Option<String>,

    /// Allowed OAuth redirect URIs
    ///
    /// List of valid redirect URIs for OAuth flows. Must be valid HTTPS URLs
    /// (http://localhost is allowed for development). Users will only be redirected
    /// to these URIs after authentication.
    #[schema(example = json!(["https://example.com/callback", "http://localhost:3000/auth/callback"]))]
    pub redirect_uris: Option<Vec<String>>,
    pub allowed_origins: Option<Vec<String>>,
    pub email_verify_return_url: Option<String>,
    pub password_reset_return_url: Option<String>,
    pub browser_refresh_absolute_ttl_seconds: Option<i32>,

    /// Whether this client application is active
    ///
    /// When set to false, the client cannot be used for new OAuth flows.
    #[schema(example = "true")]
    pub enabled: Option<bool>,

    /// URL to client application icon (favicon, logo)
    ///
    /// Optional URL to an image file that will be displayed as the app icon.
    /// Should be a valid HTTPS URL to an image resource.
    #[schema(example = "https://example.com/logo.png")]
    pub icon_url: Option<String>,

    pub device_code_grant_enabled: Option<bool>,

    /// Enable Cloudflare Turnstile human-verification for this Client App
    /// (D-PROTECT-01). Defaults to false.
    pub turnstile_enabled: Option<bool>,
    /// Cloudflare Turnstile site key (public). Optional; only used when
    /// Turnstile is enabled.
    pub turnstile_site_key: Option<String>,
    /// Cloudflare Turnstile secret key (server-side, sensitive). Write-only:
    /// never echoed back in responses.
    pub turnstile_secret_key: Option<String>,
}

// Client ID regex: alphanumeric only
// Allows letters and numbers (e.g., "testclient123")

/// Request to update an existing OAuth client application
///
/// All fields are optional - only provided fields will be updated.
#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientAppUpdateRequest {
    /// Client application name (1-100 characters)
    ///
    /// Updates the display name shown to users in authorization screens.
    #[validate(length(min = 1, max = 100))]
    #[schema(example = "Updated Web App")]
    pub name: Option<String>,

    /// Detailed description of the client application (max 500 characters)
    ///
    /// Updates the description explaining the purpose of the application.
    #[validate(length(max = 500))]
    #[schema(example = "Updated admin dashboard with new features")]
    pub description: Option<String>,

    /// Allowed OAuth redirect URIs
    ///
    /// Updates the list of valid redirect URIs for OAuth flows. Must be valid HTTPS URLs
    /// (http://localhost is allowed for development). Users will only be redirected
    /// to these URIs after authentication.
    #[schema(example = json!(["https://example.com/callback", "https://app.example.com/auth/callback"]))]
    pub redirect_uris: Option<Vec<String>>,
    pub allowed_origins: Option<Vec<String>>,
    pub email_verify_return_url: Option<String>,
    pub password_reset_return_url: Option<String>,
    pub browser_refresh_absolute_ttl_seconds: Option<i32>,

    /// Whether this client application is active
    ///
    /// When set to false, the client cannot be used for new OAuth flows.
    #[schema(example = "true")]
    pub enabled: Option<bool>,

    /// URL to client application icon (favicon, logo)
    ///
    /// Updates the URL to an image file that will be displayed as the app icon.
    /// Should be a valid HTTPS URL to an image resource.
    #[schema(example = "https://example.com/new-logo.png")]
    pub icon_url: Option<String>,

    /// Regenerate client secret
    ///
    /// Set to true to generate a new client_secret. The old secret will be immediately
    /// invalidated and cannot be recovered. The new secret will be returned in the response.
    /// **Store this securely** - it will not be retrievable after this response.
    #[schema(example = "false")]
    pub regenerate_secret: Option<bool>,

    pub device_code_grant_enabled: Option<bool>,

    /// Enable/disable Cloudflare Turnstile for this Client App (D-PROTECT-01).
    pub turnstile_enabled: Option<bool>,
    /// Update the Cloudflare Turnstile site key (public).
    pub turnstile_site_key: Option<String>,
    /// Update the Cloudflare Turnstile secret key (server-side, sensitive).
    /// Write-only: never echoed back in responses.
    pub turnstile_secret_key: Option<String>,
}

// Database model (used with sqlx::FromRow)
#[derive(Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ClientAppDbModel {
    pub id: Uuid,
    pub realm_id: String,
    pub client_id: String,
    pub name: String,
    pub description: Option<String>,

    // New fields for Client App settings
    pub redirect_uris: Json<Vec<String>>,
    pub allowed_origins: Json<Vec<String>>,
    pub email_verify_return_url: Option<String>,
    pub password_reset_return_url: Option<String>,
    pub browser_refresh_absolute_ttl_seconds: i32,
    pub is_first_party: bool,
    pub enabled: bool,
    pub icon_url: Option<String>,
    pub client_secret: Option<String>,
    pub device_code_grant_enabled: bool,

    // Turnstile (D-PROTECT-01). DB row carries all three columns.
    pub turnstile_enabled: bool,
    pub turnstile_site_key: Option<String>,
    pub turnstile_secret_key: Option<String>,
}

// API response model (used for OpenAPI documentation)
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientAppItem {
    /// Client application UUID
    ///
    /// Internal unique identifier for the client application.
    #[schema(example = "01234567-89ab-cdef-0123-456789abcdef")]
    pub id: Uuid,

    /// Realm this client belongs to
    ///
    /// The realm ID that this client application is associated with.
    #[schema(example = "my-realm")]
    pub realm_id: String,

    /// OAuth client_id (used in OAuth flows)
    ///
    /// The public identifier used in OAuth authorization flows.
    #[schema(example = "my-web-app")]
    pub client_id: String,

    /// Human-readable application name
    ///
    /// Display name shown to users in authorization screens and admin panels.
    #[schema(example = "My Web Application")]
    pub name: String,

    /// Detailed description
    ///
    /// Description explaining the purpose and functionality of the application.
    #[schema(example = "Internal admin dashboard for organization management")]
    pub description: Option<String>,

    /// Allowed OAuth redirect URIs
    ///
    /// List of valid redirect URIs for OAuth flows. Users will only be redirected
    /// to these URIs after authentication.
    #[schema(example = json!(["https://example.com/callback", "http://localhost:3000/auth/callback"]))]
    pub redirect_uris: Vec<String>,
    pub allowed_origins: Vec<String>,
    pub email_verify_return_url: Option<String>,
    pub password_reset_return_url: Option<String>,
    pub browser_refresh_absolute_ttl_seconds: i32,
    pub is_first_party: bool,

    /// Whether this client is currently enabled
    ///
    /// When false, the client cannot be used for new OAuth flows.
    #[schema(example = "true")]
    pub enabled: bool,

    /// Application icon URL
    ///
    /// URL to an image file that will be displayed as the app icon.
    #[schema(example = "https://example.com/logo.png")]
    pub icon_url: Option<String>,

    /// OAuth client secret (only shown during creation)
    ///
    /// The secret key used for confidential client authentication in OAuth flows.
    /// **Store this securely** - it will not be retrievable after the initial creation response.
    /// This field will be null in update and list responses.
    #[schema(example = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx")]
    pub client_secret: Option<String>,

    pub device_code_grant_enabled: bool,

    /// Whether Cloudflare Turnstile human-verification is enforced for this
    /// Client App (D-PROTECT-01).
    pub turnstile_enabled: bool,
    /// Cloudflare Turnstile site key (public), shown to the client widget.
    /// `None` when Turnstile is disabled.
    pub turnstile_site_key: Option<String>,
}

// Conversion from the domain entity to API response model. `client_secret`
// defaults to None: it is write-only and must never be echoed unless a handler
// explicitly overrides it (create / secret-regenerate paths).
impl From<ClientApp> for ClientAppItem {
    fn from(app: ClientApp) -> Self {
        Self {
            id: app.id,
            realm_id: app.realm_id,
            client_id: app.client_id,
            name: app.name,
            description: app.description,
            redirect_uris: app.redirect_uris,
            allowed_origins: app.allowed_origins,
            email_verify_return_url: app.email_verify_return_url,
            password_reset_return_url: app.password_reset_return_url,
            browser_refresh_absolute_ttl_seconds: app.browser_refresh_absolute_ttl_seconds,
            is_first_party: app.is_first_party,
            enabled: app.enabled,
            icon_url: app.icon_url,
            client_secret: None,
            device_code_grant_enabled: app.device_code_grant_enabled,
            turnstile_enabled: app.turnstile_enabled,
            turnstile_site_key: app.turnstile_site_key,
        }
    }
}

// Conversion from DB model to API response model
impl From<ClientAppDbModel> for ClientAppItem {
    fn from(db_model: ClientAppDbModel) -> Self {
        Self {
            id: db_model.id,
            realm_id: db_model.realm_id,
            client_id: db_model.client_id,
            name: db_model.name,
            description: db_model.description,
            redirect_uris: db_model.redirect_uris.0,
            allowed_origins: db_model.allowed_origins.0,
            email_verify_return_url: db_model.email_verify_return_url,
            password_reset_return_url: db_model.password_reset_return_url,
            browser_refresh_absolute_ttl_seconds: db_model.browser_refresh_absolute_ttl_seconds,
            is_first_party: db_model.is_first_party,
            enabled: db_model.enabled,
            icon_url: db_model.icon_url,
            // Secrets are show-once: never echoed through a DB-model
            // conversion, only on create/regenerate handler responses.
            client_secret: None,
            device_code_grant_enabled: db_model.device_code_grant_enabled,
            // turnstile_secret_key is intentionally NOT echoed in responses.
            turnstile_enabled: db_model.turnstile_enabled,
            turnstile_site_key: db_model.turnstile_site_key,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_page_size")]
    pub page_size: i64,
    // Removed realm_id - it will be extracted from path parameter
}

fn default_page() -> i64 {
    0 // Start from 0 consistently
}

fn default_page_size() -> i64 {
    20
}

#[cfg(test)]
mod tests {
    use super::ClientAppUpdateRequest;

    #[test]
    fn first_party_cannot_be_written_through_admin_request() {
        let request = serde_json::from_value::<ClientAppUpdateRequest>(serde_json::json!({
            "isFirstParty": true
        }));
        assert!(
            request.is_err(),
            "isFirstParty must remain an internal-only field"
        );
    }
}
