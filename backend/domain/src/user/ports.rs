use crate::authentication::Identity;
use crate::common::entities::app_errors::CoreError;
use crate::user::{
    entities::{Profile, User},
    value_objects::{CreateUserRequest, LoginRequest, RegisterRequest, UpdateUserRequest},
};
use std::future::Future;
use uuid::Uuid;

// ============================================================================
// Repository Ports (Traits)
// ============================================================================

#[cfg_attr(test, mockall::automock)]
pub trait UserRepository: Send + Sync {
    fn create_user(
        &self,
        request: CreateUserRequest,
        password_hash: Option<String>,
    ) -> impl Future<Output = Result<User, CoreError>> + Send;

    fn get_user_by_id(&self, id: Uuid) -> impl Future<Output = Result<User, CoreError>> + Send;

    fn get_user_by_email(
        &self,
        realm_id: &str,
        email: &str,
    ) -> impl Future<Output = Result<User, CoreError>> + Send;

    fn get_user_by_email_or_username(
        &self,
        realm_id: &str,
        email: Option<String>,
        username: Option<String>,
    ) -> impl Future<Output = Result<Option<(Uuid, Option<String>, i16)>, CoreError>> + Send;

    fn change_password(
        &self,
        realm_id: &str,
        user_id: Uuid,
        new_password_hash: String,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    fn update_user_status(
        &self,
        user_id: Uuid,
        status: i16,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    fn list_users(
        &self,
        realm_id: &str,
        page: u64,
        page_size: u64,
        email: Option<String>,
    ) -> impl Future<Output = Result<(Vec<User>, i64), CoreError>> + Send;

    fn update_user(
        &self,
        id: Uuid,
        request: UpdateUserRequest,
    ) -> impl Future<Output = Result<User, CoreError>> + Send;

    fn delete_user(&self, id: Uuid) -> impl Future<Output = Result<(), CoreError>> + Send;

    fn create_profile(
        &self,
        profile: Profile,
    ) -> impl Future<Output = Result<Profile, CoreError>> + Send;

    fn get_profile(&self, user_id: Uuid)
    -> impl Future<Output = Result<Profile, CoreError>> + Send;

    /// Update profile fields. The nickname uses `Option<Option<T>>`
    /// tri-state: `None` leaves the value unchanged, `Some(None)` clears it,
    /// `Some(Some(v))` sets it.
    fn update_profile(
        &self,
        user_id: Uuid,
        nickname: Option<Option<String>>,
    ) -> impl Future<Output = Result<Profile, CoreError>> + Send;

    /// Look up a soft-deleted account by the hash of its original email.
    ///
    /// Used by the login path after an active-user lookup fails, so that a
    /// login attempt with the original email of a deleted account can return
    /// the same `Forbidden` response used for other inactive statuses.
    fn find_deleted_user_by_email_hash(
        &self,
        realm_id: &str,
        email_hash: &str,
    ) -> impl Future<Output = Result<Option<(Uuid, i16)>, CoreError>> + Send;

    /// Anonymize a user's PII and mark the account as soft-deleted.
    ///
    /// Single atomic transaction:
    ///   - `account`: `status = 4` (Deleted),
    ///     `email = "deleted+{id}@anonymized.local"` (derived from the account
    ///     id so it is unique within `(realm_id, email)`), `password = NULL`,
    ///     `username = NULL`, `provider_ids = '{}'`,
    ///     `deleted_original_email_hash = SHA-256(original_email)`.
    ///   - `profile`: `nickname = NULL` (no-op if the optional profile row is
    ///     absent — 0 rows affected is not an error).
    ///   - `user_totp_config`: cascade-deleted (also drops backup codes).
    ///
    /// Used by the self-service deletion service. Implementations must run all
    /// three writes inside one DB transaction so the anonymization is atomic.
    fn anonymize_user_for_deletion(
        &self,
        realm_id: &str,
        user_id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}

#[cfg_attr(test, mockall::automock)]
pub trait UserVerificationRepository: Send + Sync {
    fn create_verification_code(
        &self,
        realm_id: &str,
        email: &str,
        code_type: &str,
        code: &str,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    fn verify_code(
        &self,
        realm_id: &str,
        email: &str,
        code_type: &str,
        code: &str,
    ) -> impl Future<Output = Result<bool, CoreError>> + Send;

    fn consume_code(
        &self,
        realm_id: &str,
        code: &str,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// Get email address by verification code within a realm
    fn get_email_by_code(
        &self,
        realm_id: &str,
        code: &str,
    ) -> impl Future<Output = Result<Option<String>, CoreError>> + Send;

    /// Delete verification codes by type for a specific email within a realm
    fn delete_code_by_type(
        &self,
        realm_id: &str,
        email: &str,
        code_type: &str,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}

// ============================================================================
// Service Ports (Traits)
// ============================================================================

#[cfg_attr(test, mockall::automock)]
pub trait UserService: Send + Sync {
    fn create_user(
        &self,
        identity: Identity,
        request: CreateUserRequest,
    ) -> impl Future<Output = Result<User, CoreError>> + Send;

    fn get_user(
        &self,
        identity: Identity,
        id: Uuid,
    ) -> impl Future<Output = Result<User, CoreError>> + Send;

    fn list_users(
        &self,
        identity: Identity,
        realm_id: String,
        page: u64,
        page_size: u64,
        email: Option<String>,
    ) -> impl Future<Output = Result<(Vec<User>, i64), CoreError>> + Send;

    fn update_user(
        &self,
        identity: Identity,
        id: Uuid,
        request: UpdateUserRequest,
    ) -> impl Future<Output = Result<User, CoreError>> + Send;

    fn delete_user(
        &self,
        identity: Identity,
        id: Uuid,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    fn verify_email(
        &self,
        code: &str,
        realm_id: &str,
    ) -> impl Future<Output = Result<User, CoreError>> + Send;

    fn verify_email_trigger(
        &self,
        realm_id: &str,
        email: &str,
        code_type: &str,
    ) -> impl Future<Output = Result<String, CoreError>> + Send;

    fn login(&self, request: LoginRequest) -> impl Future<Output = Result<User, CoreError>> + Send;

    fn register(
        &self,
        request: RegisterRequest,
    ) -> impl Future<Output = Result<User, CoreError>> + Send;

    /// Create user without password (for OAuth)
    fn create_user_without_password(
        &self,
        request: CreateUserRequest,
    ) -> impl Future<Output = Result<User, CoreError>> + Send;

    /// Create user without identity/realm boundary checks
    /// For internal use by system operations (e.g., realm initialization)
    fn create_user_without_identity_check(
        &self,
        request: CreateUserRequest,
    ) -> impl Future<Output = Result<User, CoreError>> + Send;

    fn change_password(
        &self,
        realm_id: &str,
        user_id: Uuid,
        old_password: String,
        new_password: String,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    fn reset_password_request(
        &self,
        realm_id: &str,
        email: &str,
        code_type: &str,
    ) -> impl Future<Output = Result<String, CoreError>> + Send;

    /// Returns the id of the user whose password was reset, so callers can
    /// revoke their sessions after a successful reset.
    fn reset_password_confirm(
        &self,
        code: &str,
        new_password: String,
        realm_id: &str,
    ) -> impl Future<Output = Result<Uuid, CoreError>> + Send;

    /// Activate user account (for realms without email verification)
    fn activate_user(&self, user_id: Uuid) -> impl Future<Output = Result<(), CoreError>> + Send;
}
