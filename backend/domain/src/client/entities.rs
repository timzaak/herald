use crate::common::entities::Entity;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct ClientApp {
    pub id: Uuid,
    pub realm_id: String,
    pub client_id: String,
    pub name: String,
    pub description: Option<String>,

    // New fields for Client App settings
    #[schema(example = json!(["https://example.com/callback"]))]
    pub redirect_uris: Vec<String>,
    pub allowed_origins: Vec<String>,
    pub email_verify_return_url: Option<String>,
    pub password_reset_return_url: Option<String>,
    pub browser_refresh_absolute_ttl_seconds: i32,
    pub is_first_party: bool,
    #[schema(example = true)]
    pub enabled: bool,
    pub icon_url: Option<String>,
    pub client_secret: Option<String>,
    pub device_code_grant_enabled: bool,

    // Turnstile (Cloudflare human-verification), delegated to the Client App
    // (D-PROTECT-01). When `turnstile_enabled` is false the other two fields
    // are ignored.
    pub turnstile_enabled: bool,
    pub turnstile_site_key: Option<String>,
    pub turnstile_secret_key: Option<String>,

    /// MCP disable generation (DEC-mcp-server-006). Bumped in the same DB
    /// update that disables the built-in MCP client; MCP credentials carry
    /// the generation they were issued under and die when it goes stale.
    /// Always 0 for non-MCP clients.
    pub mcp_token_generation: i64,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Entity for ClientApp {
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

// 辅助方法
impl ClientApp {
    pub fn id_as_string(&self) -> String {
        self.id.to_string()
    }
}
