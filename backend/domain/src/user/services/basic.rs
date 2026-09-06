// User service implementations

use std::sync::Arc;
use uuid::Uuid;

use crate::{
    authentication::Identity,
    common::{
        entities::app_errors::CoreError,
        policies::{UserPolicy, ensure_policy},
    },
    security_constants::DEFAULT_BCRYPT_COST,
    user::{
        entities::User,
        ports::{UserRepository, UserService, UserVerificationRepository},
        value_objects::{CreateUserRequest, LoginRequest, RegisterRequest, UpdateUserRequest},
    },
};

fn parse_reset_code_realm_id(code: &str) -> Result<&str, CoreError> {
    let mut parts = code.rsplitn(3, '_');
    let timestamp = parts.next();
    let uuid = parts.next();
    let realm_id = parts.next();

    match (realm_id, uuid, timestamp) {
        (Some(realm_id), Some(_uuid), Some(_timestamp)) if !realm_id.is_empty() => Ok(realm_id),
        _ => Err(CoreError::BadRequest("invalid reset code".to_string())),
    }
}

pub struct UserServiceImpl<R, V, P>
where
    R: UserRepository,
    V: UserVerificationRepository,
    P: UserPolicy,
{
    pub(crate) user_repository: Arc<R>,
    pub(crate) verification_repository: Arc<V>,
    pub(crate) policy: Arc<P>,
}

impl<R, V, P> UserServiceImpl<R, V, P>
where
    R: UserRepository,
    V: UserVerificationRepository,
    P: UserPolicy,
{
    pub fn new(user_repository: Arc<R>, verification_repository: Arc<V>, policy: Arc<P>) -> Self {
        Self {
            user_repository,
            verification_repository,
            policy,
        }
    }

    async fn hash_password(&self, password: &str) -> Result<String, CoreError> {
        bcrypt::hash(password, DEFAULT_BCRYPT_COST)
            .map_err(|_| CoreError::InternalServerError("Password hashing failed".to_string()))
    }
}

impl<R, V, P> UserService for UserServiceImpl<R, V, P>
where
    R: UserRepository,
    V: UserVerificationRepository,
    P: UserPolicy,
{
    async fn create_user(
        &self,
        identity: Identity,
        request: CreateUserRequest,
    ) -> Result<User, CoreError> {
        ensure_policy(
            self.policy.can_create_user(identity.clone()).await,
            "Insufficient permissions to create user",
        )?;

        // CRITICAL: Realm boundary check - prevent cross-realm user creation
        if !identity.has_access_to_realm(&request.realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot create user in a different realm".to_string(),
            ));
        }

        // Validate password if provided
        if let Some(ref password) = request.password {
            if password.len() < 8 || password.len() > 100 {
                return Err(CoreError::BadRequest("Password length invalid".to_string()));
            }
            let password_hash = self.hash_password(password).await?;
            let user = self
                .user_repository
                .create_user(request, Some(password_hash))
                .await?;
            Ok(user)
        } else {
            // OAuth user without password
            self.user_repository.create_user(request, None).await
        }
    }

    async fn get_user(&self, identity: Identity, id: Uuid) -> Result<User, CoreError> {
        // Policy check
        ensure_policy(
            self.policy.can_read_user(identity.clone()).await,
            "Insufficient permissions to read user",
        )?;

        // Get user and check realm boundary
        let user = self.user_repository.get_user_by_id(id).await?;

        // CRITICAL: Realm boundary check - prevent cross-realm access
        if !identity.has_access_to_realm(&user.realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: user belongs to a different realm".to_string(),
            ));
        }

        Ok(user)
    }

    async fn list_users(
        &self,
        identity: Identity,
        realm_id: String,
        page: u64,
        page_size: u64,
        email: Option<String>,
        status: Option<i16>,
    ) -> Result<(Vec<User>, i64), CoreError> {
        // Policy check
        ensure_policy(
            self.policy.can_list_users(identity.clone()).await,
            "Insufficient permissions to list users",
        )?;

        // CRITICAL: Realm boundary check - prevent cross-realm access
        if !identity.has_access_to_realm(&realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot list users from a different realm".to_string(),
            ));
        }

        self.user_repository
            .list_users(&realm_id, page, page_size, email, status)
            .await
    }

    async fn update_user(
        &self,
        identity: Identity,
        id: Uuid,
        request: UpdateUserRequest,
    ) -> Result<User, CoreError> {
        // Policy check
        ensure_policy(
            self.policy.can_update_user(identity.clone()).await,
            "Insufficient permissions to update user",
        )?;

        // Get user and check realm boundary
        let user = self.user_repository.get_user_by_id(id).await?;

        // CRITICAL: Realm boundary check - prevent cross-realm access
        if !identity.has_access_to_realm(&user.realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot update user from a different realm".to_string(),
            ));
        }

        self.user_repository.update_user(id, request).await
    }

    async fn delete_user(&self, identity: Identity, id: Uuid) -> Result<(), CoreError> {
        // Policy check
        ensure_policy(
            self.policy.can_delete_user(identity.clone()).await,
            "Insufficient permissions to delete user",
        )?;

        // Get user and check realm boundary
        let user = self.user_repository.get_user_by_id(id).await?;

        // CRITICAL: Realm boundary check - prevent cross-realm access
        if !identity.has_access_to_realm(&user.realm_id) {
            return Err(CoreError::Forbidden(
                "Access denied: cannot delete user from a different realm".to_string(),
            ));
        }

        self.user_repository.delete_user(id).await
    }

    async fn verify_email(&self, code: &str, realm_id: &str) -> Result<User, CoreError> {
        // Get email from verification code
        let email = self
            .verification_repository
            .get_email_by_code(realm_id, code)
            .await?
            .ok_or(CoreError::BadRequest(
                "verification code not found".to_string(),
            ))?;

        // Find user by email
        let user = self
            .user_repository
            .get_user_by_email(realm_id, &email)
            .await?;

        // Update user status to active
        self.user_repository.update_user_status(user.id, 1).await?;

        // Consume verification code
        self.verification_repository
            .consume_code(realm_id, code)
            .await?;

        Ok(user)
    }

    async fn verify_email_trigger(
        &self,
        realm_id: &str,
        email: &str,
        code_type: &str,
    ) -> Result<String, CoreError> {
        let code = format!(
            "{}_{}_{}",
            realm_id,
            uuid::Uuid::now_v7(),
            chrono::Utc::now().timestamp()
        );
        self.verification_repository
            .create_verification_code(realm_id, email, code_type, &code)
            .await?;
        Ok(code)
    }

    #[tracing::instrument(
        // Governance: request carries password (credential)
        // + email/username (PII) + realm_id. self holds the user repo.
        // Only the low-cardinality operation type is recorded.
        skip(self, request),
        fields(db.system = "postgres", db.operation = "user_login")
    )]
    async fn login(&self, request: LoginRequest) -> Result<User, CoreError> {
        // Use get_user_by_email_or_username to handle both email and username
        let lookup = self
            .user_repository
            .get_user_by_email_or_username(
                &request.realm_id,
                request.email.clone(),
                request.username.clone(),
            )
            .await?;

        let (user_id, password_hash, status) = match lookup {
            Some(found) => found,
            None => {
                // Burn a bcrypt verification so the unknown-identifier path
                // costs the same as the known-identifier path; without it,
                // response latency reveals whether the email is registered.
                let _ = bcrypt::verify(
                    &request.password,
                    crate::security_constants::DUMMY_BCRYPT_HASH,
                );
                return Err(CoreError::NotFound);
            }
        };

        // Verify the password BEFORE any account-status check. Status checks
        // return a distinct Forbidden error; running them first would let an
        // unauthenticated caller probe whether an email is registered (and in
        // which state) by error shape alone. Only a caller holding the correct
        // password may observe status differences.
        let Some(stored_hash) = password_hash else {
            // OAuth-only account (no password set): equalize timing, then
            // stay indistinguishable from an unknown identifier.
            let _ = bcrypt::verify(
                &request.password,
                crate::security_constants::DUMMY_BCRYPT_HASH,
            );
            return Err(CoreError::NotFound);
        };

        let password_valid = bcrypt::verify(&request.password, &stored_hash).map_err(|_| {
            CoreError::InternalServerError("Password verification failed".to_string())
        })?;

        if !password_valid {
            return Err(CoreError::Unauthorized);
        }

        // Password proven — status differences may now surface.
        if status != 1 {
            return Err(CoreError::Forbidden(
                "User account is not active".to_string(),
            ));
        }

        // Get the full user object
        self.user_repository.get_user_by_id(user_id).await
    }

    #[tracing::instrument(
        // Governance: request carries password (credential)
        // + email (PII) + realm_id. self holds the user repo.
        // Only the low-cardinality operation type is recorded.
        skip(self, request),
        fields(db.system = "postgres", db.operation = "user_register")
    )]
    async fn register(&self, request: RegisterRequest) -> Result<User, CoreError> {
        match self
            .user_repository
            .get_user_by_email(&request.realm_id, &request.email)
            .await
        {
            Ok(_) => return Err(CoreError::Conflict("Email already registered".to_string())),
            Err(CoreError::NotFound) => {}
            Err(e) => return Err(e),
        }

        // Hash password
        let password_hash = self.hash_password(&request.password).await?;

        let create_request = CreateUserRequest {
            realm_id: request.realm_id,
            email: request.email,
            password: None,
            provider_ids: None,
        };

        let user = self
            .user_repository
            .create_user(create_request, Some(password_hash))
            .await?;

        // TODO: Send verification email

        Ok(user)
    }

    async fn create_user_without_password(
        &self,
        request: CreateUserRequest,
    ) -> Result<User, CoreError> {
        match self
            .user_repository
            .get_user_by_email(&request.realm_id, &request.email)
            .await
        {
            Ok(_) => return Err(CoreError::Conflict("Email already registered".to_string())),
            Err(CoreError::NotFound) => {}
            Err(e) => return Err(e),
        }

        // Create user with no password (OAuth user)
        let user = self.user_repository.create_user(request, None).await?;

        // OAuth users are automatically verified
        // Note: You may want to update the user status to Normal
        // This depends on your repository implementation

        Ok(user)
    }

    async fn create_user_without_identity_check(
        &self,
        request: CreateUserRequest,
    ) -> Result<User, CoreError> {
        // Validate password if provided
        if let Some(ref password) = request.password {
            if password.len() < 8 || password.len() > 100 {
                return Err(CoreError::BadRequest("Password length invalid".to_string()));
            }
            let password_hash = self.hash_password(password).await?;
            let user = self
                .user_repository
                .create_user(request, Some(password_hash))
                .await?;
            Ok(user)
        } else {
            // OAuth user without password
            self.user_repository.create_user(request, None).await
        }
    }

    async fn change_password(
        &self,
        realm_id: &str,
        user_id: Uuid,
        old_password: String,
        new_password: String,
    ) -> Result<(), CoreError> {
        // Get current user to verify old password
        let user = self.user_repository.get_user_by_id(user_id).await?;

        // Verify old password
        let password_hash = user
            .password_hash
            .as_ref()
            .ok_or_else(|| CoreError::InternalServerError("Password hash not found".to_string()))?;

        let old_valid = bcrypt::verify(&old_password, password_hash).map_err(|_| {
            CoreError::InternalServerError("Password verification failed".to_string())
        })?;

        if !old_valid {
            return Err(CoreError::Unauthorized);
        }

        // Hash new password
        let new_password_hash = self.hash_password(&new_password).await?;

        // Update password
        self.user_repository
            .change_password(realm_id, user_id, new_password_hash)
            .await
    }

    async fn reset_password_request(
        &self,
        realm_id: &str,
        email: &str,
        code_type: &str,
    ) -> Result<String, CoreError> {
        let code = format!(
            "{}_{}_{}",
            realm_id,
            uuid::Uuid::now_v7(),
            chrono::Utc::now().timestamp()
        );
        self.verification_repository
            .create_verification_code(realm_id, email, code_type, &code)
            .await?;
        Ok(code)
    }

    async fn reset_password_confirm(
        &self,
        code: &str,
        new_password: String,
        realm_id: &str,
    ) -> Result<Uuid, CoreError> {
        let code_realm_id = parse_reset_code_realm_id(code)?;

        if code_realm_id != realm_id {
            return Err(CoreError::BadRequest(
                "reset code realm mismatch".to_string(),
            ));
        }

        // Get email from verification code
        let email = self
            .verification_repository
            .get_email_by_code(realm_id, code)
            .await?
            .ok_or(CoreError::BadRequest("reset code not found".to_string()))?;

        // Find user by email
        let user = self
            .user_repository
            .get_user_by_email(realm_id, &email)
            .await?;

        // Hash new password
        let new_password_hash = self.hash_password(&new_password).await?;

        // Update password
        self.user_repository
            .change_password(realm_id, user.id, new_password_hash)
            .await?;

        // Consume verification code
        self.verification_repository
            .consume_code(realm_id, code)
            .await?;

        Ok(user.id)
    }

    async fn activate_user(&self, user_id: Uuid) -> Result<(), CoreError> {
        // Update user status to active (status = 1)
        self.user_repository.update_user_status(user_id, 1).await
    }
}

impl<R, V, P> std::fmt::Debug for UserServiceImpl<R, V, P>
where
    R: UserRepository,
    V: UserVerificationRepository,
    P: UserPolicy,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserServiceImpl").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::policies::AllowAllUserPolicy;
    use crate::user::{
        entities::{User, UserStatus},
        ports::{MockUserRepository, MockUserVerificationRepository, UserService},
    };
    use chrono::Utc;
    use std::pin::Pin;
    use std::sync::Arc;
    use uuid::Uuid;

    fn test_user(realm_id: &str, email: &str) -> User {
        User {
            id: Uuid::now_v7(),
            realm_id: realm_id.to_string(),
            email: email.to_string(),
            nickname: None,
            password_hash: Some("existing-hash".to_string()),
            provider_ids: vec![],
            status: UserStatus::Normal,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn reset_password_confirm_accepts_realm_ids_with_underscores() {
        let realm_id = "test_realm";
        let email = "user@example.com";
        let user = test_user(realm_id, email);
        let user_id = user.id;
        let code = format!("{}_{}_{}", realm_id, Uuid::now_v7(), Utc::now().timestamp());

        let mut user_repository = MockUserRepository::new();
        user_repository
            .expect_get_user_by_email()
            .withf(move |actual_realm_id, actual_email| {
                actual_realm_id == realm_id && actual_email == email
            })
            .times(1)
            .return_once(move |_, _| {
                Box::pin(async move { Ok(user) })
                    as Pin<Box<dyn Future<Output = Result<User, CoreError>> + Send>>
            });
        user_repository
            .expect_change_password()
            .withf(move |actual_realm_id, actual_user_id, new_password_hash| {
                actual_realm_id == realm_id
                    && *actual_user_id == user_id
                    && !new_password_hash.is_empty()
            })
            .times(1)
            .return_once(|_, _, _| {
                Box::pin(async { Ok(()) })
                    as Pin<Box<dyn Future<Output = Result<(), CoreError>> + Send>>
            });

        let mut verification_repository = MockUserVerificationRepository::new();
        let code_for_lookup = code.clone();
        verification_repository
            .expect_get_email_by_code()
            .withf(move |actual_realm_id, actual_code| {
                actual_realm_id == realm_id && *actual_code == code_for_lookup
            })
            .times(1)
            .return_once(move |_, _| {
                Box::pin(async move { Ok(Some(email.to_string())) })
                    as Pin<Box<dyn Future<Output = Result<Option<String>, CoreError>> + Send>>
            });
        let code_for_consume = code.clone();
        verification_repository
            .expect_consume_code()
            .withf(move |actual_realm_id, actual_code| {
                actual_realm_id == realm_id && *actual_code == code_for_consume
            })
            .times(1)
            .return_once(|_, _| {
                Box::pin(async { Ok(()) })
                    as Pin<Box<dyn Future<Output = Result<(), CoreError>> + Send>>
            });

        let service = UserServiceImpl::new(
            Arc::new(user_repository),
            Arc::new(verification_repository),
            Arc::new(AllowAllUserPolicy),
        );

        let confirmed_user_id = service
            .reset_password_confirm(&code, "new-password-123".to_string(), realm_id)
            .await
            .expect("reset password should succeed");
        assert_eq!(confirmed_user_id, user_id);
    }

    #[tokio::test]
    async fn reset_password_confirm_rejects_invalid_code_structure() {
        let service = UserServiceImpl::new(
            Arc::new(MockUserRepository::new()),
            Arc::new(MockUserVerificationRepository::new()),
            Arc::new(AllowAllUserPolicy),
        );

        let err = service
            .reset_password_confirm("invalid-code", "new-password-123".to_string(), "test_realm")
            .await
            .expect_err("invalid code should be rejected");

        assert_eq!(err, CoreError::BadRequest("invalid reset code".to_string()));
    }
}

// Governance tests.
//
// Covers: `UserServiceImpl` login / register instrument skip
// correctness.
//
// WHY: login/register take `request` (carries password + email/username +
// realm_id — credential + PII). If the `#[instrument]` macro ever stops
// skipping `request`, the password/email leaks into a span field. Source-scan
// baseline, anchored per method to the immediately-preceding
// `#[tracing::instrument(...)]`.
#[cfg(test)]
mod instrument_skip_tests {
    const SRC: &str = include_str!("basic.rs");

    fn instrument_body_preceding(fn_name: &str) -> String {
        let needle = format!("fn {fn_name}");
        let fn_pos = SRC
            .find(&needle)
            .unwrap_or_else(|| panic!("fn {fn_name} not found in source"));
        let attr_start = SRC[..fn_pos]
            .rfind("#[tracing::instrument(")
            .unwrap_or_else(|| panic!("no #[tracing::instrument( preceding fn {fn_name}"));
        let body_start = attr_start + "#[tracing::instrument(".len();
        // Find the attribute close: the first line at/after body_start whose
        // trimmed content is exactly `)]`. This handles indented closes (e.g.
        // inside an `impl` block) and ignores inline `))]` sequences such as
        // `#[validate(length(...))]` that appear on struct fields.
        let tail = &SRC[body_start..];
        let mut consumed = 0usize;
        for line in tail.lines() {
            let prev = consumed;
            consumed += line.len() + 1; // +1 for the line separator
            if line.trim() == ")]" {
                return tail[..prev].to_string();
            }
        }
        panic!("unterminated #[tracing::instrument( for fn {fn_name}")
    }

    #[test]
    fn instrument_skip_user_service_login_excludes_password_email() {
        let body = instrument_body_preceding("login");
        assert!(
            body.contains("request"),
            "UserServiceImpl::login must skip `request` (carries password/email); body was:\n{body}"
        );
        for banned in ["password", "email", "token", "secret"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "user login span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }

    #[test]
    fn instrument_skip_user_service_register_excludes_password_email() {
        let body = instrument_body_preceding("register");
        assert!(
            body.contains("request"),
            "UserServiceImpl::register must skip `request` (carries password/email); body was:\n{body}"
        );
        for banned in ["password", "email", "token", "secret"] {
            assert!(
                !body.contains(&format!("{banned} ="))
                    && !body.contains(&format!("fields({banned}")),
                "user register span must not record a `{banned}` field; body was:\n{body}"
            );
        }
    }
}
