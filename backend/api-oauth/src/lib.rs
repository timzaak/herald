// Herald API OAuth Module
// OAuth provider authentication handlers

pub mod apple_native;
pub mod authorize;
pub mod callback;
pub mod config;
pub mod device_authorize;
pub mod device_confirm;
pub mod device_token;
pub mod device_verify;
pub mod google_one_tap;
pub mod helper;
pub mod login;
pub mod token;
pub mod wechat;
pub mod wechat_miniprogram;

pub use apple_native::*;
pub use authorize::*;
pub use callback::*;
pub use config::*;
pub use device_authorize::*;
pub use device_confirm::*;
pub use device_token::*;
pub use device_verify::*;
pub use google_one_tap::*;
pub use login::*;
pub use token::*;
pub use wechat::*;
pub use wechat_miniprogram::*;

/// OpenAPI specification for OAuth module
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::authorize::oauth_authorize,
        crate::login::oauth_login,
        crate::callback::oauth_callback,
        crate::config::list_oauth_configs,
        crate::config::create_oauth_config,
        crate::config::get_oauth_config,
        crate::config::update_oauth_config,
        crate::config::delete_oauth_config,
        crate::wechat::wechat_login,
        crate::wechat::wechat_callback,
        crate::wechat_miniprogram::wechat_miniprogram_login,
        crate::google_one_tap::google_one_tap,
        crate::apple_native::apple_native_login,
        crate::device_authorize::device_authorize,
        crate::device_token::device_token,
        crate::device_verify::device_verify,
        crate::device_confirm::device_confirm,
        crate::token::oauth_token,
    ),
    components(schemas(
        crate::login::OAuthLoginRequest,
        crate::login::OAuthLoginResponse,
        crate::callback::OAuthCallbackResponse,
        crate::callback::OAuthCallbackErrorResponse,
        crate::authorize::AuthorizeQueryParams,
        crate::config::CreateOAuthConfigRequest,
        crate::config::UpdateOAuthConfigRequest,
        crate::config::OAuthConfigResponse,
        crate::wechat::WeChatAuthUrlResponse,
        crate::wechat_miniprogram::WeChatMiniProgramLoginRequest,
        crate::wechat_miniprogram::WeChatMiniProgramLoginResponse,
        crate::google_one_tap::OneTapRequest,
        crate::google_one_tap::OneTapDirectResponse,
        crate::google_one_tap::OneTapCodeResponse,
        crate::apple_native::AppleNativeRequest,
        crate::apple_native::AppleNativeDirectResponse,
        crate::apple_native::AppleNativeCodeResponse,
        crate::device_authorize::DeviceAuthorizationRequest,
        crate::device_authorize::DeviceAuthorizationResponse,
        crate::device_authorize::DeviceAuthorizationErrorResponse,
        crate::device_token::DeviceTokenRequest,
        crate::device_token::DeviceTokenResponse,
        crate::device_token::DeviceTokenErrorResponse,
        crate::device_verify::DeviceVerifyRequest,
        crate::device_verify::DeviceVerifyResponse,
        crate::device_verify::DeviceVerifyErrorResponse,
        crate::device_confirm::DeviceConfirmRequest,
        crate::device_confirm::DeviceConfirmResponse,
        crate::device_confirm::DeviceConfirmErrorResponse,
        crate::token::TokenRequest,
        crate::token::TokenResponse,
    ))
)]
pub struct ApiDoc;
