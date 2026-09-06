// Sea-ORM Entity for client_api_keys table
//
// This module defines the database entity and active model for the
// client_api_keys table, following Sea-ORM best practices.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Sea-ORM Entity for client_api_keys table
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "client_api_keys")]
pub struct Model {
    /// Primary key (UUID v7, stored as VARCHAR(36))
    #[sea_orm(primary_key)]
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// SHA-256 hash of the API key
    pub api_key_hash: String,

    /// Realm ID (foreign key to realm table, stored as VARCHAR(36))
    pub realm_id: String,

    /// Client App ID (foreign key to client_app table, 1:1 relationship, optional for backward compatibility)
    pub client_app_id: Option<uuid::Uuid>,

    /// Whether the API key is enabled
    pub enabled: bool,

    /// Optional expiration time
    pub expires_at: Option<DateTimeWithTimeZone>,

    /// Creation timestamp
    pub created_at: DateTimeWithTimeZone,

    /// Last usage timestamp
    pub last_used_at: Option<DateTimeWithTimeZone>,
}

/// Relations for client_api_keys entity
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// Foreign key to realm table
    #[sea_orm(
        belongs_to = "super::realm::Entity",
        from = "Column::RealmId",
        to = "super::realm::Column::Id",
        on_delete = "Cascade"
    )]
    Realm,

    /// Foreign key to client_app table (1:1 relationship)
    #[sea_orm(
        belongs_to = "super::client_app::Entity",
        from = "Column::ClientAppId",
        to = "super::client_app::Column::Id",
        on_delete = "Restrict"
    )]
    ClientApp,
}

impl Related<super::realm::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Realm.def()
    }
}

impl Related<super::client_app::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ClientApp.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
