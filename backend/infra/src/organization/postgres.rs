use std::sync::Arc;

use chrono::{DateTime, Utc};
use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::organization::services::{
    OrgChangeFacts, evaluate_removal_protections, evaluate_upsert_protections,
};
use herald_domain::organization::{
    AppliedMemberChange, AppliedRemoval, BUILTIN_ORG_TIERS, CreateOrgRepoRequest,
    MemberRemovalPlan, MemberUpsertPlan, ORG_TIER_OWNER, OrgError, OrgMemberDetail,
    OrgMembershipRepository, Organization, OrganizationRepository, UserOrgDetail,
};
use herald_entity::organizations;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use sqlx::PgPool;
use uuid::Uuid;

fn db_err(context: &str) -> impl Fn(sqlx::Error) -> CoreError + '_ {
    move |e| {
        tracing::error!(error = %e, context, "organization repository query failed");
        CoreError::DatabaseError(format!("{context}: {e}"))
    }
}

fn to_domain_org(model: organizations::Model) -> Organization {
    Organization {
        id: model.id,
        realm_id: model.realm_id,
        client_app_id: model.client_app_id,
        external_id: model.external_id,
        name: model.name,
        owner_user_id: model.owner_user_id,
        created_at: model.created_at.into(),
        updated_at: model.updated_at.into(),
    }
}

type OrgRow = (
    Uuid,
    String,
    Uuid,
    String,
    Option<String>,
    Uuid,
    DateTime<Utc>,
    DateTime<Utc>,
);

fn to_domain_org_row(
    (id, realm_id, client_app_id, external_id, name, owner_user_id, created_at, updated_at): OrgRow,
) -> Organization {
    Organization {
        id,
        realm_id,
        client_app_id,
        external_id,
        name,
        owner_user_id,
        created_at,
        updated_at,
    }
}

pub struct PostgresOrganizationRepository {
    db: Arc<DatabaseConnection>,
    pool: PgPool,
}

impl PostgresOrganizationRepository {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        let pool = db.get_postgres_connection_pool().clone();
        Self { db, pool }
    }
}

impl OrganizationRepository for PostgresOrganizationRepository {
    async fn find_by_id_in_scope(
        &self,
        realm_id: &str,
        client_app_id: Uuid,
        org_id: Uuid,
    ) -> Result<Organization, CoreError> {
        let model = organizations::Entity::find()
            .filter(organizations::Column::RealmId.eq(realm_id))
            .filter(organizations::Column::ClientAppId.eq(client_app_id))
            .filter(organizations::Column::Id.eq(org_id))
            .one(&*self.db)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "organization scoped lookup failed");
                CoreError::DatabaseError(format!("organization scoped lookup failed: {e}"))
            })?
            .ok_or(CoreError::NotFound)?;
        Ok(to_domain_org(model))
    }

    async fn create_org(
        &self,
        req: CreateOrgRepoRequest,
    ) -> Result<(Organization, bool), CoreError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(db_err("create_org begin"))?;

        // Lazy idempotent tier seed: the first organization created for a
        // (realm, client_id) materializes the three built-in tier rows; the
        // flat rbac_init path stays untouched. Empty vocabularies are the
        // documented starting point — apps configure policies via the
        // existing role-definition surface.
        for tier_name in BUILTIN_ORG_TIERS {
            sqlx::query(
                r#"INSERT INTO roles (id, name, realm_id, client_id, is_builtin, description)
                   VALUES ($1, $2, $3, $4, TRUE, 'Built-in organization tier')
                   ON CONFLICT (name, realm_id, client_id) DO NOTHING"#,
            )
            .bind(Uuid::now_v7())
            .bind(tier_name)
            .bind(&req.realm_id)
            .bind(&req.client_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err("tier seed"))?;
        }

        let (owner_role_id,): (Uuid,) = sqlx::query_as(
            "SELECT id FROM roles WHERE name = $1 AND realm_id = $2 AND client_id = $3 AND is_builtin = TRUE",
        )
        .bind(ORG_TIER_OWNER)
        .bind(&req.realm_id)
        .bind(&req.client_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err("owner tier resolve"))?
        // The seed above only materializes a tier when the name is free; an
        // app-defined role squatting on a builtin tier name must be rejected
        // instead of silently adopted as the owner tier.
        .ok_or_else(|| {
            CoreError::Conflict(format!(
                "role name '{ORG_TIER_OWNER}' is reserved for the built-in organization tier"
            ))
        })?;

        let org_id = Uuid::now_v7();
        let inserted: Option<OrgRow> = sqlx::query_as(
            r#"INSERT INTO organizations (id, realm_id, client_app_id, external_id, name, owner_user_id)
               VALUES ($1, $2, $3, $4, $5, $6)
               ON CONFLICT (client_app_id, external_id) DO NOTHING
               RETURNING id, realm_id, client_app_id, external_id, name, owner_user_id, created_at, updated_at"#,
        )
        .bind(org_id)
        .bind(&req.realm_id)
        .bind(req.client_app_id)
        .bind(&req.external_id)
        .bind(&req.name)
        .bind(req.owner_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err("organization insert"))?;

        let (org_row, created) = match inserted {
            // Concurrent same-external_id create: the unique constraint ruled,
            // the loser falls through to the replay path below.
            Some(row) => {
                let created_org_id = row.0;
                sqlx::query(
                    "INSERT INTO org_memberships (id, org_id, user_id, role_id) VALUES ($1, $2, $3, $4)",
                )
                .bind(Uuid::now_v7())
                .bind(created_org_id)
                .bind(req.owner_user_id)
                .bind(owner_role_id)
                .execute(&mut *tx)
                .await
                .map_err(db_err("owner membership insert"))?;
                (row, true)
            }
            None => {
                let row: OrgRow = sqlx::query_as(
                    "SELECT id, realm_id, client_app_id, external_id, name, owner_user_id, created_at, updated_at
                     FROM organizations WHERE client_app_id = $1 AND external_id = $2",
                )
                .bind(req.client_app_id)
                .bind(&req.external_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_err("organization replay select"))?;
                (row, false)
            }
        };

        tx.commit().await.map_err(db_err("create_org commit"))?;

        Ok((to_domain_org_row(org_row), created))
    }

    async fn delete_org(
        &self,
        realm_id: &str,
        client_app_id: Uuid,
        org_id: Uuid,
    ) -> Result<(), CoreError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(db_err("delete_org begin"))?;

        let locked: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM organizations WHERE id = $1 AND realm_id = $2 AND client_app_id = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(realm_id)
        .bind(client_app_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err("delete_org lock"))?;

        if locked.is_none() {
            return Err(CoreError::NotFound);
        }

        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err("delete_org delete"))?;

        tx.commit().await.map_err(db_err("delete_org commit"))?;
        Ok(())
    }
}

pub struct PostgresOrgMembershipRepository {
    pool: PgPool,
}

impl PostgresOrgMembershipRepository {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self {
            pool: db.get_postgres_connection_pool().clone(),
        }
    }
}

const MEMBER_DETAIL_SQL: &str = r#"
    SELECT m.user_id, a.email, p.nickname AS nickname, m.role_id, r.name AS role_name, m.created_at
    FROM org_memberships m
    JOIN roles r ON r.id = m.role_id
    JOIN account a ON a.id = m.user_id
    LEFT JOIN profile p ON p.id = m.user_id
"#;

type MemberDetailRow = (Uuid, String, Option<String>, Uuid, String, DateTime<Utc>);

fn to_member_detail(
    (user_id, email, nickname, role_id, role_name, created_at): MemberDetailRow,
) -> OrgMemberDetail {
    OrgMemberDetail {
        user_id,
        email,
        nickname,
        role_id,
        role_name,
        created_at,
    }
}

type UserOrgRow = (
    Uuid,
    String,
    Option<String>,
    Uuid,
    String,
    bool,
    DateTime<Utc>,
);

/// Facts for the protection predicates plus the current tier name (audit
/// trail), read under the organization row lock.
struct LockedChangeFacts {
    org_owner_user_id: Uuid,
    current_role_id: Option<Uuid>,
    current_role_name: Option<String>,
    other_admin_tier_members: i64,
}

impl LockedChangeFacts {
    fn facts(&self) -> OrgChangeFacts {
        OrgChangeFacts {
            org_owner_user_id: self.org_owner_user_id,
            target_membership_role_id: self.current_role_id,
            other_admin_tier_members: self.other_admin_tier_members,
        }
    }
}

/// Shared prologue of the transactional membership changes: serialize on the
/// organization row (two concurrent demotions must not both pass the
/// last-admin count), then read the target's current tier and the other
/// admin-tier count the protection predicates judge against.
async fn load_change_facts(
    tx: &mut sqlx::PgConnection,
    org_id: Uuid,
    user_id: Uuid,
    admin_tier_role_ids: &[Uuid],
) -> Result<LockedChangeFacts, OrgError> {
    let locked_owner: Option<(Uuid,)> =
        sqlx::query_as("SELECT owner_user_id FROM organizations WHERE id = $1 FOR UPDATE")
            .bind(org_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err("member change lock"))?;
    // A concurrent org delete can remove the row between the scoped read
    // and this lock; surface it as the same 404 the scoped read would give.
    let (locked_owner,) = locked_owner.ok_or(OrgError::NotFound)?;

    let current: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT m.role_id, r.name FROM org_memberships m JOIN roles r ON r.id = m.role_id WHERE m.org_id = $1 AND m.user_id = $2",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err("member change current role"))?;

    let (other_admins,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM org_memberships WHERE org_id = $1 AND role_id = ANY($2) AND user_id <> $3",
    )
    .bind(org_id)
    .bind(admin_tier_role_ids)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err("member change admin count"))?;

    Ok(LockedChangeFacts {
        org_owner_user_id: locked_owner,
        current_role_id: current.as_ref().map(|(role_id, _)| *role_id),
        current_role_name: current.map(|(_, name)| name),
        other_admin_tier_members: other_admins,
    })
}

impl OrgMembershipRepository for PostgresOrgMembershipRepository {
    async fn get_detail(
        &self,
        org_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<OrgMemberDetail>, CoreError> {
        let row: Option<MemberDetailRow> = sqlx::query_as(&format!(
            "{MEMBER_DETAIL_SQL} WHERE m.org_id = $1 AND m.user_id = $2"
        ))
        .bind(org_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err("membership detail lookup"))?;

        Ok(row.map(to_member_detail))
    }

    async fn list_by_org(
        &self,
        org_id: Uuid,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<OrgMemberDetail>, i64), CoreError> {
        let offset = (page - 1) * page_size;
        let rows: Vec<MemberDetailRow> = sqlx::query_as(&format!(
            "{MEMBER_DETAIL_SQL} WHERE m.org_id = $1 ORDER BY m.created_at, m.id LIMIT $2 OFFSET $3"
        ))
        .bind(org_id)
        .bind(page_size as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err("membership list by org"))?;

        let (total,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM org_memberships WHERE org_id = $1")
                .bind(org_id)
                .fetch_one(&self.pool)
                .await
                .map_err(db_err("membership count by org"))?;

        let items = rows.into_iter().map(to_member_detail).collect();
        Ok((items, total))
    }

    async fn list_by_user(
        &self,
        realm_id: &str,
        client_app_id: Uuid,
        user_id: Uuid,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<UserOrgDetail>, i64), CoreError> {
        let offset = (page - 1) * page_size;
        let rows: Vec<UserOrgRow> = sqlx::query_as(
            r#"
                SELECT o.id, o.external_id, o.name, m.role_id, r.name AS role_name,
                       (o.owner_user_id = m.user_id) AS is_owner, m.created_at
                FROM org_memberships m
                JOIN organizations o ON o.id = m.org_id
                JOIN roles r ON r.id = m.role_id
                WHERE o.realm_id = $1 AND o.client_app_id = $2 AND m.user_id = $3
                ORDER BY m.created_at, m.id
                LIMIT $4 OFFSET $5
                "#,
        )
        .bind(realm_id)
        .bind(client_app_id)
        .bind(user_id)
        .bind(page_size as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err("membership list by user"))?;

        let (total,): (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM org_memberships m
            JOIN organizations o ON o.id = m.org_id
            WHERE o.realm_id = $1 AND o.client_app_id = $2 AND m.user_id = $3
            "#,
        )
        .bind(realm_id)
        .bind(client_app_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err("membership count by user"))?;

        let items = rows
            .into_iter()
            .map(
                |(org_id, external_id, name, role_id, role_name, is_owner, created_at)| {
                    UserOrgDetail {
                        org_id,
                        external_id,
                        name,
                        role_id,
                        role_name,
                        is_owner,
                        created_at,
                    }
                },
            )
            .collect();
        Ok((items, total))
    }

    async fn upsert_member(
        &self,
        org: &Organization,
        plan: MemberUpsertPlan,
    ) -> Result<AppliedMemberChange, OrgError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(db_err("upsert_member begin"))?;

        let facts =
            load_change_facts(&mut tx, org.id, plan.user_id, &plan.admin_tier_role_ids).await?;
        evaluate_upsert_protections(&facts.facts(), &plan)?;

        let (created_at, updated_at): (DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
            r#"
            INSERT INTO org_memberships (id, org_id, user_id, role_id)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (org_id, user_id) DO UPDATE SET
                role_id = EXCLUDED.role_id,
                updated_at = CASE
                    WHEN org_memberships.role_id = EXCLUDED.role_id THEN org_memberships.updated_at
                    ELSE NOW()
                END
            RETURNING created_at, updated_at
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(org.id)
        .bind(plan.user_id)
        .bind(plan.target_role_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err("upsert_member write"))?;

        let owner_transferred = plan.target_role_id == plan.owner_tier_role_id
            && facts.org_owner_user_id != plan.user_id;
        if owner_transferred {
            // Atomic ownership transfer: the new owner gains the owner tier,
            // the previous owner lands on the admin fallback tier, and the
            // organization's owner pointer flips — all under the row lock.
            sqlx::query(
                "UPDATE org_memberships SET role_id = $1, updated_at = NOW() WHERE org_id = $2 AND user_id = $3",
            )
            .bind(plan.admin_fallback_role_id)
            .bind(org.id)
            .bind(facts.org_owner_user_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err("transfer demote previous owner"))?;

            sqlx::query(
                "UPDATE organizations SET owner_user_id = $1, updated_at = NOW() WHERE id = $2",
            )
            .bind(plan.user_id)
            .bind(org.id)
            .execute(&mut *tx)
            .await
            .map_err(db_err("transfer owner pointer"))?;
        }

        tx.commit().await.map_err(db_err("upsert_member commit"))?;

        Ok(AppliedMemberChange {
            membership: herald_domain::organization::OrgMembershipView {
                org_id: org.id,
                user_id: plan.user_id,
                tier: herald_domain::organization::OrgTier {
                    role_id: plan.target_role_id,
                    name: plan.target_role_name,
                },
                is_owner: owner_transferred || facts.org_owner_user_id == plan.user_id,
                created_at,
                updated_at,
            },
            previous_role_id: facts.current_role_id,
            previous_role_name: facts.current_role_name,
            owner_transferred,
            previous_owner_id: owner_transferred.then_some(facts.org_owner_user_id),
        })
    }

    async fn remove_member(
        &self,
        org: &Organization,
        plan: MemberRemovalPlan,
    ) -> Result<AppliedRemoval, OrgError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(db_err("remove_member begin"))?;

        let facts =
            load_change_facts(&mut tx, org.id, plan.user_id, &plan.admin_tier_role_ids).await?;
        evaluate_removal_protections(&facts.facts(), &plan)?;

        sqlx::query("DELETE FROM org_memberships WHERE org_id = $1 AND user_id = $2")
            .bind(org.id)
            .bind(plan.user_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err("remove_member delete"))?;

        tx.commit().await.map_err(db_err("remove_member commit"))?;

        // `evaluate_removal_protections` already rejects a missing membership
        // (`MemberNotFound`), so this only re-shapes the verified value.
        let removed_role_id = facts.current_role_id.ok_or(OrgError::MemberNotFound)?;

        Ok(AppliedRemoval { removed_role_id })
    }
}
