use std::sync::Arc;

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::organization::services::policies_cover;
use herald_domain::organization::{
    OrgAuthorizationChecker, OrgCheckOutcome, OrgMemberRole, OrgPolicy, OrgTier, Organization,
};
use herald_domain::user::entities::UserStatus;
use sqlx::PgPool;
use uuid::Uuid;

use crate::authorization::cache::RedisCache;

/// Cache TTL for both organization cache families. Deliberately short and
/// independent from the flat `principal_perm:*` parameters: membership
/// changes invalidate explicitly, and the TTL only bounds the staleness
/// window if an invalidation DEL fails (tier vocabulary edits through the
/// admin role surface are not wired to this cache and rely on the TTL).
const CACHE_TTL_SECS: u64 = 60;

/// Organization permission checker with its own short-lived Redis caches,
/// fully isolated from the flat `RedisPermissionChecker` key family.
///
/// - `org_membership:{realmId}:{orgId}:{userId}` → [`OrgMemberRole`]
/// - `org_role_policies:{realmId}:{roleId}` → `Vec<OrgPolicy>`
///
/// Cache errors are fail-open (fall back to the database). Account status
/// and organization scoping are read from the database on every request —
/// correctness first on the security-critical paths.
///
/// Unlike the flat checker, the cache is shared as `Arc<RedisCache>` rather
/// than wrapped in an `RwLock`: every `RedisCache` method takes `&self`
/// (connections come from the pooled manager), so a lock would only
/// serialize concurrent checks behind each Redis roundtrip.
#[derive(Debug)]
pub struct RedisOrgAuthorizationChecker {
    pool: PgPool,
    cache: Arc<RedisCache>,
}

impl RedisOrgAuthorizationChecker {
    pub fn new(pool: PgPool, cache: Arc<RedisCache>) -> Self {
        Self { pool, cache }
    }

    fn membership_key(realm_id: &str, org_id: Uuid, user_id: Uuid) -> String {
        format!("org_membership:{realm_id}:{org_id}:{user_id}")
    }

    fn role_policies_key(realm_id: &str, role_id: Uuid) -> String {
        format!("org_role_policies:{realm_id}:{role_id}")
    }

    async fn get_cached<T: for<'de> serde::Deserialize<'de>>(&self, key: &str) -> Option<T> {
        self.cache
            .get(key)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, key = %key, "org cache read error, falling back to database");
            })
            .ok()
            .flatten()
    }

    async fn set_cached<T: serde::Serialize>(&self, key: &str, value: &T) {
        if let Err(e) = self.cache.set(key, value, CACHE_TTL_SECS).await {
            tracing::warn!(error = %e, key = %key, "org cache write error, proceeding without cache");
        }
    }

    /// Membership is never cached as absent: a hard-deleted account cascades
    /// its membership away, and a stale "not a member" marker would be a
    /// correctness liability with no hot-path benefit worth the risk.
    async fn load_member_role(
        &self,
        realm_id: &str,
        org_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<OrgMemberRole>, CoreError> {
        let key = Self::membership_key(realm_id, org_id, user_id);
        if let Some(cached) = self.get_cached::<OrgMemberRole>(&key).await {
            return Ok(Some(cached));
        }

        let row: Option<(Uuid, String, Uuid)> = sqlx::query_as(
            r#"
            SELECT m.role_id, r.name, o.owner_user_id
            FROM org_memberships m
            JOIN roles r ON r.id = m.role_id
            JOIN organizations o ON o.id = m.org_id
            WHERE m.org_id = $1 AND m.user_id = $2
            "#,
        )
        .bind(org_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "org membership lookup failed");
            CoreError::DatabaseError(format!("org membership lookup failed: {e}"))
        })?;

        let Some((role_id, role_name, owner_user_id)) = row else {
            return Ok(None);
        };

        let member = OrgMemberRole {
            role_id,
            role_name,
            is_owner: owner_user_id == user_id,
        };
        self.set_cached(&key, &member).await;
        Ok(Some(member))
    }

    async fn load_role_policies(
        &self,
        realm_id: &str,
        role_id: Uuid,
    ) -> Result<Vec<OrgPolicy>, CoreError> {
        let key = Self::role_policies_key(realm_id, role_id);
        if let Some(cached) = self.get_cached::<Vec<OrgPolicy>>(&key).await {
            return Ok(cached);
        }

        let policies = self.fetch_role_policies(realm_id, role_id).await?;

        self.set_cached(&key, &policies).await;
        Ok(policies)
    }

    /// Uncached `role_policies` read shared by the cached check path and the
    /// fresh-read self-hold gate — one query keeps both judging the same
    /// vocabulary.
    async fn fetch_role_policies(
        &self,
        realm_id: &str,
        role_id: Uuid,
    ) -> Result<Vec<OrgPolicy>, CoreError> {
        sqlx::query_as(
            "SELECT resource, action FROM role_policies WHERE realm_id = $1 AND role_id = $2",
        )
        .bind(realm_id)
        .bind(role_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "org tier policy lookup failed");
            CoreError::DatabaseError(format!("org tier policy lookup failed: {e}"))
        })
    }

    /// Per-request account status read (never cached): a disabled or deleted
    /// member must read as denied on the very request that follows the
    /// status change. A missing account row can only follow a hard delete,
    /// which cascades memberships away — treat it as disabled defensively.
    async fn account_denied(&self, user_id: Uuid) -> Result<bool, CoreError> {
        let status: Option<(i16,)> = sqlx::query_as("SELECT status FROM account WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "org check account status lookup failed");
                CoreError::DatabaseError(format!("account status lookup failed: {e}"))
            })?;
        Ok(status.is_none_or(|(code,)| UserStatus::from(code).is_disabled()))
    }
}

impl OrgAuthorizationChecker for RedisOrgAuthorizationChecker {
    async fn check(
        &self,
        org: &Organization,
        user_id: Uuid,
        rules: &[OrgPolicy],
    ) -> Result<OrgCheckOutcome, CoreError> {
        let Some(member) = self
            .load_member_role(&org.realm_id, org.id, user_id)
            .await?
        else {
            return Ok(OrgCheckOutcome::NonMember);
        };

        // Independent reads after the membership resolve run concurrently;
        // a denial still wins even if the policy read fails — the denial is
        // already determined by the per-request account read.
        let (denied, policies) = tokio::join!(
            self.account_denied(user_id),
            self.load_role_policies(&org.realm_id, member.role_id),
        );
        if denied? {
            // member: true, allowed: false — a disabled member stays
            // distinguishable from a non-member.
            return Ok(OrgCheckOutcome::MemberDenied);
        }
        let policies = policies?;

        let allowed = policies_cover(&policies, rules);

        Ok(OrgCheckOutcome::Member {
            allowed,
            tier: OrgTier {
                role_id: member.role_id,
                name: member.role_name,
            },
        })
    }

    async fn member_role(
        &self,
        realm_id: &str,
        org_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<OrgMemberRole>, CoreError> {
        self.load_member_role(realm_id, org_id, user_id).await
    }

    async fn role_policies(
        &self,
        realm_id: &str,
        role_id: Uuid,
    ) -> Result<Vec<OrgPolicy>, CoreError> {
        self.fetch_role_policies(realm_id, role_id).await
    }

    async fn invalidate_member(&self, realm_id: &str, org_id: Uuid, user_id: Uuid) {
        let key = Self::membership_key(realm_id, org_id, user_id);
        if let Err(e) = self.cache.delete(&key).await {
            tracing::warn!(error = %e, key = %key, "org membership cache invalidation failed; TTL bounds staleness");
        }
    }

    async fn invalidate_org(&self, realm_id: &str, org_id: Uuid) {
        let pattern = format!("org_membership:{realm_id}:{org_id}:*");
        let keys = match self.cache.find_keys(&pattern).await {
            Ok(keys) => keys,
            Err(e) => {
                tracing::warn!(error = %e, pattern = %pattern, "org cache scan failed; TTL bounds staleness");
                return;
            }
        };
        if keys.is_empty() {
            return;
        }
        if let Err(e) = self.cache.delete_keys(&keys).await {
            tracing::warn!(error = %e, count = keys.len(), "org cache batch delete failed; TTL bounds staleness");
        }
    }
}
