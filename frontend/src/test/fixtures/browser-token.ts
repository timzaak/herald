/**
 * Test fixtures for the Bearer browser-token model (design §4.4).
 *
 * Shared token-set factories used by the FE-D01 (Bearer migration) Vitest
 * suites: PKCE exchange, refresh rotation, and the API client 401 interceptor.
 *
 * `BrowserTokenResponse` is the shared camelCase token-set contract consumed by
 * the auth store and the API client. The OAuth `/token` endpoint returns the
 * RFC 6749 snake_case `TokenResponse` shape, which `performPkceTokenExchange`
 * normalizes to `BrowserTokenResponse` — `makeTokenResponse()` mirrors the raw
 * wire shape, `makeBrowserTokenResponse()` mirrors the normalized shape.
 */

import type { BrowserTokenResponse, TokenResponse } from '@/lib/api-generated'

/**
 * A canonical token pair (access + refresh). Centralizing the strings keeps the
 * interceptor / refresh assertions readable and lets a test swap one field
 * (e.g. the rotated refresh token) while keeping the rest stable.
 */
export const TOKEN_FIXTURE = {
  accessToken: 'at-original-aaaa',
  rotatedAccessToken: 'at-rotated-bbbb',
  refreshToken: 'rt-original-1111',
  rotatedRefreshToken: 'rt-rotated-2222',
  expiredAccessToken: 'at-expired-cccc',
  clientId: 'admin-web-console',
} as const

/**
 * Build a normalized (camelCase) `BrowserTokenResponse` — the shape returned by
 * the Herald SDK's token family operations and `performPkceTokenExchange`.
 */
export function makeBrowserTokenResponse(
  overrides?: Partial<BrowserTokenResponse>
): BrowserTokenResponse {
  return {
    accessToken: TOKEN_FIXTURE.accessToken,
    refreshToken: TOKEN_FIXTURE.refreshToken,
    tokenType: 'Bearer',
    expiresIn: 900,
    refreshExpiresIn: 2592000,
    ...overrides,
  }
}

/**
 * Build a rotated token set (both access AND refresh rotated) — the normal
 * response of a successful refresh, which rotates the refresh token family.
 */
export function makeRotatedBrowserTokenResponse(
  overrides?: Partial<BrowserTokenResponse>
): BrowserTokenResponse {
  return makeBrowserTokenResponse({
    accessToken: TOKEN_FIXTURE.rotatedAccessToken,
    refreshToken: TOKEN_FIXTURE.rotatedRefreshToken,
    ...overrides,
  })
}

/**
 * Build a raw RFC 6749 snake_case `TokenResponse` — the wire shape of the
 * `/api/oauth/{realmId}/token` PKCE exchange BEFORE normalization.
 */
export function makeTokenResponse(overrides?: Partial<TokenResponse>): TokenResponse {
  return {
    access_token: TOKEN_FIXTURE.accessToken,
    refresh_token: TOKEN_FIXTURE.refreshToken,
    token_type: 'Bearer',
    expires_in: 900,
    refresh_expires_in: 2592000,
    ...overrides,
  }
}
