use dashmap::DashMap;
use futures::future::join_all;
use moka::future::Cache;
use reqwest::{Client as ReqwestClient, Method};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::debug;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("unauthorized (401): {0}")]
    Unauthorized(String),
    #[error("forbidden (403): {0}")]
    Forbidden(String),
    #[error("not found (404): {0}")]
    NotFound(String),
    #[error("internal server error (500): {0}")]
    InternalServerError(String),
    #[error("api error ({status}): {message}")]
    ApiError { status: u16, message: String },
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, Eq, PartialEq)]
pub struct Rule {
    pub resource: String,
    pub action: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCheckRequest {
    /// Browser access token issued by `/api/auth/{realmId}/login`.
    ///
    /// Serialized as `accessToken` to match the `/api/ext/permission/check`
    /// request body contract (`api-ext::permission::PermissionCheckRequest`
    /// is `#[serde(rename_all = "camelCase")]` on `access_token`).
    pub access_token: String,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    pub client_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCheckResponse {
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionDetail {
    pub id: String,
    pub client_app_id: Option<String>,
    pub status: String,
    pub entitlement_key: String,
    pub payment_provider: String,
    /// Provider price id bound to this subscription.
    /// `None` for price-less providers (Creem) or when the subscription has no
    /// bound price yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_price_id: Option<String>,
    pub current_period_start: Option<String>,
    pub current_period_end: Option<String>,
    pub cancel_at: Option<String>,
    pub cancel_at_period_end: Option<bool>,
    pub created_at: String,
    pub updated_at: String,
}

/// Points balance response
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PointsBalanceResponse {
    pub user_id: String,
    pub balance: i64,
    #[serde(default)]
    pub total_paid_granted: i64,
    pub total_recharged: i64,
    pub total_consumed: i64,
    pub unit: String,
    pub updated_at: String,
}

/// Points consume request
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConsumePointsRequest {
    pub user_id: String,
    pub client_app_id: String,
    pub amount: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// Per-bucket transaction inside a multi-bucket consume response.
///
/// Single-pool consume → `transactions` has length 1 (structure unified with
/// the multi-bucket case). `amount` is the deduction magnitude (positive).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BucketTransaction {
    pub transaction_id: String,
    pub bucket_id: String,
    pub wallet_id: String,
    pub user_id: String,
    pub amount: i64,
    pub balance_after: i64,
}

/// Ledger-level allocation detail for a consume.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AllocationDetail {
    pub bucket_id: String,
    pub wallet_id: String,
    pub ledger_id: String,
    pub credit_type: String,
    pub allocated_amount: i64,
}

/// Points consume response (per-bucket multi-transaction shape — breaking
/// change from the old single-transaction response).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConsumePointsResponse {
    pub user_id: String,
    pub amount: i64,
    pub correlation_id: String,
    pub transactions: Vec<BucketTransaction>,
    pub allocations: Vec<AllocationDetail>,
}

/// Points grant request (admin/SDK)
///
/// `bucket_id` is REQUIRED: every grant must target an
/// explicit Credit Bucket.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GrantPointsRequest {
    pub user_id: String,
    pub bucket_id: String,
    pub amount: i64,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validity_days: Option<i64>,
}

/// Points grant response (admin/SDK)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GrantPointsResponse {
    pub transaction_id: String,
    pub user_id: String,
    pub bucket_id: String,
    pub amount: i64,
    pub granted_balance: i64,
    pub balance: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Per-credit-type balances (`balancesByType`).
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct BalancesByType {
    #[serde(default)]
    pub topup: i64,
    #[serde(default)]
    pub subscription: i64,
    #[serde(default)]
    pub registration: i64,
    #[serde(default)]
    pub free_periodic: i64,
    #[serde(default)]
    pub granted: i64,
}

/// Quota window read view (`QuotaWindowView`), mirrors the api-points
/// `QuotaWindowViewResponse`.
///
/// One row per distinct window `key` for a (user, bucket). `key` is the stable
/// display identity derived from the window length (e.g. `5h`/`week`/`month`),
/// NOT a row ordinal. `isTightest` flags the minimum-remaining window (the
/// spendable-from-quota constraint); `exhausted` flags `remaining == 0`.
/// `resetsAt` is an ISO8601 string (matches the SDK's string-date convention).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindowView {
    /// Stable display key (config-derived, not row ordinal).
    pub key: String,
    pub limit: i64,
    pub used: i64,
    pub remaining: i64,
    /// Sliding window length in seconds (month ≈ 30d).
    pub window_seconds: i64,
    /// Approximate next reset point of the window (ISO8601). `None` when no
    /// consume has occurred in the window yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    /// True if this window is the minimum-remaining (tightest) constraint.
    pub is_tightest: bool,
    /// True if `remaining == 0`.
    pub exhausted: bool,
}

/// Wallet balances grouped by Credit Bucket (`WalletByBucket`).
///
/// Mirrors the api-points `WalletByBucketResponse` shape. For the admin
/// (`billing/points/wallets`) view, `user_id` is populated and rows group per
/// `(user, bucket)`; for the `users/me/points/wallets` view, `user_id` is the
/// calling user.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WalletByBucket {
    pub bucket_id: Option<String>,
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub user_id: String,
    pub balances_by_type: BalancesByType,
    /// Currently spendable total for this bucket = window-available
    /// (`spendable_from_quota`) + pool balance (`spendable_from_pool`).
    pub bucket_total: i64,
    /// Per-window quota view for this (user, bucket) (points-grant-redesign
    /// §4.2.2). `None` for a pool-only bucket (no active subscription /
    /// free-periodic quota entitlement).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_windows: Option<Vec<QuotaWindowView>>,
    /// Window-quota available amount = minimum `remaining` across
    /// `quota_windows` (the tightest constraint). `None` for pool-only buckets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spendable_from_quota: Option<i64>,
    /// Pool-side balance sum (topup + registration + granted credit types)
    /// for this bucket. `None` for window-only buckets with no pool balance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spendable_from_pool: Option<i64>,
}

// Realm types

/// Request body for creating a realm
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateRealmSdkRequest {
    pub name: String,
    pub description: Option<String>,
    pub admin_user: AdminUserSdkInput,
}

/// Admin user input for realm creation
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserSdkInput {
    pub email: String,
    pub password: String,
}

/// Admin user output in realm detail
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserSdkOutput {
    pub id: String,
    pub email: String,
    pub role: String,
}

/// Realm detail (create/get response)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RealmInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub admin_user: Option<AdminUserSdkOutput>,
    pub created_at: String,
    pub updated_at: String,
}

/// Realm list item
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RealmItem {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// User types

/// Request body for creating a user
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserSdkRequest {
    pub email: String,
    pub password: String,
    pub nickname: Option<String>,
}

/// User info (create/get/list response)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub nickname: Option<String>,
    pub status: i32,
    pub created_at: String,
}

// Client App types

/// Request body for creating a client app
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateClientAppSdkRequest {
    pub name: String,
    pub description: Option<String>,
    pub redirect_uris: Vec<String>,
}

/// Client app detail (create/get response)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClientAppInfo {
    pub id: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub redirect_uris: Vec<String>,
    pub enabled: bool,
    pub created_at: String,
}

/// Client app list item
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClientAppItem {
    pub id: String,
    pub client_id: String,
    pub name: String,
    pub enabled: bool,
    pub created_at: String,
}

// Wrapper types for list API responses (single-pass deserialization)
#[derive(Debug, Deserialize)]
struct RealmListResponse {
    #[serde(rename = "realms")]
    items: Vec<RealmItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserListResponse {
    items: Vec<UserInfo>,
    #[allow(dead_code)]
    page: u64,
    #[allow(dead_code)]
    page_size: u64,
    #[allow(dead_code)]
    total: i64,
}

#[derive(Debug, Deserialize)]
struct ClientAppListResponse {
    #[serde(rename = "clientApps")]
    items: Vec<ClientAppItem>,
}

type TokenIndex = Arc<DashMap<String, Vec<PermissionCheckRequest>>>;

async fn handle_response<T>(response: reqwest::Response) -> Result<T, Error>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let status = response.status();
    let text = response.text().await.map_err(Error::Reqwest)?;

    match status.as_u16() {
        401 => Err(Error::Unauthorized(text)),
        403 => Err(Error::Forbidden(text)),
        404 => Err(Error::NotFound(text)),
        500 => Err(Error::InternalServerError(text)),
        200..299 => serde_json::from_str(&text).map_err(Error::SerdeJson),
        code => Err(Error::ApiError {
            status: code,
            message: text,
        }),
    }
}

#[derive(Clone)]
pub struct Client {
    http_client: ReqwestClient,
    base_url: String,
    cache: Cache<PermissionCheckRequest, PermissionCheckResponse>,
    token_index: TokenIndex,
    api_key: String,
    token_cache: Arc<DashMap<String, (PermissionCheckResponse, Instant)>>,
}

impl Client {
    pub fn new(base_url: String, api_key: String, cache_duration: Option<Duration>) -> Self {
        let duration = cache_duration.unwrap_or_else(|| Duration::from_secs(300));
        let token_index: TokenIndex = Arc::new(DashMap::new());

        let index_for_eviction = Arc::clone(&token_index);
        let cache = Cache::builder()
            .time_to_live(duration)
            .eviction_listener(move |key: Arc<PermissionCheckRequest>, _value, _cause| {
                let index = Arc::clone(&index_for_eviction);
                if let Some(mut keys) = index.get_mut(&key.access_token) {
                    keys.retain(|k| k != key.as_ref());
                }
            })
            .build();

        Self {
            http_client: ReqwestClient::builder()
                .no_proxy()
                .build()
                .expect("Failed to create SDK HTTP client"),
            base_url,
            cache,
            token_index,
            api_key,
            token_cache: Arc::new(DashMap::new()),
        }
    }

    fn build_request(&self, method: Method, url: &str) -> reqwest::RequestBuilder {
        self.http_client
            .request(method, url)
            .header("X-API-Key", &self.api_key)
    }

    /// Checks if a user has a specific permission
    ///
    /// # Arguments
    ///
    /// * `req` - Permission check request containing user, resource, and action
    ///
    /// # Returns
    ///
    /// Returns `Ok(PermissionCheckResponse)` if the check was performed successfully
    ///
    /// # Errors
    ///
    /// Returns `Err(Error::Network)` if the HTTP request fails
    /// Returns `Err(Error::Timeout)` if the request times out
    /// Returns `Err(Error::Unauthorized)` if the client credentials are invalid
    /// Returns `Err(Error::Api)` if the API returns an error response
    pub async fn check_permission(
        &self,
        req: PermissionCheckRequest,
    ) -> Result<PermissionCheckResponse, Error> {
        // Check if token is expired
        if self.is_token_expired(&req.access_token) {
            self.invalidate_cache(&req.access_token).await;
        }

        if let Some(resp) = self.cache.get(&req).await {
            return Ok(resp);
        }

        let url = format!("{}/api/ext/permission/check", self.base_url);
        let response = self
            .build_request(Method::POST, &url)
            .json(&req)
            .send()
            .await?;

        let resp: PermissionCheckResponse = handle_response(response).await?;

        // Update token cache timestamp
        self.token_cache
            .insert(req.access_token.clone(), (resp.clone(), Instant::now()));

        self.token_index
            .entry(req.access_token.clone())
            .or_default()
            .push(req.clone());
        self.cache.insert(req, resp.clone()).await;

        Ok(resp)
    }

    fn is_token_expired(&self, token: &str) -> bool {
        if let Some(entry) = self.token_cache.get(token) {
            let (_, timestamp) = &*entry;
            return timestamp.elapsed() > Duration::from_secs(300); // 5分钟阈值
        }
        false
    }

    pub async fn invalidate_cache(&self, token: &str) {
        // ATOMIC: Remove and get keys in one operation
        if let Some((_, keys)) = self.token_index.remove(token) {
            // Batch invalidate all cache entries for this token
            let invalidation_futures: Vec<_> = keys
                .iter()
                .map(|key| async {
                    self.cache.invalidate(key).await;
                })
                .collect();

            // Wait for all invalidations to complete
            join_all(invalidation_futures).await;
        }
    }

    /// Get subscription details for a client app
    ///
    /// # Arguments
    /// * `realm_id` - The realm ID
    /// * `client_app_id` - The client app ID
    ///
    /// # Returns
    /// * `Ok(SubscriptionDetail)` if the request succeeds
    /// * `Err(Error)` if network or parsing fails
    pub async fn get_subscription(
        &self,
        realm_id: &str,
        client_app_id: &str,
    ) -> Result<SubscriptionDetail, Error> {
        let url = format!(
            "{}/api/ext/bill/{}/client/{}/subscription",
            self.base_url, realm_id, client_app_id
        );

        let response = self.build_request(Method::GET, &url).send().await?;

        let status = response.status();
        let resp = handle_response(response).await;
        debug!(status = %status, "API Response for get_subscription: {:?}", resp);
        resp
    }

    /// Get user points balance
    ///
    /// # Arguments
    ///
    /// * `realm_id` - The realm ID
    /// * `user_id` - The user ID
    ///
    /// # Returns
    ///
    /// Returns `Ok(PointsBalanceResponse)` if the request succeeds
    ///
    /// # Errors
    ///
    /// Returns `Err(Error::Unauthorized)` if the API key is invalid
    /// Returns `Err(Error::Forbidden)` if cross-realm access is attempted
    /// Returns `Err(Error::NotFound)` if the account is not found
    pub async fn get_balance(
        &self,
        realm_id: &str,
        user_id: &str,
    ) -> Result<PointsBalanceResponse, Error> {
        let url = format!("{}/api/ext/points/{}/balance", self.base_url, realm_id);

        let response = self
            .http_client
            .request(Method::GET, &url)
            .header("X-API-Key", &self.api_key)
            .query(&[("userId", user_id)])
            .send()
            .await?;

        let status = response.status();
        let resp = handle_response(response).await;
        debug!(status = %status, "API Response for get_balance: {:?}", resp);
        resp
    }

    /// Consume points from user account
    ///
    /// # Arguments
    ///
    /// * `realm_id` - The realm ID
    /// * `user_id` - The user ID
    /// * `client_app_id` - The client app ID consuming points
    /// * `amount` - The amount of points to consume
    /// * `description` - Optional description of the consumption
    /// * `idempotency_key` - Optional idempotency key to prevent duplicate charges
    ///
    /// # Returns
    ///
    /// Returns `Ok(ConsumePointsResponse)` if the request succeeds
    ///
    /// # Errors
    ///
    /// Returns `Err(Error::Unauthorized)` if the API key is invalid
    /// Returns `Err(Error::Forbidden)` if cross-realm access is attempted
    /// Returns `Err(Error::NotFound)` if the account is not found
    /// Returns `Err(Error::ApiError)` for other API errors (e.g., insufficient points)
    pub async fn consume_points(
        &self,
        realm_id: &str,
        user_id: &str,
        client_app_id: &str,
        amount: i64,
        description: Option<String>,
        idempotency_key: Option<String>,
    ) -> Result<ConsumePointsResponse, Error> {
        let url = format!("{}/api/ext/points/{}/consume", self.base_url, realm_id);

        let request = ConsumePointsRequest {
            user_id: user_id.to_string(),
            client_app_id: client_app_id.to_string(),
            amount,
            description,
            idempotency_key,
        };

        let response = self
            .build_request(Method::POST, &url)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        let resp = handle_response(response).await;
        debug!(status = %status, "API Response for consume_points: {:?}", resp);
        resp
    }

    /// Grant points to a user
    ///
    /// # Arguments
    ///
    /// * `realm_id` - The realm ID
    /// * `user_id` - The user ID to grant points to
    /// * `bucket_id` - The target Credit Bucket (REQUIRED)
    /// * `amount` - The amount of points to grant (must be > 0)
    /// * `reason` - The reason for granting points (must be non-empty)
    /// * `validity_days` - Optional validity period in days (None = permanent)
    ///
    /// # Returns
    ///
    /// Returns `Ok(GrantPointsResponse)` on success
    ///
    /// # Errors
    ///
    /// Returns `Err(Error::Unauthorized)` if the API key is invalid
    /// Returns `Err(Error::Forbidden)` if cross-realm access or insufficient permissions
    /// Returns `Err(Error::NotFound)` if the user is not found
    /// Returns `Err(Error::ApiError)` for other API errors (e.g., missing/invalid bucketId)
    pub async fn grant_points(
        &self,
        realm_id: &str,
        user_id: &str,
        bucket_id: &str,
        amount: i64,
        reason: &str,
        validity_days: Option<i64>,
    ) -> Result<GrantPointsResponse, Error> {
        let url = format!("{}/api/ext/points/{}/grant", self.base_url, realm_id);

        let request = GrantPointsRequest {
            user_id: user_id.to_string(),
            bucket_id: bucket_id.to_string(),
            amount,
            reason: reason.to_string(),
            validity_days,
        };

        let response = self
            .build_request(Method::POST, &url)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        let resp = handle_response(response).await;
        debug!(status = %status, "API Response for grant_points: {:?}", resp);
        resp
    }

    // Realm methods

    /// Create a new realm
    pub async fn create_realm(&self, request: CreateRealmSdkRequest) -> Result<RealmInfo, Error> {
        let url = format!("{}/api/ext/realms", self.base_url);
        let response = self
            .build_request(Method::POST, &url)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        let resp = handle_response(response).await;
        debug!(status = %status, "API Response for create_realm: {:?}", resp);
        resp
    }

    /// List all realms visible to the caller
    pub async fn list_realms(&self) -> Result<Vec<RealmItem>, Error> {
        let url = format!("{}/api/ext/realms", self.base_url);
        let response = self.build_request(Method::GET, &url).send().await?;

        let status = response.status();
        let resp: Result<RealmListResponse, Error> = handle_response(response).await;
        debug!(status = %status, "API Response for list_realms: {:?}", resp);
        Ok(resp?.items)
    }

    /// Get a single realm by ID
    pub async fn get_realm(&self, realm_id: &str) -> Result<RealmInfo, Error> {
        let url = format!("{}/api/ext/realms/{}", self.base_url, realm_id);
        let response = self.build_request(Method::GET, &url).send().await?;

        let status = response.status();
        let resp = handle_response(response).await;
        debug!(status = %status, "API Response for get_realm: {:?}", resp);
        resp
    }

    // User methods

    /// Create a new user in a realm
    pub async fn create_user(
        &self,
        realm_id: &str,
        request: CreateUserSdkRequest,
    ) -> Result<UserInfo, Error> {
        let url = format!("{}/api/ext/realms/{}/users", self.base_url, realm_id);
        let response = self
            .build_request(Method::POST, &url)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        let resp = handle_response(response).await;
        debug!(status = %status, "API Response for create_user: {:?}", resp);
        resp
    }

    /// List all users in a realm
    pub async fn list_users(&self, realm_id: &str) -> Result<Vec<UserInfo>, Error> {
        let url = format!("{}/api/ext/realms/{}/users", self.base_url, realm_id);
        let response = self.build_request(Method::GET, &url).send().await?;

        let status = response.status();
        let resp: Result<UserListResponse, Error> = handle_response(response).await;
        debug!(status = %status, "API Response for list_users: {:?}", resp);
        Ok(resp?.items)
    }

    /// Get a single user by ID within a realm
    pub async fn get_user(&self, realm_id: &str, user_id: &str) -> Result<UserInfo, Error> {
        let url = format!(
            "{}/api/ext/realms/{}/users/{}",
            self.base_url, realm_id, user_id
        );
        let response = self.build_request(Method::GET, &url).send().await?;

        let status = response.status();
        let resp = handle_response(response).await;
        debug!(status = %status, "API Response for get_user: {:?}", resp);
        resp
    }

    // Client App methods

    /// Create a new client app in a realm
    pub async fn create_client_app(
        &self,
        realm_id: &str,
        request: CreateClientAppSdkRequest,
    ) -> Result<ClientAppInfo, Error> {
        let url = format!("{}/api/ext/realms/{}/client-apps", self.base_url, realm_id);
        let response = self
            .build_request(Method::POST, &url)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        let resp = handle_response(response).await;
        debug!(status = %status, "API Response for create_client_app: {:?}", resp);
        resp
    }

    /// List all client apps in a realm
    pub async fn list_client_apps(&self, realm_id: &str) -> Result<Vec<ClientAppItem>, Error> {
        let url = format!("{}/api/ext/realms/{}/client-apps", self.base_url, realm_id);
        let response = self.build_request(Method::GET, &url).send().await?;

        let status = response.status();
        let resp: Result<ClientAppListResponse, Error> = handle_response(response).await;
        debug!(status = %status, "API Response for list_client_apps: {:?}", resp);
        Ok(resp?.items)
    }

    /// Get a single client app by ID within a realm
    pub async fn get_client_app(
        &self,
        realm_id: &str,
        client_app_id: &str,
    ) -> Result<ClientAppInfo, Error> {
        let url = format!(
            "{}/api/ext/realms/{}/client-apps/{}",
            self.base_url, realm_id, client_app_id
        );
        let response = self.build_request(Method::GET, &url).send().await?;

        let status = response.status();
        let resp = handle_response(response).await;
        debug!(status = %status, "API Response for get_client_app: {:?}", resp);
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_check_permission() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let user_id = uuid::Uuid::now_v7().to_string();
        let resp = PermissionCheckResponse {
            allowed: true,
            user_id: Some(user_id.clone()),
        };

        Mock::given(method("POST"))
            .and(path("/api/ext/permission/check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&resp))
            .expect(1)
            .mount(&server)
            .await;

        let req = PermissionCheckRequest {
            access_token: "test_token".to_string(),
            rules: None,
            client_id: uuid::Uuid::now_v7().to_string(),
        };

        let result = client.check_permission(req).await.unwrap();
        assert!(result.allowed);
        assert_eq!(result.user_id, Some(user_id));

        server.verify().await;
    }

    #[tokio::test]
    async fn test_caching() {
        let server = MockServer::start().await;
        let client = Client::new(
            server.uri(),
            "test-api-key".to_string(),
            Some(Duration::from_secs(1)),
        );

        let user_id = uuid::Uuid::now_v7().to_string();
        let resp = PermissionCheckResponse {
            allowed: true,
            user_id: Some(user_id.clone()),
        };

        Mock::given(method("POST"))
            .and(path("/api/ext/permission/check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&resp))
            .expect(1)
            .mount(&server)
            .await;

        let req = PermissionCheckRequest {
            access_token: "test_token".to_string(),
            rules: None,
            client_id: uuid::Uuid::now_v7().to_string(),
        };

        // First call, should hit the server
        let _ = client.check_permission(req.clone()).await.unwrap();

        // Second call, should be cached
        let _ = client.check_permission(req.clone()).await.unwrap();

        server.verify().await;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Third call, after cache expiration, should hit the server again
        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/api/ext/permission/check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&resp))
            .expect(1)
            .mount(&server)
            .await;

        let _ = client.check_permission(req.clone()).await.unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn test_invalidate_cache() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let user_id = uuid::Uuid::now_v7().to_string();
        let resp = PermissionCheckResponse {
            allowed: true,
            user_id: Some(user_id.clone()),
        };

        Mock::given(method("POST"))
            .and(path("/api/ext/permission/check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&resp))
            .expect(3)
            .mount(&server)
            .await;

        let req1 = PermissionCheckRequest {
            access_token: "token1".to_string(),
            rules: None,
            client_id: uuid::Uuid::now_v7().to_string(),
        };

        let req2 = PermissionCheckRequest {
            access_token: "token2".to_string(),
            rules: None,
            client_id: uuid::Uuid::now_v7().to_string(),
        };

        // First calls, should hit the server
        let _ = client.check_permission(req1.clone()).await.unwrap();
        let _ = client.check_permission(req2.clone()).await.unwrap();

        // Invalidate cache for token1
        client.invalidate_cache("token1").await;

        // Call again, req1 should hit the server, req2 should be cached
        let _ = client.check_permission(req1.clone()).await.unwrap();
        let _ = client.check_permission(req2.clone()).await.unwrap();

        server.verify().await;
    }

    // Billing API Tests

    #[tokio::test]
    async fn test_get_subscription_success() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let subscription_response = json!({
            "id": "sub-123",
            "clientAppId": "client1",
            "status": "active",
            "entitlementKey": "basic-plan",
            "paymentProvider": "stripe",
            "currentPeriodStart": null,
            "currentPeriodEnd": null,
            "cancelAt": null,
            "cancelAtPeriodEnd": null,
            "createdAt": "2025-01-01T00:00:00Z",
            "updatedAt": "2025-01-01T00:00:00Z"
        });

        Mock::given(method("GET"))
            .and(path("/api/ext/bill/realm1/client/client1/subscription"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&subscription_response))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.get_subscription("realm1", "client1").await;

        assert!(
            result.is_ok(),
            "get_subscription should succeed, got error: {:?}",
            result
        );
        let subscription = result.unwrap();
        assert_eq!(subscription.status, "active");
        assert_eq!(subscription.entitlement_key, "basic-plan");

        server.verify().await;
    }

    #[tokio::test]
    async fn test_sdk_error_handling_unauthorized() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        Mock::given(method("GET"))
            .and(path("/api/ext/bill/realm1/client/client1/subscription"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.get_subscription("realm1", "client1").await;

        assert!(result.is_err(), "Invalid token should return error");

        server.verify().await;
    }

    #[tokio::test]
    async fn test_sdk_timeout_handling() {
        let server = MockServer::start().await;
        let client = Client::new(
            server.uri(),
            "test-api-key".to_string(),
            Some(std::time::Duration::from_millis(100)), // Short timeout
        );

        Mock::given(method("GET"))
            .and(path(
                "/api/realms/realm1/billing/client-apps/client1/subscription",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(2)), // 2 second delay
            )
            .mount(&server)
            .await;

        let result = client.get_subscription("realm1", "client1").await;

        assert!(result.is_err(), "Timeout should return error");
    }

    // Realm API Tests

    #[tokio::test]
    async fn test_create_realm_success() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let realm_response = json!({
            "id": "realm-001",
            "name": "test-realm",
            "description": "A test realm",
            "adminUser": {
                "id": "user-001",
                "email": "admin@test.com",
                "role": "admin"
            },
            "createdAt": "2025-01-01T00:00:00Z",
            "updatedAt": "2025-01-01T00:00:00Z"
        });

        Mock::given(method("POST"))
            .and(path("/api/ext/realms"))
            .respond_with(ResponseTemplate::new(201).set_body_json(&realm_response))
            .expect(1)
            .mount(&server)
            .await;

        let request = CreateRealmSdkRequest {
            name: "test-realm".to_string(),
            description: Some("A test realm".to_string()),
            admin_user: AdminUserSdkInput {
                email: "admin@test.com".to_string(),
                password: "password123".to_string(),
            },
        };

        let result = client.create_realm(request).await;
        assert!(
            result.is_ok(),
            "create_realm should succeed, got: {:?}",
            result
        );
        let realm = result.unwrap();
        assert_eq!(realm.id, "realm-001");
        assert_eq!(realm.name, "test-realm");
        assert!(realm.admin_user.is_some());

        server.verify().await;
    }

    #[tokio::test]
    async fn test_list_realms_success() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let list_response = json!({
            "realms": [
                {
                    "id": "realm-001",
                    "name": "realm-a",
                    "description": null,
                    "createdAt": "2025-01-01T00:00:00Z",
                    "updatedAt": "2025-01-01T00:00:00Z"
                },
                {
                    "id": "realm-002",
                    "name": "realm-b",
                    "description": "Second realm",
                    "createdAt": "2025-02-01T00:00:00Z",
                    "updatedAt": "2025-02-01T00:00:00Z"
                }
            ]
        });

        Mock::given(method("GET"))
            .and(path("/api/ext/realms"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&list_response))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.list_realms().await;
        assert!(
            result.is_ok(),
            "list_realms should succeed, got: {:?}",
            result
        );
        let realms = result.unwrap();
        assert_eq!(realms.len(), 2);
        assert_eq!(realms[0].name, "realm-a");
        assert_eq!(realms[1].name, "realm-b");

        server.verify().await;
    }

    #[tokio::test]
    async fn test_get_realm_success() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let realm_response = json!({
            "id": "realm-001",
            "name": "test-realm",
            "description": "A test realm",
            "adminUser": {
                "id": "user-001",
                "email": "admin@test.com",
                "role": "admin"
            },
            "createdAt": "2025-01-01T00:00:00Z",
            "updatedAt": "2025-01-01T00:00:00Z"
        });

        Mock::given(method("GET"))
            .and(path("/api/ext/realms/realm-001"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&realm_response))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.get_realm("realm-001").await;
        assert!(
            result.is_ok(),
            "get_realm should succeed, got: {:?}",
            result
        );
        let realm = result.unwrap();
        assert_eq!(realm.id, "realm-001");
        assert_eq!(realm.name, "test-realm");

        server.verify().await;
    }

    #[tokio::test]
    async fn test_get_realm_not_found() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        Mock::given(method("GET"))
            .and(path("/api/ext/realms/nonexistent"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.get_realm("nonexistent").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::NotFound(_) => {}
            other => panic!("Expected NotFound, got: {:?}", other),
        }

        server.verify().await;
    }

    // User API Tests

    #[tokio::test]
    async fn test_create_user_success() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let user_response = json!({
            "id": "user-001",
            "email": "test@example.com",
            "nickname": "testuser",
            "status": 1,
            "createdAt": "2025-01-01T00:00:00Z"
        });

        Mock::given(method("POST"))
            .and(path("/api/ext/realms/realm-001/users"))
            .respond_with(ResponseTemplate::new(201).set_body_json(&user_response))
            .expect(1)
            .mount(&server)
            .await;

        let request = CreateUserSdkRequest {
            email: "test@example.com".to_string(),
            password: "password123".to_string(),
            nickname: Some("testuser".to_string()),
        };

        let result = client.create_user("realm-001", request).await;
        assert!(
            result.is_ok(),
            "create_user should succeed, got: {:?}",
            result
        );
        let user = result.unwrap();
        assert_eq!(user.id, "user-001");
        assert_eq!(user.email, "test@example.com");
        assert_eq!(user.nickname, Some("testuser".to_string()));

        server.verify().await;
    }

    #[tokio::test]
    async fn test_list_users_success() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let list_response = json!({
            "items": [
                {
                    "id": "user-001",
                    "email": "a@example.com",
                    "nickname": null,
                    "status": 1,
                    "createdAt": "2025-01-01T00:00:00Z"
                },
                {
                    "id": "user-002",
                    "email": "b@example.com",
                    "nickname": "bob",
                    "status": 1,
                    "createdAt": "2025-02-01T00:00:00Z"
                }
            ],
            "page": 1,
            "pageSize": 20,
            "total": 2
        });

        Mock::given(method("GET"))
            .and(path("/api/ext/realms/realm-001/users"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&list_response))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.list_users("realm-001").await;
        assert!(
            result.is_ok(),
            "list_users should succeed, got: {:?}",
            result
        );
        let users = result.unwrap();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].email, "a@example.com");
        assert_eq!(users[1].email, "b@example.com");

        server.verify().await;
    }

    #[tokio::test]
    async fn test_get_user_success() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let user_response = json!({
            "id": "user-001",
            "email": "test@example.com",
            "nickname": "testuser",
            "status": 1,
            "createdAt": "2025-01-01T00:00:00Z"
        });

        Mock::given(method("GET"))
            .and(path("/api/ext/realms/realm-001/users/user-001"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&user_response))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.get_user("realm-001", "user-001").await;
        assert!(result.is_ok(), "get_user should succeed, got: {:?}", result);
        let user = result.unwrap();
        assert_eq!(user.id, "user-001");
        assert_eq!(user.email, "test@example.com");

        server.verify().await;
    }

    #[tokio::test]
    async fn test_create_user_forbidden() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        Mock::given(method("POST"))
            .and(path("/api/ext/realms/realm-001/users"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;

        let request = CreateUserSdkRequest {
            email: "test@example.com".to_string(),
            password: "password123".to_string(),
            nickname: None,
        };

        let result = client.create_user("realm-001", request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Forbidden(_) => {}
            other => panic!("Expected Forbidden, got: {:?}", other),
        }

        server.verify().await;
    }

    // Client App API Tests

    #[tokio::test]
    async fn test_create_client_app_success() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let app_response = json!({
            "id": "app-001",
            "clientId": "client-abc",
            "clientSecret": "secret-xyz",
            "name": "My App",
            "description": "A test app",
            "redirectUris": ["https://example.com/callback"],
            "enabled": true,
            "createdAt": "2025-01-01T00:00:00Z"
        });

        Mock::given(method("POST"))
            .and(path("/api/ext/realms/realm-001/client-apps"))
            .respond_with(ResponseTemplate::new(201).set_body_json(&app_response))
            .expect(1)
            .mount(&server)
            .await;

        let request = CreateClientAppSdkRequest {
            name: "My App".to_string(),
            description: Some("A test app".to_string()),
            redirect_uris: vec!["https://example.com/callback".to_string()],
        };

        let result = client.create_client_app("realm-001", request).await;
        assert!(
            result.is_ok(),
            "create_client_app should succeed, got: {:?}",
            result
        );
        let app = result.unwrap();
        assert_eq!(app.id, "app-001");
        assert_eq!(app.client_id, "client-abc");
        assert_eq!(app.name, "My App");
        assert!(app.enabled);

        server.verify().await;
    }

    #[tokio::test]
    async fn test_list_client_apps_success() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let list_response = json!({
            "clientApps": [
                {
                    "id": "app-001",
                    "clientId": "client-abc",
                    "name": "App A",
                    "enabled": true,
                    "createdAt": "2025-01-01T00:00:00Z"
                },
                {
                    "id": "app-002",
                    "clientId": "client-def",
                    "name": "App B",
                    "enabled": false,
                    "createdAt": "2025-02-01T00:00:00Z"
                }
            ]
        });

        Mock::given(method("GET"))
            .and(path("/api/ext/realms/realm-001/client-apps"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&list_response))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.list_client_apps("realm-001").await;
        assert!(
            result.is_ok(),
            "list_client_apps should succeed, got: {:?}",
            result
        );
        let apps = result.unwrap();
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "App A");
        assert_eq!(apps[1].name, "App B");
        assert!(apps[0].enabled);
        assert!(!apps[1].enabled);

        server.verify().await;
    }

    #[tokio::test]
    async fn test_get_client_app_success() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let app_response = json!({
            "id": "app-001",
            "clientId": "client-abc",
            "clientSecret": null,
            "name": "My App",
            "description": "A test app",
            "redirectUris": ["https://example.com/callback"],
            "enabled": true,
            "createdAt": "2025-01-01T00:00:00Z"
        });

        Mock::given(method("GET"))
            .and(path("/api/ext/realms/realm-001/client-apps/app-001"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&app_response))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.get_client_app("realm-001", "app-001").await;
        assert!(
            result.is_ok(),
            "get_client_app should succeed, got: {:?}",
            result
        );
        let app = result.unwrap();
        assert_eq!(app.id, "app-001");
        assert_eq!(app.client_id, "client-abc");
        assert!(app.enabled);

        server.verify().await;
    }

    #[tokio::test]
    async fn test_get_client_app_not_found() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        Mock::given(method("GET"))
            .and(path("/api/ext/realms/realm-001/client-apps/nonexistent"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.get_client_app("realm-001", "nonexistent").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::NotFound(_) => {}
            other => panic!("Expected NotFound, got: {:?}", other),
        }

        server.verify().await;
    }

    // Cross-cutting error tests

    #[tokio::test]
    async fn test_realm_unauthorized() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        Mock::given(method("GET"))
            .and(path("/api/ext/realms"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.list_realms().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Unauthorized(_) => {}
            other => panic!("Expected Unauthorized, got: {:?}", other),
        }

        server.verify().await;
    }

    #[tokio::test]
    async fn test_user_forbidden() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        Mock::given(method("GET"))
            .and(path("/api/ext/realms/realm-001/users"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;

        let result = client.list_users("realm-001").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Forbidden(_) => {}
            other => panic!("Expected Forbidden, got: {:?}", other),
        }

        server.verify().await;
    }

    #[tokio::test]
    async fn test_grant_points_success() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let user_id = uuid::Uuid::now_v7().to_string();
        let bucket_id = uuid::Uuid::now_v7().to_string();
        let transaction_id = uuid::Uuid::now_v7().to_string();
        let grant_response = json!({
            "transactionId": transaction_id,
            "userId": user_id,
            "bucketId": bucket_id,
            "amount": 100,
            "grantedBalance": 100,
            "balance": 150,
            "expiresAt": null
        });

        Mock::given(method("POST"))
            .and(path("/api/ext/points/realm1/grant"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&grant_response))
            .expect(1)
            .mount(&server)
            .await;

        let result = client
            .grant_points("realm1", &user_id, &bucket_id, 100, "test reason", None)
            .await;
        assert!(
            result.is_ok(),
            "grant_points should succeed, got: {:?}",
            result
        );
        let resp = result.unwrap();
        assert_eq!(resp.amount, 100);
        assert_eq!(resp.balance, 150);
        assert_eq!(resp.granted_balance, 100);
        assert_eq!(resp.bucket_id, bucket_id);
        assert!(resp.expires_at.is_none());

        server.verify().await;
    }

    #[tokio::test]
    async fn test_grant_points_with_validity() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        let user_id = uuid::Uuid::now_v7().to_string();
        let bucket_id = uuid::Uuid::now_v7().to_string();
        let transaction_id = uuid::Uuid::now_v7().to_string();
        let grant_response = json!({
            "transactionId": transaction_id,
            "userId": user_id,
            "bucketId": bucket_id,
            "amount": 200,
            "grantedBalance": 200,
            "balance": 200,
            "expiresAt": "2026-07-01T00:00:00Z"
        });

        Mock::given(method("POST"))
            .and(path("/api/ext/points/realm1/grant"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&grant_response))
            .expect(1)
            .mount(&server)
            .await;

        let result = client
            .grant_points(
                "realm1",
                &user_id,
                &bucket_id,
                200,
                "campaign reward",
                Some(30),
            )
            .await;
        assert!(
            result.is_ok(),
            "grant_points should succeed, got: {:?}",
            result
        );
        let resp = result.unwrap();
        assert_eq!(resp.amount, 200);
        assert_eq!(resp.granted_balance, 200);
        assert_eq!(resp.expires_at, Some("2026-07-01T00:00:00Z".to_string()));

        server.verify().await;
    }

    #[tokio::test]
    async fn test_grant_points_not_found() {
        let server = MockServer::start().await;
        let client = Client::new(server.uri(), "test-api-key".to_string(), None);

        Mock::given(method("POST"))
            .and(path("/api/ext/points/realm1/grant"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let result = client
            .grant_points(
                "realm1",
                "nonexistent-user",
                "00000000-0000-0000-0000-000000000000",
                100,
                "test",
                None,
            )
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::NotFound(_) => {}
            other => panic!("Expected NotFound, got: {:?}", other),
        }

        server.verify().await;
    }
}
