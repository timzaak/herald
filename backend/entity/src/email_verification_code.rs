use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "email_verification_code")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub realm_id: String,
    pub email: String,
    pub r#type: String,
    pub verification_code: String,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
