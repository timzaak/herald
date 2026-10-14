use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "organizations")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub realm_id: String,
    pub client_app_id: Uuid,
    pub external_id: String,
    pub name: Option<String>,
    pub owner_user_id: Uuid,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::org_memberships::Entity")]
    OrgMemberships,
}

impl Related<super::org_memberships::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::OrgMemberships.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
