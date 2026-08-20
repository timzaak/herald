use crate::common::entities::app_errors::CoreError;
use crate::common::generate_uuid_v7;
use crate::user::entities::User;
use crate::user_passkey::entities::UserPasskeyCredential;
use crate::user_passkey::ports::{
    PasskeyChallengeStore, PasskeyRealmConfigReader, PasskeyRealmPolicy, UserPasskeyRepository,
};
use chrono::Utc;
use passkey_auth::{
    AuthenticationChallenge, AuthenticationResponse, AuthenticationState, PasskeyCredential,
    RegistrationChallenge, RegistrationResponse, RegistrationState, Webauthn,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

const PASSKEY_CHALLENGE_TTL_SECONDS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasskeyLoginState {
    pub realm_id: String,
    pub client_id: String,
    pub client_ip: String,
    #[serde(default)]
    pub oauth_client_id: Option<String>,
    #[serde(default)]
    pub redirect_uri: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasskeyRelyingParty {
    pub id: String,
    pub origin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistrationChallengeState {
    realm_id: String,
    user_id: Uuid,
    nickname: Option<String>,
    relying_party: PasskeyRelyingParty,
    state: RegistrationState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthenticationChallengeState {
    login_state: PasskeyLoginState,
    relying_party: PasskeyRelyingParty,
    state: AuthenticationState,
}

#[derive(Debug, thiserror::Error)]
pub enum PasskeyError {
    #[error("Passkey 功能未启用")]
    Disabled,
    #[error("Passkey 验证失败")]
    VerificationFailed,
    #[error("未找到可用的 Passkey")]
    NotFound,
    #[error("challenge 已过期，请重新发起")]
    ChallengeExpired,
    #[error("浏览器或设备不支持 Passkey")]
    Unsupported,
    #[error("challenge 不属于当前用户")]
    OwnerMismatch,
    #[error(transparent)]
    Repo(#[from] CoreError),
}

pub struct UserPasskeyService<R, S, C>
where
    R: UserPasskeyRepository,
    S: PasskeyChallengeStore,
    C: PasskeyRealmConfigReader,
{
    repo: Arc<R>,
    challenge_store: Arc<S>,
    config_reader: Arc<C>,
}

impl<R, S, C> UserPasskeyService<R, S, C>
where
    R: UserPasskeyRepository,
    S: PasskeyChallengeStore,
    C: PasskeyRealmConfigReader,
{
    pub fn new(
        repo: Arc<R>,
        challenge_store: Arc<S>,
        config_reader: Arc<C>,
    ) -> Result<Self, PasskeyError> {
        Ok(Self {
            repo,
            challenge_store,
            config_reader,
        })
    }

    /// Build a per-realm `Webauthn` applying the realm's UV/attachment policy.
    ///
    /// The builder options (`require_user_verification`, `authenticator_attachment`)
    /// only affect challenge generation. Finish methods rebuild the verifier from
    /// the RP persisted with the challenge.
    fn build_policy_webauthn(
        relying_party: &PasskeyRelyingParty,
        policy: &PasskeyRealmPolicy,
    ) -> Webauthn {
        let mut builder =
            Webauthn::new(&relying_party.id, &relying_party.id, &relying_party.origin);
        if policy.user_verification.is_required() {
            builder = builder.require_user_verification(true);
        }
        if policy.cross_platform_authenticator {
            builder = builder.authenticator_attachment(passkey_auth::Attachment::Any);
        }
        builder
    }

    pub async fn begin_registration(
        &self,
        realm_id: &str,
        user: &User,
        exclude: &[Vec<u8>],
        relying_party: PasskeyRelyingParty,
    ) -> Result<(RegistrationChallenge, String), PasskeyError> {
        // Apply the realm's UV/attachment policy to the ceremony challenge.
        let policy = self
            .config_reader
            .get_policy(realm_id)
            .await
            .unwrap_or_default();
        let webauthn = Self::build_policy_webauthn(&relying_party, &policy);

        let exclude_credentials = exclude
            .iter()
            .map(|id| passkey_auth::CredentialId(id.clone()))
            .collect::<Vec<_>>();
        let display_name = user.nickname.as_deref().unwrap_or(&user.email);
        let user_handle = user.id.as_bytes();
        let (challenge, state) = webauthn.start_registration(
            user_handle,
            &user.email,
            display_name,
            &exclude_credentials,
        );

        let reg_token = generate_token();
        let payload = RegistrationChallengeState {
            realm_id: realm_id.to_string(),
            user_id: user.id,
            nickname: None,
            relying_party,
            state,
        };
        self.store_challenge(&reg_key(&reg_token), &payload).await?;

        Ok((challenge, reg_token))
    }

    pub async fn finish_registration(
        &self,
        reg_token: &str,
        resp_json: &Value,
        nickname: Option<&str>,
        expected_user_id: Uuid,
        expected_realm_id: &str,
    ) -> Result<UserPasskeyCredential, PasskeyError> {
        let key = reg_key(reg_token);
        let payload = self
            .load_challenge::<RegistrationChallengeState>(&key)
            .await?;
        // Ownership must be verified BEFORE the credential is persisted: the
        // challenge is bound to the user who ran `begin_registration`, and a
        // stolen reg_token must never plant an attacker credential on the
        // owner's account. The challenge is left intact so the legitimate
        // owner can still complete the ceremony.
        if payload.user_id != expected_user_id || payload.realm_id != expected_realm_id {
            return Err(PasskeyError::OwnerMismatch);
        }
        self.challenge_store.delete(&key).await?;

        let response: RegistrationResponse = serde_json::from_value(resp_json.clone())
            .map_err(|_| PasskeyError::VerificationFailed)?;
        let webauthn = Webauthn::new(
            &payload.relying_party.id,
            &payload.relying_party.id,
            &payload.relying_party.origin,
        );
        let passkey = webauthn
            .finish_registration(&payload.state, &response)
            .map_err(|_| PasskeyError::VerificationFailed)?;

        let now = Utc::now();
        let credential = UserPasskeyCredential {
            id: generate_uuid_v7(),
            user_id: payload.user_id,
            realm_id: payload.realm_id,
            rp_id: payload.relying_party.id,
            credential_id: passkey.id.as_bytes().to_vec(),
            credential_public_key: passkey.public_key_cose.as_bytes().to_vec(),
            counter: u64::from(passkey.counter),
            transports: passkey.transports.clone(),
            aaguid: uuid_from_aaguid(&passkey.aaguid),
            // passkey-auth 0.1 does not surface the BE/BS flags; the
            // passkey ceremony does not gate on them, so default false.
            backup_eligible: false,
            backup_state: false,
            user_verified: false,
            nickname: nickname.map(str::to_string).or(payload.nickname),
            last_used_at: None,
            created_at: now,
            updated_at: now,
        };

        self.repo
            .insert(credential)
            .await
            .map_err(PasskeyError::Repo)
    }

    pub async fn begin_login_first_factor(
        &self,
        realm_id: &str,
        state: PasskeyLoginState,
        relying_party: PasskeyRelyingParty,
    ) -> Result<(AuthenticationChallenge, String), PasskeyError> {
        // Discoverable / passwordless flow: no allow-credentials list,
        // the browser picks the credential.
        let policy = self
            .config_reader
            .get_policy(realm_id)
            .await
            .unwrap_or_default();
        let webauthn = Self::build_policy_webauthn(&relying_party, &policy);
        let (challenge, auth_state) = webauthn.start_authentication(&[]);

        let auth_token = generate_token();
        let payload = AuthenticationChallengeState {
            login_state: state,
            relying_party,
            state: auth_state,
        };
        self.store_challenge(&auth_key(&auth_token), &payload)
            .await?;

        Ok((challenge, auth_token))
    }

    pub async fn finish_login_first_factor(
        &self,
        auth_token: &str,
        resp_json: &Value,
    ) -> Result<(Uuid, UserPasskeyCredential, PasskeyLoginState), PasskeyError> {
        let (credential, login_state) = self
            .finish_authentication(&auth_key(auth_token), resp_json)
            .await?;
        let user_id = credential.user_id;

        Ok((user_id, credential, login_state))
    }

    pub async fn begin_second_factor(
        &self,
        temp_session: &PasskeyLoginState,
        user_id: Uuid,
        relying_party: PasskeyRelyingParty,
    ) -> Result<(AuthenticationChallenge, String), PasskeyError> {
        let credentials = self
            .repo
            .list_by_user_and_rp(&temp_session.realm_id, user_id, &relying_party.id)
            .await
            .map_err(PasskeyError::Repo)?;
        if credentials.is_empty() {
            return Err(PasskeyError::NotFound);
        }

        let allow = credentials_to_passkey_credentials(&credentials)?;
        let policy = self
            .config_reader
            .get_policy(&temp_session.realm_id)
            .await
            .unwrap_or_default();
        let webauthn = Self::build_policy_webauthn(&relying_party, &policy);
        let (challenge, auth_state) = webauthn.start_authentication_with_creds(&allow);

        let token = generate_token();
        let payload = AuthenticationChallengeState {
            login_state: temp_session.clone(),
            relying_party,
            state: auth_state,
        };
        self.store_challenge(&two_factor_key(&token), &payload)
            .await?;

        Ok((challenge, token))
    }

    pub async fn finish_second_factor(
        &self,
        token: &str,
        resp_json: &Value,
    ) -> Result<UserPasskeyCredential, PasskeyError> {
        let (credential, _) = self
            .finish_authentication(&two_factor_key(token), resp_json)
            .await?;
        Ok(credential)
    }

    async fn finish_authentication(
        &self,
        key: &str,
        resp_json: &Value,
    ) -> Result<(UserPasskeyCredential, PasskeyLoginState), PasskeyError> {
        let payload = self
            .load_challenge::<AuthenticationChallengeState>(key)
            .await?;
        self.challenge_store.delete(key).await?;

        let response: AuthenticationResponse = serde_json::from_value(resp_json.clone())
            .map_err(|_| PasskeyError::VerificationFailed)?;
        let credential_id = passkey_auth::CredentialId::from_b64url(&response.id)
            .map_err(|_| PasskeyError::VerificationFailed)?;
        let mut credential = self
            .repo
            .find_by_credential_id(
                &payload.login_state.realm_id,
                &payload.relying_party.id,
                credential_id.as_bytes(),
            )
            .await
            .map_err(PasskeyError::Repo)?
            .ok_or(PasskeyError::VerificationFailed)?;

        let stored = credential_to_passkey_credential(&credential)?;
        let webauthn = Webauthn::new(
            &payload.relying_party.id,
            &payload.relying_party.id,
            &payload.relying_party.origin,
        );
        let auth_result = webauthn
            .finish_authentication(&payload.state, &response, &stored)
            .map_err(|_| PasskeyError::VerificationFailed)?;
        let new_counter = u64::from(auth_result.new_counter);
        // Counter-replay is already enforced inside finish_authentication,
        // which mirrors the spec: when BOTH the stored and asserted
        // counters are zero (non-counting authenticators like Touch ID,
        // Windows Hello, synced passkeys) the assertion is accepted, and
        // otherwise the asserted counter must be strictly greater. We do
        // NOT re-check here — a naive `<=` check would reject the
        // both-zero case and lock out non-counting authenticators.

        self.repo
            .update_counter_and_used(
                credential.id,
                &payload.login_state.realm_id,
                &credential.rp_id,
                new_counter,
                auth_result.user_verified,
                Utc::now(),
            )
            .await
            .map_err(PasskeyError::Repo)?;

        credential.counter = new_counter;
        credential.user_verified = auth_result.user_verified;
        credential.last_used_at = Some(Utc::now());

        Ok((credential, payload.login_state))
    }

    async fn store_challenge<T: Serialize>(
        &self,
        key: &str,
        payload: &T,
    ) -> Result<(), PasskeyError> {
        let serialized =
            serde_json::to_vec(payload).map_err(|_| PasskeyError::VerificationFailed)?;
        self.challenge_store
            .store(key, &serialized, PASSKEY_CHALLENGE_TTL_SECONDS)
            .await?;
        Ok(())
    }

    async fn load_challenge<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
    ) -> Result<T, PasskeyError> {
        let payload = self
            .challenge_store
            .load(key)
            .await?
            .ok_or(PasskeyError::ChallengeExpired)?;
        serde_json::from_slice(&payload).map_err(|_| PasskeyError::VerificationFailed)
    }
}

/// Reconstruct a `PasskeyCredential` (COSE key + counter) from a stored
/// row so passkey-auth can verify the assertion signature. The stored
/// public key is the raw COSE_Key bytes captured at registration.
fn credential_to_passkey_credential(
    credential: &UserPasskeyCredential,
) -> Result<PasskeyCredential, PasskeyError> {
    Ok(PasskeyCredential {
        id: passkey_auth::CredentialId(credential.credential_id.clone()),
        public_key_cose: passkey_auth::CosePublicKey(credential.credential_public_key.clone()),
        counter: credential.counter as u32,
        transports: credential.transports.clone(),
        aaguid: [0u8; 16],
    })
}

fn credentials_to_passkey_credentials(
    credentials: &[UserPasskeyCredential],
) -> Result<Vec<PasskeyCredential>, PasskeyError> {
    credentials
        .iter()
        .map(credential_to_passkey_credential)
        .collect()
}

/// Convert the 16-byte AAGUID from attested credential data into a Uuid.
/// Zeroed AAGUID (attestation "none") maps to `nil` UUID.
fn uuid_from_aaguid(bytes: &[u8; 16]) -> Option<Uuid> {
    if bytes.iter().all(|b| *b == 0) {
        return None;
    }
    Some(Uuid::from_bytes(*bytes))
}

fn generate_token() -> String {
    generate_uuid_v7().to_string()
}

fn reg_key(token: &str) -> String {
    format!("passkey:reg:{token}")
}

fn auth_key(token: &str) -> String {
    format!("passkey:auth:{token}")
}

fn two_factor_key(token: &str) -> String {
    format!("passkey:2fa:{token}")
}
