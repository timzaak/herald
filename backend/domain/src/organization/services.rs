use std::future::Future;
use std::sync::Arc;

use uuid::Uuid;

use crate::audit::{
    ActorType, AuditAction, AuditCategory, AuditContext, AuditEventRepository, AuditResult,
    AuditTargetType, NewAuditEvent,
};
use crate::authorization::RoleRepository;
use crate::authorization::policy_semantics::action_matches_hierarchy;
use crate::common::entities::app_errors::CoreError;
use crate::organization::{
    AppliedMemberChange, AppliedRemoval, CreateOrgDto, CreateOrgRepoRequest, MemberRemovalPlan,
    MemberUpsertPlan, ORG_TIER_ADMIN, ORG_TIER_OWNER, OrgActorOrigin, OrgAuthorizationChecker,
    OrgCallerScope, OrgCheckOutcome, OrgCreateOutcome, OrgError, OrgMemberDetail,
    OrgMembershipRepository, OrgMembershipView, OrgPolicy, OrgTier, Organization,
    OrganizationRepository, UpsertMemberDto, UserOrgDetail,
};
use crate::user::UserRepository;
use crate::user::entities::User;

pub trait OrganizationService: Send + Sync {
    fn create_org(
        &self,
        caller: OrgCallerScope,
        req: CreateOrgDto,
        ctx: AuditContext,
    ) -> impl Future<Output = Result<OrgCreateOutcome, OrgError>> + Send;

    fn delete_org(
        &self,
        caller: OrgCallerScope,
        org_id: Uuid,
        ctx: AuditContext,
    ) -> impl Future<Output = Result<(), OrgError>> + Send;

    fn upsert_member(
        &self,
        caller: OrgCallerScope,
        org_id: Uuid,
        req: UpsertMemberDto,
        ctx: AuditContext,
    ) -> impl Future<Output = Result<OrgMembershipView, OrgError>> + Send;

    fn remove_member(
        &self,
        caller: OrgCallerScope,
        org_id: Uuid,
        user_id: Uuid,
        actor_user_id: Option<Uuid>,
        ctx: AuditContext,
    ) -> impl Future<Output = Result<(), OrgError>> + Send;

    fn find_org(
        &self,
        caller: &OrgCallerScope,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Organization, OrgError>> + Send;

    fn get_member(
        &self,
        org_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Option<OrgMemberDetail>, OrgError>> + Send;

    fn list_members(
        &self,
        org_id: Uuid,
        page: u64,
        page_size: u64,
    ) -> impl Future<Output = Result<(Vec<OrgMemberDetail>, i64), OrgError>> + Send;

    fn list_user_orgs(
        &self,
        caller: &OrgCallerScope,
        user_id: Uuid,
        page: u64,
        page_size: u64,
    ) -> impl Future<Output = Result<(Vec<UserOrgDetail>, i64), OrgError>> + Send;

    fn check(
        &self,
        org: &Organization,
        user_id: Uuid,
        rules: &[OrgPolicy],
    ) -> impl Future<Output = Result<OrgCheckOutcome, OrgError>> + Send;
}

/// Whether `holder` (an actor's current tier vocabulary) covers every policy
/// of `granted` (the tier being conferred). The in-organization counterpart
/// of the flat "granter must self-hold" anti-escalation rule: exact resource
/// match plus the shared manage→view/create action hierarchy.
pub fn policies_cover(holder: &[OrgPolicy], granted: &[OrgPolicy]) -> bool {
    granted.iter().all(|(resource, action)| {
        holder.iter().any(|(holder_resource, holder_action)| {
            holder_resource == resource && action_matches_hierarchy(holder_action, action)
        })
    })
}

/// Facts a membership change is judged against, loaded under the
/// organization row lock so concurrent changes to the same organization
/// cannot invalidate them between check and write.
#[derive(Debug, Clone)]
pub struct OrgChangeFacts {
    pub org_owner_user_id: Uuid,
    /// The target's current tier, `None` when not yet a member.
    pub target_membership_role_id: Option<Uuid>,
    /// Admin-tier member count excluding the target.
    pub other_admin_tier_members: i64,
}

pub fn evaluate_upsert_protections(
    facts: &OrgChangeFacts,
    plan: &MemberUpsertPlan,
) -> Result<(), OrgError> {
    let target_is_owner = facts.org_owner_user_id == plan.user_id;
    let target_tier_is_owner = plan.target_role_id == plan.owner_tier_role_id;
    let current_membership_admin = facts
        .target_membership_role_id
        .is_some_and(|role| plan.admin_tier_role_ids.contains(&role));
    let target_role_admin = plan.admin_tier_role_ids.contains(&plan.target_role_id);

    // Transfer gate: conferring the owner tier on someone else must be
    // initiated by the current owner. A client-credential caller (no acting
    // user) is the application itself and is exempt.
    if target_tier_is_owner
        && !target_is_owner
        && plan
            .acting_user_id
            .is_some_and(|actor| actor != facts.org_owner_user_id)
    {
        return Err(OrgError::OwnershipTransferDenied);
    }

    // Owner protection: the owner may only be re-granted the owner tier
    // (idempotent replay); any other tier is a forbidden demotion.
    if target_is_owner && !target_tier_is_owner {
        return Err(OrgError::OwnerProtected);
    }

    // Last-admin protection: leaving the admin tier requires another
    // admin-tier member to remain. A transfer never trips this — the old
    // owner lands on the admin fallback tier and the new owner gains the
    // owner tier, so the admin-tier count never drops.
    if current_membership_admin && !target_role_admin && facts.other_admin_tier_members == 0 {
        return Err(OrgError::LastAdminProtected);
    }

    Ok(())
}

pub fn evaluate_removal_protections(
    facts: &OrgChangeFacts,
    plan: &MemberRemovalPlan,
) -> Result<(), OrgError> {
    let membership_role = facts
        .target_membership_role_id
        .ok_or(OrgError::MemberNotFound)?;

    if facts.org_owner_user_id == plan.user_id {
        return Err(OrgError::OwnerProtected);
    }

    if plan.admin_tier_role_ids.contains(&membership_role) && facts.other_admin_tier_members == 0 {
        return Err(OrgError::LastAdminProtected);
    }

    Ok(())
}

pub struct OrganizationServiceImpl<O, M, R, C, A, U>
where
    O: OrganizationRepository,
    M: OrgMembershipRepository,
    R: RoleRepository,
    C: OrgAuthorizationChecker,
    A: AuditEventRepository,
    U: UserRepository,
{
    org_repo: Arc<O>,
    membership_repo: Arc<M>,
    role_repo: Arc<R>,
    checker: Arc<C>,
    audit_repo: Arc<A>,
    user_repo: Arc<U>,
}

impl<O, M, R, C, A, U> OrganizationServiceImpl<O, M, R, C, A, U>
where
    O: OrganizationRepository,
    M: OrgMembershipRepository,
    R: RoleRepository,
    C: OrgAuthorizationChecker,
    A: AuditEventRepository,
    U: UserRepository,
{
    pub fn new(
        org_repo: Arc<O>,
        membership_repo: Arc<M>,
        role_repo: Arc<R>,
        checker: Arc<C>,
        audit_repo: Arc<A>,
        user_repo: Arc<U>,
    ) -> Self {
        Self {
            org_repo,
            membership_repo,
            role_repo,
            checker,
            audit_repo,
            user_repo,
        }
    }

    async fn find_org_by_scope(
        &self,
        caller: &OrgCallerScope,
        org_id: Uuid,
    ) -> Result<Organization, OrgError> {
        self.org_repo
            .find_by_id_in_scope(&caller.realm_id, caller.client_app_id, org_id)
            .await
            .map_err(|e| match e {
                CoreError::NotFound => OrgError::NotFound,
                other => OrgError::Core(other),
            })
    }

    async fn resolve_tier(&self, caller: &OrgCallerScope, name: &str) -> Result<OrgTier, OrgError> {
        match self
            .role_repo
            .find_by_name(name, &caller.realm_id, &caller.client_id)
            .await
        {
            Ok(role) => Ok(OrgTier {
                role_id: role.id,
                name: role.name,
            }),
            Err(CoreError::NotFound) => Err(OrgError::RoleNotFound),
            Err(e) => Err(OrgError::Core(e)),
        }
    }

    async fn resolve_protection_tiers(
        &self,
        caller: &OrgCallerScope,
    ) -> Result<(OrgTier, OrgTier), OrgError> {
        let (owner_tier, admin_tier) = tokio::try_join!(
            self.resolve_tier(caller, ORG_TIER_OWNER),
            self.resolve_tier(caller, ORG_TIER_ADMIN),
        )?;
        Ok((owner_tier, admin_tier))
    }

    async fn load_realm_user(&self, realm_id: &str, user_id: Uuid) -> Result<User, OrgError> {
        let user = self.user_repo.get_user_by_id(user_id).await?;
        if user.realm_id != realm_id {
            return Err(OrgError::AccountScopeMismatch);
        }
        Ok(user)
    }

    /// Management + self-hold + transfer gates for a user-initiated change.
    /// Read-only (cached membership reads); the ownership transfer gate is
    /// additionally re-verified under the organization row lock by the
    /// repository. Returns the acting user for audit attribution.
    async fn pass_actor_gates(
        &self,
        org: &Organization,
        actor_user_id: Uuid,
        target_tier: Option<&OrgTier>,
        owner_tier: &OrgTier,
        admin_tier_role_ids: &[Uuid],
    ) -> Result<User, OrgError> {
        let (actor, snapshot) = tokio::try_join!(
            self.user_repo.get_user_by_id(actor_user_id),
            self.checker
                .member_role(&org.realm_id, org.id, actor_user_id),
        )?;

        let is_manager = snapshot
            .as_ref()
            .is_some_and(|s| admin_tier_role_ids.contains(&s.role_id));
        if !is_manager {
            return Err(OrgError::ActorNotManager);
        }

        if let (Some(snapshot), Some(target_tier)) = (&snapshot, target_tier) {
            let (granted, holder) = tokio::try_join!(
                self.checker
                    .role_policies(&org.realm_id, target_tier.role_id),
                self.checker.role_policies(&org.realm_id, snapshot.role_id),
            )?;
            if !policies_cover(&holder, &granted) {
                return Err(OrgError::ActorSelfHoldRequired);
            }
        }

        if let Some(target_tier) = target_tier
            && target_tier.role_id == owner_tier.role_id
            && org.owner_user_id != actor.id
        {
            return Err(OrgError::OwnershipTransferDenied);
        }

        Ok(actor)
    }

    fn audit_event(
        &self,
        caller: &OrgCallerScope,
        ctx: &AuditContext,
        acting_user: Option<&User>,
        action: AuditAction,
        target_type: AuditTargetType,
        target_id: String,
        target_name: Option<String>,
        result: AuditResult,
        mut details: serde_json::Value,
    ) -> NewAuditEvent {
        let (actor_id, actor_type, actor_name) = match (&caller.origin, acting_user) {
            (OrgActorOrigin::OnBehalfOfUser { .. }, Some(user)) => (
                user.id.to_string(),
                Some(ActorType::User),
                Some(user.email.clone()),
            ),
            _ => (ctx.actor_id.clone(), ctx.actor_type, ctx.actor_name.clone()),
        };
        if let Some(obj) = details.as_object_mut() {
            obj.insert(
                "viaApiKeyId".to_string(),
                serde_json::json!(caller.api_key_id),
            );
            if matches!(caller.origin, OrgActorOrigin::OnBehalfOfUser { .. }) {
                obj.insert(
                    "viaClientId".to_string(),
                    serde_json::json!(caller.client_id),
                );
            }
        }
        NewAuditEvent {
            realm_id: caller.realm_id.clone(),
            category: AuditCategory::Rbac,
            action,
            actor_id,
            actor_type,
            actor_name,
            target_type,
            target_id,
            target_name,
            result,
            details: Some(details),
            ip_address: ctx.ip_address.clone(),
            user_agent: ctx.user_agent.clone(),
            trace_id: ctx.trace_id.clone(),
        }
    }

    async fn record_audit(&self, event: NewAuditEvent) {
        if let Err(e) = self.audit_repo.create(event).await {
            tracing::warn!(error = %e, "Failed to record organization audit event");
        }
    }

    /// Audit a member-change rejection when it is security-shaped, then hand
    /// the error back for propagation. Non-security errors (validation,
    /// missing org/membership) pass through unaudited. Gate failures
    /// attribute the caller credential; repository protection failures
    /// attribute the acting user when one exists.
    #[allow(clippy::too_many_arguments)]
    async fn audit_member_rejection(
        &self,
        caller: &OrgCallerScope,
        ctx: &AuditContext,
        acting_user: Option<&User>,
        action: AuditAction,
        target_user_id: Uuid,
        org: &Organization,
        role_name: Option<&str>,
        err: OrgError,
    ) -> OrgError {
        if !is_security_rejection(&err) {
            return err;
        }
        let mut details = serde_json::json!({
            "orgId": org.id,
            "externalId": org.external_id,
            "reason": err.to_string(),
        });
        if let Some(role_name) = role_name {
            details["roleName"] = serde_json::json!(role_name);
        }
        let event = self.audit_event(
            caller,
            ctx,
            acting_user,
            action,
            AuditTargetType::User,
            target_user_id.to_string(),
            None,
            AuditResult::Failure,
            details,
        );
        self.record_audit(event).await;
        err
    }
}

impl<O, M, R, C, A, U> OrganizationService for OrganizationServiceImpl<O, M, R, C, A, U>
where
    O: OrganizationRepository,
    M: OrgMembershipRepository,
    R: RoleRepository,
    C: OrgAuthorizationChecker,
    A: AuditEventRepository,
    U: UserRepository,
{
    async fn create_org(
        &self,
        caller: OrgCallerScope,
        req: CreateOrgDto,
        ctx: AuditContext,
    ) -> Result<OrgCreateOutcome, OrgError> {
        self.load_realm_user(&caller.realm_id, req.owner_user_id)
            .await?;

        let (org, created) = self
            .org_repo
            .create_org(CreateOrgRepoRequest {
                realm_id: caller.realm_id.clone(),
                client_app_id: caller.client_app_id,
                client_id: caller.client_id.clone(),
                external_id: req.external_id.clone(),
                name: req.name.clone(),
                owner_user_id: req.owner_user_id,
            })
            .await?;

        let event = self.audit_event(
            &caller,
            &ctx,
            None,
            AuditAction::OrgCreate,
            AuditTargetType::Organization,
            org.id.to_string(),
            Some(org.external_id.clone()),
            AuditResult::Success,
            serde_json::json!({
                "created": created,
                "externalId": org.external_id,
                "ownerUserId": org.owner_user_id,
            }),
        );
        self.record_audit(event).await;

        Ok(OrgCreateOutcome {
            organization: org,
            created,
        })
    }

    async fn delete_org(
        &self,
        caller: OrgCallerScope,
        org_id: Uuid,
        ctx: AuditContext,
    ) -> Result<(), OrgError> {
        let org = self.find_org_by_scope(&caller, org_id).await?;

        self.org_repo
            .delete_org(&caller.realm_id, caller.client_app_id, org.id)
            .await
            .map_err(|e| match e {
                CoreError::NotFound => OrgError::NotFound,
                other => OrgError::Core(other),
            })?;

        // Best-effort invalidation after commit; the 60s TTL bounds staleness.
        self.checker.invalidate_org(&org.realm_id, org.id).await;

        let event = self.audit_event(
            &caller,
            &ctx,
            None,
            AuditAction::OrgDelete,
            AuditTargetType::Organization,
            org.id.to_string(),
            Some(org.external_id.clone()),
            AuditResult::Success,
            serde_json::json!({ "externalId": org.external_id }),
        );
        self.record_audit(event).await;

        Ok(())
    }

    async fn upsert_member(
        &self,
        caller: OrgCallerScope,
        org_id: Uuid,
        req: UpsertMemberDto,
        ctx: AuditContext,
    ) -> Result<OrgMembershipView, OrgError> {
        let org = self.find_org_by_scope(&caller, org_id).await?;
        let target_tier = self.resolve_tier(&caller, &req.role).await?;
        let (owner_tier, admin_tier) = self.resolve_protection_tiers(&caller).await?;
        let admin_tier_role_ids = vec![admin_tier.role_id, owner_tier.role_id];

        self.load_realm_user(&org.realm_id, req.user_id).await?;

        let mut acting_user = None;
        if let Some(actor_user_id) = req.actor_user_id {
            match self
                .pass_actor_gates(
                    &org,
                    actor_user_id,
                    Some(&target_tier),
                    &owner_tier,
                    &admin_tier_role_ids,
                )
                .await
            {
                Ok(actor) => acting_user = Some(actor),
                Err(e) => {
                    return Err(self
                        .audit_member_rejection(
                            &caller,
                            &ctx,
                            None,
                            AuditAction::OrgMemberUpsert,
                            req.user_id,
                            &org,
                            Some(&target_tier.name),
                            e,
                        )
                        .await);
                }
            }
        }

        let plan = MemberUpsertPlan {
            user_id: req.user_id,
            target_role_id: target_tier.role_id,
            target_role_name: target_tier.name.clone(),
            admin_tier_role_ids,
            owner_tier_role_id: owner_tier.role_id,
            admin_fallback_role_id: admin_tier.role_id,
            acting_user_id: req.actor_user_id,
        };

        let applied: AppliedMemberChange =
            match self.membership_repo.upsert_member(&org, plan).await {
                Ok(applied) => applied,
                Err(e) => {
                    return Err(self
                        .audit_member_rejection(
                            &caller,
                            &ctx,
                            acting_user.as_ref(),
                            AuditAction::OrgMemberUpsert,
                            req.user_id,
                            &org,
                            Some(&target_tier.name),
                            e,
                        )
                        .await);
                }
            };

        self.checker
            .invalidate_member(&org.realm_id, org.id, req.user_id)
            .await;
        if applied.owner_transferred
            && let Some(previous_owner) = applied.previous_owner_id
        {
            self.checker
                .invalidate_member(&org.realm_id, org.id, previous_owner)
                .await;
        }

        let mut details = serde_json::json!({
            "orgId": org.id,
            "externalId": org.external_id,
            "roleId": applied.membership.tier.role_id,
            "roleName": applied.membership.tier.name,
            "created": applied.previous_role_id.is_none(),
            "ownerTransferred": applied.owner_transferred,
        });
        if let Some(from_role) = &applied.previous_role_name {
            details["fromRole"] = serde_json::json!(from_role);
        }
        let event = self.audit_event(
            &caller,
            &ctx,
            acting_user.as_ref(),
            AuditAction::OrgMemberUpsert,
            AuditTargetType::User,
            req.user_id.to_string(),
            None,
            AuditResult::Success,
            details,
        );
        self.record_audit(event).await;

        Ok(applied.membership)
    }

    async fn remove_member(
        &self,
        caller: OrgCallerScope,
        org_id: Uuid,
        user_id: Uuid,
        actor_user_id: Option<Uuid>,
        ctx: AuditContext,
    ) -> Result<(), OrgError> {
        let org = self.find_org_by_scope(&caller, org_id).await?;
        let (owner_tier, admin_tier) = self.resolve_protection_tiers(&caller).await?;
        let admin_tier_role_ids = vec![admin_tier.role_id, owner_tier.role_id];

        let mut acting_user = None;
        if let Some(actor_user_id) = actor_user_id {
            match self
                .pass_actor_gates(
                    &org,
                    actor_user_id,
                    // Removal is not bound by the self-hold rule — the flat
                    // counterpart likewise exempts unassignment.
                    None,
                    &owner_tier,
                    &admin_tier_role_ids,
                )
                .await
            {
                Ok(actor) => acting_user = Some(actor),
                Err(e) => {
                    return Err(self
                        .audit_member_rejection(
                            &caller,
                            &ctx,
                            None,
                            AuditAction::OrgMemberRemove,
                            user_id,
                            &org,
                            None,
                            e,
                        )
                        .await);
                }
            }
        }

        let plan = MemberRemovalPlan {
            user_id,
            admin_tier_role_ids,
        };

        let applied: AppliedRemoval = match self.membership_repo.remove_member(&org, plan).await {
            Ok(applied) => applied,
            Err(e) => {
                return Err(self
                    .audit_member_rejection(
                        &caller,
                        &ctx,
                        acting_user.as_ref(),
                        AuditAction::OrgMemberRemove,
                        user_id,
                        &org,
                        None,
                        e,
                    )
                    .await);
            }
        };

        self.checker
            .invalidate_member(&org.realm_id, org.id, user_id)
            .await;

        let event = self.audit_event(
            &caller,
            &ctx,
            acting_user.as_ref(),
            AuditAction::OrgMemberRemove,
            AuditTargetType::User,
            user_id.to_string(),
            None,
            AuditResult::Success,
            serde_json::json!({
                "orgId": org.id,
                "externalId": org.external_id,
                "removedRoleId": applied.removed_role_id,
            }),
        );
        self.record_audit(event).await;

        Ok(())
    }

    async fn find_org(
        &self,
        caller: &OrgCallerScope,
        org_id: Uuid,
    ) -> Result<Organization, OrgError> {
        self.find_org_by_scope(caller, org_id).await
    }

    async fn get_member(
        &self,
        org_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<OrgMemberDetail>, OrgError> {
        Ok(self.membership_repo.get_detail(org_id, user_id).await?)
    }

    async fn list_members(
        &self,
        org_id: Uuid,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<OrgMemberDetail>, i64), OrgError> {
        Ok(self
            .membership_repo
            .list_by_org(org_id, page, page_size)
            .await?)
    }

    async fn list_user_orgs(
        &self,
        caller: &OrgCallerScope,
        user_id: Uuid,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<UserOrgDetail>, i64), OrgError> {
        let user = self.user_repo.get_user_by_id(user_id).await?;
        if user.realm_id != caller.realm_id {
            // A user outside the realm is indistinguishable from a missing
            // one on this listing surface.
            return Err(OrgError::Core(CoreError::NotFound));
        }
        Ok(self
            .membership_repo
            .list_by_user(
                &caller.realm_id,
                caller.client_app_id,
                user_id,
                page,
                page_size,
            )
            .await?)
    }

    async fn check(
        &self,
        org: &Organization,
        user_id: Uuid,
        rules: &[OrgPolicy],
    ) -> Result<OrgCheckOutcome, OrgError> {
        Ok(self.checker.check(org, user_id, rules).await?)
    }
}

/// The rejections that carry security meaning and must land in the audit
/// trail with `result = failure` (protection rules and in-org
/// anti-escalation). Validation-shaped errors (unknown role, cross-realm
/// account, missing org/membership) are not audited.
fn is_security_rejection(e: &OrgError) -> bool {
    matches!(
        e,
        OrgError::OwnerProtected
            | OrgError::LastAdminProtected
            | OrgError::ActorNotManager
            | OrgError::ActorSelfHoldRequired
            | OrgError::OwnershipTransferDenied
    )
}

// `policies_cover` is the in-org anti-escalation core; scenario tests would
// need elaborate tier vocabularies to reach its edges, so the semantics are
// pinned here as pure-function unit tests instead.
#[cfg(test)]
mod policies_cover_tests {
    use super::policies_cover;

    fn policy(resource: &str, action: &str) -> (String, String) {
        (resource.to_string(), action.to_string())
    }

    #[test]
    fn exact_policies_cover() {
        let holder = vec![policy("article", "view"), policy("article", "edit")];
        let granted = vec![policy("article", "view")];
        assert!(policies_cover(&holder, &granted));
    }

    #[test]
    fn manage_covers_view_and_create() {
        let holder = vec![policy("article", "manage")];
        assert!(policies_cover(
            &holder,
            &[policy("article", "view"), policy("article", "create")]
        ));
    }

    #[test]
    fn one_uncovered_policy_fails_the_whole_grant() {
        let holder = vec![policy("article", "view")];
        let granted = vec![policy("article", "view"), policy("article", "delete")];
        assert!(!policies_cover(&holder, &granted));
    }

    #[test]
    fn empty_granted_set_is_vacuously_covered() {
        assert!(policies_cover(&[], &[]));
        assert!(policies_cover(&[policy("article", "view")], &[]));
    }

    #[test]
    fn empty_holder_covers_nothing_nonempty() {
        assert!(!policies_cover(&[], &[policy("article", "view")]));
    }

    #[test]
    fn resources_must_match_exactly() {
        let holder = vec![policy("article", "manage")];
        assert!(!policies_cover(&holder, &[policy("workspace", "view")]));
    }
}
