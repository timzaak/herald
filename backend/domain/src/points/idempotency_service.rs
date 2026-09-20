// Idempotency Service - Handles idempotency for points operations
//
// Ensures that duplicate requests with the same idempotency key
// return the same result, preventing duplicate charges.

use std::future::Future;
use std::sync::Arc;

use crate::common::entities::app_errors::CoreError;
use crate::points::entities::{IdempotencyResult, IdempotencyStatus, PointsTransaction};
use crate::points::errors::PointsErrorExt;

/// Idempotency Store Trait
///
/// Abstracts the storage backend for idempotency keys.
/// This allows the domain layer to depend on the trait instead of concrete Redis implementation.
pub trait IdempotencyStore: Send + Sync {
    /// Get cached transaction from the store
    fn get_from_cache(
        &self,
        cache_key: &str,
    ) -> impl Future<Output = Option<PointsTransaction>> + Send;

    /// Get the status of an idempotency key
    fn get_status_from_cache(
        &self,
        cache_key: &str,
    ) -> impl Future<Output = Option<IdempotencyStatus>> + Send;

    /// Try to create a lock for the idempotency key
    /// Returns Ok(true) if lock was created (new request)
    /// Returns Ok(false) if key already exists
    /// Returns Err if the operation failed
    fn try_create_lock(
        &self,
        cache_key: &str,
        request_data: &str,
    ) -> impl Future<Output = Result<bool, CoreError>> + Send;

    /// Atomically transition a Failed status marker back to Processing for a
    /// retry under the same key. Returns Ok(true) when this caller won the
    /// transition, Ok(false) when the marker is not Failed (another retry is
    /// already in flight, the record completed, or the marker is absent).
    /// Without this the failed→retrying classification alone is not one-shot:
    /// two concurrent retries would both read Failed and both re-execute.
    fn claim_failed_for_retry(
        &self,
        cache_key: &str,
    ) -> impl Future<Output = Result<bool, CoreError>> + Send;

    /// Save the result to the store
    fn save_to_cache(
        &self,
        cache_key: &str,
        transaction: &PointsTransaction,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// Mark an idempotency key as failed
    fn mark_failed(&self, cache_key: &str) -> impl Future<Output = Result<(), CoreError>> + Send;
}

/// Idempotency Service
///
/// Manages idempotency keys with pluggable storage backend.
/// Ensures idempotent behavior for financial operations.
pub struct IdempotencyService<S: IdempotencyStore> {
    store: Arc<S>,
}

impl<S: IdempotencyStore> IdempotencyService<S> {
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    /// Check or create an idempotency key
    ///
    /// Returns IdempotencyResult::New if the key is new (proceed with processing)
    /// Returns IdempotencyResult::Cached if the key exists and was completed (return cached response)
    /// Returns error if the key is currently being processed
    pub async fn check_or_create(
        &self,
        realm_id: &str,
        idempotency_key: &str,
        request_data: &str,
    ) -> Result<IdempotencyResult<PointsTransaction>, CoreError> {
        // First, try to get from cache
        let cache_key = Self::cache_key(realm_id, idempotency_key);
        let cached_result = self.store.get_from_cache(&cache_key).await;

        if let Some(transaction) = cached_result {
            tracing::info!(
                realm_id = %realm_id,
                idempotency_key = %idempotency_key,
                "Returning cached result for idempotency key"
            );
            return Ok(IdempotencyResult::Cached { transaction });
        }

        // Try to create the key with NX (only if not exists)
        let lock_result = self.store.try_create_lock(&cache_key, request_data).await;

        match lock_result {
            Ok(true) => {
                // Key created successfully, proceed with processing
                tracing::info!(
                    realm_id = %realm_id,
                    idempotency_key = %idempotency_key,
                    "Created new idempotency key"
                );
                Ok(IdempotencyResult::New)
            }
            Ok(false) => {
                // Key already exists, check status. The main key EXISTING
                // means a request under this key already started — its state
                // marker decides, and the marker's ABSENCE must never be
                // classified as "new" (audit run-2:
                // domain/points/idempotency/status-loss-failopen-double-consume):
                // the marker is lost on a failed status write, a crash between
                // fulfillment and save_result, or the 60s in-flight TTL
                // expiring under a long request, and the main key's value
                // still holds the raw request JSON until save_result runs.
                // Re-classifying any of those as New re-executes a financial
                // side effect whose outcome is unknown-or-completed.
                let status = self.store.get_status_from_cache(&cache_key).await;
                match status {
                    Some(IdempotencyStatus::Processing) => {
                        // Currently being processed
                        Err(CoreError::idempotency_processing())
                    }
                    // An explicitly failed request may be retried under the
                    // same key — no side effect landed for it. The
                    // failed→processing transition is an atomic claim: the
                    // loser of two concurrent redeliveries must not also
                    // classify as New and re-execute the financial side
                    // effect (review 20260920: failed-retry claim).
                    Some(IdempotencyStatus::Failed) => {
                        match self.store.claim_failed_for_retry(&cache_key).await {
                            Ok(true) => {
                                tracing::info!(
                                    realm_id = %realm_id,
                                    idempotency_key = %idempotency_key,
                                    "Prior attempt under this idempotency key failed; retrying"
                                );
                                Ok(IdempotencyResult::New)
                            }
                            Ok(false) => {
                                // Lost the claim — another retry is in
                                // flight or the state moved on. Re-read and
                                // reclassify instead of proceeding.
                                match self.store.get_status_from_cache(&cache_key).await {
                                    Some(IdempotencyStatus::Processing) => {
                                        Err(CoreError::idempotency_processing())
                                    }
                                    other => {
                                        tracing::warn!(
                                            realm_id = %realm_id,
                                            idempotency_key = %idempotency_key,
                                            status = ?other,
                                            "Idempotency retry lost the failed-claim race with an unknown outcome — refusing to re-execute"
                                        );
                                        Err(CoreError::Conflict(
                                            "Prior request under this idempotency key has an unknown outcome"
                                                .to_string(),
                                        ))
                                    }
                                }
                            }
                            // Store failure: fail closed — re-classifying as
                            // New without the claim would re-open the very
                            // double-execution window this guard closes.
                            Err(e) => {
                                tracing::error!(
                                    realm_id = %realm_id,
                                    idempotency_key = %idempotency_key,
                                    error = %e,
                                    "Failed to claim failed idempotency key for retry"
                                );
                                Err(CoreError::Conflict(
                                    "Prior request under this idempotency key has an unknown outcome"
                                        .to_string(),
                                ))
                            }
                        }
                    }
                    // Completed with an unreadable cached result, or marker
                    // lost: fail closed — the client may retry after the
                    // marker recovers or with a fresh key.
                    other => {
                        tracing::warn!(
                            realm_id = %realm_id,
                            idempotency_key = %idempotency_key,
                            status = ?other,
                            "Idempotency key exists with unknown/completed state marker — refusing to re-execute"
                        );
                        Err(CoreError::Conflict(
                            "Prior request under this idempotency key has an unknown outcome"
                                .to_string(),
                        ))
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    realm_id = %realm_id,
                    idempotency_key = %idempotency_key,
                    error = %e,
                    "Failed to check idempotency key"
                );
                // On store failure, allow the request to proceed but log the error
                // This is a graceful degradation to avoid blocking requests
                Ok(IdempotencyResult::New)
            }
        }
    }

    /// Save the result of an idempotent operation
    pub async fn save_result(
        &self,
        realm_id: &str,
        idempotency_key: &str,
        transaction: &PointsTransaction,
    ) -> Result<(), CoreError> {
        let cache_key = Self::cache_key(realm_id, idempotency_key);

        // Cache the result
        self.store.save_to_cache(&cache_key, transaction).await?;

        tracing::info!(
            realm_id = %realm_id,
            idempotency_key = %idempotency_key,
            transaction_id = %transaction.id,
            "Saved result for idempotency key"
        );

        Ok(())
    }

    /// Mark an idempotency key as failed
    pub async fn mark_failed(
        &self,
        realm_id: &str,
        idempotency_key: &str,
    ) -> Result<(), CoreError> {
        let cache_key = Self::cache_key(realm_id, idempotency_key);

        // Update status
        self.store.mark_failed(&cache_key).await?;

        tracing::info!(
            realm_id = %realm_id,
            idempotency_key = %idempotency_key,
            "Marked idempotency key as failed"
        );

        Ok(())
    }

    // Helper methods

    fn cache_key(realm_id: &str, idempotency_key: &str) -> String {
        idempotency_record_key(realm_id, idempotency_key)
    }
}

/// Redis key of the main idempotency record for a (scope, key) pair.
///
/// Public so callers that guard around the record itself — e.g. the consume
/// endpoint's fingerprint-horizon check, which must probe the exact record
/// the service reads and writes — derive the key from the same single
/// definition instead of re-spelling the format.
pub fn idempotency_record_key(scope: &str, idempotency_key: &str) -> String {
    format!("idempotency:{}:{}", scope, idempotency_key)
}
