// Herald API Auth Module
// Authentication handlers (login, register, password reset, TOTP, email verification)

pub mod browser_token;
pub mod change_email;
pub mod consent_gate;
pub mod email_otp;
pub mod ldap_login;
pub mod login;
pub mod logout;
mod mailflow;
mod passkey_rp;
pub mod reauth;
pub mod register;
pub mod registration_status;
pub mod reset_password;
pub mod signup;
pub mod status;
pub mod turnstile_status;
pub mod user_passkey;
pub mod user_totp;
pub mod verify_email;
pub mod verify_passkey;
pub mod verify_totp;

use axum::routing::get;
use axum::{Router, routing::post};
use herald_api_base::application::http::state::AppState;

// Preserve the API crate's module facade while implementations live in api-base.
pub mod util {
    pub use herald_api_base::application::http::auth::util::*;
}
pub mod identity_middleware {
    pub use herald_api_base::application::http::auth::identity_middleware::*;
}
pub mod error {
    pub use herald_api_base::application::http::auth::error::*;
}

// Re-export commonly used types and functions
pub use login::{LoginRequestPayload, LoginResponse};

// Re-export utoipa path markers
pub use browser_token::__path_refresh as __path_browser_token_refresh;
pub use browser_token::__path_switch_client as __path_browser_token_switch_client;
pub use change_email::__path_confirm as __path_change_email_confirm;
pub use change_email::__path_request as __path_change_email_request;
pub use email_otp::__path_send as __path_email_otp_send;
pub use email_otp::__path_status as __path_email_otp_status;
pub use email_otp::__path_verify as __path_email_otp_verify;
pub use ldap_login::__path_ldap_login;
pub use ldap_login::__path_ldap_status;
pub use login::__path_login;
pub use logout::__path_logout;
pub use reauth::{__path_handle_begin_reauth, __path_handle_verify_reauth};
pub use register::__path_register;
pub use reset_password::__path_confirm as __path_reset_password_confirm;
pub use reset_password::__path_request as __path_reset_password_request;
pub use signup::__path_get_signup_status;
pub use signup::__path_signup;
pub use status::__path_status;
pub use turnstile_status::__path_get_turnstile_status;
pub use user_passkey::__path_handle_begin_passkey_registration;
pub use user_passkey::__path_handle_delete_passkey_credential;
pub use user_passkey::__path_handle_finish_passkey_registration;
pub use user_passkey::__path_handle_list_passkey_credentials;
pub use user_passkey::__path_handle_rename_passkey_credential;
pub use user_totp::__path_handle_disable_totp;
pub use user_totp::__path_handle_enable_totp;
pub use user_totp::__path_handle_get_totp_status;
pub use user_totp::__path_handle_regenerate_totp;
pub use user_totp::__path_handle_verify_totp_setup;
pub use verify_email::__path_confirm as __path_verify_email_confirm;
pub use verify_email::__path_trigger as __path_verify_email_trigger;
pub use verify_passkey::__path_handle_passkey_2fa_options;
pub use verify_passkey::__path_handle_passkey_2fa_verify;
pub use verify_passkey::__path_handle_passkey_options;
pub use verify_passkey::__path_handle_passkey_verify;
pub use verify_passkey::__path_status as __path_passkey_status;
pub use verify_totp::__path_handle_verify_totp as __path_verify_totp;

/// OpenAPI specification for auth module
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::login::login,
        crate::browser_token::refresh,
        crate::browser_token::switch_client,
        crate::email_otp::send,
        crate::email_otp::verify,
        crate::email_otp::status,
        crate::ldap_login::ldap_login,
        crate::ldap_login::ldap_status,
        crate::register::register,
        crate::reauth::handle_begin_reauth,
        crate::reauth::handle_verify_reauth,
        crate::logout::logout,
        crate::status::status,
        crate::signup::signup,
        crate::signup::get_signup_status,
        crate::turnstile_status::get_turnstile_status,
        crate::verify_email::trigger,
        crate::verify_email::confirm,
        crate::reset_password::request,
        crate::reset_password::confirm,
        crate::change_email::request,
        crate::change_email::confirm,
        crate::verify_totp::handle_verify_totp,
        crate::verify_passkey::handle_passkey_options,
        crate::verify_passkey::handle_passkey_verify,
        crate::verify_passkey::handle_passkey_2fa_options,
        crate::verify_passkey::handle_passkey_2fa_verify,
        crate::verify_passkey::status,
        crate::user_totp::handle_enable_totp,
        crate::user_totp::handle_verify_totp_setup,
        crate::user_totp::handle_disable_totp,
        crate::user_totp::handle_regenerate_totp,
        crate::user_totp::handle_get_totp_status,
        crate::user_passkey::handle_begin_passkey_registration,
        crate::user_passkey::handle_finish_passkey_registration,
        crate::user_passkey::handle_list_passkey_credentials,
        crate::user_passkey::handle_rename_passkey_credential,
        crate::user_passkey::handle_delete_passkey_credential,
    ),
    components(schemas(
        crate::login::LoginRequestPayload,
        crate::login::LoginResponse,
        crate::browser_token::BrowserTokenResponse,
        crate::browser_token::RefreshBrowserTokenRequest,
        crate::browser_token::SwitchClientRequest,
        crate::browser_token::SwitchClientResponse,
        crate::email_otp::EmailOtpSendRequest,
        crate::email_otp::EmailOtpSendResponse,
        crate::email_otp::EmailOtpVerifyRequest,
        crate::email_otp::EmailOtpStatusResponse,
        crate::email_otp::EmailOtpConflictResponse,
        crate::ldap_login::LdapLoginRequest,
        crate::ldap_login::LdapStatusResponse,
        crate::register::RegisterRequest,
        crate::register::RegisterResponse,
        crate::reauth::ReauthBeginRequest,
        crate::reauth::ReauthBeginResponse,
        crate::reauth::ReauthVerifyRequest,
        crate::reauth::PasskeyAssertion,
        crate::reauth::ReauthTicket,
        crate::status::StatusResponse,
        crate::signup::SignupRequest,
        crate::signup::SignupResponse,
        crate::signup::SignupStatusResponse,
        crate::turnstile_status::TurnstileStatusRequest,
        crate::turnstile_status::TurnstileStatusResponse,
        crate::verify_email::VerifyEmailTriggerRequest,
        crate::verify_email::VerifyEmailConfirmResponse,
        crate::reset_password::ResetPasswordRequestRequest,
        crate::reset_password::ResetPasswordRequestResponse,
        crate::reset_password::ResetPasswordConfirmRequest,
        crate::reset_password::ResetPasswordConfirmResponse,
        crate::change_email::ChangeEmailRequest,
        crate::change_email::ChangeEmailResponse,
        crate::verify_totp::VerifyTotpRequest,
        crate::verify_totp::VerifyTotpResponse,
        crate::verify_passkey::PasskeyOptionsRequest,
        crate::verify_passkey::PasskeyOAuthRequest,
        crate::verify_passkey::PasskeyOptionsResponse,
        crate::verify_passkey::PasskeyVerifyRequest,
        crate::verify_passkey::PasskeyVerifyResponse,
        crate::verify_passkey::Passkey2faOptionsRequest,
        crate::verify_passkey::Passkey2faOptionsResponse,
        crate::verify_passkey::Passkey2faVerifyRequest,
        crate::verify_passkey::PasskeyStatusResponse,
        crate::user_totp::EnableTotpRequest,
        crate::user_totp::EnableTotpResponse,
        crate::user_totp::VerifyTotpSetupRequest,
        crate::user_totp::VerifyTotpSetupResponse,
        crate::user_totp::DisableTotpRequest,
        crate::user_totp::DisableTotpResponse,
        crate::user_totp::RegenerateTotpRequest,
        crate::user_totp::RegenerateTotpResponse,
        crate::user_totp::TotpStatusResponse,
        crate::user_totp::BackupCodeStatsResponse,
        crate::user_passkey::BeginRegistrationRequest,
        crate::user_passkey::BeginRegistrationResponse,
        crate::user_passkey::FinishRegistrationRequest,
        crate::user_passkey::FinishRegistrationResponse,
        crate::user_passkey::PasskeyCredentialViewResponse,
        crate::user_passkey::ListPasskeysResponse,
        crate::user_passkey::RenamePasskeyRequest,
    ))
)]
pub struct ApiDoc;

pub fn auth_router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register::register))
        .route("/signup", post(signup::signup))
        .route("/signup/status", get(signup::get_signup_status))
        .route("/login", post(login::login))
        .route("/login/ldap", post(ldap_login::ldap_login))
        .route("/login/email-otp/send", post(email_otp::send))
        .route("/login/email-otp/verify", post(email_otp::verify))
        .route("/email-otp/status", get(email_otp::status))
        .route("/ldap/status", get(ldap_login::ldap_status))
        .route("/passkey/status", get(verify_passkey::status))
        .route("/login/verify-totp", post(verify_totp::handle_verify_totp))
        .route(
            "/login/passkey/options",
            post(verify_passkey::handle_passkey_options),
        )
        .route(
            "/login/passkey/verify",
            post(verify_passkey::handle_passkey_verify),
        )
        .route(
            "/login/passkey/2fa/options",
            post(verify_passkey::handle_passkey_2fa_options),
        )
        .route(
            "/login/passkey/2fa/verify",
            post(verify_passkey::handle_passkey_2fa_verify),
        )
        .route(
            "/turnstile/status",
            get(turnstile_status::get_turnstile_status),
        )
        .route(
            "/registration/status",
            post(registration_status::get_registration_status),
        )
        .route("/verify_email/trigger", post(verify_email::trigger))
        .route(
            "/verify_email/confirm/{email_verification_code}",
            get(verify_email::confirm),
        )
        .route("/reset_password/request", post(reset_password::request))
        .route(
            "/reset_password/confirm/{reset_code}",
            post(reset_password::confirm),
        )
        .route("/change_email/request", post(change_email::request))
        .route(
            "/change_email/confirm/{change_code}",
            get(change_email::confirm),
        )
}

pub fn browser_token_router() -> Router<AppState> {
    Router::new().route("/browser-token/refresh", post(browser_token::refresh))
}

/// Routes whose realm and Client App are derived exclusively from a Bearer token.
pub fn token_router() -> Router<AppState> {
    Router::new()
        .route(
            "/browser-token/switch-client",
            post(browser_token::switch_client),
        )
        .route("/logout", post(logout::logout))
        .route("/status", get(status::status))
}

/// Re-authentication routes documented under `/api/user`.
pub fn reauth_router() -> Router<AppState> {
    reauth::router()
}
