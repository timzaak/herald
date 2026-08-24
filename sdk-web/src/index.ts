/**
 * herald-auth-web — Herald official browser JavaScript SDK (design §4.4 / §5).
 *
 * Framework-agnostic, zero runtime dependencies. See README for integration.
 */

// Public client factory + config.
export { createHeraldClient } from './config'
export type { HeraldClient, HeraldClientConfig, SetTokensPayload, TokenBridge } from './config'

// Errors.
export { HeraldError } from './errors'
export type { HeraldErrorKind, HeraldErrorInit } from './errors'

// Storage adapters.
export { localStorageStorage, memoryStorage } from './storage'
export type { TokenStorage } from './storage'

// WebAuthn assertion helper (integrators call this between passkey loginBegin
// and loginFinish).
export { performPasskeyAssertion } from './webauthn'
export type { AssertionResultJSON, PublicKeyCredentialRequestOptionsJSON } from './webauthn'

// Public types.
export type {
  BrowserTokenResponse,
  ConsentAgreement,
  EmailOtpConflict,
  EmailOtpSendResult,
  EmailOtpSent,
  HeraldCredentialClass,
  HeraldSession,
  LoginConsentRequired,
  LoginOauthRedirect,
  LoginRequiresSecondFactor,
  LoginResult,
  LoginSuccess,
  PasskeyLoginBeginResult,
  SecondFactor,
  SessionEvent,
  StatusResponse,
} from './types'

// Method payload types.
export type {
  EmailOtpSendPayload,
  EmailOtpVerifyPayload,
  LoginPayload,
  PasskeyLoginBeginPayload,
  PasskeyLoginFinishPayload,
  RegisterPayload,
  RequestPasswordResetPayload,
  TriggerVerifyEmailPayload,
  VerifyTotpPayload,
} from './auth'
