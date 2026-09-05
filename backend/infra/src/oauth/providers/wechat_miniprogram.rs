// WeChat Mini Program OAuth provider implementation (code2session)

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::oauth::{
    entities::ProviderType,
    http_client::HttpClient,
    ports::OAuthProviderHandler,
    value_objects::{OAuthConfig, OAuthUserInfo},
};
use serde::Deserialize;
use urlencoding::encode;

pub struct WeChatMiniProgramProvider;

impl WeChatMiniProgramProvider {
    pub const CODE2SESSION_URL: &'static str = "https://api.weixin.qq.com/sns/jscode2session";
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct WeChatMiniProgramSession {
    pub openid: String,
    pub session_key: String,
    pub unionid: Option<String>,
    // Error fields - only present when API returns error
    #[serde(default)]
    pub errcode: Option<i32>,
    #[serde(default)]
    pub errmsg: Option<String>,
}

impl OAuthProviderHandler for WeChatMiniProgramProvider {
    fn provider_type(&self) -> &'static str {
        "wechat_miniprogram"
    }

    fn display_name(&self) -> &'static str {
        "WeChat Mini Program"
    }

    fn get_auth_url(&self, _state: &str, _config: &OAuthConfig) -> Result<String, CoreError> {
        // Mini programs use wx.login() in the frontend to get the code
        // This method is not used for mini programs
        Err(CoreError::BadRequest(
            "WeChat Mini Program does not use authorization URL. Use wx.login() in the mini program to get the code.".to_string(),
        ))
    }

    #[allow(clippy::manual_async_fn)]
    fn exchange_code_and_get_user<H>(
        &self,
        code: String,
        config: &OAuthConfig,
        http_client: &H,
    ) -> impl Future<Output = Result<OAuthUserInfo, CoreError>> + Send
    where
        H: HttpClient + Send + Sync,
    {
        async move {
            // Call jscode2session API
            let url = format!(
                "{}?appid={}&secret={}&js_code={}&grant_type=authorization_code",
                Self::CODE2SESSION_URL,
                encode(&config.client_id),
                encode(&config.client_secret),
                encode(&code)
            );

            let response = http_client.get(&url).await?;

            if !response.is_success() {
                return Err(CoreError::InternalServerError(
                    "code2session request failed".to_string(),
                ));
            }

            let response_body = response.body_as_string()?;
            let session_data: WeChatMiniProgramSession = serde_json::from_str(&response_body)
                .map_err(|e| {
                    CoreError::InternalServerError(format!("Failed to parse session data: {}", e))
                })?;

            // Check for WeChat API errors
            if let Some(errcode) = session_data.errcode {
                let error_msg = session_data
                    .errmsg
                    .unwrap_or_else(|| "Unknown error".to_string());
                return Err(match errcode {
                    -1 => {
                        CoreError::InternalServerError(format!("WeChat API error: {}", error_msg))
                    }
                    40001 => CoreError::BadRequest("Invalid appsecret".to_string()),
                    40029 => CoreError::BadRequest("Invalid code".to_string()),
                    45011 => CoreError::BadRequest("WeChat API rate limit exceeded".to_string()),
                    _ => CoreError::BadRequest(format!("WeChat error {}: {}", errcode, error_msg)),
                });
            }

            // Generate placeholder email (WeChat doesn't provide real email)
            // Priority: unionid > openid
            let id_for_email = session_data
                .unionid
                .as_ref()
                .or(Some(&session_data.openid))
                .ok_or_else(|| CoreError::InternalServerError("Missing openid".to_string()))?;

            let placeholder_email = format!("{}@wechat.placeholder", id_for_email);

            Ok(OAuthUserInfo {
                provider_type: ProviderType::WeChatMiniProgram,
                provider_user_id: session_data.openid.clone(),
                email: placeholder_email,
                verified: false, // Placeholder email is not verified
                avatar: None,    // Mini program doesn't provide avatar by default
                name: None,      // Mini program doesn't provide nickname by default
                union_id: session_data.unionid,
                open_id: Some(session_data.openid),
            })
        }
    }
}
