//! Organization authorization domain: Herald-hosted organizations mapping
//! app-side groupings (workspace/team) with per-organization membership and
//! tier roles. The tier vocabulary reuses the flat `roles`/`role_policies`
//! metadata (a tier = a `(realm, client_id)` roles row); membership and
//! checks are a separate runtime surface that never reads or writes flat
//! `user_roles`.

pub mod services;

pub use services::{
    OrgChangeFacts, OrganizationService, OrganizationServiceImpl, evaluate_removal_protections,
    evaluate_upsert_protections, policies_cover,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::future::Future;
use uuid::Uuid;

use crate::common::entities::app_errors::CoreError;

/// Built-in organization tier names, seeded lazily per `(realm, client_id)`
/// on first organization creation. The `org-` prefix keeps them from
/// colliding with app-defined flat role names under the same client_id.
pub const ORG_TIER_OWNER: &str = "org-owner";
pub const ORG_TIER_ADMIN: &str = "org-admin";
pub const ORG_TIER_MEMBER: &str = "org-member";

pub const BUILTIN_ORG_TIERS: [&str; 3] = [ORG_TIER_OWNER, ORG_TIER_ADMIN, ORG_TIER_MEMBER];

#[derive(Debug, Clone, PartialEq)]
pub struct Organization {
    pub id: Uuid,
    pub realm_id: String,
    pub client_app_id: Uuid,
    pub external_id: String,
    pub name: Option<String>,
    pub owner_user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A tier (organization role) resolved to its defining `roles` row.
#[derive(Debug, Clone, PartialEq)]
pub struct OrgTier {
    pub role_id: Uuid,
    pub name: String,
}

/// One `resource.action` policy entry of a tier's vocabulary.
pub type OrgPolicy = (String, String);

/// Who initiated an organization operation: the application itself (its
/// bound API key), or a user the application acts on behalf of. The
/// application is the trust anchor for its own organizations and holds full
/// management rights; passing an actor is the application voluntarily
/// narrowing itself to that user's in-org authority.
#[derive(Debug, Clone)]
pub enum OrgActorOrigin {
    ClientCredential,
    OnBehalfOfUser { user_id: Uuid },
}

/// Caller scope every organization operation runs under: the bound Client
/// App of the API key plus the initiating origin. All repository queries are
/// scoped to `(realm_id, client_app_id)`.
#[derive(Debug, Clone)]
pub struct OrgCallerScope {
    pub realm_id: String,
    pub client_app_id: Uuid,
    pub client_id: String,
    pub client_app_name: String,
    pub api_key_id: String,
    pub origin: OrgActorOrigin,
}

/// Cached membership view used by the check path and the management gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrgMemberRole {
    pub role_id: Uuid,
    pub role_name: String,
    pub is_owner: bool,
}

/// Result shape of an organization permission check. `MemberDenied` keeps
/// `member: true` semantics distinct from `NonMember` (a disabled account is
/// a member whose checks must read as denied).
#[derive(Debug, Clone, PartialEq)]
pub enum OrgCheckOutcome {
    NonMember,
    MemberDenied,
    Member { allowed: bool, tier: OrgTier },
}

// ============================================================================
// DTOs
// ============================================================================

#[derive(Debug, Clone)]
pub struct CreateOrgDto {
    pub external_id: String,
    pub name: Option<String>,
    pub owner_user_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct UpsertMemberDto {
    pub user_id: Uuid,
    pub role: String,
    pub actor_user_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct OrgCreateOutcome {
    pub organization: Organization,
    pub created: bool,
}

#[derive(Debug, Clone)]
pub struct OrgMembershipView {
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub tier: OrgTier,
    pub is_owner: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Membership row joined with the account display fields, for member lists
/// and single-member queries.
#[derive(Debug, Clone)]
pub struct OrgMemberDetail {
    pub user_id: Uuid,
    pub email: String,
    pub nickname: Option<String>,
    pub role_id: Uuid,
    pub role_name: String,
    pub created_at: DateTime<Utc>,
}

/// A user's membership joined with its organization, for the user-org list.
#[derive(Debug, Clone)]
pub struct UserOrgDetail {
    pub org_id: Uuid,
    pub external_id: String,
    pub name: Option<String>,
    pub role_id: Uuid,
    pub role_name: String,
    pub is_owner: bool,
    pub created_at: DateTime<Utc>,
}

// ============================================================================
// Repository ports
// ============================================================================

/// Transactional inputs for `OrgMembershipRepository::upsert_member`. The
/// tier ids are resolved by the domain service; the repository re-evaluates
/// the protection predicates under the organization row lock using
/// [`evaluate_upsert_protections`] so concurrent changes cannot slip past
/// them.
#[derive(Debug, Clone)]
pub struct MemberUpsertPlan {
    pub user_id: Uuid,
    pub target_role_id: Uuid,
    /// Canonical tier name as resolved by the domain service; the repository
    /// returns it instead of re-reading the roles row inside the lock.
    pub target_role_name: String,
    pub admin_tier_role_ids: Vec<Uuid>,
    pub owner_tier_role_id: Uuid,
    /// Tier the previous owner is demoted to on an ownership transfer
    /// (always the built-in `org-admin` tier).
    pub admin_fallback_role_id: Uuid,
    /// `Some` when the caller acts on behalf of a user: the ownership
    /// transfer gate is re-verified against the locked owner row.
    pub acting_user_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct MemberRemovalPlan {
    pub user_id: Uuid,
    pub admin_tier_role_ids: Vec<Uuid>,
}

#[derive(Debug, Clone)]
pub struct AppliedMemberChange {
    pub membership: OrgMembershipView,
    pub previous_role_id: Option<Uuid>,
    pub previous_role_name: Option<String>,
    pub owner_transferred: bool,
    pub previous_owner_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct AppliedRemoval {
    pub removed_role_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct CreateOrgRepoRequest {
    pub realm_id: String,
    pub client_app_id: Uuid,
    pub client_id: String,
    pub external_id: String,
    pub name: Option<String>,
    pub owner_user_id: Uuid,
}

pub trait OrganizationRepository: Send + Sync {
    /// Scoped point lookup by `(realm_id, client_app_id, org_id)`. A miss is
    /// `CoreError::NotFound` — the single implementation point of the
    /// existence-probing guard (out-of-scope callers get the same 404 as a
    /// missing organization).
    fn find_by_id_in_scope(
        &self,
        realm_id: &str,
        client_app_id: Uuid,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Organization, CoreError>> + Send;

    /// Idempotent create in one transaction: seed the built-in tiers for the
    /// `(realm, client_id)` (first time only), insert the organization and
    /// the owner membership. Returns `(org, created)`; a replay of the same
    /// `(client_app_id, external_id)` returns the existing organization with
    /// `created = false` and does not apply the replayed payload.
    fn create_org(
        &self,
        req: CreateOrgRepoRequest,
    ) -> impl Future<Output = Result<(Organization, bool), CoreError>> + Send;

    /// Delete the organization; memberships cascade via FK. Zero rows
    /// affected is `CoreError::NotFound`.
    fn delete_org(
        &self,
        realm_id: &str,
        client_app_id: Uuid,
        org_id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}

pub trait OrgMembershipRepository: Send + Sync {
    fn get_detail(
        &self,
        org_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Option<OrgMemberDetail>, CoreError>> + Send;

    fn list_by_org(
        &self,
        org_id: Uuid,
        page: u64,
        page_size: u64,
    ) -> impl Future<Output = Result<(Vec<OrgMemberDetail>, i64), CoreError>> + Send;

    fn list_by_user(
        &self,
        realm_id: &str,
        client_app_id: Uuid,
        user_id: Uuid,
        page: u64,
        page_size: u64,
    ) -> impl Future<Output = Result<(Vec<UserOrgDetail>, i64), CoreError>> + Send;

    /// Transactional upsert: lock the organization row, re-evaluate the
    /// protection predicates on locked facts, then write the membership (and
    /// the ownership transfer writes when the target tier is the owner
    /// tier).
    fn upsert_member(
        &self,
        org: &Organization,
        plan: MemberUpsertPlan,
    ) -> impl Future<Output = Result<AppliedMemberChange, OrgError>> + Send;

    /// Transactional removal under the organization row lock.
    fn remove_member(
        &self,
        org: &Organization,
        plan: MemberRemovalPlan,
    ) -> impl Future<Output = Result<AppliedRemoval, OrgError>> + Send;
}

/// Organization permission checking with its own short-lived cache
/// (`org_membership:*` / `org_role_policies:*`), fully isolated from the
/// flat `principal_perm:*` cache family. Account status is read per request
/// (never cached). Invalidation is best-effort: the 60s TTL bounds staleness
/// if a DEL fails.
pub trait OrgAuthorizationChecker: Send + Sync {
    fn check(
        &self,
        org: &Organization,
        user_id: Uuid,
        rules: &[OrgPolicy],
    ) -> impl Future<Output = Result<OrgCheckOutcome, CoreError>> + Send;

    /// Cached membership read used by the management gate.
    fn member_role(
        &self,
        realm_id: &str,
        org_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Option<OrgMemberRole>, CoreError>> + Send;

    /// Uncached tier-vocabulary read for the self-hold grant gate: the
    /// granting decision must judge fresh data, never the check path's
    /// short-lived cache.
    fn role_policies(
        &self,
        realm_id: &str,
        role_id: Uuid,
    ) -> impl Future<Output = Result<Vec<OrgPolicy>, CoreError>> + Send;

    fn invalidate_member(
        &self,
        realm_id: &str,
        org_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = ()> + Send;

    fn invalidate_org(&self, realm_id: &str, org_id: Uuid) -> impl Future<Output = ()> + Send;
}

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum OrgError {
    #[error("organization not found within caller scope")]
    NotFound,
    #[error("owner membership is protected; transfer ownership instead")]
    OwnerProtected,
    #[error("operation would leave the organization without an admin-tier member")]
    LastAdminProtected,
    #[error("requested org role does not exist for this client app")]
    RoleNotFound,
    #[error("membership not found")]
    MemberNotFound,
    #[error("user account is outside the organization's realm")]
    AccountScopeMismatch,
    #[error("acting user is not an admin-tier member of this organization")]
    ActorNotManager,
    #[error("acting user does not self-hold the granted role's capabilities")]
    ActorSelfHoldRequired,
    #[error("ownership transfer requires the current owner")]
    OwnershipTransferDenied,
    #[error(transparent)]
    Core(#[from] CoreError),
}
