/** Shared test constants + factories for the Herald SDK suite. */

import { createHeraldClient, memoryStorage } from '../src'
import type { HeraldClient, HeraldSession, SessionEvent, TokenStorage } from '../src'

export const BASE_URL = 'http://localhost:3000'
export const REALM = 'realm-1'
export const CLIENT = 'client-1'

const realmPath = (suffix: string) => `${BASE_URL}/api/auth/${REALM}${suffix}`

export const urls = {
  login: realmPath('/login'),
  verifyTotp: realmPath('/login/verify-totp'),
  passkeyOptions: realmPath('/login/passkey/options'),
  passkeyVerify: realmPath('/login/passkey/verify'),
  passkey2faOptions: realmPath('/login/passkey/2fa/options'),
  passkey2faVerify: realmPath('/login/passkey/2fa/verify'),
  emailOtpSend: realmPath('/login/email-otp/send'),
  emailOtpVerify: realmPath('/login/email-otp/verify'),
  register: realmPath('/register'),
  verifyEmailTrigger: realmPath('/verify_email/trigger'),
  resetPasswordRequest: realmPath('/reset_password/request'),
  status: `${BASE_URL}/api/auth/status`,
  logout: `${BASE_URL}/api/auth/logout`,
  refresh: `${BASE_URL}/api/auth/browser-token/refresh`,
} as const

export interface FakeTokens {
  accessToken: string
  refreshToken: string
  expiresIn: number
  refreshExpiresIn: number
  tokenType: string
}

export function makeTokens(overrides: Partial<FakeTokens> = {}): FakeTokens {
  return {
    accessToken: 'at-1',
    refreshToken: 'rt-1',
    expiresIn: 3600,
    refreshExpiresIn: 86400,
    tokenType: 'Bearer',
    ...overrides,
  }
}

export function makeStatus(overrides: Partial<HeraldSession> = {}): Record<string, unknown> {
  return {
    authenticated: true,
    clientAppId: 'ca-1',
    clientId: CLIENT,
    credentialClass: 'custom_user_ui',
    permissions: ['profile_read'],
    realmId: REALM,
    scopes: ['profile_read'],
    userId: 'u-1',
    ...overrides,
  }
}

export interface TestClient {
  client: HeraldClient
  events: SessionEvent[]
}

export function makeClient(
  opts: { storage?: TokenStorage; onSessionChange?: (e: SessionEvent) => void } = {},
): TestClient {
  const events: SessionEvent[] = []
  const client = createHeraldClient({
    baseUrl: BASE_URL,
    realmId: REALM,
    clientId: CLIENT,
    storage: opts.storage ?? memoryStorage(),
    onSessionChange: opts.onSessionChange ?? ((e) => events.push(e)),
  })
  return { client, events }
}

/** Build a minimal WebAuthn credential object for `navigator.credentials.get`. */
export function fakeCredential() {
  const enc = (s: string) => new TextEncoder().encode(s).buffer
  return {
    id: 'cred-id',
    rawId: enc('rawId'),
    type: 'public-key',
    response: {
      authenticatorData: enc('authData'),
      clientDataJSON: enc('clientData'),
      signature: enc('sig'),
      userHandle: enc('u-1'),
    },
    getClientExtensionResults: () => ({ appid: false }),
  }
}

/** Temporarily make `localStorage` unavailable to exercise the SSR guard. */
export function disableLocalStorage(): () => void {
  const orig = (globalThis as { localStorage?: Storage }).localStorage
  Object.defineProperty(globalThis, 'localStorage', {
    value: undefined,
    configurable: true,
    writable: true,
  })
  return () => {
    Object.defineProperty(globalThis, 'localStorage', {
      value: orig,
      configurable: true,
      writable: true,
    })
  }
}

