use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::client::entities::ClientApp;
use herald_core::domain::client::ports::ClientService;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

const MAILFLOW_TTL_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MailflowType {
    VerifyEmail,
    ResetPassword,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct MailflowState {
    pub realm_id: String,
    pub client_app_id: String,
    pub flow_type: MailflowType,
}

fn key(code: &str) -> String {
    format!("mailflow:{code}")
}

pub(crate) async fn require_enabled_client(
    state: &AppState,
    realm_id: &str,
    client_id: &str,
) -> Result<ClientApp, ApiError> {
    let client = state
        .service
        .client_service()
        .get_client_app_by_client_id(realm_id, client_id)
        .await
        .map_err(|_| ApiError::bad_request("Invalid clientId".to_string()))?;

    validate_client(
        &client.realm_id,
        &client.client_id,
        client.enabled,
        realm_id,
        client_id,
    )?;
    Ok(client)
}

fn validate_client(
    actual_realm_id: &str,
    actual_client_id: &str,
    enabled: bool,
    expected_realm_id: &str,
    expected_client_id: &str,
) -> Result<(), ApiError> {
    if actual_realm_id != expected_realm_id || actual_client_id != expected_client_id {
        return Err(ApiError::bad_request("Invalid mailflow client".to_string()));
    }
    if !enabled {
        return Err(ApiError::bad_request("Client app is disabled".to_string()));
    }
    Ok(())
}

pub(crate) async fn store(
    state: &AppState,
    code: &str,
    realm_id: &str,
    client_id: &str,
    flow_type: MailflowType,
) -> Result<(), ApiError> {
    let value = serde_json::to_string(&MailflowState {
        realm_id: realm_id.to_string(),
        client_app_id: client_id.to_string(),
        flow_type,
    })
    .map_err(|_| ApiError::internal("Failed to serialize mailflow state".to_string()))?;
    let mut connection = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Failed to store mailflow state".to_string()))?;
    connection
        .set_ex::<_, _, ()>(key(code), value, MAILFLOW_TTL_SECONDS)
        .await
        .map_err(|_| ApiError::internal("Failed to store mailflow state".to_string()))
}

pub(crate) async fn load_client(
    state: &AppState,
    code: &str,
    realm_id: &str,
    expected_type: MailflowType,
) -> Result<ClientApp, ApiError> {
    let mut connection = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Failed to load mailflow state".to_string()))?;
    let value: Option<String> = connection
        .get(key(code))
        .await
        .map_err(|_| ApiError::internal("Failed to load mailflow state".to_string()))?;
    let flow: MailflowState = serde_json::from_str(
        &value.ok_or_else(|| ApiError::bad_request("Invalid or expired mailflow".to_string()))?,
    )
    .map_err(|_| ApiError::bad_request("Invalid mailflow state".to_string()))?;

    if flow.realm_id != realm_id || flow.flow_type != expected_type {
        return Err(ApiError::bad_request("Invalid mailflow state".to_string()));
    }
    require_enabled_client(state, realm_id, &flow.client_app_id).await
}

pub(crate) fn return_url(registered: Option<&str>, realm_fallback: String) -> String {
    registered
        .filter(|url| !url.trim().is_empty())
        .unwrap_or(&realm_fallback)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn mailflow_uses_only_the_registered_return_url() {
        assert_eq!(
            return_url(
                Some("https://app.example/verified"),
                "https://realm.example".to_string()
            ),
            "https://app.example/verified"
        );
    }

    #[test]
    fn mailflow_falls_back_to_the_realm_public_url() {
        assert_eq!(
            return_url(None, "https://realm.example".to_string()),
            "https://realm.example"
        );
    }

    #[test]
    fn mailflow_rejects_a_disabled_client() {
        let response = validate_client("realm", "web", false, "realm", "web")
            .unwrap_err()
            .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }
}
