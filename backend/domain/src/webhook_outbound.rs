// Outbound webhook event push (webhook-event-push): deliberately one flat
// module with no event bus — emit points hand events straight to the fan-out
// service.

use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use url::Url;
use uuid::Uuid;

use crate::common::entities::generate_uuid_v7;

pub const WEBHOOK_URL_MAX_LEN: usize = 2048;
/// Per-realm subscription ceiling — a fan-out explosion guard.
pub const MAX_SUBSCRIPTIONS_PER_REALM: i64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutboundEventCategory {
    UserLifecycle,
    RoleEvents,
    SubscriptionLifecycle,
}

impl OutboundEventCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutboundEventCategory::UserLifecycle => "user_lifecycle",
            OutboundEventCategory::RoleEvents => "role_events",
            OutboundEventCategory::SubscriptionLifecycle => "subscription_lifecycle",
        }
    }

    pub fn try_from_str(value: &str) -> Option<Self> {
        match value {
            "user_lifecycle" => Some(OutboundEventCategory::UserLifecycle),
            "role_events" => Some(OutboundEventCategory::RoleEvents),
            "subscription_lifecycle" => Some(OutboundEventCategory::SubscriptionLifecycle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutboundEventType {
    UserCreated,
    UserDisabled,
    UserDeleted,
    RoleGranted,
    RoleRevoked,
    SubscriptionActivated,
    SubscriptionRenewed,
    SubscriptionExpired,
    SubscriptionRefunded,
}

impl OutboundEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutboundEventType::UserCreated => "user.created",
            OutboundEventType::UserDisabled => "user.disabled",
            OutboundEventType::UserDeleted => "user.deleted",
            OutboundEventType::RoleGranted => "role.granted",
            OutboundEventType::RoleRevoked => "role.revoked",
            OutboundEventType::SubscriptionActivated => "subscription.activated",
            OutboundEventType::SubscriptionRenewed => "subscription.renewed",
            OutboundEventType::SubscriptionExpired => "subscription.expired",
            OutboundEventType::SubscriptionRefunded => "subscription.refunded",
        }
    }

    pub fn category(&self) -> OutboundEventCategory {
        match self {
            OutboundEventType::UserCreated
            | OutboundEventType::UserDisabled
            | OutboundEventType::UserDeleted => OutboundEventCategory::UserLifecycle,
            OutboundEventType::RoleGranted | OutboundEventType::RoleRevoked => {
                OutboundEventCategory::RoleEvents
            }
            OutboundEventType::SubscriptionActivated
            | OutboundEventType::SubscriptionRenewed
            | OutboundEventType::SubscriptionExpired
            | OutboundEventType::SubscriptionRefunded => {
                OutboundEventCategory::SubscriptionLifecycle
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookSubscriptionStatus {
    Active,
    Paused,
    Disabled,
}

impl WebhookSubscriptionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WebhookSubscriptionStatus::Active => "active",
            WebhookSubscriptionStatus::Paused => "paused",
            WebhookSubscriptionStatus::Disabled => "disabled",
        }
    }
}

/// Target states an administrator may set through the status endpoint.
/// `paused` is system-only and intentionally not representable here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookStatusTarget {
    Active,
    Disabled,
}

impl WebhookStatusTarget {
    pub fn try_from_str(value: &str) -> Option<Self> {
        match value {
            "active" => Some(WebhookStatusTarget::Active),
            "disabled" => Some(WebhookStatusTarget::Disabled),
            _ => None,
        }
    }
}

/// The JSON body POSTed to subscriber endpoints. Serialized camelCase.
/// Serialize-only: Herald produces envelopes, it never consumes them.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboundEventEnvelope<'a> {
    /// eventId — globally unique idempotency key, identical across every
    /// delivery of the same state change.
    pub id: Uuid,
    pub r#type: &'a str,
    pub realm_id: &'a str,
    pub created_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
    pub data: &'a serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct UserEventData {
    pub user_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleEventSource {
    Manual,
    Payment,
}

impl RoleEventSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            RoleEventSource::Manual => "manual",
            RoleEventSource::Payment => "payment",
        }
    }

    /// Inverse of the DB encoding used by `as_str`; unknown values decode to
    /// `Manual` (payment provenance is the only special case).
    pub fn from_db_str(value: &str) -> Self {
        if value == "payment" {
            RoleEventSource::Payment
        } else {
            RoleEventSource::Manual
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoleEventData {
    pub user_id: Uuid,
    pub role_id: Uuid,
    /// Snapshot of the role name at event time; `None` lets the fan-out
    /// service resolve it from the roles table.
    pub role_name: Option<String>,
    pub source: RoleEventSource,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct SubscriptionEventData {
    pub user_id: Uuid,
    pub subscription_id: Option<Uuid>,
    pub entitlement_key: Option<String>,
    pub current_period_start: Option<DateTime<Utc>>,
    pub current_period_end: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub enum OutboundEventData {
    User(UserEventData),
    Role(RoleEventData),
    Subscription(SubscriptionEventData),
}

impl OutboundEventData {
    /// Missing role names are filled from `names`; absent lookups serialize
    /// as `null` rather than blocking the event.
    fn to_payload(&self, names: &HashMap<Uuid, String>) -> serde_json::Value {
        match self {
            OutboundEventData::User(d) => serde_json::json!({ "userId": d.user_id }),
            OutboundEventData::Role(d) => serde_json::json!({
                "userId": d.user_id,
                "roleId": d.role_id,
                "roleName": d.role_name.clone().or_else(|| names.get(&d.role_id).cloned()),
                "source": d.source.as_str(),
                "expiresAt": d.expires_at,
            }),
            OutboundEventData::Subscription(d) => serde_json::json!({
                "userId": d.user_id,
                "subscriptionId": d.subscription_id,
                "entitlementKey": d.entitlement_key,
                "currentPeriodStart": d.current_period_start,
                "currentPeriodEnd": d.current_period_end,
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OutboundEventInput {
    pub realm_id: String,
    pub event_type: OutboundEventType,
    pub occurred_at: DateTime<Utc>,
    pub data: OutboundEventData,
}

impl OutboundEventInput {
    /// Single-user lifecycle event (`user.created` / `user.disabled` /
    /// `user.deleted`) — the payload is just the user id.
    pub fn user_event(
        realm_id: impl Into<String>,
        event_type: OutboundEventType,
        occurred_at: DateTime<Utc>,
        user_id: Uuid,
    ) -> Self {
        Self {
            realm_id: realm_id.into(),
            event_type,
            occurred_at,
            data: OutboundEventData::User(UserEventData { user_id }),
        }
    }
}

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Post-commit, best-effort outbound event fan-out. Implementations must
/// never propagate errors to business callers; delivery outcome must never
/// affect an already-committed business change.
pub trait OutboundEventPublisher: Send + Sync {
    fn emit<'a>(&'a self, events: Vec<OutboundEventInput>) -> BoxFut<'a, ()>;
}

#[derive(Debug, thiserror::Error)]
pub enum WebhookOutboundError {
    #[error("callback endpoint must be a publicly reachable http(s) URL")]
    InvalidEndpointUrl,
    #[error("eventCategories must contain at least one known category")]
    InvalidEventCategories,
    #[error("a subscription with this endpoint URL already exists in this realm")]
    DuplicateEndpointUrl,
    #[error("subscription limit exceeded for this realm")]
    SubscriptionLimitExceeded,
    #[error("webhook subscription not found")]
    NotFound,
    #[error("database error: {0}")]
    Database(String),
}

#[derive(Debug, Clone)]
pub struct WebhookSubscription {
    pub id: Uuid,
    pub realm_id: String,
    pub endpoint_url: String,
    pub secret: String,
    pub event_categories: Vec<OutboundEventCategory>,
    pub status: WebhookSubscriptionStatus,
    pub consecutive_failures: i32,
    pub last_error: Option<String>,
    pub paused_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WebhookSubscription {
    /// Last 4 secret characters for confirming which key version is live;
    /// never the secret itself.
    pub fn secret_hint(&self) -> String {
        let tail: String = self.secret.chars().rev().take(4).collect();
        let tail: String = tail.chars().rev().collect();
        format!("…{tail}")
    }
}

pub trait WebhookSubscriptionRepository: Send + Sync {
    fn find_active_by_category(
        &self,
        realm_id: &str,
        category: OutboundEventCategory,
    ) -> impl Future<Output = Result<Vec<WebhookSubscription>, WebhookOutboundError>> + Send;

    fn insert(
        &self,
        subscription: WebhookSubscription,
    ) -> impl Future<Output = Result<WebhookSubscription, WebhookOutboundError>> + Send;

    fn find_by_id_in_realm(
        &self,
        realm_id: &str,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<WebhookSubscription>, WebhookOutboundError>> + Send;

    /// Persists URL/category replacement; `reset_failures` clears the
    /// consecutive-failure counter when the endpoint URL changed.
    fn update(
        &self,
        id: Uuid,
        realm_id: &str,
        endpoint_url: &str,
        event_categories: &[OutboundEventCategory],
        reset_failures: bool,
    ) -> impl Future<Output = Result<WebhookSubscription, WebhookOutboundError>> + Send;

    /// `reset_failure_state` clears the failure counter and `paused_at`
    /// marker (resume semantics).
    fn set_status(
        &self,
        id: Uuid,
        realm_id: &str,
        status: WebhookSubscriptionStatus,
        reset_failure_state: bool,
    ) -> impl Future<Output = Result<WebhookSubscription, WebhookOutboundError>> + Send;

    fn rotate_secret(
        &self,
        id: Uuid,
        realm_id: &str,
        new_secret: &str,
    ) -> impl Future<Output = Result<Option<WebhookSubscription>, WebhookOutboundError>> + Send;

    fn delete(
        &self,
        realm_id: &str,
        id: Uuid,
    ) -> impl Future<Output = Result<bool, WebhookOutboundError>> + Send;

    fn list_page(
        &self,
        realm_id: &str,
        offset: i64,
        limit: i64,
    ) -> impl Future<Output = Result<(Vec<WebhookSubscription>, i64), WebhookOutboundError>> + Send;

    fn count_by_realm(
        &self,
        realm_id: &str,
    ) -> impl Future<Output = Result<i64, WebhookOutboundError>> + Send;
}

pub trait OutboundEventRepository: Send + Sync {
    /// Single-transaction insert of the event row plus one pending delivery
    /// row per subscription (atomic fan-out snapshot).
    fn insert_event_with_deliveries(
        &self,
        event_id: Uuid,
        realm_id: &str,
        event_type: OutboundEventType,
        payload: serde_json::Value,
        occurred_at: DateTime<Utc>,
        subscription_ids: &[Uuid],
    ) -> impl Future<Output = Result<(), WebhookOutboundError>> + Send;

    fn role_names_by_ids(
        &self,
        role_ids: &[Uuid],
    ) -> impl Future<Output = Result<HashMap<Uuid, String>, WebhookOutboundError>> + Send;
}

pub struct WebhookOutboundService<SR, ER>
where
    SR: WebhookSubscriptionRepository,
    ER: OutboundEventRepository,
{
    subscription_repository: std::sync::Arc<SR>,
    event_repository: std::sync::Arc<ER>,
}

impl<SR, ER> WebhookOutboundService<SR, ER>
where
    SR: WebhookSubscriptionRepository,
    ER: OutboundEventRepository,
{
    pub fn new(
        subscription_repository: std::sync::Arc<SR>,
        event_repository: std::sync::Arc<ER>,
    ) -> Self {
        Self {
            subscription_repository,
            event_repository,
        }
    }

    pub async fn create_subscription(
        &self,
        realm_id: &str,
        endpoint_url: Url,
        categories: Vec<OutboundEventCategory>,
    ) -> Result<(WebhookSubscription, String), WebhookOutboundError> {
        if categories.is_empty() {
            return Err(WebhookOutboundError::InvalidEventCategories);
        }
        if self
            .subscription_repository
            .count_by_realm(realm_id)
            .await?
            >= MAX_SUBSCRIPTIONS_PER_REALM
        {
            return Err(WebhookOutboundError::SubscriptionLimitExceeded);
        }
        let secret = generate_webhook_secret();
        let now = Utc::now();
        let subscription = WebhookSubscription {
            id: generate_uuid_v7(),
            realm_id: realm_id.to_string(),
            endpoint_url: endpoint_url.to_string(),
            secret: secret.clone(),
            event_categories: categories,
            status: WebhookSubscriptionStatus::Active,
            consecutive_failures: 0,
            last_error: None,
            paused_at: None,
            created_at: now,
            updated_at: now,
        };
        let created = self.subscription_repository.insert(subscription).await?;
        Ok((created, secret))
    }

    pub async fn update_subscription(
        &self,
        realm_id: &str,
        id: Uuid,
        endpoint_url: Url,
        categories: Vec<OutboundEventCategory>,
    ) -> Result<WebhookSubscription, WebhookOutboundError> {
        if categories.is_empty() {
            return Err(WebhookOutboundError::InvalidEventCategories);
        }
        let existing = self
            .subscription_repository
            .find_by_id_in_realm(realm_id, id)
            .await?
            .ok_or(WebhookOutboundError::NotFound)?;
        // A changed endpoint starts with a clean failure history — new
        // address, new reliability track record.
        let reset_failures = existing.endpoint_url != endpoint_url.as_str();
        self.subscription_repository
            .update(
                id,
                realm_id,
                endpoint_url.as_str(),
                &categories,
                reset_failures,
            )
            .await
    }

    /// `paused` cannot be requested through the management face; the auto
    /// pause belongs to the delivery pipeline alone. Returns the previous
    /// status (for audit from→to diffs) alongside the post-update row.
    pub async fn set_status(
        &self,
        realm_id: &str,
        id: Uuid,
        target: WebhookStatusTarget,
    ) -> Result<(WebhookSubscriptionStatus, WebhookSubscription), WebhookOutboundError> {
        let existing = self
            .subscription_repository
            .find_by_id_in_realm(realm_id, id)
            .await?
            .ok_or(WebhookOutboundError::NotFound)?;
        let target_status = match target {
            WebhookStatusTarget::Active => WebhookSubscriptionStatus::Active,
            WebhookStatusTarget::Disabled => WebhookSubscriptionStatus::Disabled,
        };
        let previous_status = existing.status;
        if existing.status == target_status {
            return Ok((previous_status, existing));
        }
        // Any resume clears the failure counter and the paused_at marker so
        // a recovered subscription cannot trip the auto-pause again
        // instantly (including the paused → disabled → active detour).
        let reset_failure_state = target_status == WebhookSubscriptionStatus::Active;
        let updated = self
            .subscription_repository
            .set_status(id, realm_id, target_status, reset_failure_state)
            .await?;
        Ok((previous_status, updated))
    }

    pub async fn rotate_secret(
        &self,
        realm_id: &str,
        id: Uuid,
    ) -> Result<(WebhookSubscription, String), WebhookOutboundError> {
        let new_secret = generate_webhook_secret();
        let updated = self
            .subscription_repository
            .rotate_secret(id, realm_id, &new_secret)
            .await?
            .ok_or(WebhookOutboundError::NotFound)?;
        Ok((updated, new_secret))
    }

    pub async fn delete_subscription(
        &self,
        realm_id: &str,
        id: Uuid,
    ) -> Result<(), WebhookOutboundError> {
        let deleted = self.subscription_repository.delete(realm_id, id).await?;
        if !deleted {
            return Err(WebhookOutboundError::NotFound);
        }
        Ok(())
    }

    pub async fn get_subscription(
        &self,
        realm_id: &str,
        id: Uuid,
    ) -> Result<Option<WebhookSubscription>, WebhookOutboundError> {
        self.subscription_repository
            .find_by_id_in_realm(realm_id, id)
            .await
    }

    pub async fn list_subscriptions(
        &self,
        realm_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<WebhookSubscription>, i64), WebhookOutboundError> {
        self.subscription_repository
            .list_page(realm_id, offset, limit)
            .await
    }

    /// Fan-out algorithm: only realms with a matching active subscription
    /// get an event row; everything else stays zero-growth. Each input
    /// becomes exactly one event row (its UUIDv7 id) plus one pending
    /// delivery per matched subscription.
    async fn fan_out(&self, events: Vec<OutboundEventInput>) {
        for event in events {
            if let Err(error) = self.fan_out_one(&event).await {
                tracing::warn!(
                    realm_id = %event.realm_id,
                    event_type = event.event_type.as_str(),
                    error = %error,
                    "outbound webhook emit failed"
                );
            }
        }
    }

    async fn fan_out_one(&self, event: &OutboundEventInput) -> Result<(), WebhookOutboundError> {
        let category = event.event_type.category();
        let subscriptions = self
            .subscription_repository
            .find_active_by_category(&event.realm_id, category)
            .await?;
        if subscriptions.is_empty() {
            return Ok(());
        }
        let names = self.resolve_role_names(&event.data).await?;
        let payload = event.data.to_payload(&names);
        let subscription_ids: Vec<Uuid> = subscriptions.iter().map(|s| s.id).collect();
        self.event_repository
            .insert_event_with_deliveries(
                generate_uuid_v7(),
                &event.realm_id,
                event.event_type,
                payload,
                event.occurred_at,
                &subscription_ids,
            )
            .await
    }

    async fn resolve_role_names(
        &self,
        data: &OutboundEventData,
    ) -> Result<HashMap<Uuid, String>, WebhookOutboundError> {
        match data {
            OutboundEventData::Role(role) if role.role_name.is_none() => {
                self.event_repository
                    .role_names_by_ids(&[role.role_id])
                    .await
            }
            _ => Ok(HashMap::new()),
        }
    }
}

impl<SR, ER> OutboundEventPublisher for WebhookOutboundService<SR, ER>
where
    SR: WebhookSubscriptionRepository,
    ER: OutboundEventRepository,
{
    fn emit<'a>(&'a self, events: Vec<OutboundEventInput>) -> BoxFut<'a, ()> {
        Box::pin(self.fan_out(events))
    }
}

/// `"whsec_"` + base64url of 32 random bytes.
pub fn generate_webhook_secret() -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!(
        "whsec_{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

#[derive(Debug, thiserror::Error)]
pub enum WebhookUrlError {
    #[error("callback endpoint must be a publicly reachable http(s) URL")]
    NotPublic,
    #[error("callback endpoint must be a publicly reachable http(s) URL")]
    Unresolvable,
}

/// Synchronous shape check: length, http(s) scheme, host present, no
/// userinfo, no fragment.
pub fn validate_webhook_url_shape(raw: &str) -> Result<Url, WebhookUrlError> {
    if raw.len() > WEBHOOK_URL_MAX_LEN {
        return Err(WebhookUrlError::NotPublic);
    }
    let url = Url::parse(raw).map_err(|_| WebhookUrlError::NotPublic)?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(WebhookUrlError::NotPublic);
    }
    // Empty hosts parse successfully for some inputs ("https:///cb") — they
    // are not usable callback targets either.
    if url.host_str().map(str::is_empty).unwrap_or(true) {
        return Err(WebhookUrlError::NotPublic);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(WebhookUrlError::NotPublic);
    }
    if url.fragment().is_some() {
        return Err(WebhookUrlError::NotPublic);
    }
    Ok(url)
}

/// Registration-time check: URL shape plus every resolved IP must be
/// public. Unresolvable hosts are rejected so a name cannot slip through
/// registration only to resolve privately later.
pub async fn validate_public_endpoint(raw: &str) -> Result<Url, WebhookUrlError> {
    let url = validate_webhook_url_shape(raw)?;
    let host = url.host_str().expect("shape check guarantees a host");

    if let Ok(ip) = host.parse::<IpAddr>() {
        return if is_public_ip(ip) {
            Ok(url)
        } else {
            Err(WebhookUrlError::NotPublic)
        };
    }

    let host_with_port = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => format!("{host}:0"),
    };
    let resolved = tokio::net::lookup_host(&host_with_port)
        .await
        .map_err(|_| WebhookUrlError::Unresolvable)?;
    for addr in resolved {
        if !is_public_ip(addr.ip()) {
            return Err(WebhookUrlError::NotPublic);
        }
    }
    Ok(url)
}

/// Rejection list covering loopback, private, CGNAT, link-local (incl. cloud
/// metadata 169.254.169.254), benchmarking, multicast and reserved ranges.
/// IPv4-mapped IPv6 addresses are judged by their embedded IPv4 rule.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_ipv4(mapped);
            }
            let seg = v6.segments();
            // :: (unspecified), ::1 (loopback), fc00::/7 (ULA),
            // fe80::/10 (link local), ff00::/8 (multicast)
            if seg == [0, 0, 0, 0, 0, 0, 0, 0] || seg == [0, 0, 0, 0, 0, 0, 0, 1] {
                return false;
            }
            if (seg[0] & 0xfe00) == 0xfc00 {
                return false;
            }
            if (seg[0] & 0xffc0) == 0xfe80 {
                return false;
            }
            if (seg[0] & 0xff00) == 0xff00 {
                return false;
            }
            true
        }
    }
}

fn is_public_ipv4(ip: std::net::Ipv4Addr) -> bool {
    let o = ip.octets();
    let in_range = |prefix: [u8; 2], bits: u8| -> bool {
        match bits {
            4 => (o[0] & 0xf0) == (prefix[0] & 0xf0),
            8 => o[0] == prefix[0],
            10 => o[0] == prefix[0] && (o[1] >> 6) == (prefix[1] >> 6),
            12 => o[0] == prefix[0] && (o[1] >> 4) == (prefix[1] >> 4),
            15 => o[0] == prefix[0] && (o[1] >> 1) == (prefix[1] >> 1),
            16 => o[0] == prefix[0] && o[1] == prefix[1],
            _ => false,
        }
    };
    // 0.0.0.0/8, 10/8, 100.64/10 (CGNAT), 127/8, 169.254/16, 172.16/12,
    // 192.168/16, 198.18/15, 224/4 (multicast), 240/4 (reserved incl.
    // 255.255.255.255).
    !(in_range([0, 0], 8)
        || in_range([10, 0], 8)
        || in_range([100, 64], 10)
        || in_range([127, 0], 8)
        || in_range([169, 254], 16)
        || in_range([172, 16], 12)
        || in_range([192, 168], 16)
        || in_range([198, 18], 15)
        || in_range([224, 0], 4)
        || in_range([240, 0], 4))
}

/// reqwest DNS resolver that filters resolved IPs through `is_public_ip` at
/// connection time — closes the TOCTOU window between registration-time
/// validation and the dial. All-private resolutions become connection
/// errors (counted as delivery failures).
pub struct SafeDnsResolver;

impl reqwest::dns::Resolve for SafeDnsResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let addrs = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            let public: Vec<std::net::SocketAddr> =
                addrs.filter(|addr| is_public_ip(addr.ip())).collect();
            if public.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "webhook host resolves only to non-public addresses",
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            let boxed: Box<dyn Iterator<Item = std::net::SocketAddr> + Send> =
                Box::new(public.into_iter());
            Ok(boxed)
        })
    }
}

/// Stripe-style timestamped signature:
/// `v1={hex(HMAC-SHA256(key, "{unix_ts}.{body}"))}`.
pub fn sign_delivery(secret: &str, unix_ts: i64, body: &[u8]) -> String {
    use hmac::Mac;
    type HmacSha256 = hmac::Hmac<sha2::Sha256>;
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(unix_ts.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    format!("v1={}", hex::encode(digest))
}

/// Deduplicate and validate a raw category list into catalog members.
pub fn parse_categories(
    raw: &[String],
) -> Result<Vec<OutboundEventCategory>, WebhookOutboundError> {
    if raw.is_empty() {
        return Err(WebhookOutboundError::InvalidEventCategories);
    }
    let mut seen = HashSet::new();
    let mut parsed = Vec::new();
    for value in raw {
        let category = OutboundEventCategory::try_from_str(value)
            .ok_or(WebhookOutboundError::InvalidEventCategories)?;
        if seen.insert(category) {
            parsed.push(category);
        }
    }
    Ok(parsed)
}

/// `[webhook_delivery]` runtime tuning knobs (consumed by the delivery job
/// and the ApiConfig `[webhook_delivery]` section). Operational defaults,
/// not a product contract — all values may be overridden per deployment.
#[derive(Debug, serde::Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct WebhookDeliveryConfig {
    /// Sweep cadence; also bounds first-delivery latency (p99 ≈ sweep +
    /// HTTP timeout).
    #[serde(default = "default_sweep_interval_secs")]
    pub sweep_interval_secs: u64,
    /// Realms claimed per tick (with batch_per_realm, caps the per-tick
    /// claim volume against retry storms).
    #[serde(default = "default_realms_per_tick")]
    pub realms_per_tick: i64,
    #[serde(default = "default_batch_per_realm")]
    pub batch_per_realm: i64,
    #[serde(default = "default_http_concurrency")]
    pub http_concurrency: usize,
    #[serde(default = "default_http_timeout_secs")]
    pub http_timeout_secs: u64,
    /// Backoff: min(base × 2^(attempt-1), max), no jitter — storm smoothing
    /// is carried by the per-realm/tick batch caps.
    #[serde(default = "default_backoff_base_secs")]
    pub backoff_base_secs: i64,
    #[serde(default = "default_backoff_max_secs")]
    pub backoff_max_secs: i64,
    /// Consecutive failed attempts before the subscription auto-pauses.
    #[serde(default = "default_pause_threshold")]
    pub pause_threshold: i32,
}

fn default_sweep_interval_secs() -> u64 {
    10
}
fn default_realms_per_tick() -> i64 {
    25
}
fn default_batch_per_realm() -> i64 {
    4
}
fn default_http_concurrency() -> usize {
    16
}
fn default_http_timeout_secs() -> u64 {
    10
}
fn default_backoff_base_secs() -> i64 {
    60
}
fn default_backoff_max_secs() -> i64 {
    28800
}
fn default_pause_threshold() -> i32 {
    16
}

impl Default for WebhookDeliveryConfig {
    fn default() -> Self {
        Self {
            sweep_interval_secs: default_sweep_interval_secs(),
            realms_per_tick: default_realms_per_tick(),
            batch_per_realm: default_batch_per_realm(),
            http_concurrency: default_http_concurrency(),
            http_timeout_secs: default_http_timeout_secs(),
            backoff_base_secs: default_backoff_base_secs(),
            backoff_max_secs: default_backoff_max_secs(),
            pause_threshold: default_pause_threshold(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// User Story: US-WEP-002 (integration contract — signature scheme).
    /// Covers: HMAC-SHA256 over "{timestamp}.{body}" with the subscription
    /// secret, rendered as `v1={hex}`.
    ///
    /// WHY: consumers verify this signature with their own HMAC
    /// recomputation; any drift in the signed-bytes layout, the hex
    /// encoding, or the `v1=` prefix silently breaks every integration in
    /// the field while our own tests still pass (they recompute with the
    /// same code). A fixed vector pins the wire format independently of the
    /// implementation.
    #[test]
    fn sign_delivery_matches_fixed_vectors() {
        let body = br#"{"type":"user.created"}"#;
        // Independently computed with Python hmac/stdlib:
        // hex(HMAC-SHA256("whsec_test_key", "1763640000." + body))
        assert_eq!(
            sign_delivery("whsec_test_key", 1763640000, body),
            "v1=03d317fbd891f5d25a1b48149824ef6a4cb981d249b5c029fd792720d418d0c0"
        );
        // Empty-body edge: the "." separator is still signed.
        assert_eq!(
            sign_delivery("whsec_test_key", 1763640000, b""),
            "v1=5175027c75fed714286b89d9b4cbf090d701a102b61128d91adf3886103099cf"
        );
    }

    /// User Story: US-WEP-001 (security baseline — private/loopback callback
    /// URLs must be rejected).
    /// Covers: every rejected range plus representative public passes.
    ///
    /// WHY: the callback URL is tenant-controlled and Herald dials it — a
    /// range missed here is an SSRF path into the deployment's internal
    /// network (cloud metadata, admin panels). The matrix pins each range
    /// from the security review so a future refactor of the range checks
    /// cannot silently reopen one.
    #[test]
    fn is_public_ip_rejects_the_full_private_matrix() {
        let rejected = [
            "0.0.0.0",
            "0.1.2.3", // 0.0.0.0/8
            "10.0.0.1",
            "10.255.255.255", // 10/8
            "100.64.0.1",
            "100.127.255.254", // 100.64/10 CGNAT
            "127.0.0.1",
            "127.8.8.8", // 127/8 loopback
            "169.254.169.254",
            "169.254.0.1", // 169.254/16 link-local + metadata
            "172.16.0.1",
            "172.31.255.255", // 172.16/12
            "192.168.1.1",
            "192.168.0.0", // 192.168/16
            "198.18.0.1",
            "198.19.255.255", // 198.18/15 benchmarking
            "224.0.0.1",
            "239.1.1.1", // 224/4 multicast
            "240.0.0.1",
            "255.1.2.3", // 240/4 reserved
            "::",
            "::1", // v6 unspecified/loopback
            "fc00::1",
            "fd12:3456::1", // fc00::/7 ULA
            "fe80::1",
            "fe80::ffff:ffff:ffff:ffff", // fe80::/10 link-local
            "ff02::1",
            "ffff::1", // ff00::/8 multicast
            "::ffff:10.0.0.1",
            "::ffff:127.0.0.1", // IPv4-mapped judged as v4
        ];
        for raw in rejected {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(!is_public_ip(ip), "{raw} must be rejected");
        }

        let public = [
            "1.1.1.1",
            "8.8.8.8",
            "100.63.255.255", // just below CGNAT
            "100.128.0.1",    // just above CGNAT
            "172.32.0.1",     // just above 172.16/12
            "198.20.0.1",     // just above 198.18/15
            "203.0.113.10",
            "2606:4700::1111",
            "::ffff:8.8.8.8", // IPv4-mapped public
        ];
        for raw in public {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(is_public_ip(ip), "{raw} must be public");
        }
    }

    /// User Story: US-WEP-001 (early rejection — URL shape validation).
    /// Covers: userinfo, fragment, non-http scheme, missing host, overlong.
    ///
    /// WHY: these malformed/misleading shapes are rejected before any DNS
    /// resolution; letting one through would either push a broken URL into
    /// the delivery loop or reintroduce a parser-differential surface
    /// between validation and the HTTP client.
    #[test]
    fn validate_webhook_url_shape_rejects_structural_violations() {
        assert!(validate_webhook_url_shape("https://example.com/cb").is_ok());
        assert!(validate_webhook_url_shape("http://example.com/cb?x=1").is_ok());

        assert!(validate_webhook_url_shape("ftp://example.com/cb").is_err());
        assert!(validate_webhook_url_shape("https://user:pass@example.com/cb").is_err());
        assert!(validate_webhook_url_shape("https://example.com/cb#frag").is_err());
        // True empty host ("https://:8443/cb"). Note "https:///cb" is NOT
        // one — the URL parser collapses the extra slash and reads "cb" as
        // the host; that shape is rejected later by the DNS resolution
        // stage of validate_public_endpoint.
        assert!(validate_webhook_url_shape("https://:8443/cb").is_err());
        assert!(validate_webhook_url_shape("not a url").is_err());
        let overlong = format!("https://example.com/{}", "a".repeat(WEBHOOK_URL_MAX_LEN));
        assert!(validate_webhook_url_shape(&overlong).is_err());
    }

    /// User Story: US-WEP-001 (security baseline — IP-literal endpoints).
    /// Covers: private IP literals rejected without DNS, public literals
    /// pass. Runs without a network: the literal branch never resolves.
    #[tokio::test]
    async fn validate_public_endpoint_judges_ip_literals_without_dns() {
        assert!(
            validate_public_endpoint("http://127.0.0.1:9/cb")
                .await
                .is_err()
        );
        assert!(
            validate_public_endpoint("http://10.1.2.3/cb")
                .await
                .is_err()
        );
        assert!(
            validate_public_endpoint("http://169.254.169.254/latest/meta-data")
                .await
                .is_err()
        );
        assert!(validate_public_endpoint("http://[::1]/cb").await.is_err());
        assert!(
            validate_public_endpoint("https://93.184.216.34/cb")
                .await
                .is_ok()
        );
    }

    /// User Story: US-WEP-002 (category filtering — catalog membership).
    /// Covers: empty list, unknown values, dedup.
    ///
    /// WHY: the fan-out query filters on these exact strings; an unknown
    /// value accepted here would create a subscription that matches nothing
    /// while appearing configured, and a missed dedup would inflate the
    /// overlap query.
    #[test]
    fn parse_categories_validates_and_dedups() {
        assert!(parse_categories(&[]).is_err());
        assert!(parse_categories(&["nope".to_string()]).is_err());
        assert!(parse_categories(&["user_lifecycle".to_string()]).is_ok());
        let parsed =
            parse_categories(&["user_lifecycle".to_string(), "user_lifecycle".to_string()])
                .unwrap();
        assert_eq!(parsed.len(), 1);
        let all = parse_categories(&[
            "user_lifecycle".to_string(),
            "role_events".to_string(),
            "subscription_lifecycle".to_string(),
        ])
        .unwrap();
        assert_eq!(all.len(), 3);
    }

    /// User Story: US-WEP-002 (integration contract — event catalog).
    /// Covers: every event type round-trips its wire name and maps to the
    /// right category.
    ///
    /// WHY: the wire names are the integration contract (docs-web publishes
    /// them verbatim) and the DB CHECK constraint accepts exactly these
    /// nine strings; a rename here would strand stored rows and consumer
    /// switches while the stringly-typed columns keep compiling.
    #[test]
    fn event_catalog_wire_names_and_categories_are_stable() {
        let expected: &[(&str, OutboundEventCategory)] = &[
            ("user.created", OutboundEventCategory::UserLifecycle),
            ("user.disabled", OutboundEventCategory::UserLifecycle),
            ("user.deleted", OutboundEventCategory::UserLifecycle),
            ("role.granted", OutboundEventCategory::RoleEvents),
            ("role.revoked", OutboundEventCategory::RoleEvents),
            (
                "subscription.activated",
                OutboundEventCategory::SubscriptionLifecycle,
            ),
            (
                "subscription.renewed",
                OutboundEventCategory::SubscriptionLifecycle,
            ),
            (
                "subscription.expired",
                OutboundEventCategory::SubscriptionLifecycle,
            ),
            (
                "subscription.refunded",
                OutboundEventCategory::SubscriptionLifecycle,
            ),
        ];
        let all_types = [
            OutboundEventType::UserCreated,
            OutboundEventType::UserDisabled,
            OutboundEventType::UserDeleted,
            OutboundEventType::RoleGranted,
            OutboundEventType::RoleRevoked,
            OutboundEventType::SubscriptionActivated,
            OutboundEventType::SubscriptionRenewed,
            OutboundEventType::SubscriptionExpired,
            OutboundEventType::SubscriptionRefunded,
        ];
        assert_eq!(all_types.len(), expected.len());
        for (wire, category) in expected {
            let matched = all_types
                .iter()
                .find(|t| t.as_str() == *wire)
                .unwrap_or_else(|| panic!("{wire} missing from catalog"));
            assert_eq!(matched.category(), *category, "{wire} category");
        }
    }

    /// User Story: US-WEP-001 (secret lifecycle — one-time reveal).
    /// Covers: whsec_ prefix, base64url charset, 32-byte entropy body, and
    /// secret_hint exposing only the last 4 characters.
    ///
    /// WHY: the hint is the only secret-derived value ever returned after
    /// create/rotate; if it ever leaked more than the tail, the "shown once"
    /// promise is broken. The charset/prefix pins the format consumers see
    /// in the reveal page and docs.
    #[test]
    fn generate_secret_shape_and_hint_never_leak_the_key() {
        let secret = generate_webhook_secret();
        assert!(secret.starts_with("whsec_"));
        let body = &secret["whsec_".len()..];
        assert_eq!(body.len(), 43); // base64url(32 bytes) unpadded
        assert!(
            body.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );

        let subscription = WebhookSubscription {
            id: Uuid::now_v7(),
            realm_id: "r".into(),
            endpoint_url: "https://example.com/cb".into(),
            secret: secret.clone(),
            event_categories: vec![OutboundEventCategory::RoleEvents],
            status: WebhookSubscriptionStatus::Active,
            consecutive_failures: 0,
            last_error: None,
            paused_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let hint = subscription.secret_hint();
        assert!(hint.starts_with('…'));
        // "…" is 3 UTF-8 bytes; count characters, not bytes.
        assert_eq!(hint.chars().count(), 5); // ellipsis + 4 chars
        assert_ne!(hint, secret);
        assert!(secret.ends_with(&hint[3..]));
    }
}
