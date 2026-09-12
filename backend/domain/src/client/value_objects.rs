use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct CreateClientAppRequest {
    #[validate(length(min = 1))]
    pub realm_id: String,
    #[validate(length(min = 3, max = 36))]
    pub client_id: String,
    #[validate(length(min = 1))]
    pub name: String,
    pub description: Option<String>,

    // New fields for Client App settings
    // redirect_uris is optional during creation (can be added later)
    pub redirect_uris: Option<Vec<String>>,
    pub allowed_origins: Option<Vec<String>>,
    pub email_verify_return_url: Option<String>,
    pub password_reset_return_url: Option<String>,
    pub browser_refresh_absolute_ttl_seconds: Option<i32>,
    pub enabled: Option<bool>,
    pub icon_url: Option<String>,
    pub device_code_grant_enabled: Option<bool>,

    // Turnstile (D-PROTECT-01): optional on creation, defaults to disabled.
    pub turnstile_enabled: Option<bool>,
    pub turnstile_site_key: Option<String>,
    pub turnstile_secret_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct UpdateClientAppRequest {
    #[validate(length(min = 1))]
    pub name: Option<String>,
    pub description: Option<String>,

    // New fields for Client App settings
    pub redirect_uris: Option<Vec<String>>,
    pub allowed_origins: Option<Vec<String>>,
    pub email_verify_return_url: Option<String>,
    pub password_reset_return_url: Option<String>,
    pub browser_refresh_absolute_ttl_seconds: Option<i32>,
    pub enabled: Option<bool>,
    pub icon_url: Option<String>,
    pub device_code_grant_enabled: Option<bool>,
    pub regenerate_secret: Option<bool>,

    // Turnstile (D-PROTECT-01): all three optional on update.
    pub turnstile_enabled: Option<bool>,
    pub turnstile_site_key: Option<String>,
    pub turnstile_secret_key: Option<String>,
}
