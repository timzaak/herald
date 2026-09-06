// Realm validators

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ListRealmsQuery {
    pub user_id: Option<String>,
}

/// Paginated query parameters for listing realms
#[derive(Debug, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ListRealmsPaginatedQuery {
    /// Page number (0-indexed, default 0)
    #[validate(range(min = 0))]
    pub page: Option<i32>,

    /// Number of items per page (default 20, max 100)
    #[validate(range(min = 1, max = 100))]
    pub page_size: Option<i32>,

    /// Search term to filter by realm_id or name
    pub search: Option<String>,

    /// Column to sort by (default: created_at)
    /// Valid values: realm_id, name, created_at, updated_at
    pub sort_by: Option<String>,

    /// Sort order: asc or desc (default: desc)
    pub sort_order: Option<String>,
}

impl ListRealmsPaginatedQuery {
    /// Convert to domain layer filters with default values
    pub fn to_filters(&self) -> herald_core::domain::realm::ListRealmsFilters {
        herald_core::domain::realm::ListRealmsFilters {
            search: self.search.clone(),
            page: self.page.unwrap_or(0).max(0) as u64,
            page_size: self.page_size.unwrap_or(20).clamp(1, 100) as u64,
            sort_by: self
                .sort_by
                .clone()
                .or_else(|| Some("created_at".to_string())),
            sort_order: self.sort_order.clone().or_else(|| Some("desc".to_string())),
            accessible_realm_id: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateRealmValidator {
    #[validate(length(min = 3, max = 36))]
    pub id: Option<String>,

    #[validate(length(min = 3, max = 50))]
    pub name: String,

    pub description: Option<String>,

    pub admin_user: InitialAdminUserValidator,
}

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct InitialAdminUserValidator {
    #[validate(email)]
    pub email: String,

    #[validate(length(min = 8, max = 100))]
    pub password: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRealmValidator {
    #[validate(length(min = 3, max = 50))]
    pub name: String,

    pub description: Option<String>,
}
