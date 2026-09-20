use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DistributionRuleErrorResponse {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ErrorResponse {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
    #[serde(rename = "requestId")]
    pub request_id: Option<String>,
}

impl Serialize for ErrorResponse {
    /// Custom serialize to include both `message` and `error` fields for
    /// backward compatibility with API consumers that read `error`.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ErrorResponse", 6)?;
        state.serialize_field("status", &self.status)?;
        state.serialize_field("code", &self.code)?;
        state.serialize_field("message", &self.message)?;
        state.serialize_field("error", &self.message)?;
        state.serialize_field("details", &self.details)?;
        state.serialize_field("requestId", &self.request_id)?;
        state.end()
    }
}

#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    body: ErrorBody,
}

#[derive(Debug, Clone)]
enum ErrorBody {
    Standard(ErrorResponse),
    Custom(serde_json::Value),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.body {
            ErrorBody::Standard(body) => write!(f, "{}: {}", self.status, body.message),
            ErrorBody::Custom(_) => write!(f, "{}: Error", self.status),
        }
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    fn status_error_code(status: StatusCode) -> &'static str {
        match status {
            StatusCode::BAD_REQUEST => "bad_request",
            StatusCode::UNAUTHORIZED => "unauthorized",
            StatusCode::FORBIDDEN => "forbidden",
            StatusCode::NOT_FOUND => "not_found",
            StatusCode::CONFLICT => "conflict",
            StatusCode::UNPROCESSABLE_ENTITY => "validation_error",
            StatusCode::TOO_MANY_REQUESTS => "rate_limit_exceeded",
            StatusCode::BAD_GATEWAY => "upstream_error",
            StatusCode::SERVICE_UNAVAILABLE => "service_unavailable",
            _ if status.is_server_error() => "internal_error",
            _ => "request_failed",
        }
    }

    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self::with_error_code(status, Self::status_error_code(status), message)
    }

    /// The HTTP status this error will render with. Callers that branch on
    /// the error class (e.g. request-input 4xx vs configuration 5xx) read it
    /// here instead of matching on message text.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn with_error_code(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let mut message = message.into();
        if status.is_server_error() {
            tracing::error!(%status, error = %message, "API request failed");
            message =
                if status == StatusCode::BAD_GATEWAY || status == StatusCode::SERVICE_UNAVAILABLE {
                    "Upstream service unavailable".to_string()
                } else {
                    "Internal server error".to_string()
                };
        }
        Self {
            status,
            body: ErrorBody::Standard(ErrorResponse {
                status: status.as_u16(),
                code: code.into(),
                message,
                details: None,
                request_id: crate::application::http::request_context::current_request_id(),
            }),
        }
    }

    pub fn with_json<T: Serialize>(status: StatusCode, body: T) -> Self {
        let mut json_body = serde_json::to_value(&body).unwrap_or_else(|_| {
            tracing::warn!(
                status = %status,
                "Failed to serialize error body to JSON, using empty object"
            );
            serde_json::Value::Object(Default::default())
        });
        if let serde_json::Value::Object(fields) = &mut json_body {
            fields
                .entry("status")
                .or_insert_with(|| serde_json::json!(status.as_u16()));
            if let Some(request_id) =
                crate::application::http::request_context::current_request_id()
            {
                fields
                    .entry("requestId")
                    .or_insert_with(|| serde_json::Value::String(request_id));
            }
        }
        Self {
            status,
            body: ErrorBody::Custom(json_body),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub fn too_many_requests(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    pub fn unprocessable_entity(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, message)
    }

    pub fn unprocessable_entity_json<T: Serialize>(body: T) -> Self {
        Self::with_json(StatusCode::UNPROCESSABLE_ENTITY, body)
    }

    pub fn bad_request_json<T: Serialize>(body: T) -> Self {
        Self::with_json(StatusCode::BAD_REQUEST, body)
    }

    pub fn conflict_json<T: Serialize>(body: T) -> Self {
        Self::with_json(StatusCode::CONFLICT, body)
    }

    pub fn distribution_rule_error(
        status: StatusCode,
        body: DistributionRuleErrorResponse,
    ) -> Self {
        let json_body = serde_json::to_value(&body).unwrap_or_else(|_| {
            tracing::warn!(
                status = %status,
                "Failed to serialize distribution rule error body"
            );
            serde_json::json!({
                "code": "invalid_distribution_rule",
                "message": "Invalid distribution rule"
            })
        });
        Self {
            status,
            body: ErrorBody::Custom(json_body),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self.body {
            ErrorBody::Standard(body) => (self.status, Json(body)).into_response(),
            ErrorBody::Custom(json_body) => (self.status, Json(json_body)).into_response(),
        }
    }
}

impl From<herald_core::domain::common::entities::app_errors::CoreError> for ApiError {
    fn from(err: herald_core::domain::common::entities::app_errors::CoreError) -> Self {
        use herald_core::domain::common::entities::app_errors::CoreError;

        match err {
            CoreError::NotFound => Self::not_found("Resource not found"),
            CoreError::InvalidRealm(msg) => Self::bad_request(msg),
            CoreError::InternalServerError(msg) => {
                tracing::error!(error = %msg, "Internal server error");
                Self::internal("Internal server error")
            }
            CoreError::Forbidden(msg) => Self::forbidden(msg),
            CoreError::Unauthorized => Self::unauthorized("Unauthorized"),
            CoreError::BadRequest(msg) => Self::bad_request(msg),
            CoreError::Conflict(msg) => Self::conflict(msg),
            CoreError::DatabaseError(msg) => {
                tracing::error!(error = %msg, "Database operation failed");
                Self::internal("Internal server error")
            }
            CoreError::RateLimitExceeded => Self::too_many_requests("Rate limit exceeded"),
            CoreError::EmailVerificationFailed => Self::with_error_code(
                StatusCode::BAD_REQUEST,
                "email_verification_failed",
                "Email verification failed",
            ),
            CoreError::PasswordResetFailed => Self::with_error_code(
                StatusCode::BAD_REQUEST,
                "password_reset_failed",
                "Password reset failed",
            ),
            CoreError::BillingError(msg) => {
                tracing::error!(error = %msg, "Billing operation failed");
                Self::internal("Internal server error")
            }
            CoreError::SubscriptionNotFound(msg) => {
                Self::not_found(format!("Subscription not found: {msg}"))
            }
            CoreError::InvalidSubscriptionStatus(msg) => {
                Self::bad_request(format!("Invalid subscription status: {msg}"))
            }
            CoreError::CreemApiError(msg) => {
                tracing::error!(error = %msg, "Creem API request failed");
                Self::new(StatusCode::BAD_GATEWAY, "Upstream service unavailable")
            }
            CoreError::InvalidWebhookSignature => Self::unauthorized("Invalid webhook signature"),
            CoreError::WebhookTimestampExpired => Self::unauthorized("Webhook timestamp expired"),
            CoreError::InvalidWebhookPayload => Self::bad_request("Invalid webhook payload"),
            CoreError::InvalidWebhookSecret => {
                tracing::error!("Invalid webhook secret configuration");
                Self::internal("Internal server error")
            }
            CoreError::DuplicateWebhookEvent(msg) => {
                Self::conflict(format!("Duplicate webhook event: {msg}"))
            }
            CoreError::SerializationError(msg) => {
                tracing::error!(error = %msg, "Serialization failed");
                Self::internal("Internal server error")
            }
            CoreError::EntitlementMappingNotFound => Self::with_error_code(
                StatusCode::NOT_FOUND,
                "entitlement_mapping_not_found",
                "Entitlement mapping not found",
            ),
            // Credit-bucket routing errors.
            // These surface from consume / grant / fulfillment write paths.
            CoreError::EntitlementMappingNotAttachedToBucket { mapping_id } => Self::bad_request(
                format!("Entitlement mapping {mapping_id} is not attached to a credit bucket"),
            ),
            CoreError::SubscriptionBucketNotResolved { subscription_id } => {
                tracing::error!(%subscription_id, "Subscription bucket was not resolved");
                Self::internal("Internal server error")
            }
            CoreError::NoCoveredPointsPool { client_app_id } => Self::conflict(format!(
                "Client app {client_app_id} does not cover any available credit bucket"
            )),
            CoreError::GrantBucketRequired => Self::with_error_code(
                StatusCode::BAD_REQUEST,
                "grant_bucket_required",
                "Points grant requires an explicit target bucket",
            ),
            // Purchase resolution refuses to charge when the mapping row has
            // no readable price/currency; surfacing as 422 keeps it a client
            // visible data error rather than a 500.
            CoreError::PriceInfoMissing { entitlement_key } => Self::with_error_code(
                StatusCode::UNPROCESSABLE_ENTITY,
                "price_info_missing",
                format!(
                    "Price info missing for the selected entitlement mapping ({entitlement_key})"
                )
                .as_str(),
            ),
        }
    }
}

impl From<crate::application::http::auth::error::AuthError> for ApiError {
    fn from(err: crate::application::http::auth::error::AuthError) -> Self {
        match err {
            crate::application::http::auth::error::AuthError::BadRequest(msg) => {
                Self::bad_request(msg)
            }
            crate::application::http::auth::error::AuthError::Unauthorized(msg) => {
                Self::unauthorized(msg)
            }
            crate::application::http::auth::error::AuthError::Forbidden(msg) => {
                Self::forbidden(msg)
            }
            crate::application::http::auth::error::AuthError::NotFound(msg) => Self::not_found(msg),
            crate::application::http::auth::error::AuthError::TooManyRequests(msg) => {
                Self::new(StatusCode::TOO_MANY_REQUESTS, msg)
            }
            crate::application::http::auth::error::AuthError::Conflict(msg) => Self::conflict(msg),
            crate::application::http::auth::error::AuthError::InternalServerError(msg) => {
                tracing::error!("AuthError::InternalServerError: {}", msg);
                Self::internal("Internal server error")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use herald_core::domain::common::entities::app_errors::CoreError;

    #[tokio::test]
    async fn internal_errors_hide_the_original_cause() {
        let response =
            ApiError::from(CoreError::DatabaseError("password=secret".into())).into_response();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], 500);
        assert_eq!(json["code"], "internal_error");
        assert_eq!(json["message"], "Internal server error");
        assert!(!String::from_utf8_lossy(&body).contains("secret"));
    }

    #[tokio::test]
    async fn business_error_body_has_stable_code_and_public_message() {
        let response = ApiError::with_error_code(
            StatusCode::CONFLICT,
            "email_already_exists",
            "Email already registered",
        )
        .into_response();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], 409);
        assert_eq!(json["code"], "email_already_exists");
        assert_eq!(json["message"], "Email already registered");
    }

    #[tokio::test]
    async fn error_body_contains_the_scoped_request_id() {
        crate::application::http::request_context::REQUEST_ID
            .scope("req-error-test".to_owned(), async {
                let response = ApiError::bad_request("Invalid input").into_response();
                let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(json["requestId"], "req-error-test");
            })
            .await;
    }
}
