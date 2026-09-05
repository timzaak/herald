use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use rand::{RngCore, rngs::OsRng};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use uuid::Uuid;

use crate::redis::RedisConnectionManager;
use herald_domain::authentication::{
    CredentialClass, CredentialScope,
    entities::{
        BrowserAccessTokenData, BrowserRefreshTokenData, BrowserTokenSet, FamilyLifecycle,
        ReauthResult, RefreshError, TargetOperation, UserSessionSummary,
    },
    ports::BrowserTokenService,
};
use herald_domain::client::entities::ClientApp;
use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::user::entities::User;

const BROWSER_ACCESS_TOKEN_TTL_SECONDS: u64 = 900;
pub const REAUTH_TTL_SECONDS: u64 = 120;

const AUTHENTICATION_FUNCTION_LIBRARY: &str = "herald_authentication";
const AUTHENTICATION_FUNCTION_CODE: &str = r#"#!lua name=herald_authentication

local function revoke_family_data(family_key, client_key, user_key)
  local raw_family = redis.call('GET', family_key)
  if not raw_family then return end
  local family = cjson.decode(raw_family)
  for _, digest in ipairs(family.access_digests) do
    redis.call('DEL', 'bt:at:' .. digest)
  end
  for _, digest in ipairs(family.refresh_digests) do
    local key = 'bt:rt:' .. digest
    local raw = redis.call('GET', key)
    if raw then
      local data = cjson.decode(raw)
      data.revoked = true
      redis.call('SET', key, cjson.encode(data), 'KEEPTTL')
    end
  end
  family.revoked = true
  redis.call('SET', family_key, cjson.encode(family), 'KEEPTTL')
  redis.call('SREM', client_key, family.family_id)
  redis.call('SREM', user_key, family.family_id)
  redis.call('DEL', 'bt:meta:' .. family.family_id)
end

local function browser_token_create_family(keys, args)
  redis.call('SETEX', keys[1], args[1], args[2])
  redis.call('SETEX', keys[2], args[3], args[4])
  redis.call('SETEX', keys[3], args[3], args[5])
  redis.call('SADD', keys[4], args[6])
  redis.call('SADD', keys[5], args[6])
  return 'OK'
end

local function browser_token_refresh(keys, args)
  local raw = redis.call('GET', keys[1])
  if not raw then return 'INVALID' end
  local current = cjson.decode(raw)

  local raw_family = redis.call('GET', keys[2])
  if not raw_family then return 'INVALID' end
  local family = cjson.decode(raw_family)
  if current.successor_digest ~= cjson.null then
    revoke_family_data(keys[2], keys[5], keys[6])
    return 'REUSE'
  end
  if current.revoked or family.revoked or tonumber(args[1]) >= family.absolute_expires_at_ts then
    return 'INVALID'
  end

  current.successor_digest = args[2]
  current.revoked = true
  if redis.call('TTL', keys[1]) <= 0 or redis.call('TTL', keys[2]) <= 0 then
    return 'INVALID'
  end
  redis.call('SET', keys[1], cjson.encode(current), 'KEEPTTL')
  redis.call('SETEX', keys[3], args[3], args[4])
  redis.call('SETEX', keys[4], args[5], args[6])
  table.insert(family.access_digests, args[7])
  table.insert(family.refresh_digests, args[2])
  redis.call('SET', keys[2], cjson.encode(family), 'KEEPTTL')
  return 'OK'
end

local function browser_token_revoke_family(keys, args)
  revoke_family_data(keys[1], keys[2], keys[3])
  return 'OK'
end

local function reauth_consume(keys, args)
local raw = redis.call('GET', keys[1])
if not raw then return 'INVALID' end
local ttl = redis.call('PTTL', keys[1])
if ttl <= 0 then return 'INVALID' end
local result = cjson.decode(raw)
if result.realm_id ~= args[1]
    or result.client_app_id ~= args[2]
    or result.user_id ~= args[3] then
  return 'INVALID'
end
if result.consumed then return 'CONSUMED' end
if result.target_operation ~= args[4] then return 'TARGET_MISMATCH' end
result.consumed = true
redis.call('SET', keys[1], cjson.encode(result), 'KEEPTTL')
return 'OK'
end

redis.register_function('browser_token_create_family', browser_token_create_family)
redis.register_function('browser_token_refresh', browser_token_refresh)
redis.register_function('browser_token_revoke_family', browser_token_revoke_family)
redis.register_function('reauth_consume', reauth_consume)
"#;

pub async fn init_authentication_functions(
    redis: &RedisConnectionManager,
) -> Result<(), CoreError> {
    let mut connection = redis.get().await.map_err(|error| {
        CoreError::DatabaseError(format!("Failed to get Redis connection: {error}"))
    })?;
    redis::cmd("FUNCTION")
        .arg("LOAD")
        .arg("REPLACE")
        .arg(AUTHENTICATION_FUNCTION_CODE)
        .query_async::<String>(&mut connection)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to load authentication Redis Function library");
            CoreError::DatabaseError(format!(
                "Failed to load authentication Redis Function library: {error}"
            ))
        })?;
    tracing::info!(
        "Redis Function library '{}' loaded successfully",
        AUTHENTICATION_FUNCTION_LIBRARY
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReauthConsumeError {
    Invalid,
    Consumed,
    TargetMismatch,
}

pub struct RedisReauthStore {
    manager: RedisConnectionManager,
}

impl RedisReauthStore {
    pub fn new(manager: RedisConnectionManager) -> Self {
        Self { manager }
    }

    fn token_digest(token: &str) -> String {
        Sha256::digest(token.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn key(token: &str) -> String {
        format!("reauth:{}", Self::token_digest(token))
    }

    pub async fn issue(&self, result: ReauthResult) -> Result<String, CoreError> {
        self.issue_with_ttl(result, REAUTH_TTL_SECONDS).await
    }

    /// Issue a reauth ticket with an explicit Redis TTL.
    ///
    /// Production callers always pair `expires_at` with the same TTL via
    /// `issue`. Exposing a TTL override lets test fixtures plant a ticket that
    /// is already past its Redis expiry window without lying about the
    /// business `expires_at` (which the Lua consume path does not read).
    pub async fn issue_with_ttl(
        &self,
        result: ReauthResult,
        ttl_seconds: u64,
    ) -> Result<String, CoreError> {
        let mut bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let token = URL_SAFE_NO_PAD.encode(bytes);
        let mut connection = self.manager.get().await.map_err(|error| {
            CoreError::InternalServerError(format!("Redis connection error: {error}"))
        })?;
        let _: () = connection
            .set_ex(
                Self::key(&token),
                serde_json::to_string(&result)?,
                ttl_seconds,
            )
            .await?;
        Ok(token)
    }

    pub async fn consume(
        &self,
        token: &str,
        realm_id: &str,
        client_app_id: Uuid,
        user_id: &str,
        target: TargetOperation,
    ) -> Result<Result<(), ReauthConsumeError>, CoreError> {
        let mut connection = self.manager.get().await.map_err(|error| {
            CoreError::InternalServerError(format!("Redis connection error: {error}"))
        })?;
        let result: String = redis::cmd("FCALL")
            .arg("reauth_consume")
            .arg(1)
            .arg(Self::key(token))
            .arg(realm_id)
            .arg(client_app_id.to_string())
            .arg(user_id)
            .arg(target.as_str())
            .query_async(&mut connection)
            .await?;
        Ok(match result.as_str() {
            "OK" => Ok(()),
            "CONSUMED" => Err(ReauthConsumeError::Consumed),
            "TARGET_MISMATCH" => Err(ReauthConsumeError::TargetMismatch),
            _ => Err(ReauthConsumeError::Invalid),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct BrowserTokenFamilyData {
    family_id: Uuid,
    realm_id: String,
    client_app_id: Uuid,
    user_id: String,
    credential_class: CredentialClass,
    allowed_scopes: HashSet<CredentialScope>,
    absolute_expires_at_ts: i64,
    access_digests: Vec<String>,
    refresh_digests: Vec<String>,
    revoked: bool,
}

// Session metadata index payload, stored at `bt:meta:{familyId}`. Written at
// login independently of the family record so that session listing (admin) and
// token verification (hot path) stay decoupled.
// Carries only the listing-only display fields the family record does not.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BrowserTokenFamilyMeta {
    client_app_name: Option<String>,
    user_agent: Option<String>,
    client_ip: Option<String>,
    created_at: i64,
}

pub struct RedisBrowserTokenService {
    manager: RedisConnectionManager,
}

impl RedisBrowserTokenService {
    pub fn new(manager: RedisConnectionManager) -> Self {
        Self { manager }
    }

    async fn get_connection(&self) -> Result<redis::aio::ConnectionManager, CoreError> {
        self.manager.get().await.map_err(|error| {
            tracing::error!(%error, "Failed to get Redis connection for browser tokens");
            CoreError::InternalServerError(format!("Redis connection error: {error}"))
        })
    }

    fn token_key(prefix: &str, digest: &str) -> String {
        format!("bt:{prefix}:{digest}")
    }

    fn family_key(family_id: Uuid) -> String {
        format!("bt:fam:{family_id}")
    }

    fn family_meta_key(family_id: Uuid) -> String {
        format!("bt:meta:{family_id}")
    }

    fn client_families_key(client_app_id: Uuid) -> String {
        format!("bt:client_fams:{client_app_id}")
    }

    fn user_families_key(user_id: &str) -> String {
        format!("bt:user_fams:{user_id}")
    }

    fn generate_token() -> String {
        let mut bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut bytes);
        URL_SAFE_NO_PAD.encode(bytes)
    }

    fn token_digest(token: &str) -> String {
        Sha256::digest(token.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn custom_user_ui_scopes() -> HashSet<CredentialScope> {
        use CredentialScope::*;
        [
            FeatureRead,
            ProfileRead,
            ProfileWriteNickname,
            ChangePassword,
            DeleteAccount,
            TotpManage,
            PasskeyManage,
            Logout,
            PointsRead,
            PointsTransactionsRead,
            PurchaseRead,
            PurchaseInitiate,
            PurchaseStatusRead,
            InvoiceRead,
            InvoiceApply,
            SubscriptionRead,
            SubscriptionCancel,
        ]
        .into_iter()
        .collect()
    }

    fn first_party_scopes() -> HashSet<CredentialScope> {
        use CredentialScope::*;
        [
            FeatureRead,
            ProfileRead,
            ProfileWriteNickname,
            ChangePassword,
            ChangeEmail,
            DeleteAccount,
            TotpManage,
            PasskeyManage,
            Logout,
            PointsRead,
            PointsTransactionsRead,
            PurchaseRead,
            PurchaseInitiate,
            PurchaseStatusRead,
            InvoiceRead,
            InvoiceApply,
            SubscriptionRead,
            SubscriptionCancel,
        ]
        .into_iter()
        .collect()
    }

    /// Issue a browser family that can only inspect reauthentication factors,
    /// delete the account, or log out. This is used after valid credentials
    /// when legal consent blocks a normal session: refusal must not grant
    /// application access, but it must not strand the user's deletion right.
    pub async fn create_consent_restricted_token_family(
        &self,
        user: &User,
        client_app: &ClientApp,
        user_agent: Option<String>,
        client_ip: Option<String>,
    ) -> Result<BrowserTokenSet, CoreError> {
        use CredentialScope::*;
        self.create_family(
            user.realm_id.clone(),
            user.id.to_string(),
            client_app.id,
            CredentialClass::CustomUserUi,
            [ProfileRead, DeleteAccount, Logout].into_iter().collect(),
            client_app.browser_refresh_absolute_ttl_seconds as u64,
            Some(client_app.name.clone()),
            user_agent,
            client_ip,
        )
        .await
    }

    async fn create_family(
        &self,
        realm_id: String,
        user_id: String,
        client_app_id: Uuid,
        credential_class: CredentialClass,
        allowed_scopes: HashSet<CredentialScope>,
        refresh_absolute_ttl_seconds: u64,
        client_app_name: Option<String>,
        user_agent: Option<String>,
        client_ip: Option<String>,
    ) -> Result<BrowserTokenSet, CoreError> {
        let now = Utc::now();
        let family_id = Uuid::now_v7();
        let absolute_expires_at = now + Duration::seconds(refresh_absolute_ttl_seconds as i64);
        let access_expires_at = now + Duration::seconds(BROWSER_ACCESS_TOKEN_TTL_SECONDS as i64);
        let access_token = Self::generate_token();
        let refresh_token = Self::generate_token();
        let access_digest = Self::token_digest(&access_token);
        let refresh_digest = Self::token_digest(&refresh_token);

        let access_data = BrowserAccessTokenData {
            realm_id: realm_id.clone(),
            client_app_id,
            user_id: user_id.clone(),
            family_id,
            credential_class,
            allowed_scopes: allowed_scopes.clone(),
            expires_at: access_expires_at,
        };
        let refresh_data = BrowserRefreshTokenData {
            realm_id: realm_id.clone(),
            client_app_id,
            user_id: user_id.clone(),
            family_id,
            successor_digest: None,
            expires_at: absolute_expires_at,
            absolute_expires_at,
            revoked: false,
        };
        let family_data = BrowserTokenFamilyData {
            family_id,
            realm_id,
            client_app_id,
            user_id: user_id.clone(),
            credential_class,
            allowed_scopes,
            absolute_expires_at_ts: absolute_expires_at.timestamp(),
            access_digests: vec![access_digest.clone()],
            refresh_digests: vec![refresh_digest.clone()],
            revoked: false,
        };
        let meta = BrowserTokenFamilyMeta {
            client_app_name,
            user_agent,
            client_ip,
            created_at: now.timestamp(),
        };

        let mut connection = self.get_connection().await?;
        let result: String = redis::cmd("FCALL")
            .arg("browser_token_create_family")
            .arg(5)
            .arg(Self::token_key("at", &access_digest))
            .arg(Self::token_key("rt", &refresh_digest))
            .arg(Self::family_key(family_id))
            .arg(Self::client_families_key(client_app_id))
            .arg(Self::user_families_key(&user_id))
            .arg(BROWSER_ACCESS_TOKEN_TTL_SECONDS)
            .arg(serde_json::to_string(&access_data)?)
            .arg(refresh_absolute_ttl_seconds)
            .arg(serde_json::to_string(&refresh_data)?)
            .arg(serde_json::to_string(&family_data)?)
            .arg(family_id.to_string())
            .query_async(&mut connection)
            .await?;
        if result != "OK" {
            return Err(CoreError::InternalServerError(
                "Browser token family creation failed".to_string(),
            ));
        }

        // Session metadata index. Stored
        // independently of the family record so the token-verification hot path
        // stays untouched. Best-effort: the family record is already committed
        // (FCALL returned OK) and is authoritative for auth; meta is auxiliary
        // for session listing. A failure here must not break login (which would
        // orphan the just-created family on caller retry) — listing tolerates
        // missing meta (legacy families → None fields).
        if let Err(error) = connection
            .set_ex::<_, _, ()>(
                Self::family_meta_key(family_id),
                serde_json::to_string(&meta)?,
                refresh_absolute_ttl_seconds,
            )
            .await
        {
            tracing::warn!(
                %error,
                family_id = %family_id,
                "Failed to write browser token family session metadata; \
                 login proceeds with missing meta"
            );
        }

        Ok(BrowserTokenSet {
            access_token,
            refresh_token,
            expires_in: BROWSER_ACCESS_TOKEN_TTL_SECONDS,
            refresh_expires_in: refresh_absolute_ttl_seconds,
            token_type: "Bearer".to_string(),
        })
    }

    async fn refresh_inner(&self, refresh_token: &str) -> Result<BrowserTokenSet, RefreshError> {
        let old_digest = Self::token_digest(refresh_token);
        let old_key = Self::token_key("rt", &old_digest);
        let mut connection = self.get_connection().await.map_err(|error| {
            tracing::error!(%error, "Browser token refresh could not connect to Redis");
            RefreshError::Invalid
        })?;
        let raw: Option<String> = connection.get(&old_key).await.map_err(|error| {
            tracing::error!(%error, "Browser refresh token lookup failed");
            RefreshError::Invalid
        })?;
        let current: BrowserRefreshTokenData = serde_json::from_str(
            raw.as_deref().ok_or(RefreshError::Invalid)?,
        )
        .map_err(|error| {
            tracing::error!(%error, "Stored browser refresh token is invalid");
            RefreshError::Invalid
        })?;
        let family_raw: Option<String> = connection
            .get(Self::family_key(current.family_id))
            .await
            .map_err(|error| {
                tracing::error!(%error, "Browser token family lookup failed");
                RefreshError::Invalid
            })?;
        let family: BrowserTokenFamilyData = serde_json::from_str(
            family_raw.as_deref().ok_or(RefreshError::Invalid)?,
        )
        .map_err(|error| {
            tracing::error!(%error, "Stored browser token family is invalid");
            RefreshError::Invalid
        })?;

        let now = Utc::now();
        let remaining = (current.absolute_expires_at - now).num_seconds();
        if remaining <= 0 {
            return Err(RefreshError::Invalid);
        }
        let access_ttl = BROWSER_ACCESS_TOKEN_TTL_SECONDS.min(remaining as u64);
        let access_token = Self::generate_token();
        let next_refresh_token = Self::generate_token();
        let access_digest = Self::token_digest(&access_token);
        let refresh_digest = Self::token_digest(&next_refresh_token);
        let access_data = BrowserAccessTokenData {
            realm_id: current.realm_id.clone(),
            client_app_id: current.client_app_id,
            user_id: current.user_id.clone(),
            family_id: current.family_id,
            credential_class: family.credential_class,
            allowed_scopes: family.allowed_scopes,
            expires_at: now + Duration::seconds(access_ttl as i64),
        };
        let next_refresh_data = BrowserRefreshTokenData {
            successor_digest: None,
            expires_at: current.absolute_expires_at,
            revoked: false,
            ..current.clone()
        };

        let result: String = redis::cmd("FCALL")
            .arg("browser_token_refresh")
            .arg(6)
            .arg(&old_key)
            .arg(Self::family_key(current.family_id))
            .arg(Self::token_key("at", &access_digest))
            .arg(Self::token_key("rt", &refresh_digest))
            .arg(Self::client_families_key(current.client_app_id))
            .arg(Self::user_families_key(&current.user_id))
            .arg(now.timestamp())
            .arg(&refresh_digest)
            .arg(access_ttl)
            .arg(serde_json::to_string(&access_data).map_err(|_| RefreshError::Invalid)?)
            .arg(remaining as u64)
            .arg(serde_json::to_string(&next_refresh_data).map_err(|_| RefreshError::Invalid)?)
            .arg(&access_digest)
            .query_async(&mut connection)
            .await
            .map_err(|error| {
                tracing::error!(%error, "Browser token rotation function failed");
                RefreshError::Invalid
            })?;

        match result.as_str() {
            "OK" => Ok(BrowserTokenSet {
                access_token,
                refresh_token: next_refresh_token,
                expires_in: access_ttl,
                refresh_expires_in: remaining as u64,
                token_type: "Bearer".to_string(),
            }),
            "REUSE" => Err(RefreshError::ReuseDetected),
            _ => Err(RefreshError::Invalid),
        }
    }

    /// Remove family IDs from a set whose family key has expired or been deleted.
    /// Returns the IDs that still exist and should be revoked.
    async fn prune_stale_family_ids(&self, set_key: &str) -> Result<Vec<Uuid>, CoreError> {
        let mut connection = self.get_connection().await?;
        let family_ids: Vec<String> = connection.smembers(set_key).await?;
        let mut valid_ids = Vec::with_capacity(family_ids.len());
        for family_id in family_ids {
            let parsed = Uuid::parse_str(&family_id).map_err(|e| {
                CoreError::InternalServerError(format!("Invalid stored token family id: {e}"))
            })?;
            let family_key = Self::family_key(parsed);
            let exists: bool = connection.exists(&family_key).await?;
            if exists {
                valid_ids.push(parsed);
            } else {
                let _: usize = connection.srem(set_key, &family_id).await?;
            }
        }
        Ok(valid_ids)
    }
}

impl BrowserTokenService for RedisBrowserTokenService {
    async fn lookup_access_token(
        &self,
        access_token: &str,
    ) -> Result<Option<BrowserAccessTokenData>, CoreError> {
        let digest = Self::token_digest(access_token);
        let mut connection = self.get_connection().await?;
        let raw: Option<String> = connection.get(Self::token_key("at", &digest)).await?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        let data: BrowserAccessTokenData = serde_json::from_str(&raw)?;
        if data.expires_at <= Utc::now() {
            return Ok(None);
        }
        Ok(Some(data))
    }

    async fn create_token_family(
        &self,
        user: &User,
        client_app: &ClientApp,
        user_agent: Option<String>,
        client_ip: Option<String>,
    ) -> Result<BrowserTokenSet, CoreError> {
        self.create_family(
            user.realm_id.clone(),
            user.id.to_string(),
            client_app.id,
            CredentialClass::CustomUserUi,
            Self::custom_user_ui_scopes(),
            client_app.browser_refresh_absolute_ttl_seconds as u64,
            Some(client_app.name.clone()),
            user_agent,
            client_ip,
        )
        .await
    }

    async fn create_first_party_token_family(
        &self,
        user: &User,
        client_app: &ClientApp,
        user_agent: Option<String>,
        client_ip: Option<String>,
    ) -> Result<BrowserTokenSet, CoreError> {
        if !client_app.is_first_party {
            return Err(CoreError::Forbidden(
                "First-party token requires a first-party Client App".to_string(),
            ));
        }
        self.create_family(
            user.realm_id.clone(),
            user.id.to_string(),
            client_app.id,
            CredentialClass::FirstParty,
            Self::first_party_scopes(),
            client_app.browser_refresh_absolute_ttl_seconds as u64,
            Some(client_app.name.clone()),
            user_agent,
            client_ip,
        )
        .await
    }

    async fn refresh(&self, refresh_token: &str) -> Result<BrowserTokenSet, RefreshError> {
        self.refresh_inner(refresh_token).await
    }

    async fn revoke_family(&self, family_id: Uuid) -> Result<(), CoreError> {
        let mut connection = self.get_connection().await?;
        let family_key = Self::family_key(family_id);
        let raw: Option<String> = connection.get(&family_key).await?;
        let Some(raw) = raw else {
            return Ok(());
        };
        let family: BrowserTokenFamilyData = serde_json::from_str(&raw)?;
        let result: String = redis::cmd("FCALL")
            .arg("browser_token_revoke_family")
            .arg(3)
            .arg(family_key)
            .arg(Self::client_families_key(family.client_app_id))
            .arg(Self::user_families_key(&family.user_id))
            .query_async(&mut connection)
            .await?;
        if result == "OK" {
            Ok(())
        } else {
            Err(CoreError::InternalServerError(
                "Browser token family revocation failed".to_string(),
            ))
        }
    }

    async fn revoke_client_families(&self, client_app_id: Uuid) -> Result<(), CoreError> {
        let set_key = Self::client_families_key(client_app_id);
        let family_ids = self.prune_stale_family_ids(&set_key).await?;
        for family_id in family_ids {
            self.revoke_family(family_id).await?;
        }
        Ok(())
    }

    async fn revoke_user_families(&self, user_id: &str) -> Result<(), CoreError> {
        let set_key = Self::user_families_key(user_id);
        let family_ids = self.prune_stale_family_ids(&set_key).await?;
        for family_id in family_ids {
            self.revoke_family(family_id).await?;
        }
        Ok(())
    }

    async fn list_user_sessions(
        &self,
        user_id: &str,
    ) -> Result<Vec<UserSessionSummary>, CoreError> {
        let mut connection = self.get_connection().await?;
        let set_key = Self::user_families_key(user_id);
        let family_ids: Vec<String> = connection.smembers(&set_key).await?;
        let now_ts = Utc::now().timestamp();

        let mut summaries = Vec::with_capacity(family_ids.len());
        for family_id_str in family_ids {
            let family_id = match Uuid::parse_str(&family_id_str) {
                Ok(id) => id,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        raw = %family_id_str,
                        "Invalid stored token family id in user families set; skipping"
                    );
                    let _: usize = connection.srem(&set_key, &family_id_str).await?;
                    continue;
                }
            };

            let family_key = Self::family_key(family_id);
            let raw: Option<String> = connection.get(&family_key).await?;
            let Some(raw) = raw else {
                // Stale entry (family key expired/deleted). Prune and skip.
                let _: usize = connection.srem(&set_key, family_id_str.as_str()).await?;
                continue;
            };
            let family: BrowserTokenFamilyData = serde_json::from_str(&raw)?;
            if family.revoked || family.absolute_expires_at_ts <= now_ts {
                // Filter revoked / expired; opportunistically clean stale set entry.
                let _: usize = connection.srem(&set_key, family_id_str.as_str()).await?;
                continue;
            }

            // meta may be absent for legacy families (pre-index) or if the
            // best-effort write at login failed. Surface None for those fields.
            let meta_raw: Option<String> = connection.get(Self::family_meta_key(family_id)).await?;
            let meta: Option<BrowserTokenFamilyMeta> = match meta_raw {
                Some(json) => serde_json::from_str(&json).map_err(|error| {
                    CoreError::InternalServerError(format!(
                        "Invalid session metadata for family {family_id}: {error}"
                    ))
                })?,
                None => None,
            };

            let absolute_expires_at =
                DateTime::<Utc>::from_timestamp(family.absolute_expires_at_ts, 0).ok_or_else(
                    || {
                        CoreError::InternalServerError(format!(
                            "Invalid absolute_expires_at_ts for family {family_id}"
                        ))
                    },
                )?;

            summaries.push(UserSessionSummary {
                family_id,
                realm_id: family.realm_id,
                client_app_id: family.client_app_id,
                client_app_name: meta.as_ref().and_then(|m| m.client_app_name.clone()),
                credential_class: family.credential_class,
                user_agent: meta.as_ref().and_then(|m| m.user_agent.clone()),
                client_ip: meta.as_ref().and_then(|m| m.client_ip.clone()),
                created_at: meta
                    .as_ref()
                    .and_then(|m| DateTime::<Utc>::from_timestamp(m.created_at, 0)),
                absolute_expires_at,
            });
        }
        Ok(summaries)
    }

    /// Read a single family's ownership + lifecycle status directly from
    /// `bt:fam:{familyId}`, without the active-only filtering applied by
    /// `list_user_sessions`. Returns `Ok(None)` when the family record is
    /// absent (caller returns 404). `expired` is computed at read time.
    async fn get_family_lifecycle(
        &self,
        family_id: Uuid,
    ) -> Result<Option<FamilyLifecycle>, CoreError> {
        let mut connection = self.get_connection().await?;
        let raw: Option<String> = connection.get(Self::family_key(family_id)).await?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        let family: BrowserTokenFamilyData = serde_json::from_str(&raw)?;
        let now_ts = Utc::now().timestamp();
        Ok(Some(FamilyLifecycle {
            user_id: family.user_id,
            realm_id: family.realm_id,
            revoked: family.revoked,
            expired: family.absolute_expires_at_ts <= now_ts,
        }))
    }
}

#[cfg(test)]
mod browser_token_tests {
    use super::*;
    use crate::redis::ManagerConfig;

    async fn browser_token_service() -> RedisBrowserTokenService {
        let manager = RedisConnectionManager::new(ManagerConfig {
            url: std::env::var("TEST_REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6382/0".to_string()),
            default_db: 0,
            test_mode: true,
            test_db: 13,
        })
        .await
        .expect("browser token tests require Redis");
        init_authentication_functions(&manager)
            .await
            .expect("authentication functions should load");
        RedisBrowserTokenService::new(manager)
    }

    async fn reauth_store() -> RedisReauthStore {
        let manager = RedisConnectionManager::new(ManagerConfig {
            url: std::env::var("TEST_REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6382/0".to_string()),
            default_db: 0,
            test_mode: true,
            test_db: 13,
        })
        .await
        .expect("reauth tests require Redis");
        init_authentication_functions(&manager)
            .await
            .expect("authentication functions should load");
        RedisReauthStore::new(manager)
    }

    fn reauth_result(client_app_id: Uuid, target: TargetOperation) -> ReauthResult {
        ReauthResult {
            realm_id: "reauth-realm".to_string(),
            client_app_id,
            user_id: "reauth-user".to_string(),
            target_operation: target,
            expires_at: Utc::now() + Duration::seconds(REAUTH_TTL_SECONDS as i64),
            consumed: false,
        }
    }

    #[tokio::test]
    async fn reauth_token_is_consumed_once() {
        let store = reauth_store().await;
        let client_app_id = Uuid::now_v7();
        let token = store
            .issue(reauth_result(
                client_app_id,
                TargetOperation::ChangePassword,
            ))
            .await
            .unwrap();

        assert_eq!(
            store
                .consume(
                    &token,
                    "reauth-realm",
                    client_app_id,
                    "reauth-user",
                    TargetOperation::ChangePassword,
                )
                .await
                .unwrap(),
            Ok(())
        );
        assert_eq!(
            store
                .consume(
                    &token,
                    "reauth-realm",
                    client_app_id,
                    "reauth-user",
                    TargetOperation::ChangePassword,
                )
                .await
                .unwrap(),
            Err(ReauthConsumeError::Consumed)
        );
    }

    #[tokio::test]
    async fn reauth_target_mismatch_does_not_consume_token() {
        let store = reauth_store().await;
        let client_app_id = Uuid::now_v7();
        let token = store
            .issue(reauth_result(client_app_id, TargetOperation::DeleteAccount))
            .await
            .unwrap();

        assert_eq!(
            store
                .consume(
                    &token,
                    "reauth-realm",
                    client_app_id,
                    "reauth-user",
                    TargetOperation::ChangePassword,
                )
                .await
                .unwrap(),
            Err(ReauthConsumeError::TargetMismatch)
        );
        assert!(
            store
                .consume(
                    &token,
                    "reauth-realm",
                    client_app_id,
                    "reauth-user",
                    TargetOperation::DeleteAccount,
                )
                .await
                .unwrap()
                .is_ok(),
            "a target mismatch must not burn a valid token"
        );
    }

    #[tokio::test]
    async fn reauth_expired_token_is_invalid() {
        let store = reauth_store().await;
        let client_app_id = Uuid::now_v7();
        let token = store
            .issue_with_ttl(
                reauth_result(client_app_id, TargetOperation::DeleteAccount),
                1,
            )
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;

        assert_eq!(
            store
                .consume(
                    &token,
                    "reauth-realm",
                    client_app_id,
                    "reauth-user",
                    TargetOperation::DeleteAccount,
                )
                .await
                .unwrap(),
            Err(ReauthConsumeError::Invalid)
        );
    }

    async fn create_test_family(
        service: &RedisBrowserTokenService,
        client_app_id: Uuid,
        absolute_ttl_seconds: u64,
    ) -> BrowserTokenSet {
        service
            .create_family(
                format!("browser-token-realm-{}", Uuid::now_v7()),
                Uuid::now_v7().to_string(),
                client_app_id,
                CredentialClass::CustomUserUi,
                RedisBrowserTokenService::custom_user_ui_scopes(),
                absolute_ttl_seconds,
                Some("Test Client App".to_string()),
                Some("test-user-agent/1.0".to_string()),
                Some("203.0.113.7".to_string()),
            )
            .await
            .expect("test token family should be created")
    }

    #[tokio::test]
    async fn browser_token_access_lookup_stops_after_family_revocation() {
        let service = browser_token_service().await;
        let client_app_id = Uuid::now_v7();
        let tokens = create_test_family(&service, client_app_id, 60).await;

        let access = service
            .lookup_access_token(&tokens.access_token)
            .await
            .expect("access lookup should succeed")
            .expect("newly issued access token should exist");
        assert_eq!(access.client_app_id, client_app_id);
        assert_eq!(access.credential_class, CredentialClass::CustomUserUi);

        service
            .revoke_family(access.family_id)
            .await
            .expect("family revocation should succeed");
        assert!(
            service
                .lookup_access_token(&tokens.access_token)
                .await
                .expect("revoked access lookup should succeed")
                .is_none(),
            "family revocation must invalidate every access token"
        );
    }

    #[tokio::test]
    async fn browser_token_first_party_family_contains_full_scope() {
        let service = browser_token_service().await;
        let tokens = service
            .create_family(
                format!("first-party-realm-{}", Uuid::now_v7()),
                Uuid::now_v7().to_string(),
                Uuid::now_v7(),
                CredentialClass::FirstParty,
                RedisBrowserTokenService::first_party_scopes(),
                60,
                Some("First-Party App".to_string()),
                Some("test-user-agent/1.0".to_string()),
                Some("203.0.113.7".to_string()),
            )
            .await
            .expect("first-party token family should be created");
        let access = service
            .lookup_access_token(&tokens.access_token)
            .await
            .expect("first-party access lookup should succeed")
            .expect("first-party access token should exist");
        assert_eq!(access.credential_class, CredentialClass::FirstParty);
        assert_eq!(
            access.allowed_scopes,
            RedisBrowserTokenService::first_party_scopes(),
            "Herald's first-party token must retain the complete browser capability set"
        );
    }

    #[tokio::test]
    async fn browser_token_refresh_rotates_refresh_token() {
        let service = browser_token_service().await;
        let client_app_id = Uuid::now_v7();
        let initial = create_test_family(&service, client_app_id, 60).await;

        let rotated = service
            .refresh(&initial.refresh_token)
            .await
            .expect("current refresh token should rotate");
        let rotated_again = service
            .refresh(&rotated.refresh_token)
            .await
            .expect("successor refresh token should rotate");

        assert_ne!(initial.access_token, rotated.access_token);
        assert_ne!(initial.refresh_token, rotated.refresh_token);
        assert_ne!(rotated.refresh_token, rotated_again.refresh_token);
        assert_ne!(rotated.access_token, rotated_again.access_token);
    }

    #[tokio::test]
    async fn browser_token_concurrent_refresh_allows_only_one_rotation() {
        let service = browser_token_service().await;
        let client_app_id = Uuid::now_v7();
        let initial = create_test_family(&service, client_app_id, 60).await;

        let (first, second) = tokio::join!(
            service.refresh(&initial.refresh_token),
            service.refresh(&initial.refresh_token)
        );

        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        assert!(matches!(
            (first, second),
            (Ok(_), Err(RefreshError::ReuseDetected)) | (Err(RefreshError::ReuseDetected), Ok(_))
        ));
    }

    #[tokio::test]
    async fn browser_token_reuse_revokes_the_successor_family() {
        let service = browser_token_service().await;
        let client_app_id = Uuid::now_v7();
        let initial = create_test_family(&service, client_app_id, 60).await;
        let rotated = service
            .refresh(&initial.refresh_token)
            .await
            .expect("first use should rotate");

        assert_eq!(
            service.refresh(&initial.refresh_token).await,
            Err(RefreshError::ReuseDetected)
        );
        assert_eq!(
            service.refresh(&rotated.refresh_token).await,
            Err(RefreshError::Invalid)
        );
    }

    #[tokio::test]
    async fn browser_token_absolute_ttl_rejects_refresh() {
        let service = browser_token_service().await;
        let client_app_id = Uuid::now_v7();
        let initial = create_test_family(&service, client_app_id, 1).await;

        tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;

        assert_eq!(
            service.refresh(&initial.refresh_token).await,
            Err(RefreshError::Invalid)
        );
    }

    // Helper: create a family bound to a known user_id so list_user_sessions can
    // enumerate it. Returns the access-token lookup result (to get family_id).
    async fn create_test_family_for_user(
        service: &RedisBrowserTokenService,
        realm_id: String,
        user_id: String,
        client_app_id: Uuid,
        ttl_seconds: u64,
        user_agent: Option<String>,
        client_ip: Option<String>,
    ) -> Uuid {
        let tokens = service
            .create_family(
                realm_id,
                user_id,
                client_app_id,
                CredentialClass::CustomUserUi,
                RedisBrowserTokenService::custom_user_ui_scopes(),
                ttl_seconds,
                Some("Test Client App".to_string()),
                user_agent,
                client_ip,
            )
            .await
            .expect("test token family should be created");
        service
            .lookup_access_token(&tokens.access_token)
            .await
            .expect("access lookup should succeed")
            .expect("access token should exist")
            .family_id
    }

    #[tokio::test]
    async fn list_user_sessions_returns_active_with_meta() {
        let service = browser_token_service().await;
        let client_app_id = Uuid::now_v7();
        let user_id = format!("list-user-{}", Uuid::now_v7());
        let family_id = create_test_family_for_user(
            &service,
            "list-realm".to_string(),
            user_id.clone(),
            client_app_id,
            60,
            Some("Mozilla/5.0 UA".to_string()),
            Some("198.51.100.4".to_string()),
        )
        .await;

        let sessions = service
            .list_user_sessions(&user_id)
            .await
            .expect("list should succeed");
        assert_eq!(sessions.len(), 1, "exactly one active session expected");
        let session = &sessions[0];
        assert_eq!(session.family_id, family_id);
        assert_eq!(session.realm_id, "list-realm");
        assert_eq!(session.client_app_id, client_app_id);
        assert_eq!(
            session.client_app_name.as_deref(),
            Some("Test Client App"),
            "meta-driven client_app_name must be populated"
        );
        assert_eq!(session.user_agent.as_deref(), Some("Mozilla/5.0 UA"));
        assert_eq!(session.client_ip.as_deref(), Some("198.51.100.4"));
        assert!(
            session.created_at.is_some(),
            "meta-driven created_at must be populated"
        );
    }

    #[tokio::test]
    async fn list_user_sessions_filters_revoked_and_expired() {
        let service = browser_token_service().await;
        let client_app_id = Uuid::now_v7();
        let user_id = format!("list-filter-user-{}", Uuid::now_v7());

        // Family A: stays active.
        let _active = create_test_family_for_user(
            &service,
            "filter-realm".to_string(),
            user_id.clone(),
            client_app_id,
            60,
            None,
            None,
        )
        .await;
        // Family B: gets revoked.
        let revoked_family = create_test_family_for_user(
            &service,
            "filter-realm".to_string(),
            user_id.clone(),
            client_app_id,
            60,
            None,
            None,
        )
        .await;
        // Family C: short TTL so it expires.
        let _expired = create_test_family_for_user(
            &service,
            "filter-realm".to_string(),
            user_id.clone(),
            client_app_id,
            1,
            None,
            None,
        )
        .await;

        service
            .revoke_family(revoked_family)
            .await
            .expect("revocation should succeed");

        // Let the short-TTL family expire.
        tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;

        let sessions = service
            .list_user_sessions(&user_id)
            .await
            .expect("list should succeed");
        assert_eq!(
            sessions.len(),
            1,
            "only the non-revoked, non-expired family should remain"
        );
        assert_ne!(sessions[0].family_id, revoked_family);
    }

    #[tokio::test]
    async fn list_user_sessions_handles_missing_meta_for_legacy_families() {
        let service = browser_token_service().await;
        let client_app_id = Uuid::now_v7();
        let user_id = format!("legacy-user-{}", Uuid::now_v7());

        // Create a real family, then delete its meta to simulate a legacy
        // session created before the metadata index existed.
        let family_id = create_test_family_for_user(
            &service,
            "legacy-realm".to_string(),
            user_id.clone(),
            client_app_id,
            60,
            Some("UA".to_string()),
            None,
        )
        .await;
        {
            let mut conn = service
                .manager
                .get()
                .await
                .expect("connection should be available");
            let _: () = conn
                .del(RedisBrowserTokenService::family_meta_key(family_id))
                .await
                .expect("meta delete should succeed");
        }

        let sessions = service
            .list_user_sessions(&user_id)
            .await
            .expect("list should succeed");
        assert_eq!(sessions.len(), 1, "legacy family still listed");
        let session = &sessions[0];
        assert_eq!(session.family_id, family_id);
        assert!(
            session.client_app_name.is_none(),
            "missing meta → client_app_name is None"
        );
        assert!(session.user_agent.is_none());
        assert!(session.client_ip.is_none());
        assert!(session.created_at.is_none());
    }

    #[tokio::test]
    async fn revoke_family_also_clears_meta() {
        let service = browser_token_service().await;
        let client_app_id = Uuid::now_v7();
        let user_id = format!("revoke-meta-user-{}", Uuid::now_v7());
        let family_id = create_test_family_for_user(
            &service,
            "revoke-meta-realm".to_string(),
            user_id,
            client_app_id,
            60,
            Some("UA".to_string()),
            None,
        )
        .await;

        let meta_key = RedisBrowserTokenService::family_meta_key(family_id);
        {
            let mut conn = service
                .manager
                .get()
                .await
                .expect("connection should be available");
            let exists: bool = conn.exists(&meta_key).await.expect("EXISTS should succeed");
            assert!(exists, "meta must exist right after family creation");
        }

        service
            .revoke_family(family_id)
            .await
            .expect("revocation should succeed");

        let mut conn = service
            .manager
            .get()
            .await
            .expect("connection should be available");
        let exists: bool = conn.exists(&meta_key).await.expect("EXISTS should succeed");
        assert!(
            !exists,
            "revocation must DEL the session metadata hash as well"
        );
    }

    #[tokio::test]
    async fn create_family_writes_meta_with_ttl() {
        let service = browser_token_service().await;
        let client_app_id = Uuid::now_v7();
        let ttl = 120u64;
        let family_id = create_test_family_for_user(
            &service,
            "meta-ttl-realm".to_string(),
            format!("meta-ttl-user-{}", Uuid::now_v7()),
            client_app_id,
            ttl,
            Some("UA".to_string()),
            None,
        )
        .await;

        let mut conn = service
            .manager
            .get()
            .await
            .expect("connection should be available");
        let meta_key = RedisBrowserTokenService::family_meta_key(family_id);
        let exists: bool = conn.exists(&meta_key).await.expect("EXISTS should succeed");
        assert!(exists, "meta must exist right after family creation");
        let ttl_seconds: i64 = conn.ttl(&meta_key).await.expect("TTL should succeed");
        assert!(
            ttl_seconds > 0,
            "meta TTL must be positive, got {ttl_seconds}"
        );
        assert!(
            ttl_seconds <= ttl as i64,
            "meta TTL must not exceed refresh_absolute_ttl_seconds ({ttl}), got {ttl_seconds}"
        );
    }
}
