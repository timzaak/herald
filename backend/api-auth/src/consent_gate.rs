use herald_api_base::application::http::state::AppState;
use herald_core::domain::audit::{ActorType, AuditContext};
use herald_core::domain::authentication::BrowserTokenSet;
use herald_core::domain::client::entities::ClientApp;
use herald_core::domain::common::entities::app_errors::CoreError;
use herald_core::domain::legal::{AgreementType, ConsentSource, LegalAgreementSummary};
use herald_core::domain::user::entities::User;
use herald_core::infrastructure::authentication::RedisBrowserTokenService;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthConsentAgreement {
    pub agreement_type: String,
    pub version_id: Uuid,
}

pub async fn evaluate_login_consent_gate(
    state: &AppState,
    user: &User,
    realm_id: &str,
    accepted_agreements: Option<&[AuthConsentAgreement]>,
    ip_address: Option<String>,
    user_agent: Option<String>,
) -> Option<Vec<LegalAgreementSummary>> {
    let actor_meta = AuditContext {
        actor_id: user.id.to_string(),
        actor_type: Some(ActorType::User),
        actor_name: Some(user.email.clone()),
        ip_address,
        user_agent,
        trace_id: None,
    };

    let status_items = match state.legal_service.consent_status(user.id, realm_id).await {
        Ok(items) => items,
        Err(e) => {
            tracing::warn!(
                user_id = %user.id,
                realm_id = %realm_id,
                error = %e,
                "consent_status lookup failed; skipping consent gate (fail-open)"
            );
            Vec::new()
        }
    };

    let needs_reconsent = status_items.iter().any(|i| i.needs_reconsent);
    if needs_reconsent {
        let mut summaries = Vec::with_capacity(status_items.len());
        for item in &status_items {
            if let Ok(Some(version)) = state
                .legal_service
                .current_effective(realm_id, item.agreement_type.clone())
                .await
            {
                summaries.push(LegalAgreementSummary {
                    agreement_type: item.agreement_type.as_str().to_string(),
                    version_id: version.id,
                    version_no: version.version_no,
                    effective_at: version.published_at,
                    title: None,
                    summary: None,
                    mode: version.mode,
                    external_url: version.external_url,
                });
            }
        }

        if let Some(accepted_agreements) = accepted_agreements
            && !accepted_agreements.is_empty()
        {
            let mut record_items = Vec::with_capacity(accepted_agreements.len());
            for item in accepted_agreements {
                let Ok(agreement_type) = AgreementType::try_from(item.agreement_type.as_str())
                else {
                    tracing::warn!(
                        user_id = %user.id,
                        realm_id = %realm_id,
                        agreement_type = %item.agreement_type,
                        "Invalid agreement type in login re-consent payload"
                    );
                    return Some(summaries);
                };
                record_items.push((agreement_type, item.version_id));
            }

            match state
                .legal_service
                .record_consent(
                    user.id,
                    realm_id,
                    record_items,
                    ConsentSource::Reconsent,
                    actor_meta.clone(),
                )
                .await
            {
                Ok(()) => match state.legal_service.consent_status(user.id, realm_id).await {
                    Ok(items) if !items.iter().any(|i| i.needs_reconsent) => {
                        tracing::info!(
                            user_id = %user.id,
                            realm_id = %realm_id,
                            "Login re-consent recorded; continuing login"
                        );
                        return None;
                    }
                    Ok(_) => {
                        tracing::warn!(
                            user_id = %user.id,
                            realm_id = %realm_id,
                            "Login re-consent payload did not satisfy all current agreements"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            user_id = %user.id,
                            realm_id = %realm_id,
                            error = %e,
                            "consent_status lookup failed after login re-consent"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        user_id = %user.id,
                        realm_id = %realm_id,
                        error = %e,
                        "record_consent(Reconsent) failed during login"
                    );
                }
            }
        }

        tracing::info!(
            user_id = %user.id,
            realm_id = %realm_id,
            "Login blocked at consent gate (stale consent); returning consent_required"
        );

        return Some(summaries);
    }

    // Current consent only opens the login gate. A normal login is not a new
    // affirmative consent action, so it must not refresh consent or emit
    // agreement.consent audit events.
    None
}

/// Mint the restricted browser family issued when the consent gate blocks a
/// normal session. Every login entrance that answers `consent_required`
/// issues the same limited family, so the pairing with
/// `evaluate_login_consent_gate` stays a single policy here.
pub async fn mint_consent_restricted_session(
    state: &AppState,
    user: &User,
    client_app: &ClientApp,
    user_agent: Option<String>,
    client_ip: Option<String>,
) -> Result<BrowserTokenSet, CoreError> {
    RedisBrowserTokenService::new(state.redis_manager.clone())
        .create_consent_restricted_token_family(user, client_app, user_agent, client_ip)
        .await
}
