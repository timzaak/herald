use axum::http::{HeaderMap, Uri, header::ORIGIN};

use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::custom_domain::CustomDomainMappingRepository;
use herald_core::domain::realm_config::ConfigType;
use herald_core::domain::user_passkey::PasskeyRelyingParty;

/// Read the realm's passkey `enabled` flag from `realm_config`
/// (`config_type='passkey'`, `config_key='settings'`).
///
/// Returns `Ok(false)` when the config row is absent or its inner `enabled`
/// flag is missing/false — passkey is opt-in per realm. Propagates a 500 only
/// when the lookup itself fails. This is the single read path shared by the
/// passkey gate below and the public `/passkey/status` status endpoint.
///
/// Note: the structurally identical `read_realm_passkey_enabled` in
/// `api-billing/feature_availability.rs` is intentionally kept separate — it
/// deliberately swallows lookup errors (returns `false`) so a passkey flag
/// lookup can never hard-fail the aggregated feature-availability response.
pub async fn is_passkey_enabled(state: &AppState, realm_id: &str) -> Result<bool, ApiError> {
    Ok(read_realm_passkey_config(state, realm_id).await?.0)
}

/// Read the full passkey enablement pair `(enabled, force_enabled)` from the
/// realm config row. `force_enabled` drives the frontend guidance described
/// by the passkey PRD (users without a passkey are guided to register one);
/// it never blocks login on the backend.
pub async fn read_realm_passkey_config(
    state: &AppState,
    realm_id: &str,
) -> Result<(bool, bool), ApiError> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT config_value FROM realm_config
         WHERE realm_id = $1 AND config_type = $2 AND config_key = 'settings' AND enabled = true",
    )
    .bind(realm_id)
    .bind(ConfigType::Passkey.as_ref())
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query passkey realm config: {e}");
        ApiError::internal("Internal server error")
    })?;

    let config = row.and_then(|(value,)| serde_json::from_str::<serde_json::Value>(&value).ok());
    let enabled = config
        .as_ref()
        .and_then(|value| value.get("enabled"))
        .and_then(|enabled| enabled.as_bool())
        .unwrap_or(false);
    let force_enabled = config
        .as_ref()
        .and_then(|value| value.get("force_enabled"))
        .and_then(|force| force.as_bool())
        .unwrap_or(false);

    Ok((enabled, force_enabled))
}

/// Gate a passkey operation on the realm having Passkey enabled.
///
/// Returns a 404 "Passkey is not enabled for this realm" when [`is_passkey_enabled`]
/// reports `false`. Shared by the user self-service handlers
/// (list/rename/delete/register-begin) and the login begin handlers so all
/// passkey paths reject a disabled realm consistently.
pub async fn ensure_passkey_enabled(state: &AppState, realm_id: &str) -> Result<(), ApiError> {
    if !is_passkey_enabled(state, realm_id).await? {
        return Err(ApiError::not_found("Passkey is not enabled for this realm"));
    }
    Ok(())
}

pub async fn resolve_passkey_rp(
    state: &AppState,
    realm_id: &str,
    headers: &HeaderMap,
    target_client_app_id: Option<uuid::Uuid>,
) -> Result<PasskeyRelyingParty, ApiError> {
    let configured_id =
        std::env::var("RP_ID").map_err(|_| ApiError::internal("RP_ID is not configured"))?;
    let configured_origin = std::env::var("RP_ORIGIN")
        .map_err(|_| ApiError::internal("RP_ORIGIN is not configured"))?;

    let request_origin = headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(configured_origin.as_str());
    let request_uri = parse_origin(request_origin)?;
    let configured_uri = parse_origin(&configured_origin)?;

    if same_origin(&request_uri, &configured_uri) {
        return Ok(PasskeyRelyingParty {
            id: configured_id,
            origin: normalized_origin(&configured_uri),
        });
    }

    let normalized_request_origin = normalized_origin(&request_uri);
    let allowed_origins = match target_client_app_id {
        Some(client_app_id) => sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT allowed_origins FROM client_app WHERE realm_id = $1 AND id = $2 AND enabled = true",
        )
        .bind(realm_id)
        .bind(client_app_id)
        .fetch_all(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, %realm_id, %client_app_id, "Failed to resolve Passkey Client App origin");
            ApiError::internal("Failed to resolve Passkey origin")
        })?,
        None => sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT allowed_origins FROM client_app WHERE realm_id = $1 AND enabled = true",
        )
        .bind(realm_id)
        .fetch_all(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, %realm_id, "Failed to resolve Passkey Client App origin");
            ApiError::internal("Failed to resolve Passkey origin")
        })?,
    };
    let matches_enabled_client = allowed_origins.iter().any(|origins| {
        serde_json::from_value::<Vec<String>>(origins.clone())
            .map(|origins| {
                origins
                    .iter()
                    .any(|origin| origin == &normalized_request_origin)
            })
            .unwrap_or(false)
    });
    if let Some(relying_party) = select_client_app_rp(&request_uri, matches_enabled_client)? {
        return Ok(relying_party);
    }

    let hostname = request_uri
        .host()
        .ok_or_else(|| ApiError::bad_request("Passkey origin has no hostname"))?;
    let mapping = state
        .custom_domain_mapping_repo
        .find_by_hostname(hostname)
        .await
        .map_err(|error| {
            tracing::error!(%error, %hostname, "Failed to resolve Passkey custom domain");
            ApiError::internal("Failed to resolve Passkey origin")
        })?;

    select_custom_domain_rp(
        realm_id,
        hostname,
        &normalized_origin(&request_uri),
        mapping.as_ref().map(|mapping| mapping.realm_id.as_str()),
    )
}

fn select_client_app_rp(
    request_uri: &Uri,
    matches_enabled_client: bool,
) -> Result<Option<PasskeyRelyingParty>, ApiError> {
    if !matches_enabled_client {
        return Ok(None);
    }
    let hostname = request_uri
        .host()
        .ok_or_else(|| ApiError::bad_request("Passkey origin has no hostname"))?;
    Ok(Some(PasskeyRelyingParty {
        id: hostname.to_string(),
        origin: normalized_origin(request_uri),
    }))
}

fn select_custom_domain_rp(
    realm_id: &str,
    hostname: &str,
    origin: &str,
    mapped_realm_id: Option<&str>,
) -> Result<PasskeyRelyingParty, ApiError> {
    if mapped_realm_id != Some(realm_id) {
        return Err(ApiError::bad_request(
            "Passkey origin is not configured for this realm",
        ));
    }
    Ok(PasskeyRelyingParty {
        id: hostname.to_string(),
        origin: origin.to_string(),
    })
}

fn parse_origin(origin: &str) -> Result<Uri, ApiError> {
    let uri = origin
        .trim_end_matches('/')
        .parse::<Uri>()
        .map_err(|_| ApiError::internal("Invalid Passkey origin configuration"))?;
    if uri.scheme().is_none() || uri.authority().is_none() || uri.path() != "/" {
        return Err(ApiError::internal("Invalid Passkey origin configuration"));
    }
    Ok(uri)
}

fn same_origin(left: &Uri, right: &Uri) -> bool {
    left.scheme_str() == right.scheme_str() && left.authority() == right.authority()
}

fn normalized_origin(uri: &Uri) -> String {
    format!(
        "{}://{}",
        uri.scheme_str().expect("validated origin has scheme"),
        uri.authority().expect("validated origin has authority")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_comparison_includes_scheme_and_port() {
        let base = parse_origin("http://localhost:3000").unwrap();

        assert!(same_origin(
            &base,
            &parse_origin("http://localhost:3000/").unwrap()
        ));
        assert!(!same_origin(
            &base,
            &parse_origin("http://localhost:3001").unwrap()
        ));
        assert!(!same_origin(
            &base,
            &parse_origin("https://localhost:3000").unwrap()
        ));
    }

    #[test]
    fn origin_must_not_contain_a_path() {
        assert!(parse_origin("https://login.example.com/path").is_err());
    }

    #[test]
    fn custom_domain_must_belong_to_requested_realm() {
        let rp = select_custom_domain_rp(
            "realm-a",
            "login.customer.test",
            "https://login.customer.test",
            Some("realm-a"),
        )
        .unwrap();
        assert_eq!(rp.id, "login.customer.test");
        assert_eq!(rp.origin, "https://login.customer.test");

        assert!(
            select_custom_domain_rp(
                "realm-b",
                "login.customer.test",
                "https://login.customer.test",
                Some("realm-a"),
            )
            .is_err()
        );
        assert!(
            select_custom_domain_rp("realm-a", "unknown.test", "https://unknown.test", None,)
                .is_err()
        );
    }

    #[test]
    fn enabled_client_app_origin_uses_its_host_as_rp_id() {
        let origin = parse_origin("https://app.customer.test:8443").unwrap();

        let rp = select_client_app_rp(&origin, true).unwrap().unwrap();
        assert_eq!(rp.id, "app.customer.test");
        assert_eq!(rp.origin, "https://app.customer.test:8443");
        assert!(select_client_app_rp(&origin, false).unwrap().is_none());
    }
}
