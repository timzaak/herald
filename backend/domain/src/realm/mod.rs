use crate::audit::AuditContext;
use crate::authentication::Identity;
use crate::common::entities::{Entity, app_errors::CoreError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::future::Future;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

/// The platform-level realm. Realm lifecycle operations that affect the whole
/// tenant fleet (e.g. self-service signup entry) are hosted here and only here.
pub const ADMIN_REALM_ID: &str = "admin";

// ============================================================================
// Pagination Types
// ============================================================================

/// Filters for listing realms with pagination and search
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ListRealmsFilters {
    /// Search term to filter by realm_id or name
    pub search: Option<String>,

    /// Page number (0-indexed)
    #[validate(range(min = 0))]
    pub page: u64,

    /// Number of items per page
    #[validate(range(min = 1, max = 100))]
    pub page_size: u64,

    /// Column to sort by (realm_id, name, created_at, updated_at)
    pub sort_by: Option<String>,

    /// Sort order (asc, desc)
    pub sort_order: Option<String>,

    #[serde(skip)]
    #[schema(value_type = Option<String>, read_only = true)]
    pub accessible_realm_id: Option<String>,
}

/// Pagination metadata response
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginationResponse {
    /// Current page number (0-indexed)
    pub page: u64,

    /// Number of items per page
    pub page_size: u64,

    /// Total number of items
    pub total: i64,

    /// Total number of pages
    pub total_pages: u64,
}

/// Paginated realms response
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedRealmsResponse {
    pub realms: Vec<Realm>,
    pub pagination: PaginationResponse,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Realm {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub admin_user: Option<CreatedAdminUser>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct CreatedAdminUser {
    pub id: String,
    pub email: String,
    pub role: String,
}

impl Entity for Realm {
    fn id(&self) -> Uuid {
        Uuid::now_v7() // Placeholder
    }

    fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct InitialAdminUser {
    #[validate(email)]
    pub email: String,

    #[validate(length(min = 8, max = 100))]
    pub password: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateRealmRequest {
    /// Optional custom realm ID. If not provided, UUID v7 will be used.
    /// Must be 3-36 alphanumeric characters if provided.
    #[validate(length(min = 3, max = 36))]
    pub id: Option<String>,

    #[validate(length(min = 3, max = 50))]
    pub name: String,

    pub description: Option<String>,

    /// Initial realm administrator (required)
    pub admin_user: InitialAdminUser,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct UpdateRealmRequest {
    #[validate(length(min = 3, max = 50))]
    pub name: Option<String>,

    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealmSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[cfg_attr(test, mockall::automock)]
pub trait RealmRepository: Send + Sync {
    fn create_realm(
        &self,
        request: CreateRealmRequest,
    ) -> impl Future<Output = Result<Realm, CoreError>> + Send;

    fn get_realm_by_id(&self, id: &str) -> impl Future<Output = Result<Realm, CoreError>> + Send;

    fn get_realm_by_name(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Realm, CoreError>> + Send;

    fn list_realms(&self) -> impl Future<Output = Result<Vec<Realm>, CoreError>> + Send;

    fn list_realms_paginated(
        &self,
        filters: ListRealmsFilters,
    ) -> impl Future<Output = Result<PaginatedRealmsResponse, CoreError>> + Send;

    fn update_realm(
        &self,
        id: &str,
        name: String,
        description: Option<String>,
    ) -> impl Future<Output = Result<Realm, CoreError>> + Send;

    fn delete_realm(&self, id: &str) -> impl Future<Output = Result<(), CoreError>> + Send;
}

#[cfg_attr(test, mockall::automock)]
pub trait RealmService: Send + Sync {
    fn create_realm(
        &self,
        identity: Identity,
        ctx: AuditContext,
        request: CreateRealmRequest,
    ) -> impl Future<Output = Result<Realm, CoreError>> + Send;

    /// Provision a realm from a public, unauthenticated entry (self-service
    /// signup). The caller is responsible for all pre-flight checks (platform
    /// toggle, human verification, IP quota) before invoking this; the method
    /// intentionally performs **no** policy gate, unlike `create_realm`.
    /// `actor_realm_id` scopes the resulting audit events and must be the realm
    /// the signup originates from (the admin realm).
    fn create_realm_self_service(
        &self,
        request: CreateRealmRequest,
        actor_realm_id: String,
        ctx: AuditContext,
    ) -> impl Future<Output = Result<Realm, CoreError>> + Send;

    fn get_realm(
        &self,
        identity: Identity,
        id: String,
    ) -> impl Future<Output = Result<Realm, CoreError>> + Send;

    fn list_realms(
        &self,
        identity: Identity,
    ) -> impl Future<Output = Result<Vec<Realm>, CoreError>> + Send;

    fn list_realms_paginated(
        &self,
        identity: Identity,
        filters: ListRealmsFilters,
    ) -> impl Future<Output = Result<PaginatedRealmsResponse, CoreError>> + Send;

    fn update_realm(
        &self,
        identity: Identity,
        id: String,
        request: UpdateRealmRequest,
    ) -> impl Future<Output = Result<Realm, CoreError>> + Send;

    fn get_public_realm_info(
        &self,
        id: String,
    ) -> impl Future<Output = Result<RealmSummary, CoreError>> + Send;
}

// Service implementations
pub mod policies;
pub mod services;
pub mod validation;

#[cfg(test)]
mod tests;

pub use policies::RealmPolicy;
pub use validation::validate_realm_id;
