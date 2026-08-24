/**
 * Public SDK types (design §5.1 / §5.2 / DEC-js-sdk-010).
 *
 * DTO response types are re-exported from the (internal) generated layer so
 * consumers get a stable, typed surface without depending on generated paths.
 */

import type {
  BrowserTokenResponse,
  CredentialClass,
  CredentialScope,
  StatusResponse,
} from './generated/types.gen'

export type { BrowserTokenResponse, StatusResponse }

export type HeraldCredentialClass = CredentialClass

/** A normalized session view, derived from `/api/auth/status`. */
export interface HeraldSession {
  authenticated: boolean
  realmId: string | null
  userId: string | null
  clientAppId: string | null
  clientId: string | null
  credentialClass: HeraldCredentialClass | null
  permissions: string[]
  scopes: CredentialScope[]
}

export type SessionEvent =
  | { type: 'authenticated'; session: HeraldSession }
  | {
      type: 'session-expired'
      reason: 'refresh-failed' | 'family-revoked' | 'client-app-disabled'
    }
  | { type: 'logged-out' }

/** Second factors the backend may request on `POST /login` (DEC-js-sdk-010). */
export type SecondFactor = 'totp' | 'passkey'

/** Agreement a caller must re-submit (via `agreements`) to pass a consent gate. */
export interface ConsentAgreement {
  agreementType: string
  versionId: string
  /**
   * The backend's original agreement summary (snake_case display fields:
   * `title`, `version_no`, `effective_at`, `mode`, ...), passed through for
   * host apps that render the consent list. Optional — the re-submit shape
   * above is the contract; `raw` is display metadata only.
   */
  raw?: Record<string, unknown>
}

// --- Login result discriminated union (DEC-js-sdk-010) ---

export interface LoginSuccess {
  kind: 'success'
  session: HeraldSession
}

export interface LoginRequiresSecondFactor {
  kind: 'requires-second-factor'
  tempToken: string
  expiresInSeconds: number
  secondFactors: SecondFactor[]
  userId: string
  realmId: string
}

export interface LoginConsentRequired {
  kind: 'consent-required'
  /** Agreements the integrator must render + re-submit via `login`/`verify` `agreements`. */
  agreements: ConsentAgreement[]
}

export interface LoginOauthRedirect {
  kind: 'oauth-redirect'
  redirectTo: string
}

// --- Email-OTP send result (DEC-js-sdk-014) ---

/** A real send: the code was issued and is valid for `expiresInSeconds`. */
export interface EmailOtpSent {
  kind: 'sent'
  message: string
  expiresInSeconds: number
}

/**
 * A 409 control-flow outcome — NOT an error. `consent_required` carries the
 * agreement list the integrator must render and re-send via `agreements`;
 * `email_not_registered` means auto-register is off for the realm.
 */
export interface EmailOtpConflict {
  kind: 'conflict'
  /** `consent_required` | `email_not_registered` (backend `email_otp.rs`). */
  code: string
  message: string
  consentRequired: boolean
  /** Agreement summaries (with `raw` display passthrough) for the consent gate. */
  agreements: ConsentAgreement[]
}

export type EmailOtpSendResult = EmailOtpSent | EmailOtpConflict

export type LoginResult =
  | LoginSuccess
  | LoginRequiresSecondFactor
  | LoginConsentRequired
  | LoginOauthRedirect

/** Result of `passkey.loginBegin` (1FA or 2FA). `options` is the WebAuthn
 *  `PublicKeyCredentialRequestOptions` JSON returned by the server. */
export interface PasskeyLoginBeginResult {
  authToken: string
  options: unknown
}
