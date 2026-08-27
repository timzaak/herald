// OAuth entities

use crate::common::CoreError;
use crate::common::entities::Entity;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

/// OAuth provider configuration entity (stored per realm)
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct OAuthProviderConfig {
    pub id: Uuid,
    pub realm_id: String,
    pub provider_type: ProviderType,
    pub client_id: String,
    pub client_secret: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Entity for OAuthProviderConfig {
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

impl OAuthProviderConfig {
    pub fn new(config: CreateOAuthProviderConfigRequest) -> Result<Self, CoreError> {
        // LDAP identity links reuse the `provider` table (type='ldap') but are
        // never created through the OAuth provider configuration surface;
        // refusing here keeps the admin API from minting an OAuth config row
        // that would collide with directory-login links (design D2-2).
        if config.provider_type == ProviderType::Ldap {
            return Err(CoreError::BadRequest(
                "LDAP is not an OAuth provider".to_string(),
            ));
        }

        let now = Utc::now();
        let scopes = config
            .scopes
            .unwrap_or_else(|| default_scopes(&config.provider_type));

        // Validate scopes
        validate_scopes(&config.provider_type, &scopes)?;

        Ok(Self {
            id: crate::common::entities::generate_uuid_v7(),
            realm_id: config.realm_id,
            provider_type: config.provider_type,
            client_id: config.client_id,
            client_secret: config.client_secret,
            scopes,
            enabled: config.enabled.unwrap_or(true),
            created_at: now,
            updated_at: now,
        })
    }
}

fn default_scopes(provider_type: &ProviderType) -> Vec<String> {
    match provider_type {
        ProviderType::Google => vec![
            "openid".to_string(),
            "email".to_string(),
            "profile".to_string(),
        ],
        ProviderType::GitHub => vec!["user:email".to_string()],
        ProviderType::Facebook => vec!["email".to_string()],
        ProviderType::Apple => vec!["name".to_string(), "email".to_string()],
        ProviderType::WeChat => vec!["snsapi_login".to_string()],
        ProviderType::WeChatMiniProgram => vec![],
        // LDAP links carry no OAuth scope concept; the variant exists so the
        // `provider` table round-trips type='ldap' rows (design D2-2).
        ProviderType::Ldap => vec![],
    }
}

fn validate_scopes(provider_type: &ProviderType, scopes: &[String]) -> Result<(), CoreError> {
    match provider_type {
        // WeChat website only accepts snsapi_login
        ProviderType::WeChat => {
            for scope in scopes {
                if scope != "snsapi_login" {
                    return Err(CoreError::BadRequest(format!(
                        "Invalid scope for WeChat: {}. Only 'snsapi_login' is allowed.",
                        scope
                    )));
                }
            }
        }
        // WeChat Mini Program doesn't use scopes
        ProviderType::WeChatMiniProgram if !scopes.is_empty() => {
            return Err(CoreError::BadRequest(
                "WeChat Mini Program does not support scopes.".to_string(),
            ));
        }
        ProviderType::WeChatMiniProgram => {}
        // Unreachable via OAuthProviderConfig::new, which rejects Ldap;
        // listed so the match stays exhaustive as the enum grows.
        ProviderType::Ldap => {}
        // Other providers accept any scope for now
        // Could add more strict validation in the future
        _ => {}
    }
    Ok(())
}

/// OAuth provider entity (user's linked OAuth account)
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct OAuthProvider {
    pub id: Uuid,
    pub realm_id: String,
    pub provider_type: ProviderType,
    pub open_id: String,
    pub union_id: Option<String>,
    pub email: Option<String>,
    pub user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Entity for OAuthProvider {
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

/// OAuth provider type
///
/// The `Ldap` variant is NOT an OAuth provider: it exists so the `provider`
/// identity-link table can round-trip `type='ldap'` rows created by LDAP
/// login (`PostgresOAuthRepository` parses the type column through this
/// enum). `OAuthProviderConfig::new` rejects it, keeping the OAuth
/// configuration surface from producing ldap rows (design D2-2).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    Google,
    GitHub,
    Facebook,
    Apple,
    WeChat,
    WeChatMiniProgram,
    Ldap,
}

impl FromStr for ProviderType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "google" => Ok(ProviderType::Google),
            "github" => Ok(ProviderType::GitHub),
            "facebook" => Ok(ProviderType::Facebook),
            "apple" => Ok(ProviderType::Apple),
            "wechat" => Ok(ProviderType::WeChat),
            "wechat_miniprogram" => Ok(ProviderType::WeChatMiniProgram),
            "ldap" => Ok(ProviderType::Ldap),
            _ => Err(format!("Unknown provider type: {}", s)),
        }
    }
}

impl ProviderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderType::Google => "google",
            ProviderType::GitHub => "github",
            ProviderType::Facebook => "facebook",
            ProviderType::Apple => "apple",
            ProviderType::WeChat => "wechat",
            ProviderType::WeChatMiniProgram => "wechat_miniprogram",
            ProviderType::Ldap => "ldap",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ProviderType::Google => "Google",
            ProviderType::GitHub => "GitHub",
            ProviderType::Facebook => "Facebook",
            ProviderType::Apple => "Apple",
            ProviderType::WeChat => "WeChat",
            ProviderType::WeChatMiniProgram => "WeChat Mini Program",
            ProviderType::Ldap => "LDAP",
        }
    }
}

impl OAuthProvider {
    pub fn new(config: CreateOAuthProviderConfig) -> Self {
        let now = Utc::now();
        Self {
            id: crate::common::entities::generate_uuid_v7(),
            realm_id: config.realm_id,
            provider_type: config.provider_type,
            open_id: config.open_id,
            union_id: config.union_id,
            email: config.email,
            user_id: config.user_id,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateOAuthProviderConfig {
    pub realm_id: String,
    pub provider_type: ProviderType,
    pub open_id: String,
    pub union_id: Option<String>,
    pub email: Option<String>,
    pub user_id: Option<Uuid>,
}

/// Request to create/update OAuth provider configuration
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct CreateOAuthProviderConfigRequest {
    pub realm_id: String,
    pub provider_type: ProviderType,
    #[validate(length(min = 1))]
    pub client_id: String,
    #[validate(length(min = 1))]
    pub client_secret: String,
    pub scopes: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

/// Request to update OAuth provider configuration
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct UpdateOAuthProviderConfigRequest {
    #[validate(length(min = 1))]
    pub client_id: Option<String>,
    #[validate(length(min = 1))]
    pub client_secret: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub enabled: Option<bool>,
}
