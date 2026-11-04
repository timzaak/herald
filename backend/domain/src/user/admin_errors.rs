// User Admin Domain Errors

use crate::common::entities::app_errors::CoreError;

/// User Admin domain-specific errors
#[derive(Debug, thiserror::Error)]
pub enum UserAdminError {
    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("Role not found: {0}")]
    RoleNotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid role assignment: {0}")]
    InvalidRoleAssignment(String),

    #[error("Duplicate email: {0}")]
    DuplicateEmail(String),

    /// The target user is in the `Deleted(3)` anonymizing terminal state
    /// (users.md §4.2: 不可恢复). Admin mutations must not edit the tombstone.
    #[error("User is deleted: {0}")]
    UserDeleted(String),

    /// Operation not applicable in the target's current state (e.g. unbinding
    /// an unbound phone) — a client-visible 400, not an internal inconsistency.
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Phone already bound to another account in the realm — state conflict, 409.
    #[error("Duplicate phone: {0}")]
    DuplicatePhone(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

impl From<UserAdminError> for CoreError {
    fn from(err: UserAdminError) -> Self {
        match err {
            UserAdminError::UserNotFound(msg) => {
                tracing::debug!("User not found: {}", msg);
                CoreError::NotFound
            }
            UserAdminError::RoleNotFound(msg) => {
                tracing::debug!("Role not found: {}", msg);
                CoreError::NotFound
            }
            UserAdminError::PermissionDenied(msg) => CoreError::Forbidden(msg),
            UserAdminError::InvalidRoleAssignment(msg) => CoreError::BadRequest(msg),
            // Email conflicts are state conflicts (users.md §4.1: the ext and
            // self-service faces return 409, and the admin create/update
            // handlers map this explicitly) — CoreError::Conflict → 409.
            UserAdminError::DuplicateEmail(msg) => CoreError::Conflict(msg),
            // Deleted is a terminal state (users.md §4.2), so mutating it is a
            // state conflict — same convention as self_delete's already-deleted
            // rejection (CoreError::Conflict → 409).
            UserAdminError::UserDeleted(msg) => CoreError::Conflict(msg),
            UserAdminError::InvalidOperation(msg) => CoreError::BadRequest(msg),
            // Phone conflicts mirror DuplicateEmail: state conflict → 409.
            UserAdminError::DuplicatePhone(msg) => CoreError::Conflict(msg),
            UserAdminError::DatabaseError(msg) => CoreError::InternalServerError(msg),
            UserAdminError::InternalError(msg) => CoreError::InternalServerError(msg),
        }
    }
}

pub type UserAdminResult<T> = Result<T, UserAdminError>;
