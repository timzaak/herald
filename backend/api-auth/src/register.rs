use axum::{
    Json,
    extract::{Path, State},
};
use axum_valid::Valid;
use herald_api_base::application::http::auth::util::{
    ClientIp, is_email_verification_required, is_registration_email_domain_allowed,
    is_registration_enabled, normalize_email, rate_limit_hit, verify_turnstile_for_client_app,
};
use herald_api_base::application::http::common::public_helper::realm_public_url_parts;
pub use herald_api_base::application::http::server::api_entities::ErrorResponse;
use herald_api_base::application::http::server::api_entities::{ApiError, ApiResult};
use herald_api_base::application::http::state::AppState;
use herald_core::domain::security_constants::{REGISTER_EMAIL_RATE_LIMIT, REGISTER_IP_RATE_LIMIT};
use herald_core::domain::user::ports::UserService;
use herald_core::domain::user::value_objects::RegisterRequest as DomainRegisterRequest;
use herald_core::third::email::{EmailService, EmailTemplateKind};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::mailflow::{self, MailflowType};

#[derive(Serialize, Deserialize, ToSchema, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    #[validate(length(min = 1, max = 255))]
    pub client_id: String,
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 1, max = 36))]
    pub username: Option<String>, // Optional username
    #[validate(length(min = 8, max = 100))]
    pub password: String,
    pub turnstile_token: Option<String>, // Optional: required only if Turnstile is enabled for realm
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResponse {
    pub message: String,
    pub verification_required: bool,
}

/// Register new user account
///
/// Creates a new user account with email and optional username. If email verification is required
/// for realm, account will be created in a pending state and a verification email will be sent.
#[utoipa::path(
  post,
  path = "/api/auth/{realmId}/register",
  tag = "auth",
  params(
    ("realmId" = String, Path, description = "Realm ID")
  ),
  request_body = RegisterRequest,
  responses(
    (status = 200, description = "Successful registration.", body = RegisterResponse),
    (status = 400, description = "Bad request", body = ErrorResponse),
    (status = 409, description = "Email already registered", body = ErrorResponse),
    (status = 429, description = "Too many requests", body = ErrorResponse),
    (status = 500, description = "Internal server error", body = ErrorResponse)
  )
)]
#[tracing::instrument(
    // Governance: payload carries password (credential),
    // turnstile_token, email/username (PII); realm_id conservatively skipped.
    // state holds service/db handles; ip is client PII. Only the low-cardinality
    // operation type is recorded.
    skip(state, payload, realm_id, ip),
    fields(db.system = "postgres", db.operation = "register")
)]
pub async fn register(
    Path(realm_id): Path<String>,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Valid(Json(payload)): Valid<Json<RegisterRequest>>,
) -> Result<ApiResult<RegisterResponse>, ApiError> {
    let email = normalize_email(&payload.email);

    // Resolve the Client App (validates realm/enabled) before Turnstile so the
    // human-verification check can read its Turnstile config (D-PROTECT-01).
    let client_app =
        mailflow::require_enabled_client(&state, &realm_id, &payload.client_id).await?;

    tracing::info!(
        realm_id = %realm_id,
        "Registration attempt"
    );

    let registration_enabled = is_registration_enabled(&state, &realm_id).await?;
    if !registration_enabled {
        tracing::debug!(
            realm_id = %realm_id,
            "Registration failed: registration not enabled for realm"
        );
        return Err(ApiError::bad_request(
            "Registration is not enabled for this realm".to_string(),
        ));
    }
    if !is_registration_email_domain_allowed(&state, &realm_id, &email).await? {
        return Err(ApiError::bad_request(
            "Email domain is not allowed for registration".to_string(),
        ));
    }

    // ip comes from ClientIp extractor
    // turnstile 校验（按 Client App 配置，D-PROTECT-01）
    verify_turnstile_for_client_app(&state, &client_app, payload.turnstile_token.as_deref(), &ip)
        .await?;

    // ip + email 限流
    rate_limit_hit(
        &state,
        format!("rl:register:ip:{ip}"),
        REGISTER_IP_RATE_LIMIT.0,
        REGISTER_IP_RATE_LIMIT.1,
    )
    .await?;
    rate_limit_hit(
        &state,
        format!("rl:register:email:{email}"),
        REGISTER_EMAIL_RATE_LIMIT.0,
        REGISTER_EMAIL_RATE_LIMIT.1,
    )
    .await?;

    // Check if email verification is required
    let verification_required = is_email_verification_required(&state, &realm_id).await?;

    // Use UserService to register the user
    let user = state
        .service
        .user_service()
        .register(DomainRegisterRequest {
            realm_id: realm_id.clone(),
            email: email.clone(),
            password: payload.password,
        })
        .await
        .map_err(|e| {
            tracing::error!(
                realm_id = %realm_id,
                error = %e,
                "Registration failed"
            );
            match e {
                herald_core::domain::common::entities::app_errors::CoreError::Conflict(msg) => {
                    ApiError::conflict(msg)
                }
                _ => ApiError::internal("Failed to create user".to_string()),
            }
        })?;

    // Default repository behavior creates users in pending status.
    // If this realm does not require email verification, activate immediately.
    if !verification_required {
        state
            .service
            .user_service()
            .activate_user(user.id)
            .await
            .map_err(|e| {
                tracing::error!(
                    user_id = %user.id,
                    error = %e,
                    "Failed to activate user after registration"
                );
                ApiError::internal("Failed to activate user".to_string())
            })?;

        if let Err(e) = state
            .registration_service
            .handle_user_registration(user.id, &realm_id)
            .await
        {
            tracing::error!(
                realm_id = %realm_id,
                user_id = %user.id,
                error = %e,
                "Failed to grant registration points, but user was created successfully"
            );
            // Don't fail registration if points grant fails
        }
    }

    // Create and send email verification code ONLY if required
    if verification_required {
        let code = state
            .service
            .user_service()
            .verify_email_trigger(&realm_id, &email, "register")
            .await
            .map_err(|e| {
                tracing::error!("Failed to create email verification code: {}", e);
                ApiError::internal("Failed to store email verification code".to_string())
            })?;

        mailflow::store(
            &state,
            &code,
            &realm_id,
            &payload.client_id,
            MailflowType::VerifyEmail,
        )
        .await?;

        // Send email (best effort only when configured)
        let (public_base, _) = realm_public_url_parts(&state, &realm_id).await?;
        let link = format!("{public_base}/api/auth/{realm_id}/verify_email/confirm/{code}");
        EmailService::send_templated_email(
            &state.pool,
            &realm_id,
            &email,
            EmailTemplateKind::VerifyEmail,
            &link,
            None,
        )
        .await
        .map_err(|e| {
            tracing::error!("Failed to send verification email: {e}");
            ApiError::internal("Failed to send verification email".to_string())
        })?;
    }

    tracing::info!(
        realm_id = %realm_id,
        verification_required = %verification_required,
        "Registration successful"
    );

    // Record consent to the current effective ToS + Privacy at registration
    // time ("register = consent"). Best-effort: a missing
    // effective version (seed anomaly) or any repository error is logged and
    // does NOT block registration — registration is the primary path and a
    // consent gap must not prevent account creation. The user will be asked
    // to re-consent at next login if the record is missing/stale.
    {
        let mut items = Vec::new();
        for agreement_type in [
            herald_core::domain::legal::AgreementType::TermsOfService,
            herald_core::domain::legal::AgreementType::PrivacyPolicy,
        ] {
            match state
                .legal_service
                .current_effective(&realm_id, agreement_type.clone())
                .await
            {
                Ok(Some(version)) => {
                    items.push((agreement_type, version.id));
                }
                Ok(None) => {
                    tracing::warn!(
                        realm_id = %realm_id,
                        agreement_type = %agreement_type.as_ref(),
                        user_id = %user.id,
                        "No effective agreement version deployed (seed missing); skipping register-consent for this type"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        realm_id = %realm_id,
                        agreement_type = %agreement_type.as_ref(),
                        user_id = %user.id,
                        error = %e,
                        "current_effective lookup failed; skipping register-consent for this type"
                    );
                }
            }
        }

        if !items.is_empty() {
            let actor_meta = herald_core::domain::audit::AuditContext {
                actor_id: user.id.to_string(),
                actor_type: Some(herald_core::domain::audit::ActorType::User),
                actor_name: Some(email.clone()),
                ip_address: Some(ip.clone()),
                user_agent: None,
                trace_id: None,
            };
            if let Err(e) = state
                .legal_service
                .record_consent(
                    user.id,
                    &realm_id,
                    items,
                    herald_core::domain::legal::ConsentSource::Register,
                    actor_meta,
                )
                .await
            {
                tracing::warn!(
                    realm_id = %realm_id,
                    user_id = %user.id,
                    error = %e,
                    "record_consent(Register) failed; registration proceeds (user will re-consent at login)"
                );
            }
        }
    }
    let response = RegisterResponse {
        message: if verification_required {
            "Registration successful. Please check your email to verify your account.".to_string()
        } else {
            "Registration successful.".to_string()
        },
        verification_required,
    };

    Ok(ApiResult::ok(response))
}
