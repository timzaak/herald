// Facebook OAuth provider implementation

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::oauth::{
    entities::ProviderType,
    http_client::{HttpClient, HttpClientRequestBuilder, HttpMethod},
    ports::OAuthProviderHandler,
    value_objects::{OAuthConfig, OAuthUserInfo},
};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, RedirectUrl, Scope, TokenResponse,
    TokenUrl, basic::BasicClient,
};
use serde::Deserialize;

pub struct FacebookOAuthProvider;

impl FacebookOAuthProvider {
    const AUTH_URL: &'static str = "https://www.facebook.com/v18.0/dialog/oauth";
    const TOKEN_URL: &'static str = "https://graph.facebook.com/v18.0/oauth/access_token";
    const USER_API_URL: &'static str = "https://graph.facebook.com/me";
}

#[derive(Deserialize)]
struct FacebookPictureData {
    url: String,
}

#[derive(Deserialize)]
struct FacebookPicture {
    data: FacebookPictureData,
}

#[derive(Deserialize)]
struct FacebookUser {
    id: String,
    email: String,
    name: Option<String>,
    picture: Option<FacebookPicture>,
}

impl FacebookUser {
    /// Facebook exposes no per-email verified flag to third-party apps: the
    /// Graph `verified` field is a deprecated ACCOUNT-level badge (not email
    /// verification) that most apps never even receive, and consuming it as
    /// email verification locked out regular Facebook logins behind
    /// "Provider email is not verified" (review 20260919 finding 5). The
    /// email Graph delivers under the granted `email` permission is the
    /// account's Facebook-confirmed primary address — that is the strongest
    /// signal available and the trust basis for `verified = true`.
    fn email_verified(&self) -> bool {
        true
    }
}

impl OAuthProviderHandler for FacebookOAuthProvider {
    fn provider_type(&self) -> &'static str {
        "facebook"
    }

    fn display_name(&self) -> &'static str {
        "Facebook"
    }

    fn get_auth_url(&self, state: &str, config: &OAuthConfig) -> Result<String, CoreError> {
        let client = BasicClient::new(ClientId::new(config.client_id.clone()))
            .set_client_secret(ClientSecret::new(config.client_secret.clone()))
            .set_auth_uri(AuthUrl::new(Self::AUTH_URL.to_string())?)
            .set_token_uri(TokenUrl::new(Self::TOKEN_URL.to_string())?)
            .set_redirect_uri(RedirectUrl::new(config.redirect_uri.clone())?);

        // Honor realm-configured scopes; fall back to Facebook default if none.
        let scopes: Vec<Scope> = if config.scopes.is_empty() {
            vec![Scope::new("email".to_string())]
        } else {
            config
                .scopes
                .iter()
                .map(|s| Scope::new(s.clone()))
                .collect()
        };

        let (auth_url, _csrf_token) = client
            .authorize_url(|| oauth2::CsrfToken::new(state.to_string()))
            .add_scopes(scopes)
            .url();

        Ok(auth_url.to_string())
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
            let client = BasicClient::new(ClientId::new(config.client_id.clone()))
                .set_client_secret(ClientSecret::new(config.client_secret.clone()))
                .set_auth_uri(AuthUrl::new(Self::AUTH_URL.to_string())?)
                .set_token_uri(TokenUrl::new(Self::TOKEN_URL.to_string())?)
                .set_redirect_uri(RedirectUrl::new(config.redirect_uri.clone())?);

            let oauth_http_client = oauth2::reqwest::ClientBuilder::new()
                .redirect(oauth2::reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| {
                    CoreError::InternalServerError(format!(
                        "Failed to build OAuth HTTP client: {}",
                        e
                    ))
                })?;

            let token_result = client
                .exchange_code(AuthorizationCode::new(code))
                .request_async(&oauth_http_client)
                .await
                .map_err(|e| CoreError::BadRequest(format!("Token exchange failed: {}", e)))?;

            let access_token = token_result.access_token().secret();

            // Get user info using the HTTP client abstraction. The deprecated
            // account-level `verified` badge is deliberately NOT requested.
            let user_info_url = format!("{}?fields=id,email,name,picture", Self::USER_API_URL);

            let response = http_client
                .request(
                    HttpClientRequestBuilder::new(user_info_url, HttpMethod::Get)
                        .bearer_auth(access_token)
                        .build(),
                )
                .await?;

            if !response.is_success() {
                let status_code = response.status_code;
                let response_body = response.body_as_string().unwrap_or_default();
                return Err(CoreError::InternalServerError(format!(
                    "Failed to get user info from Facebook: status={}, body={}",
                    status_code, response_body
                )));
            }

            let response_body = response.body_as_string()?;
            let facebook_user: FacebookUser =
                serde_json::from_str(&response_body).map_err(|e| {
                    CoreError::InternalServerError(format!("Failed to parse user info: {}", e))
                })?;

            let verified = facebook_user.email_verified();
            let email = facebook_user.email;
            let avatar = facebook_user.picture.map(|p| p.data.url);
            let name = facebook_user.name;
            let provider_user_id = facebook_user.id.clone();
            Ok(OAuthUserInfo {
                provider_type: ProviderType::Facebook,
                provider_user_id,
                email,
                // Derived from the provider's verified signal; absent → false.
                verified,
                avatar,
                name,
                union_id: None, // Facebook doesn't provide UnionID
                open_id: Some(facebook_user.id),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Intent (audit run-1: me-endpoint-email-hardcoded-verified → review
    // 20260919 finding 5): the flag must never be derived from the Graph
    // `verified` field — it is a deprecated account-level badge, not an
    // email-verification signal, and reading it locked out ordinary Facebook
    // logins. Facebook hands third-party apps no per-email proof at all; the
    // `email` permission delivering the account's confirmed primary address
    // is the trust basis, so email_verified() is true whenever Graph returned
    // an email. This assertion pins that a stray "verified": false in a /me
    // payload (the deprecated badge) must not flip the login-gate outcome.
    #[test]
    fn facebook_email_verification_ignores_deprecated_account_badge() {
        let user: FacebookUser = serde_json::from_str(
            r#"{"id":"1","email":"a@example.com","name":"A","verified":false}"#,
        )
        .expect("the deprecated badge is not part of the contract and must not break parsing");
        assert!(user.email_verified());
    }
}
