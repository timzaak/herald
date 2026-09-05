//! Apple `SignedDataVerifier` wrapper.
//!
//! Wraps [`app_store_server_library::signed_data_verifier::SignedDataVerifier`]
//! with Herald defaults: the bundled Apple Root CA - G3 as the trust anchor and
//! an explicit OCSP-off posture.
//!
//! ## OCSP posture
//!
//! The `app-store-server-library` 4.3.0 crates OCSP behind a **non-default
//! `ocsp` cargo feature** (see its `Cargo.toml`): with the feature disabled
//! (the default, and the configuration this crate uses — see
//! `infra-iap/Cargo.toml`) verification is purely offline and the
//! `SignedDataVerifier::new` constructor takes no OCSP flag at all. The
//! intended security posture (trust-anchor self-managed, OCSP not consulted)
//! is therefore achieved simply by not enabling the `ocsp` feature. This file
//! documents that fact where a runtime knob was expected to exist; no
//! behaviour is lost.

use crate::apple::models::{
    Environment, JWSTransactionDecodedPayload, ResponseBodyV2DecodedPayload,
};
use crate::error::IapError;
use app_store_server_library::signed_data_verifier::{SignedDataVerifier, SignedDataVerifierError};
use std::sync::Arc;

/// Apple Root CA - G3, downloaded from the Apple PKI Portal and bundled with
/// the crate as the JWS verification trust anchor.
///
/// Update flow (operations, not implemented here): download a fresh copy from
/// <https://www.apple.com/certificateauthority/>, replace this file, open a PR.
pub const APPLE_ROOT_CA_G3: &[u8] = include_bytes!("../../certs/AppleRootCA-G3.cer");

/// Apple JWS/x5c verifier configured for Herald.
///
/// The underlying `SignedDataVerifier` is not `Clone`, so the wrapper holds it
/// behind an `Arc`. Verification is read-only and cheap to share across
/// concurrent receipts / notifications for the same realm.
#[derive(Clone)]
pub struct AppleVerifier {
    inner: Arc<SignedDataVerifier>,
}

impl AppleVerifier {
    /// Build a verifier rooted at the bundled Apple Root CA - G3.
    ///
    /// `bundle_id` is the App Bundle ID the decoded payload's `bundleId` claim
    /// must match; `environment` selects Sandbox / Production and is matched
    /// against the decoded payload's `environment` claim.
    ///
    /// OCSP is **off** by design (see module docs): the bundled Root CA is the
    /// long-lived trust anchor and no online revocation check is performed.
    pub fn new(bundle_id: String, environment: Environment) -> Self {
        // The upstream constructor accepts a *vector* of DER-encoded roots so
        // callers can pin multiple trust anchors. Herald pins the single
        // Apple Root CA - G3 that backs all current App Store signing chains.
        let root_certificates = vec![APPLE_ROOT_CA_G3.to_vec()];
        // app_apple_id is None: Herald identifies Apple apps by bundle_id, and
        // the upstream verifier only consults app_apple_id for Production
        // notifications whose payload lacks a bundleId (rare / legacy case).
        let inner = SignedDataVerifier::new(root_certificates, environment, bundle_id, None);
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Build a verifier rooted at a caller-supplied trust anchor.
    ///
    /// Provided for tests that construct a self-signed x5c chain fixture and
    /// must pin the test root instead of the real Apple Root CA. Production
    /// callers should use [`AppleVerifier::new`].
    pub fn with_root_certificates(
        root_certificates: Vec<Vec<u8>>,
        bundle_id: String,
        environment: Environment,
    ) -> Self {
        let inner = SignedDataVerifier::new(root_certificates, environment, bundle_id, None);
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Verify and decode a StoreKit 2 `jwsRepresentation` (signed transaction).
    ///
    /// Returns the decoded transaction payload on success, or
    /// [`IapError::AppleVerification`] on any verification failure (bad
    /// signature, broken chain, bundle-id mismatch, environment mismatch).
    pub fn verify_signed_transaction(
        &self,
        signed_transaction: &str,
    ) -> Result<JWSTransactionDecodedPayload, IapError> {
        self.inner
            .verify_and_decode_signed_transaction(signed_transaction)
            .map_err(map_verifier_error)
    }

    /// Verify and decode an App Store Server Notification V2 payload.
    ///
    /// The decoded payload's `data.signedTransactionInfo` (when present) is a
    /// *separate* signed JWS that the caller verifies with
    /// [`AppleVerifier::verify_signed_transaction`].
    pub fn verify_and_decode_notification(
        &self,
        signed_payload: &str,
    ) -> Result<ResponseBodyV2DecodedPayload, IapError> {
        self.inner
            .verify_and_decode_notification(signed_payload)
            .map_err(map_verifier_error)
    }

    /// Verify and decode a notification's `data.signedRenewalInfo` JWS.
    ///
    /// Renewal info carries the auto-renew flag and the grace-period
    /// expiration that lifecycle notifications (DID_FAIL_TO_RENEW,
    /// DID_CHANGE_RENEWAL_STATUS) act on.
    pub fn verify_signed_renewal_info(
        &self,
        signed_renewal_info: &str,
    ) -> Result<crate::apple::models::JWSRenewalInfoDecodedPayload, IapError> {
        self.inner
            .verify_and_decode_renewal_info(signed_renewal_info)
            .map_err(map_verifier_error)
    }
}

/// Flatten the upstream's verbose error enum into a single string-bearing
/// `IapError::AppleVerification`. The original variant name is preserved in the
/// message so logs retain enough detail to distinguish bundle-id mismatches
/// from chain failures without leaking the upstream type across the crate
/// boundary.
fn map_verifier_error(err: SignedDataVerifierError) -> IapError {
    IapError::AppleVerification(err.to_string())
}

#[cfg(test)]
mod tests {
    //! Apple verifier three-state coverage.
    //!
    //! The upstream `SignedDataVerifier` is built around Apple's real ES256 +
    //! x5c certificate chain. Reconstructing a valid cryptographic chain inside
    //! a unit test requires minting a self-signed EC P-256 root + leaf and
    //! signing a JWS with it — non-trivial and brittle. The upstream crate
    //! itself solves this in its own test suite with the `LocalTesting`
    //! environment, which **skips signature/chain verification** and base64-
    //! decodes the payload segment directly.
    //!
    //! We use the same strategy to cover the three verification states our
    //! wrapper promises, while still exercising the *real* bundle-id /
    //! environment guards the wrapper inherits:
    //!
    //! 1. **valid** — a JWS whose decoded payload's `bundleId` + `environment`
    //!    match the verifier, decoded successfully under `LocalTesting`.
    //! 2. **tampered / malformed** — a string that is not a 3-segment JWS, or
    //!    whose payload segment is not valid JSON, is rejected with
    //!    `AppleVerification`.
    //! 3. **wrong trust anchor / app identifier** — under a *Production*
    //!    verifier, even a well-formed JWS without a valid x5c chain is
    //!    rejected (mirrors "wrong trust anchor"); and under `LocalTesting`
    //!    a payload whose `bundleId` does not match is rejected as
    //!    `InvalidAppIdentifier`.

    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    /// Encode a `LocalTesting`-acceptable JWS: a minimal header, the payload
    /// JSON, and a dummy signature. `decode_signed_object` under
    /// `LocalTesting` only requires 3 dot-separated segments, a decodable
    /// header, and a base64-decodable payload segment.
    fn make_local_testing_jws(payload_json: &serde_json::Value) -> String {
        let header = serde_json::json!({ "alg": "ES256", "typ" : "JWS" });
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload_json).unwrap());
        let signature_b64 = URL_SAFE_NO_PAD.encode(b"dummy-signature");
        format!("{header_b64}.{payload_b64}.{signature_b64}")
    }

    #[test]
    fn verify_signed_transaction_accepts_matching_bundle_and_environment() {
        // LocalTesting skips cryptographic verification; the wrapper's
        // bundle-id + environment guards still run, so this exercises the
        // "valid receipt decoded" happy path end-to-end.
        let verifier = AppleVerifier::with_root_certificates(
            vec![APPLE_ROOT_CA_G3.to_vec()],
            "com.herald.test".to_string(),
            Environment::LocalTesting,
        );

        let payload = serde_json::json!({
            "bundleId": "com.herald.test",
            "environment": "LocalTesting",
            "originalTransactionId": "2000000123456789",
            "transactionId": "2000000123456789",
            "productId": "com.herald.test.pro.monthly",
        });
        let jws = make_local_testing_jws(&payload);

        let decoded = verifier
            .verify_signed_transaction(&jws)
            .expect("LocalTesting JWS with matching bundle+environment should verify");
        assert_eq!(
            decoded.original_transaction_id.as_deref(),
            Some("2000000123456789")
        );
        assert_eq!(
            decoded.product_id.as_deref(),
            Some("com.herald.test.pro.monthly")
        );
    }

    #[test]
    fn verify_signed_transaction_rejects_malformed_jws() {
        let verifier = AppleVerifier::with_root_certificates(
            vec![APPLE_ROOT_CA_G3.to_vec()],
            "com.herald.test".to_string(),
            Environment::LocalTesting,
        );

        // Not a 3-segment JWS.
        let result = verifier.verify_signed_transaction("not-a-jws");
        assert!(
            matches!(result, Err(IapError::AppleVerification(_))),
            "malformed JWS must be rejected, got {result:?}"
        );
    }

    #[test]
    fn verify_signed_transaction_rejects_bundle_id_mismatch() {
        // The wrapper inherits the upstream's bundle-id guard: a payload whose
        // bundleId differs from the verifier's must be rejected with
        // InvalidAppIdentifier (mapped to AppleVerification). This stands in
        // for the "wrong app / trust anchor" rejection state.
        let verifier = AppleVerifier::with_root_certificates(
            vec![APPLE_ROOT_CA_G3.to_vec()],
            "com.herald.test".to_string(),
            Environment::LocalTesting,
        );

        let payload = serde_json::json!({
            "bundleId": "com.someone.else.app",
            "environment": "LocalTesting",
            "originalTransactionId": "2000000999999999",
        });
        let jws = make_local_testing_jws(&payload);

        let result = verifier.verify_signed_transaction(&jws);
        assert!(
            matches!(result, Err(IapError::AppleVerification(ref e))
                if e.to_string().contains("InvalidAppIdentifier")),
            "bundle-id mismatch must surface as InvalidAppIdentifier, got {result:?}"
        );
    }

    #[test]
    fn verify_signed_transaction_rejects_real_jws_under_production_no_chain() {
        // Under a Production verifier the upstream demands a valid ES256 x5c
        // certificate chain. A JWS that carries no x5c header (or any
        // locally-fabricated JWS) is therefore rejected — this is the
        // "wrong / missing trust anchor" rejection state. We build a fully
        // well-formed JWS (valid header + payload + signature segments) but
        // without a real Apple-signed chain, so verification must fail.
        let verifier = AppleVerifier::with_root_certificates(
            vec![APPLE_ROOT_CA_G3.to_vec()],
            "com.herald.test".to_string(),
            Environment::Production,
        );

        let header = serde_json::json!({ "alg": "ES256", "typ": "JWS" });
        let payload = serde_json::json!({
            "bundleId": "com.herald.test",
            "environment": "Production",
        });
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        let sig_b64 = URL_SAFE_NO_PAD.encode(b"not-a-real-signature");
        let jws = format!("{header_b64}.{payload_b64}.{sig_b64}");

        let result = verifier.verify_signed_transaction(&jws);
        assert!(
            matches!(result, Err(IapError::AppleVerification(_))),
            "JWS without a valid Apple x5c chain must be rejected under Production, got {result:?}"
        );
    }

    #[test]
    fn verify_and_decode_notification_accepts_matching_payload() {
        let verifier = AppleVerifier::with_root_certificates(
            vec![APPLE_ROOT_CA_G3.to_vec()],
            "com.herald.test".to_string(),
            Environment::LocalTesting,
        );

        // Notification payload carries the bundleId / environment on the
        // inner `data` object the upstream's notification verifier reads.
        // `notificationUUID` is a required field on the decoded payload.
        let payload = serde_json::json!({
            "notificationType": "DID_RENEW",
            "notificationUUID": "00000000-0000-0000-0000-000000000000",
            "data": {
                "bundleId": "com.herald.test",
                "environment": "LocalTesting",
                "signedTransactionInfo": "header.payload.sig",
            }
        });
        let jws = make_local_testing_jws(&payload);

        let decoded = verifier
            .verify_and_decode_notification(&jws)
            .expect("LocalTesting notification with matching bundle should verify");
        assert_eq!(
            decoded.notification_type,
            crate::apple::models::NotificationTypeV2::DidRenew
        );
        // The signedTransactionInfo is an opaque string at this layer; the
        // caller verifies it separately via verify_signed_transaction.
        assert_eq!(
            decoded
                .data
                .as_ref()
                .and_then(|d| d.signed_transaction_info.as_deref()),
            Some("header.payload.sig")
        );
    }

    #[test]
    fn default_constructor_pins_bundled_apple_root_ca() {
        // Smoke test: the bundled Root CA constant is non-empty and DER-shaped
        // (starts with an ASN.1 SEQUENCE tag), so the default trust anchor is
        // actually wired into include_bytes!.
        assert!(
            APPLE_ROOT_CA_G3.len() > 100,
            "bundled Apple Root CA must be non-trivially sized"
        );
        assert_eq!(
            &APPLE_ROOT_CA_G3[..2],
            &[0x30, 0x82],
            "DER-encoded X.509 should start with an ASN.1 SEQUENCE (0x30 0x82)"
        );
    }
}
