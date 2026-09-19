// GitHub OAuth provider implementation

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::oauth::{
    entities::ProviderType,
    http_client::{HttpClient, HttpClientRequest, HttpClientRequestBuilder, HttpMethod},
    ports::OAuthProviderHandler,
    value_objects::{OAuthConfig, OAuthUserInfo},
};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, RedirectUrl, Scope, TokenResponse,
    TokenUrl, basic::BasicClient,
};
use serde::Deserialize;

pub struct GitHubOAuthProvider;

impl GitHubOAuthProvider {
    const AUTH_URL: &'static str = "https://github.com/login/oauth/authorize";
    const TOKEN_URL: &'static str = "https://github.com/login/oauth/access_token";
    const USER_API_URL: &'static str = "https://api.github.com/user";
    const USER_EMAILS_URL: &'static str = "https://api.github.com/user/emails";
    const USER_AGENT: &'static str = "Herald";

    fn authenticated_request(url: &str, access_token: &str) -> HttpClientRequest {
        HttpClientRequestBuilder::new(url, HttpMethod::Get)
            .bearer_auth(access_token)
            .header("User-Agent", Self::USER_AGENT)
            .header("Accept", "application/vnd.github+json")
            .build()
    }

    /// Verified flag for the /user profile email, derived from the matching
    /// /user/emails entry. The profile email is a user-selectable public
    /// presentation field with no verification proof of its own, so an entry
    /// that is missing or unverified MUST fail closed — never assert a proof
    /// the provider did not give. (A failed /user/emails fetch never reaches
    /// this function: the caller propagates it as a loud error instead of
    /// silently misclassifying the login as "email not verified" — review
    /// 20260919 finding 7.)
    fn profile_email_verified(emails: &[GitHubEmail], profile_email: &str) -> bool {
        emails
            .iter()
            .find(|e| e.email.eq_ignore_ascii_case(profile_email))
            .is_some_and(|e| e.verified)
    }
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct GitHubUser {
    id: i64,
    email: Option<String>,
    login: String,
    avatar_url: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct GitHubEmail {
    email: String,
    primary: bool,
    verified: bool,
}

impl OAuthProviderHandler for GitHubOAuthProvider {
    fn provider_type(&self) -> &'static str {
        "github"
    }

    fn display_name(&self) -> &'static str {
        "GitHub"
    }

    fn get_auth_url(&self, state: &str, config: &OAuthConfig) -> Result<String, CoreError> {
        let client = BasicClient::new(ClientId::new(config.client_id.clone()))
            .set_client_secret(ClientSecret::new(config.client_secret.clone()))
            .set_auth_uri(AuthUrl::new(Self::AUTH_URL.to_string())?)
            .set_token_uri(TokenUrl::new(Self::TOKEN_URL.to_string())?)
            .set_redirect_uri(RedirectUrl::new(config.redirect_uri.clone())?);

        // Honor realm-configured scopes; fall back to GitHub default if none.
        let scopes: Vec<Scope> = if config.scopes.is_empty() {
            vec![Scope::new("user:email".to_string())]
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

            let user_response = http_client
                .request(Self::authenticated_request(
                    Self::USER_API_URL,
                    access_token,
                ))
                .await?;

            if !user_response.is_success() {
                let status_code = user_response.status_code;
                let response_body = user_response.body_as_string().unwrap_or_default();
                return Err(CoreError::InternalServerError(format!(
                    "Failed to get user info from GitHub: status={}, body={}",
                    status_code, response_body
                )));
            }

            let response_body = user_response.body_as_string()?;
            let github_user: GitHubUser = serde_json::from_str(&response_body).map_err(|e| {
                CoreError::InternalServerError(format!("Failed to parse user info: {}", e))
            })?;

            // Both branches need /user/emails: the /user `email` attribute is
            // the account's PUBLIC profile email — a user-selectable
            // presentation field that carries no verification proof of its
            // own, so the flag must come from the matching /user/emails
            // entry (a fetched list without a verified match fails closed,
            // verified=false); with no profile email the address itself is
            // selected from the list. A FAILED fetch, however, is a loud
            // error — an OAuth app whose scope list omits user:email (GitHub
            // answers a deliberate 404) or a transient failure would
            // otherwise silently downgrade every login to "email not
            // verified" with no trace of the real cause (review 20260919
            // finding 7).
            let emails_response = http_client
                .request(Self::authenticated_request(
                    Self::USER_EMAILS_URL,
                    access_token,
                ))
                .await?;

            if !emails_response.is_success() {
                let status_code = emails_response.status_code;
                let response_body = emails_response.body_as_string().unwrap_or_default();
                return Err(CoreError::InternalServerError(format!(
                    "Failed to get user emails from GitHub: status={}, body={}",
                    status_code, response_body
                )));
            }

            let response_body = emails_response.body_as_string()?;
            let emails: Vec<GitHubEmail> = serde_json::from_str(&response_body).map_err(|e| {
                CoreError::InternalServerError(format!("Failed to parse emails: {}", e))
            })?;

            // Get email if not provided in user info
            let (email, verified) = if let Some(email) = github_user.email {
                let verified = Self::profile_email_verified(&emails, email.as_str());
                (email, verified)
            } else {
                let primary_email = emails.iter().find(|e| e.primary).or_else(|| emails.first());

                match primary_email {
                    Some(email) => (email.email.clone(), email.verified),
                    None => return Err(CoreError::BadRequest("No email found".to_string())),
                }
            };

            Ok(OAuthUserInfo {
                provider_type: ProviderType::GitHub,
                provider_user_id: github_user.id.to_string(),
                email,
                verified,
                avatar: github_user.avatar_url,
                name: github_user.name,
                union_id: None, // GitHub doesn't provide UnionID
                open_id: Some(github_user.id.to_string()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email_entry(address: &str, verified: bool) -> GitHubEmail {
        GitHubEmail {
            email: address.to_string(),
            primary: false,
            verified,
        }
    }

    // Intent (audit run-1: user-api-public-email-hardcoded-verified): the
    // /user profile email is a public, user-selectable field — the verified
    // flag must be derived from the matching /user/emails entry, not asserted
    // from the mere presence of the attribute. find_or_create_user_by_email
    // trusts this flag as the sole gate for logging into an existing account
    // matched by email, so a fabricated true converts a profile-field spoof
    // into account takeover.
    #[test]
    fn github_profile_email_verified_requires_verified_emails_entry() {
        let profile = "public@example.com";

        // Matching entry, provider-verified → true.
        let list = vec![
            email_entry(profile, true),
            email_entry("other@example.com", false),
        ];
        assert!(GitHubOAuthProvider::profile_email_verified(&list, profile));

        // Matching entry exists but is NOT verified → false.
        let list = vec![email_entry(profile, false)];
        assert!(!GitHubOAuthProvider::profile_email_verified(&list, profile));

        // No entry matches the profile email (spoofed/foreign address) → false.
        let list = vec![email_entry("real@example.com", true)];
        assert!(!GitHubOAuthProvider::profile_email_verified(&list, profile));
    }
}
