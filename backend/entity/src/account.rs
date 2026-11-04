use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "account")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub realm_id: Option<String>,
    pub email: String,
    pub username: Option<String>,
    pub password: Option<String>,
    /// E.164-normalized phone number bound by the user (phone login); NULL = unbound.
    pub phone: Option<String>,
    #[sea_orm(default_value = "ARRAY[]::UUID[]")]
    pub provider_ids: Vec<Uuid>,
    pub status: i16,
    pub deleted_original_email_hash: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
