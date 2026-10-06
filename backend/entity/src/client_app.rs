use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "client_app")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub realm_id: String,
    pub client_id: String,
    pub name: String,
    pub description: Option<String>,

    // New fields for Client App settings
    pub redirect_uris: JsonValue,
    pub allowed_origins: JsonValue,
    pub email_verify_return_url: Option<String>,
    pub password_reset_return_url: Option<String>,
    pub browser_refresh_absolute_ttl_seconds: i32,
    pub is_first_party: bool,
    pub enabled: bool,
    pub icon_url: Option<String>,
    pub client_secret: Option<String>,
    pub device_code_grant_enabled: bool,

    // Turnstile (Cloudflare human-verification), delegated to the Client App
    // (D-PROTECT-01). When `turnstile_enabled` is false the other two columns
    // are ignored.
    pub turnstile_enabled: bool,
    pub turnstile_site_key: Option<String>,
    pub turnstile_secret_key: Option<String>,

    /// MCP disable generation (DEC-mcp-server-006): bumped by the atomic
    /// disable UPDATE on the built-in MCP client; 0 for every other client.
    pub mcp_token_generation: i64,

    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

// 辅助方法：UUID 转换
impl Model {
    pub fn id_as_string(&self) -> String {
        self.id.to_string()
    }
}
