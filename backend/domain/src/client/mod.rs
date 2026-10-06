pub mod entities;
pub mod ports;
pub mod services;
pub mod validation;
pub mod value_objects;

use crate::client_api_keys::constants::ADMIN_API_CLIENT_ID;

pub use entities::ClientApp;
pub use validation::{
    normalize_origins, validate_icon_url, validate_origin, validate_redirect_uri,
    validate_redirect_uris,
};

pub const ADMIN_WEB_CONSOLE_CLIENT_ID: &str = "admin-web-console";
pub const USER_ACCOUNT_CENTER_CLIENT_ID: &str = "user-account-center";
/// Reserved client_id of the per-realm built-in public MCP client
/// (DEC-mcp-server-007). PKCE-only, `is_first_party = false`, no secret;
/// only `enabled` is administrable and it can never be deleted.
pub const MCP_CLIENT_ID: &str = "herald-mcp";

/// Whether a client_id names the built-in MCP client — the request-side
/// discriminator for the MCP-specific OAuth behaviors (resource binding,
/// RFC 6749 error bodies). Stored-code and request-code sides both resolve
/// through here so the two can never disagree about the client class.
pub fn is_mcp_client(client_id: &str) -> bool {
    client_id == MCP_CLIENT_ID
}

/// The canonical per-realm MCP resource URI (RFC 8707 audience): the exact
/// string authorize binds into codes, token families carry, and the /mcp
/// middleware compares against. Single source — a per-side retelling would
/// let issuance and verification drift apart silently.
pub fn mcp_resource_uri(origin: &str, realm_id: &str) -> String {
    format!("{origin}/mcp/{realm_id}")
}

/// The RFC 9728 protected-resource metadata URL advertising the MCP
/// endpoint (the `resource_metadata` challenge parameter).
pub fn mcp_prm_uri(origin: &str, realm_id: &str) -> String {
    format!("{origin}/.well-known/oauth-protected-resource/mcp/{realm_id}")
}

pub fn is_builtin_first_party_client(client_id: &str) -> bool {
    matches!(
        client_id,
        ADMIN_WEB_CONSOLE_CLIENT_ID | USER_ACCOUNT_CENTER_CLIENT_ID
    )
}

/// System-built-in client ids: the two UI clients, the realm's built-in API
/// Key client, and the built-in MCP client. A resource-protection flag —
/// read-only surface for admins, never a user permission.
pub fn is_system_builtin_client(client_id: &str) -> bool {
    is_builtin_first_party_client(client_id)
        || client_id == ADMIN_API_CLIENT_ID
        || client_id == MCP_CLIENT_ID
}
