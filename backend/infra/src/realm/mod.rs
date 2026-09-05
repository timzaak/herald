use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use std::sync::Arc;

use herald_domain::common::entities::app_errors::CoreError;
use herald_domain::realm::validate_realm_id;
use herald_domain::realm::{
    CreateRealmRequest, ListRealmsFilters, PaginatedRealmsResponse, PaginationResponse, Realm,
    RealmRepository,
};
use herald_entity::realm;

pub struct PostgresRealmRepository {
    db: Arc<sea_orm::DatabaseConnection>,
}

impl PostgresRealmRepository {
    pub fn new(db: Arc<sea_orm::DatabaseConnection>) -> Self {
        Self { db }
    }

    fn to_domain(model: &realm::Model) -> Realm {
        Realm {
            id: model.id.clone(),
            name: model.name.clone(),
            description: model.description.clone(),
            created_at: model.created_at.into(),
            updated_at: model.updated_at.into(),
            admin_user: None, // Not stored in database, only returned on creation
        }
    }
}

impl RealmRepository for PostgresRealmRepository {
    async fn create_realm(&self, request: CreateRealmRequest) -> Result<Realm, CoreError> {
        let now = chrono::Utc::now();

        // Process realm ID
        let realm_id = if let Some(custom_id) = &request.id {
            // User-specified ID - validate using domain validation function
            validate_realm_id(custom_id)?;

            // Check if realm already exists
            match self.get_realm_by_id(custom_id).await {
                Ok(_) => {
                    return Err(CoreError::BadRequest(format!(
                        "Realm with ID '{}' already exists",
                        custom_id
                    )));
                }
                Err(CoreError::NotFound) => {
                    // OK, realm doesn't exist
                }
                Err(e) => return Err(e),
            }

            custom_id.clone()
        } else {
            // Use UUID v7 (time-ordered, better for indexing and sorting)
            herald_domain::common::entities::generate_uuid_v7().to_string()
        };

        // Debug logging before insert
        tracing::info!(
            realm_id_before_insert = ?realm_id,
            realm_name = %request.name,
            "realm_repository: About to insert into database"
        );

        let active_model = realm::ActiveModel {
            id: sea_orm::Set(realm_id.clone()),
            name: sea_orm::Set(request.name),
            description: sea_orm::Set(request.description),
            created_at: sea_orm::Set(now.into()),
            updated_at: sea_orm::Set(now.into()),
        };

        let result = active_model.insert(&*self.db).await?;

        // Debug logging after insert
        tracing::info!(
            realm_id_after_insert = ?result.id,
            "realm_repository: Inserted into database"
        );

        Ok(Self::to_domain(&result))
    }

    async fn get_realm_by_id(&self, id: &str) -> Result<Realm, CoreError> {
        let result = realm::Entity::find_by_id(id.to_string())
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        Ok(Self::to_domain(&result))
    }

    async fn get_realm_by_name(&self, name: &str) -> Result<Realm, CoreError> {
        let result = realm::Entity::find()
            .filter(realm::Column::Name.eq(name))
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?;

        Ok(Self::to_domain(&result))
    }

    async fn list_realms(&self) -> Result<Vec<Realm>, CoreError> {
        let results = realm::Entity::find().all(&*self.db).await?;

        Ok(results.iter().map(Self::to_domain).collect())
    }

    async fn list_realms_paginated(
        &self,
        filters: ListRealmsFilters,
    ) -> Result<PaginatedRealmsResponse, CoreError> {
        // Validate page bounds
        let page = filters.page;
        let page_size = filters.page_size.clamp(1, 100);
        let offset = page * page_size;

        // Build base query
        let mut query = realm::Entity::find();

        if let Some(realm_id) = &filters.accessible_realm_id {
            query = query.filter(realm::Column::Id.eq(realm_id));
        }

        // Apply search filter
        if let Some(search_term) = &filters.search {
            let search_pattern = format!("%{}%", search_term);
            query = query.filter(
                realm::Column::Id
                    .like(&search_pattern)
                    .or(realm::Column::Name.like(&search_pattern)),
            );
        }

        // Get total count
        let total = query.clone().count(&*self.db).await? as i64;

        // Apply sorting
        let sort_by = filters.sort_by.as_deref().unwrap_or("created_at");
        let sort_order = filters.sort_order.as_deref().unwrap_or("desc");

        query = match sort_by {
            "id" | "realm_id" => {
                if sort_order == "asc" {
                    query.order_by_asc(realm::Column::Id)
                } else {
                    query.order_by_desc(realm::Column::Id)
                }
            }
            "name" => {
                if sort_order == "asc" {
                    query.order_by_asc(realm::Column::Name)
                } else {
                    query.order_by_desc(realm::Column::Name)
                }
            }
            "created_at" => {
                if sort_order == "asc" {
                    query.order_by_asc(realm::Column::CreatedAt)
                } else {
                    query.order_by_desc(realm::Column::CreatedAt)
                }
            }
            "updated_at" => {
                if sort_order == "asc" {
                    query.order_by_asc(realm::Column::UpdatedAt)
                } else {
                    query.order_by_desc(realm::Column::UpdatedAt)
                }
            }
            _ => {
                // Default to created_at desc for invalid sort_by
                query.order_by_desc(realm::Column::CreatedAt)
            }
        };

        // Apply pagination
        let results = query.limit(page_size).offset(offset).all(&*self.db).await?;

        let realms = results.iter().map(Self::to_domain).collect();

        // Calculate total pages
        let total_pages = if total == 0 {
            0
        } else {
            ((total as u64 - 1) / page_size) + 1
        };

        Ok(PaginatedRealmsResponse {
            realms,
            pagination: PaginationResponse {
                page,
                page_size,
                total,
                total_pages,
            },
        })
    }

    async fn update_realm(
        &self,
        id: &str,
        name: String,
        description: Option<String>,
    ) -> Result<Realm, CoreError> {
        let mut active_model: realm::ActiveModel = realm::Entity::find_by_id(id.to_string())
            .one(&*self.db)
            .await?
            .ok_or(CoreError::NotFound)?
            .into();

        active_model.name = sea_orm::Set(name);
        active_model.description = sea_orm::Set(description);
        active_model.updated_at = sea_orm::Set(chrono::Utc::now().into());

        let result = active_model.update(&*self.db).await?;
        Ok(Self::to_domain(&result))
    }

    async fn delete_realm(&self, _id: &str) -> Result<(), CoreError> {
        // Realm deletion is not supported
        todo!("Realm deletion is not supported")
    }
}
