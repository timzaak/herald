// Points-specific error types

use crate::common::entities::app_errors::CoreError;

/// Trait for Points-specific error extensions
pub trait PointsErrorExt {
    /// Points wallet not found
    fn wallet_not_found(user_id: &str) -> Self;

    /// Insufficient points balance
    fn insufficient_points(required: i64, available: i64) -> Self;

    /// Invalid points amount
    fn invalid_amount(reason: &str) -> Self;

    /// Points plan config not found
    fn plan_config_not_found(plan_id: &str) -> Self;

    /// Concurrent modification (optimistic lock)
    fn concurrent_modification() -> Self;

    /// Integer overflow error
    fn overflow_error(reason: &str) -> Self;

    /// Idempotency key is already being processed
    fn idempotency_processing() -> Self;
}

/// Points-specific error variants that extend CoreError
impl PointsErrorExt for CoreError {
    /// Points wallet not found
    fn wallet_not_found(user_id: &str) -> Self {
        tracing::debug!("Points wallet not found for user: {}", user_id);
        CoreError::NotFound
    }

    /// Insufficient points balance
    fn insufficient_points(required: i64, available: i64) -> Self {
        tracing::debug!(
            "Insufficient points: required={}, available={}",
            required,
            available
        );
        CoreError::BadRequest(format!(
            "Insufficient points balance. Required: {}, Available: {}",
            required, available
        ))
    }

    /// Invalid points amount
    fn invalid_amount(reason: &str) -> Self {
        tracing::warn!("Invalid points amount: {}", reason);
        CoreError::BadRequest(format!("Invalid points amount: {}", reason))
    }

    /// Points plan config not found
    fn plan_config_not_found(plan_id: &str) -> Self {
        tracing::debug!("Points plan config not found for plan: {}", plan_id);
        CoreError::NotFound
    }

    /// Concurrent modification (optimistic lock)
    fn concurrent_modification() -> Self {
        tracing::debug!("Concurrent modification detected in points wallet");
        CoreError::Conflict(
            "Wallet was modified by another transaction. Please try again.".to_string(),
        )
    }

    /// Integer overflow error
    fn overflow_error(reason: &str) -> Self {
        tracing::warn!("Integer overflow detected: {}", reason);
        CoreError::BadRequest(format!("Arithmetic overflow: {}", reason))
    }

    /// Idempotency key is already being processed
    fn idempotency_processing() -> Self {
        tracing::debug!("Idempotency key is already being processed");
        CoreError::Conflict(
            "Request with this idempotency key is already being processed. Please wait."
                .to_string(),
        )
    }
}
