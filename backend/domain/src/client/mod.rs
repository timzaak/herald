pub mod entities;
pub mod ports;
pub mod services;
pub mod validation;
pub mod value_objects;

pub use entities::ClientApp;
pub use validation::{
    normalize_origins, validate_icon_url, validate_origin, validate_redirect_uri,
    validate_redirect_uris,
};

pub const ADMIN_WEB_CONSOLE_CLIENT_ID: &str = "admin-web-console";
pub const USER_ACCOUNT_CENTER_CLIENT_ID: &str = "user-account-center";

pub fn is_builtin_first_party_client(client_id: &str) -> bool {
    matches!(
        client_id,
        ADMIN_WEB_CONSOLE_CLIENT_ID | USER_ACCOUNT_CENTER_CLIENT_ID
    )
}
