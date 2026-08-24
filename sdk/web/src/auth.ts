/**
 * Authentication orchestration (DEC-js-sdk-008 / DEC-js-sdk-010).
 *
 * Each method calls a generated op through the per-instance transport client
 * (auto Bearer + silent refresh), maps the result to SDK public types, and
 * updates the session/token state. Login-family methods normalize the
 * multi-branch 200 into the `LoginResult` discriminated union.
 */

import {
  handlePasskey2FaOptions,
  handlePasskey2FaVerify,
  handlePasskeyOptions,
  handlePasskeyVerify,
  handleVerifyTotp,
  login,
  logout,
  register,
  resetPasswordRequest,
  send,
  status,
  verify,
  verifyEmailTrigger,
} from './generated/sdk.gen'
import type { Client } from './generated/client/types.gen'
import type { BrowserTokenResponse, StatusResponse } from './generated/types.gen'
import { HeraldError, toHeraldError } from './errors'
import { resolveOp } from './transport'
import type { AccessTokenHolder, SessionStore } from './session'
import type { TokenStorage } from './storage'
import type {
  ConsentAgreement,
  EmailOtpSendResult,
  HeraldSession,
  LoginResult,
  PasskeyLoginBeginResult,
  SecondFactor,
} from './types'
import type { AssertionResultJSON } from './webauthn'

// --- public payloads ---

export interface RegisterPayload {
  email: string
  password: string
  username?: string
  turnstileToken?: string
}

export interface TriggerVerifyEmailPayload {
  email: string
  turnstileToken?: string
}

export interface RequestPasswordResetPayload {
  email: string
  turnstileToken?: string
}

export interface LoginPayload {
  username?: string
  email?: string
  password: string
  turnstileToken?: string
  /** Agreements to satisfy a prior `consent-required` gate. */
  agreements?: ConsentAgreement[]
  /**
   * Optional OAuth context for host apps that drive an authorization-code flow
   * themselves (e.g. Herald's own frontend with PKCE). When present the backend
   * answers with `redirectTo`, surfaced as `{ kind: 'oauth-redirect' }`; the SDK
   * does NOT perform the token exchange (DEC-js-sdk-008 — that stays with the
   * caller).
   */
  oauthClientId?: string
  redirectUri?: string
  state?: string
}

export interface VerifyTotpPayload {
  tempToken: string
  code?: string
  backupCode?: string
  agreements?: ConsentAgreement[]
}

export interface PasskeyLoginBeginPayload {
  /** Present for 2FA (after a `requires-second-factor` login); absent for 1FA. */
  tempToken?: string
  turnstileToken?: string
  /**
   * Optional OAuth context for host apps driving an authorization-code flow
   * (first-party passkey logins from an OAuth-linked login page). Passkey
   * verify then answers with `redirectTo` (kind: 'oauth-redirect'); the SDK
   * does NOT perform the exchange (DEC-js-sdk-008).
   */
  oauth?: {
    clientId: string
    redirectUri: string
    state: string
  }
}

export interface PasskeyLoginFinishPayload {
  authToken: string
  assertion: AssertionResultJSON
  /** Present when finishing a 2FA passkey login. */
  tempToken?: string
  agreements?: ConsentAgreement[]
}

export interface EmailOtpSendPayload {
  email: string
  turnstileToken?: string
  /** Agreements to satisfy a prior `consent_required` conflict on re-send. */
  agreements?: ConsentAgreement[]
}

export interface EmailOtpVerifyPayload {
  email: string
  code: string
  agreements?: ConsentAgreement[]
}

// --- internal helpers ---

export interface AuthDeps {
  realmId: string
  clientId: string
  /** Per-instance transport client; routes every op through its interceptors. */
  client: Client
  accessTokenHolder: AccessTokenHolder
  storage: TokenStorage
  session: SessionStore
  /** Shared single-flight refresh core (from the transport). */
  refreshTokens: () => Promise<BrowserTokenResponse | null>
}

function applyTokens(tokens: BrowserTokenResponse, deps: AuthDeps): void {
  deps.accessTokenHolder.set(tokens.accessToken)
  deps.storage.setRefreshToken(tokens.refreshToken)
}

function sessionFromLoginSuccess(deps: AuthDeps): HeraldSession {
  return {
    authenticated: true,
    realmId: deps.realmId,
    userId: null,
    clientAppId: null,
    clientId: deps.clientId,
    credentialClass: 'custom_user_ui',
    permissions: [],
    scopes: [],
  }
}

function sessionFromStatus(s: StatusResponse, deps: AuthDeps): HeraldSession {
  return {
    authenticated: s.authenticated,
    realmId: s.realmId ?? deps.realmId,
    userId: s.userId ?? null,
    clientAppId: s.clientAppId ?? null,
    clientId: s.clientId ?? deps.clientId,
    credentialClass: s.credentialClass ?? null,
    permissions: s.permissions ?? [],
    scopes: s.scopes ?? [],
  }
}

function normalizeAgreements(raw: unknown): ConsentAgreement[] {
  if (!Array.isArray(raw)) return []
  return raw.map((a) => {
    const o = (a ?? {}) as Record<string, unknown>
    return {
      agreementType: String(o['agreementType'] ?? o['agreement_type'] ?? ''),
      versionId: String(o['versionId'] ?? o['version_id'] ?? ''),
      // Display passthrough for host apps that render the consent list.
      ...(a && typeof a === 'object' ? { raw: o } : {}),
    }
  })
}

function filterSecondFactors(arr: unknown): SecondFactor[] {
  return (arr as unknown[]).filter((f): f is SecondFactor => f === 'totp' || f === 'passkey')
}

/**
 * Discriminate a multi-branch login 200 body (DEC-js-sdk-010) into a
 * `LoginResult`. Success also stores the token set + emits `authenticated`.
 */
function toLoginResult(body: unknown, deps: AuthDeps): LoginResult {
  const b = (body ?? null) as Record<string, unknown> | null

  if (b && typeof b['accessToken'] === 'string') {
    applyTokens(b as unknown as BrowserTokenResponse, deps)
    const session = sessionFromLoginSuccess(deps)
    deps.session.emit({ type: 'authenticated', session })
    return { kind: 'success', session }
  }
  if (b && b['consentRequired'] === true) {
    return { kind: 'consent-required', agreements: normalizeAgreements(b['agreements']) }
  }
  if (b && Array.isArray(b['secondFactors']) && (b['secondFactors'] as unknown[]).length > 0) {
    return {
      kind: 'requires-second-factor',
      tempToken: String(b['tempToken'] ?? ''),
      expiresInSeconds: Number(b['expiresInSeconds'] ?? 0),
      secondFactors: filterSecondFactors(b['secondFactors']),
      userId: String(b['userId'] ?? ''),
      realmId: String(b['realmId'] ?? deps.realmId),
    }
  }
  if (b && typeof b['redirectTo'] === 'string') {
    return { kind: 'oauth-redirect', redirectTo: b['redirectTo'] }
  }
  throw new HeraldError({ kind: 'api', message: 'Unrecognized login response shape.' })
}

// --- factory ---

export function createAuth(deps: AuthDeps) {
  const { realmId, client } = deps

  return {
    async register(payload: RegisterPayload) {
      const data = await resolveOp(
        register({
          client,
          path: { realmId },
          body: {
            clientId: deps.clientId,
            email: payload.email,
            password: payload.password,
            ...(payload.username ? { username: payload.username } : {}),
            ...(payload.turnstileToken ? { turnstileToken: payload.turnstileToken } : {}),
          },
        }),
      )
      return { message: data.message, verificationRequired: data.verificationRequired }
    },

    async triggerVerifyEmail(payload: TriggerVerifyEmailPayload) {
      const data = await resolveOp(
        verifyEmailTrigger({
          client,
          path: { realmId },
          body: {
            clientId: deps.clientId,
            email: payload.email,
            ...(payload.turnstileToken ? { turnstileToken: payload.turnstileToken } : {}),
          },
        }),
      )
      return { message: data.message }
    },

    async requestPasswordReset(payload: RequestPasswordResetPayload) {
      const data = await resolveOp(
        resetPasswordRequest({
          client,
          path: { realmId },
          body: {
            clientId: deps.clientId,
            email: payload.email,
            ...(payload.turnstileToken ? { turnstileToken: payload.turnstileToken } : {}),
          },
        }),
      )
      return { message: data.message }
    },

    async login(payload: LoginPayload): Promise<LoginResult> {
      const body = await resolveOp<unknown>(
        login({
          client,
          path: { realmId },
          body: {
            clientId: deps.clientId,
            password: payload.password,
            ...(payload.username ? { username: payload.username } : {}),
            ...(payload.email ? { email: payload.email } : {}),
            ...(payload.turnstileToken ? { turnstileToken: payload.turnstileToken } : {}),
            ...(payload.agreements ? { agreements: payload.agreements } : {}),
            ...(payload.oauthClientId ? { oauthClientId: payload.oauthClientId } : {}),
            ...(payload.redirectUri ? { redirectUri: payload.redirectUri } : {}),
            ...(payload.state ? { state: payload.state } : {}),
          },
        }),
      )
      return toLoginResult(body, deps)
    },

    async verifyTotp(payload: VerifyTotpPayload): Promise<LoginResult> {
      const body = await resolveOp<unknown>(
        handleVerifyTotp({
          client,
          path: { realmId },
          body: {
            tempToken: payload.tempToken,
            ...(payload.code ? { code: payload.code } : {}),
            ...(payload.backupCode ? { backupCode: payload.backupCode } : {}),
            ...(payload.agreements ? { agreements: payload.agreements } : {}),
          },
        }),
      )
      return toLoginResult(body, deps)
    },

    passkey: {
      async loginBegin(payload: PasskeyLoginBeginPayload): Promise<PasskeyLoginBeginResult> {
        const data =
          payload.tempToken !== undefined
            ? await resolveOp(
                handlePasskey2FaOptions({
                  client,
                  path: { realmId },
                  body: { tempToken: payload.tempToken },
                }),
              )
            : await resolveOp(
                handlePasskeyOptions({
                  client,
                  path: { realmId },
                  body: {
                    clientId: deps.clientId,
                    ...(payload.turnstileToken ? { turnstileToken: payload.turnstileToken } : {}),
                    ...(payload.oauth ? { oauth: payload.oauth } : {}),
                  },
                }),
              )
        return { authToken: data.authToken, options: data.options }
      },

      async loginFinish(payload: PasskeyLoginFinishPayload): Promise<LoginResult> {
        const body = await resolveOp<unknown>(
          payload.tempToken !== undefined
            ? handlePasskey2FaVerify({
                client,
                path: { realmId },
                body: {
                  tempToken: payload.tempToken,
                  authToken: payload.authToken,
                  assertion: payload.assertion,
                  ...(payload.agreements ? { agreements: payload.agreements } : {}),
                },
              })
            : handlePasskeyVerify({
                client,
                path: { realmId },
                body: {
                  authToken: payload.authToken,
                  assertion: payload.assertion,
                  ...(payload.agreements ? { agreements: payload.agreements } : {}),
                },
              }),
        )
        return toLoginResult(body, deps)
      },
    },

    loginWithEmailOtp: {
      /**
       * Send a passwordless login code. The two 409 control-flow outcomes
       * (DEC-js-sdk-014) — `consent_required` (auto-register consent gate) and
       * `email_not_registered` (auto-register off) — resolve as
       * `{ kind: 'conflict' }` instead of throwing, mirroring the multi-branch
       * normalization `login()` applies to its 200 bodies. All other HTTP
       * failures throw `HeraldError`.
       */
      async send(payload: EmailOtpSendPayload): Promise<EmailOtpSendResult> {
        const { data, error, response } = await send({
          client,
          path: { realmId },
          body: {
            clientId: deps.clientId,
            email: payload.email,
            ...(payload.turnstileToken ? { turnstileToken: payload.turnstileToken } : {}),
            ...(payload.agreements ? { agreements: payload.agreements } : {}),
          },
        })
        if (data) {
          return { kind: 'sent', message: data.message, expiresInSeconds: data.expiresInSeconds }
        }
        const body = (error ?? {}) as Record<string, unknown>
        const code = body['code']
        if (
          response?.status === 409 &&
          (code === 'consent_required' || code === 'email_not_registered')
        ) {
          return {
            kind: 'conflict',
            code: String(code),
            message: String(body['message'] ?? ''),
            consentRequired: body['consentRequired'] === true,
            agreements: normalizeAgreements(body['agreements']),
          }
        }
        throw toHeraldError(error, response)
      },

      async verify(payload: EmailOtpVerifyPayload): Promise<LoginResult> {
        const body = await resolveOp<unknown>(
          verify({
            client,
            path: { realmId },
            body: {
              clientId: deps.clientId,
              email: payload.email,
              code: payload.code,
              ...(payload.agreements ? { agreements: payload.agreements } : {}),
            },
          }),
        )
        return toLoginResult(body, deps)
      },
    },

    async getStatus(): Promise<StatusResponse> {
      const data = await resolveOp(status({ client }))
      deps.session.emit({ type: 'authenticated', session: sessionFromStatus(data, deps) })
      return data
    },

    /**
     * Explicitly refresh the Bearer token family (startup restore, proactive
     * refresh). Single-flight: concurrent calls share one HTTP request with the
     * 401 auto-refresh interceptor. On success both the in-memory access token
     * and the stored refresh token are rotated.
     *
     * @throws {HeraldError} `kind: 'session-expired'` when no refresh token is
     *   stored or the refresh failed (reuse / expiry / family revocation); a
     *   `session-expired` event is emitted either way.
     */
    async refresh(): Promise<BrowserTokenResponse> {
      const tokens = await deps.refreshTokens()
      if (!tokens) {
        throw new HeraldError({
          kind: 'session-expired',
          message: 'Session refresh failed; sign in again.',
        })
      }
      return tokens
    },

    async logout() {
      const data = await resolveOp(logout({ client }))
      deps.accessTokenHolder.clear()
      deps.storage.setRefreshToken(null)
      deps.session.emit({ type: 'logged-out' })
      return { message: data?.message ?? 'Logged out' }
    },
  }
}
