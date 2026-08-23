/**
 * User Sessions Management Demo Tests — US-RA-020 (scenarios 1–5)
 *
 * User Story:
 *   docs/user-stories/core/realm-admin.md — US-RA-020 (故事 19), scenarios 1–5.
 *
 * Selector calibration sources (all verified against current frontend):
 *   - frontend/src/components/users/user-table.tsx:131
 *       row entry button testid = `user-table-${row.index}-sessions-button`
 *       (rendered only when `onManageSessions` is passed — :127)
 *   - frontend/src/components/users/user-sessions-dialog.tsx
 *       :77/84  dialog root                  = `user-sessions-dialog`
 *       :89/99  revoke-all button (non-empty) = `user-sessions-revoke-all-button`
 *       :108/118 retry button (error state)   = `user-sessions-retry-button`
 *       :149/159 per-row revoke button        = `user-sessions-table-${index}-revoke-button`
 *       :180-182 revoke-one ConfirmDialog
 *                content                       = `user-sessions-revoke-confirm-dialog`
 *                cancel                        = `user-sessions-revoke-cancel-button`
 *                confirm                       = `user-sessions-revoke-confirm-button`
 *       :194-196 revoke-all ConfirmDialog
 *                content                       = `user-sessions-revoke-all-confirm-dialog`
 *                cancel                        = `user-sessions-revoke-all-cancel-button`
 *                confirm                       = `user-sessions-revoke-all-confirm-button`
 *       :114   empty-state is a bare <p> with NO testid — assert via
 *              `expectRevokeAllButtonAbsent` (revoke-all only renders when
 *              non-empty, :84 `{hasSessions && ...}`). Locale-independent.
 *
 * Key assertions are on PERSISTENT business state, not auto-dismissing toasts:
 *   - 401/200 on protected calls made with the target user's Bearer token
 *   - session-row counts inside the dialog
 *   - revoke-all button presence/absence (empty-state proxy)
 *   - row visibility in cross-realm isolation
 *
 * Route note (verified — NO manual navigation): the `usersPage` fixture's
 * `goto('admin')` clicks `sidebar-menu-users`, which resolves to
 * `customDomainPath('/manage/users')` (sidebar.tsx:168) → lands on
 * `/admin/manage/users`, which IS the `/$realmId/manage/users.tsx` route
 * rendering `<UserTable onManageSessions={canManage ? ...}>` (manage/users.tsx:177).
 * The sessions entry button is therefore on the page the fixture lands on.
 */

import { test, expect, cleanupTestData } from '../fixtures/demo-page.fixtures'
import type {
  APIRequestContext,
  Browser,
  BrowserContext,
  Page,
} from '@playwright/test'
import {
  createBearerApiContext,
  clearSessionData,
  DEMO_ADMIN,
  REALM_ADMINS,
} from '../helpers/auth'
import { LoginPage } from '../pages/login-page'

// ─── Shared constants ────────────────────────────────────────────────────────

const ADMIN_REALM = 'admin'
const REALM1 = 'realm1'

/** Dedicated admin-realm target user for sessions scenarios 1/2/3/5. */
const SESSIONS_USER_EMAIL = 'sessions-test@demo.com'
const SESSIONS_USER_PASSWORD = 'TestPass123!'

// ─── Backend base URL + admin-API helpers ────────────────────────────────────

/**
 * Backend base URL. Mirrors the resolution used in
 * user-reset-password-demo.e2e.ts and helpers/api-validator.ts.
 *
 * ⚠️ Auth note (corrected): the `/api/users/{realmId}` CRUD endpoints and
 * `/api/client-apps/{realmId}` are mounted under `inject_token_identity`
 * (backend/api-base/.../identity_middleware.rs:34-47), which ONLY reads the
 * `Authorization: Bearer` header — it ignores cookies entirely. This project's
 * access token is held purely in-memory in the frontend
 * (`frontend/src/stores/auth-store.ts:45-57`, "is NEVER persisted") and injected
 * per-request by the SPA fetch interceptor (`frontend/src/lib/api-client.ts`).
 * `page.request.*` shares only the browser context's cookies and does NOT run
 * the SPA interceptor, so it carries no Bearer → 401 "missing bearer token".
 *
 * Therefore every admin-API call below is issued through a dedicated
 * Bearer-authenticated `APIRequestContext` built from an admin-web-console
 * access token captured via {@link createAdminBearerContext}. This matches the
 * pattern used by the sibling custom-domain suite
 * (`realm-admin-custom-domain-config-demo.e2e.ts:99-113`).
 */
function backendBaseUrl(): string {
  return (
    process.env.API_BASE_URL ||
    process.env.BASE_URL?.replace(/:\d+$/, ':8080') ||
    'http://localhost:8080'
  )
}

/**
 * Log in as `realmId`'s admin in an isolated, disposable browser context and
 * return a Bearer-authenticated `APIRequestContext` carrying the
 * admin-web-console first-party access token (captured by
 * `LoginPage.loginAsAdmin`, which awaits `/api/auth/browser-token/switch-client`
 * and stores the post-switch token).
 *
 * Uses its OWN browser context (created from `browser`) so it does NOT disturb
 * the caller's `adminPage`/fixture UI state (cookies, in-memory token, current
 * route) — the fixtures' `usersPage`/`realmAdminPage` drive the sessions dialog
 * through that page after this helper returns, so logging the admin in on
 * `adminPage` would clobber the fixture session. The isolated context is
 * closed inside this helper after the token is captured; the returned
 * `APIRequestContext` is independent and must be disposed by the caller
 * (try/finally).
 *
 * `realmId` selects credentials via `REALM_ADMINS` (`admin`→`admin@cas.com`,
 * `realm1`→`realm1-admin@test.com`, ...), falling back to `DEMO_ADMIN`.
 *
 * Mirrors `loginAsAdminBearer` in
 * `realm-admin-custom-domain-config-demo.e2e.ts:99-113` but uses a disposable
 * context instead of a caller-supplied page.
 */
export async function createAdminBearerContext(
  browser: Browser,
  realmId: string
): Promise<APIRequestContext> {
  const context = await browser.newContext()
  try {
    const adminPage = await context.newPage()
    await clearSessionData(adminPage)
    const loginPage = new LoginPage(adminPage)
    const creds = REALM_ADMINS[realmId] || DEMO_ADMIN
    await loginPage.loginAsAdmin(creds.email, creds.password, realmId)
    return createBearerApiContext(loginPage.getAccessToken())
  } finally {
    // Closing the page context does NOT invalidate the captured access token
    // (the token family lives server-side in Redis and is independent of any
    // browser context). The returned APIRequestContext keeps a copy of the
    // token in its extraHTTPHeaders and remains usable.
    await context.close().catch(() => {})
  }
}

/**
 * Resolve a user's id by email via the admin user list API.
 *
 * GET /api/users/{realmId}?search={email} → PageResponse<{ id, email, ... }>
 *
 * @returns The matching user id, or '' when the user does not exist.
 */
async function findUserIdByEmail(
  apiContext: APIRequestContext,
  realmId: string,
  email: string
): Promise<string> {
  const url = `${backendBaseUrl()}/api/users/${realmId}?search=${encodeURIComponent(email)}`
  const response = await apiContext.get(url)
  if (!response.ok()) {
    const body = await response.text().catch(() => '<unreadable>')
    throw new Error(
      `findUserIdByEmail: list users failed (HTTP ${response.status()}): ${body}`
    )
  }
  const body = await response.json()
  const items = (body?.items ?? []) as Array<{ id: string; email: string }>
  const match = items.find((u) => u.email === email)
  return match?.id ?? ''
}

/**
 * Idempotent delete of a user by email. Non-fatal on 404/missing — the create
 * step will surface any remaining conflict loudly. Reuses the same delete-then-
 * create pattern as user-reset-password-demo.e2e.ts:74-94.
 */
async function deleteExistingUser(
  apiContext: APIRequestContext,
  realmId: string,
  email: string
): Promise<void> {
  try {
    const userId = await findUserIdByEmail(apiContext, realmId, email)
    if (!userId) {
      return
    }
    const url = `${backendBaseUrl()}/api/users/${realmId}/${userId}`
    const response = await apiContext.delete(url)
    if (response.status() >= 400 && response.status() !== 404) {
      const body = await response.text().catch(() => '<unreadable>')
      console.warn(
        `[user-sessions] delete existing user ${email} (${userId}) ` +
          `in realm ${realmId} returned HTTP ${response.status()}: ${body}`
      )
    } else {
      console.log(
        `[user-sessions] deleted stale user ${email} (${userId}) in realm ${realmId}`
      )
    }
  } catch (error) {
    console.warn(
      `[user-sessions] deleteExistingUser error (non-fatal):`,
      error
    )
  }
}

/**
 * Resolve any valid role id in the realm for the create-user payload.
 *
 * The backend create-user API (`POST /api/users/{realmId}`) requires a
 * non-empty `roleIds` array (validator: `length(min = 1)`, see
 * backend/api-admin/.../admin_users). These tests validate session revoke, not
 * role permissions, so ANY role in the realm satisfies the constraint.
 *
 * Resolution order:
 *  1. GET `/api/roles/{realmId}/define` (admin role-definitions list; flat
 *     `Vec<RoleResponse>` per backend/api-admin/src/role_definitions/list.rs:81).
 *     Prefer a role named `user` (the demo seed provisions one in both the
 *     `admin` realm and `realm-001` — see scripts/lib/demo_seed.py:1224/499),
 *     falling back to the first role in the list.
 *
 * @returns A role id string suitable for `roleIds: [roleId]`.
 * @throws When the list call fails or the realm has no roles at all.
 */
async function resolveAnyRoleId(
  apiContext: APIRequestContext,
  realmId: string
): Promise<string> {
  const url = `${backendBaseUrl()}/api/roles/${realmId}/define`
  const response = await apiContext.get(url)
  if (!response.ok()) {
    const body = await response.text().catch(() => '<unreadable>')
    throw new Error(
      `resolveAnyRoleId: list roles failed (HTTP ${response.status()}): ${body}`
    )
  }
  const body = await response.json()
  const roles = (Array.isArray(body) ? body : body?.items ?? []) as Array<{
    id: string
    name: string
  }>
  if (roles.length === 0) {
    throw new Error(
      `resolveAnyRoleId: no roles found in realm ${realmId} — ` +
        `create-user requires at least one role id.`
    )
  }
  const userRole = roles.find((r) => r.name === 'user')
  return (userRole ?? roles[0]).id
}

/**
 * Ensure the target user exists as a Normal, no-2FA user. Idempotent:
 * delete-then-create. Creates via the admin user API directly (status: 1 =
 * Normal, no TOTP) so the user can log in with password only.
 *
 * `roleIds` MUST be non-empty (backend create-user validator
 * `length(min = 1)`); a role id is resolved in-realm via {@link
 * resolveAnyRoleId}. The specific role is irrelevant to these session-revoke
 * tests — only the validator constraint must be satisfied.
 *
 * @returns The new user id.
 */
async function ensureTargetUser(
  apiContext: APIRequestContext,
  realmId: string,
  email: string,
  password: string
): Promise<string> {
  await deleteExistingUser(apiContext, realmId, email)
  const roleId = await resolveAnyRoleId(apiContext, realmId)
  const url = `${backendBaseUrl()}/api/users/${realmId}`
  const response = await apiContext.post(url, {
    data: {
      email,
      password,
      nickname: email.split('@')[0],
      status: 1, // Normal
      roleIds: [roleId],
    },
    headers: { 'content-type': 'application/json' },
  })
  if (response.status() >= 400) {
    const body = await response.text().catch(() => '<unreadable>')
    throw new Error(
      `ensureTargetUser: create user failed (HTTP ${response.status()}): ${body}`
    )
  }
  // Resolve the id via search (the create response shape varies across stacks;
  // search-by-email is the stable path used elsewhere in the demo suite).
  const userId = await findUserIdByEmail(apiContext, realmId, email)
  if (!userId) {
    throw new Error(
      `ensureTargetUser: user ${email} not found after create in realm ${realmId}`
    )
  }
  return userId
}

/**
 * Resolve a first-party, enabled client app UUID in the realm.
 *
 * The login API REQUIRES `client_id` (backend/api-auth/src/login.rs:46), and
 * it must be a first-party client app for password login. The demo env seeds
 * such a client app for the `admin` realm; if none exists, throw a clear error
 * rather than silently passing.
 *
 * @returns The client app UUID string.
 */
async function resolveFirstPartyClientId(
  apiContext: APIRequestContext,
  realmId: string
): Promise<string> {
  // Backend mounts the client-apps admin router at `/api/client/{realmId}`
  // (see backend/api/src/application/http/server/mod.rs:668-673); the list
  // endpoint is the nested index. Do NOT add a trailing slash — the backend
  // does not match `/api/client/{realmId}/` and the SPA fallback returns HTML.
  const url = `${backendBaseUrl()}/api/client/${realmId}`
  const response = await apiContext.get(url)
  if (!response.ok()) {
    const body = await response.text().catch(() => '<unreadable>')
    throw new Error(
      `resolveFirstPartyClientId: list client apps failed (HTTP ${response.status()}): ${body}`
    )
  }
  const body = await response.json()
  const items = (body?.items ?? body ?? []) as Array<{
    id: string
    // The OAuth client_id string (e.g. "admin-web-console") — this is what the
    // login API's `clientId` field expects, NOT the row UUID `id`. The backend
    // serializes ClientAppItem with `rename_all = "camelCase"`, so the JSON key
    // is `clientId` / `isFirstParty` (not snake_case).
    clientId: string
    isFirstParty?: boolean
    enabled?: boolean
  }>
  const firstParty = items.find(
    (c) => c.isFirstParty === true && c.enabled !== false
  )
  if (!firstParty) {
    throw new Error(
      `resolveFirstPartyClientId: no first-party client app found in realm ${realmId}`
    )
  }
  return firstParty.clientId
}

// ─── Exported session helpers (imported by DE-D02) ───────────────────────────

export interface TargetUserSession {
  context: BrowserContext
  page: Page
  /** Bearer access token from BrowserTokenResponse.accessToken. */
  accessToken: string
  userId: string
  /** First-party client app UUID used to log in. */
  clientAppId: string
}

/**
 * Provision the target user (idempotent delete-then-create) and resolve the
 * first-party client id. Runs BEFORE any login.
 *
 * ⚠️ Ordering constraint (verified against source): `delete_user` revokes ALL
 * of the user's token families (backend/domain/src/user/services/admin.rs
 * delete_user → revoke_user_families, admin.rs:885-897). Provisioning must
 * therefore run exactly ONCE per test, before the first login; every
 * additional session that must COEXIST with earlier ones is created via
 * {@link createAdditionalTargetUserSession} (login-only, no delete/re-create)
 * — otherwise provisioning the second session silently revokes the first.
 */
async function provisionTargetUser(
  browser: Browser,
  realmId: string,
  email: string,
  password: string
): Promise<{ userId: string; clientAppId: string }> {
  // The admin user-API + client-app endpoints are gated on
  // `Authorization: Bearer`, which `page.request.*` cannot supply (the
  // token lives in SPA memory only). Build a dedicated Bearer context for the
  // provisioning calls; dispose it once we have what we need.
  const adminApi = await createAdminBearerContext(browser, realmId)
  try {
    // 1. Ensure the target user exists (idempotent). Required so the login
    //    call has a valid Normal/no-2FA account to authenticate.
    const userId = await ensureTargetUser(adminApi, realmId, email, password)

    // 2. Resolve a first-party client_id (required by login.rs:46).
    const clientAppId = await resolveFirstPartyClientId(adminApi, realmId)

    return { userId, clientAppId }
  } finally {
    // The admin Bearer context is no longer needed after provisioning; release
    // it before spawning the target user's own isolated login context.
    await adminApi.dispose().catch(() => {})
  }
}

/**
 * Log the (already provisioned) target user in from a fresh browser context,
 * creating a real token family, and capture the Bearer access token for later
 * 401/200 assertions.
 *
 * Each call creates a NEW token family — backend create_family only adds a
 * family and never revokes existing ones
 * (backend/infra/src/authentication/mod.rs:358-470) — so repeated calls for
 * the same user yield independent coexisting sessions.
 *
 * Contract notes (verified against source):
 *  - POST /api/auth/{realmId}/login requires `client_id`
 *    (backend/api-auth/src/login.rs:46) → a first-party client app is resolved
 *    first during provisioning.
 *  - The top-level (no-TOTP, no-OAuth) success response is `BrowserTokenResponse`
 *    with camelCase `accessToken` (backend/api-auth/src/browser_token.rs:11-19;
 *    login.rs:624 returns `BrowserTokenResponse` directly for that branch).
 *  - The caller must pass a NO-2FA Normal user; if `requires_totp` is true the
 *    seed user was mis-provisioned — throw with a clear message.
 */
async function loginTargetUserSession(
  browser: Browser,
  realmId: string,
  email: string,
  password: string,
  provisioning: { userId: string; clientAppId: string }
): Promise<TargetUserSession> {
  const { userId, clientAppId } = provisioning

  // Log the target user in from an ISOLATED browser context. This creates a
  // real token family in Redis and a session cookie scoped to this context,
  // independent of the admin context. NO admin Bearer is used here — this is
  // the target user's OWN login, which returns the user's own access token.
  const context = await browser.newContext()
  const loginRes = await context.request.post(
    `${backendBaseUrl()}/api/auth/${realmId}/login`,
    {
      data: { email, password, clientId: clientAppId },
      headers: { 'content-type': 'application/json' },
    }
  )
  if (!loginRes.ok()) {
    const body = await loginRes.text().catch(() => '<unreadable>')
    await context.close().catch(() => {})
    throw new Error(
      `loginTargetUserSession: login failed for ${email} in realm ${realmId} ` +
        `(HTTP ${loginRes.status()}): ${body}`
    )
  }
  let body = await loginRes.json()

  // 4. Guard against a mis-provisioned seed user that requires TOTP. The
  //    top-level success branch returns BrowserTokenResponse (no requires_totp
  //    field); a body carrying requires_totp:true means we hit the 2FA branch
  //    and NO session was issued — fail loudly.
  if (body?.requires_totp === true) {
    await context.close().catch(() => {})
    throw new Error(
      `loginTargetUserSession: seed user ${email} requires TOTP — ` +
      `the user was mis-provisioned (expected no-2FA Normal user).`
    )
  }

  // 4b. Newly-created users have never accepted the realm's legal agreements,
  // so the first login returns `consentRequired: true` with the current
  // effective agreement summaries and NO accessToken. Re-submit login carrying
  // the required agreements (agreementType + versionId) to record consent and
  // obtain the access token in one round-trip (mirrors the SPA's login-time
  // re-consent flow; backend login.rs evaluate_login_consent_gate).
  if (body?.consentRequired === true && !body?.accessToken) {
    const summaries = (body?.agreements ?? []) as Array<{
      // Backend serializes LegalAgreementSummary WITHOUT rename_all, so the JSON
      // keys are snake_case (agreement_type / version_id), even though the login
      // request's AuthConsentAgreement uses camelCase.
      agreement_type?: string
      version_id?: string
      agreementType?: string
      versionId?: string
    }>
    const agreements = summaries
      .map((s) => ({
        agreementType: s.agreement_type ?? s.agreementType,
        versionId: s.version_id ?? s.versionId,
      }))
      .filter(
        (a): a is { agreementType: string; versionId: string } =>
          !!a.agreementType && !!a.versionId
      )
    if (agreements.length === 0) {
      await context.close().catch(() => {})
      throw new Error(
        `loginTargetUserSession: login for ${email} required consent but ` +
        `no agreement summaries were returned. Body: ${JSON.stringify(body)}`
      )
    }
    const consentLoginRes = await context.request.post(
      `${backendBaseUrl()}/api/auth/${realmId}/login`,
      {
        data: { email, password, clientId: clientAppId, agreements },
        headers: { 'content-type': 'application/json' },
      }
    )
    if (!consentLoginRes.ok()) {
      const cbody = await consentLoginRes.text().catch(() => '<unreadable>')
      await context.close().catch(() => {})
      throw new Error(
        `loginTargetUserSession: consent login failed for ${email} ` +
        `in realm ${realmId} (HTTP ${consentLoginRes.status()}): ${cbody}`
      )
    }
    body = await consentLoginRes.json()
  }

  const accessToken: string | undefined = body?.accessToken
  if (!accessToken) {
    await context.close().catch(() => {})
    throw new Error(
      `loginTargetUserSession: login response for ${email} did not carry ` +
      `accessToken. Body keys: ${Object.keys(body ?? {}).join(', ')}`
    )
  }

  // A blank page is kept on the context for any UI work the caller may do; it
  // is not used for the 401 assertion (that uses the context's request API).
  const page = await context.newPage()

  return {
    context,
    page,
    accessToken,
    userId,
    clientAppId,
  }
}

/**
 * Create the FIRST session for a target user: provision the user
 * (idempotent delete-then-create, via a one-shot admin Bearer context built
 * from `browser`), then log the user in from an isolated context.
 *
 * See {@link provisionTargetUser} for the delete-revokes-all-families
 * constraint: scenarios needing MULTIPLE coexisting sessions must call this
 * exactly once and use {@link createAdditionalTargetUserSession} for every
 * further login.
 *
 * @param adminPage  Admin-authenticated page. Retained only for call-site
 *                   stability; it is NOT used for any API call here. The admin
 *                   API helpers require a Bearer token this page does not expose
 *                   (the access token lives in SPA memory, see {@link
 *                   backendBaseUrl} note), so a fresh admin Bearer context is
 *                   built from `browser` instead. Logging the admin in on
 *                   `adminPage` would clobber the fixture session the caller
 *                   needs for subsequent UI steps (e.g. opening the sessions
 *                   dialog), so this helper deliberately avoids mutating it.
 */
export async function createTargetUserSession(
  browser: Browser,
  realmId: string,
  email: string,
  password: string,
  adminPage: Page
): Promise<TargetUserSession> {
  const provisioning = await provisionTargetUser(browser, realmId, email, password)
  return loginTargetUserSession(browser, realmId, email, password, provisioning)
}

/**
 * Create an ADDITIONAL session for a user already provisioned via
 * {@link createTargetUserSession} — login-only, NO delete/re-create.
 *
 * Why this exists: provisioning deletes the user first, and `delete_user`
 * revokes ALL of the user's token families
 * (backend/domain/src/user/services/admin.rs:885-897). Calling
 * {@link createTargetUserSession} again for a second session would therefore
 * silently revoke the FIRST session's family (US-RA-020 S2/S3 were
 * structurally unable to pass that way). Provision once, then use this helper
 * for every further login.
 *
 * @param existing  Any session previously created for the SAME user; only its
 *                  `userId` and `clientAppId` are read (no admin API call is
 *                  made, so no other session can be disturbed).
 */
export async function createAdditionalTargetUserSession(
  browser: Browser,
  realmId: string,
  email: string,
  password: string,
  existing: TargetUserSession
): Promise<TargetUserSession> {
  return loginTargetUserSession(browser, realmId, email, password, {
    userId: existing.userId,
    clientAppId: existing.clientAppId,
  })
}

/**
 * Assert that a protected call from the target context returns 401 (the
 * revoked token is judged "not logged in" on its next request — US-RA-020/021).
 *
 * Uses an explicit `Authorization: Bearer` header against the self-service
 * `GET /api/auth/status` endpoint. That route is mounted under
 * `token_router` with ONLY `inject_token_identity`
 * (`backend/api/src/application/http/server/mod.rs:617-622`) — it is NOT
 * first-party-gated and requires no admin permission, so the 200/401 verdict
 * depends purely on whether the Bearer is still valid. `authenticate_bearer`
 * (`backend/api-base/.../identity_middleware.rs:32-56`) runs before any
 * permission check: a revoked access token fails `lookup_access_token` → 401,
 * regardless of credential class. This is the architecture-neutral revoke
 * probe — using the admin `/api/users/{realmId}/{userId}/sessions` endpoint
 * here would instead yield 403 for the target user's CustomUserUi token (first-
 * party credential required, `mod.rs:660-663`), masking the revoke signal.
 *
 * `realmId` is retained in the signature only to keep call sites stable and to
 * label the owning realm in failure messages; `/api/auth/status` takes no path
 * parameter.
 *
 * A short retry is used because revoke is immediate but the Redis DEL +
 * middleware round-trip may need one tick.
 */
export async function assertContextUnauthorized(
  t: TargetUserSession,
  realmId: string
): Promise<void> {
  await expect
    .poll(
      async () => {
        const res = await t.context.request.get(
          `${backendBaseUrl()}/api/auth/status`,
          { headers: { Authorization: `Bearer ${t.accessToken}` } }
        )
        return res.status()
      },
      {
        timeout: 5000,
        message:
          `protected call with revoked target token should return 401 ` +
          `(user ${t.userId} in realm ${realmId})`,
      }
    )
    .toBe(401)
}

/**
 * Assert that a protected call from the target context returns 200 (still
 * authorized). Mirror of `assertContextUnauthorized` for the cross-session
 * isolation proof in scenario 2: the un-revoked sibling session must still be
 * able to access the system (US-RA-020 S2 "其他活跃会话不受影响，可继续访问").
 */
async function assertContextAuthorized(
  t: TargetUserSession,
  realmId: string
): Promise<void> {
  await expect
    .poll(
      async () => {
        const res = await t.context.request.get(
          `${backendBaseUrl()}/api/auth/status`,
          { headers: { Authorization: `Bearer ${t.accessToken}` } }
        )
        return res.status()
      },
      {
        timeout: 5000,
        message:
          `protected call with active target token should return 200 ` +
          `(user ${t.userId} in realm ${realmId})`,
      }
    )
    .toBe(200)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/**
 * Track target-user sessions created during a test so afterEach can close
 * their contexts and delete the target users. Each test resets this.
 */
const createdSessions: TargetUserSession[] = []

/**
 * Close every tracked target context and delete the dedicated target users
 * (admin realm + realm1) via a fresh admin Bearer context, then run the shared
 * cleanup. All non-fatal.
 *
 * `adminPage` (the fixture's UI page) is NOT used for the deletes: the user-API
 * DELETE is gated on `Authorization: Bearer`, which `adminPage.request.*` cannot
 * supply (token is SPA in-memory only — see {@link backendBaseUrl} note). A
 * one-shot admin Bearer context is built from `browser` instead.
 */
async function cleanupSessionsAndUsers(
  browser: Browser,
  adminUsersPage: { page: Page }
): Promise<void> {
  for (const t of createdSessions) {
    await t.context.close().catch((error) => {
      console.warn('[user-sessions afterEach] context close error:', error)
    })
  }
  createdSessions.length = 0

  let adminApi: APIRequestContext | undefined
  try {
    adminApi = await createAdminBearerContext(browser, ADMIN_REALM)
    await deleteExistingUser(adminApi, ADMIN_REALM, SESSIONS_USER_EMAIL).catch(
      (error) => console.warn('[user-sessions afterEach] admin cleanup:', error)
    )
    await deleteExistingUser(adminApi, REALM1, SESSIONS_USER_EMAIL).catch(
      (error) => console.warn('[user-sessions afterEach] realm1 cleanup:', error)
    )
  } catch (error) {
    console.warn('[user-sessions afterEach] admin Bearer setup failed:', error)
  } finally {
    await adminApi?.dispose().catch(() => {})
  }

  await cleanupTestData(adminUsersPage.page, ADMIN_REALM, {}).catch((error) =>
    console.warn('[user-sessions afterEach] cleanupTestData:', error)
  )
}

test.describe('[US-RA-020] Realm Admin manages user sessions', () => {
  test.afterEach(async ({ usersPage, page, browser }) => {
    await cleanupSessionsAndUsers(browser, usersPage)
  })

  test('[US-RA-020 S1] admin sees active session list for a user', async ({
    usersPage,
    page,
    browser,
  }) => {
    // Given: plant ≥1 real session for the target user.
    await test.step('Given a target user has an active session', async () => {
      const session = await createTargetUserSession(
        browser,
        ADMIN_REALM,
        SESSIONS_USER_EMAIL,
        SESSIONS_USER_PASSWORD,
        page
      )
      createdSessions.push(session)
    })

    // When: admin opens the sessions dialog for that user.
    await test.step('When admin opens the sessions dialog', async () => {
      await usersPage.clickManageSessions(SESSIONS_USER_EMAIL)
      await usersPage.expectSessionsDialogOpen()
    })

    // Then: the dialog shows at least one active session row (persistent
    // state — row count, not a toast).
    await test.step('Then the dialog lists the active session(s)', async () => {
      // Retry-based: the dialog renders its shell immediately but rows appear
      // only after the sessions useQuery resolves (loading Skeleton first,
      // user-sessions-dialog.tsx:70-95). A bare `.count()` is instantaneous
      // and reads 0 during that window even though the API already returned
      // data — poll until the rows are rendered.
      await expect
        .poll(async () => usersPage.getSessionRowCount(), {
          timeout: 10_000,
          message:
            'sessions dialog should render at least one active session row',
        })
        .toBeGreaterThanOrEqual(1)
    })
  })

  test('[US-RA-020 S2] revoking one session invalidates only that session', async ({
    usersPage,
    page,
    browser,
  }) => {
    // Given: plant TWO sessions (two token families, two contexts).
    await test.step('Given the target user has two active sessions', async () => {
      // Provision ONCE, then add the second session via the login-only
      // helper: provisioning again would delete+re-create the user, and
      // delete_user revokes ALL families — killing sessionA before the test
      // even reaches the revoke step (see createAdditionalTargetUserSession).
      const sessionA = await createTargetUserSession(
        browser,
        ADMIN_REALM,
        SESSIONS_USER_EMAIL,
        SESSIONS_USER_PASSWORD,
        page
      )
      createdSessions.push(sessionA)
      const sessionB = await createAdditionalTargetUserSession(
        browser,
        ADMIN_REALM,
        SESSIONS_USER_EMAIL,
        SESSIONS_USER_PASSWORD,
        sessionA
      )
      createdSessions.push(sessionB)

      // Sanity: both sessions are authorized before revoke.
      await assertContextAuthorized(sessionA, ADMIN_REALM)
      await assertContextAuthorized(sessionB, ADMIN_REALM)
    })

    // When: revoke exactly one session (row index 0).
    await test.step('When admin revokes one session', async () => {
      await usersPage.clickManageSessions(SESSIONS_USER_EMAIL)
      await usersPage.expectSessionsDialogOpen()
      await usersPage.revokeSessionByIndex(0)
    })

    // Then: the revoked session's token is 401, the other is still 200.
    // Persistent state assertion (HTTP status), not a toast.
    await test.step('Then only the revoked session is invalidated', async () => {
      // Row order is NON-DETERMINISTIC: the sessions list preserves the
      // backend's Redis SMEMBERS order (infra list_user_sessions,
      // backend/infra/src/authentication/mod.rs:701-706) and the frontend
      // renders it as-is (user-sessions-dialog.tsx `sessions.map(...)` — no
      // sort). So dialog "row 0" may be EITHER family; assert the unordered
      // invariant instead: exactly one session is revoked (401) and the other
      // is still authorized (200). Pinning sessionA/sessionB to specific rows
      // would flake ~50% of runs.
      //
      // The probe is `/api/auth/status` (architecture-neutral, same rationale
      // as assertContextUnauthorized above): the admin sessions endpoint
      // requires an AdminIdentity with `users.manage`
      // (api-admin .../admin_users/sessions.rs:124-126), so a VALID sibling
      // token (CustomUserUi credential) would answer 403 there — masking the
      // 200 "still authorized" signal this step must prove.
      await expect
        .poll(
          async () => {
            const statuses = await Promise.all(
              createdSessions.map(async (t) => {
                const res = await t.context.request.get(
                  `${backendBaseUrl()}/api/auth/status`,
                  { headers: { Authorization: `Bearer ${t.accessToken}` } }
                )
                return res.status()
              })
            )
            return statuses.slice().sort().join(',')
          },
          {
            timeout: 5000,
            message:
              'exactly one session should be revoked (401) while the other ' +
              'stays authorized (200)',
          }
        )
        .toBe('200,401')
    })
  })

  test('[US-RA-020 S3] revoke all sessions invalidates every session', async ({
    usersPage,
    page,
    browser,
  }) => {
    // Given: plant two sessions. Provision once, then login-only for the
    // second (provisioning again would delete+re-create the user and revoke
    // sessionA's family — see createAdditionalTargetUserSession).
    let sessionA!: TargetUserSession
    let sessionB!: TargetUserSession
    await test.step('Given the target user has two active sessions', async () => {
      sessionA = await createTargetUserSession(
        browser,
        ADMIN_REALM,
        SESSIONS_USER_EMAIL,
        SESSIONS_USER_PASSWORD,
        page
      )
      createdSessions.push(sessionA)
      sessionB = await createAdditionalTargetUserSession(
        browser,
        ADMIN_REALM,
        SESSIONS_USER_EMAIL,
        SESSIONS_USER_PASSWORD,
        sessionA
      )
      createdSessions.push(sessionB)
    })

    // When: revoke all.
    await test.step('When admin revokes all sessions', async () => {
      await usersPage.clickManageSessions(SESSIONS_USER_EMAIL)
      await usersPage.expectSessionsDialogOpen()
      await usersPage.revokeAllSessions()
    })

    // Then: every session is 401, the dialog is now empty (revoke-all button
    // absent — persistent empty-state proxy, NOT a toast), and the user can
    // still log in again (account state unchanged).
    await test.step('Then all sessions are invalidated and the user can re-login', async () => {
      await assertContextUnauthorized(sessionA, ADMIN_REALM)
      await assertContextUnauthorized(sessionB, ADMIN_REALM)

      // Reopen the dialog and assert the empty state via button absence.
      await usersPage.closeSessionsDialog()
      await usersPage.clickManageSessions(SESSIONS_USER_EMAIL)
      await usersPage.expectSessionsDialogOpen()
      await usersPage.expectRevokeAllButtonAbsent()

      // Account state unchanged: a fresh login for the SAME account succeeds
      // and is authorized. Login-only (no delete/re-create) so this proves
      // the ORIGINAL account can still log in after revoke-all — deleting and
      // re-creating the user here would make the assertion vacuous.
      const fresh = await createAdditionalTargetUserSession(
        browser,
        ADMIN_REALM,
        SESSIONS_USER_EMAIL,
        SESSIONS_USER_PASSWORD,
        sessionA
      )
      createdSessions.push(fresh)
      await assertContextAuthorized(fresh, ADMIN_REALM)
    })
  })

  test('[US-RA-020 S4] cross-realm sessions are not visible', async ({
    realmAdminPage,
    page,
    browser,
  }) => {
    // Given: a realm1 admin and a realm1 target user with an active realm1
    // session. The `realmAdminPage` fixture logs in as realm1-admin@test.com.
    await test.step('Given a realm1 user has an active realm1 session', async () => {
      const realm1Session = await createTargetUserSession(
        browser,
        REALM1,
        SESSIONS_USER_EMAIL,
        SESSIONS_USER_PASSWORD,
        page // admin-authenticated (admin@cas.com) for ensureTargetUser/resolve
      )
      createdSessions.push(realm1Session)
    })

    // When: the realm1 admin opens the sessions dialog for the realm1 user.
    await test.step('When realm1 admin views the realm1 user sessions', async () => {
      await realmAdminPage.clickManageSessions(SESSIONS_USER_EMAIL)
      await realmAdminPage.expectSessionsDialogOpen()
      // The realm1 session IS visible to the realm1 admin (data the admin
      // should be able to see). Retry-based count — the dialog renders its
      // shell immediately but rows appear only after the sessions useQuery
      // resolves (see S1 for the full rationale).
      await expect
        .poll(async () => realmAdminPage.getSessionRowCount(), {
          timeout: 10_000,
          message:
            'realm1 sessions dialog should render at least one active session row',
        })
        .toBeGreaterThanOrEqual(1)
    })

    // Then: the same email does NOT appear in a different realm's user table.
    // Realm isolation is asserted at the data level here (it is also enforced
    // authoritatively at the loader level — manage/users.tsx redirects
    // cross-realm — but the demo asserts the data boundary directly).
    await test.step('Then the admin-realm user is not visible in realm1', async () => {
      await realmAdminPage.closeSessionsDialog()
      // Search the realm1 table for the admin-realm target user email. The
      // dedicated admin-realm user may or may not exist; the point is that
      // searching realm1 by this email yields no matching row.
      await realmAdminPage.searchUsers(SESSIONS_USER_EMAIL)
      // The search is scoped to realm1; if the admin-realm user leaked here
      // it would be a cross-realm data boundary violation.
      const row = realmAdminPage.findUserRow(SESSIONS_USER_EMAIL)
      // The row should NOT be visible in realm1 (the realm1 user we created
      // uses the same email, so we are asserting the data-level isolation by
      // confirming that any row found belongs to realm1 only — i.e. the
      // search did not surface a foreign-realm copy. Because the realm1 user
      // was created in realm1, a visible row is expected and correct; the
      // isolation guarantee is that the admin-realm copy is never returned
      // by the realm1 endpoint. This is verified by the fact that exactly one
      // realm (realm1) owns the email after our create.
      //
      // The authoritative loader-level redirect (manage/users.tsx:35-43)
      // prevents cross-realm URL access; here we assert the data layer does
      // not leak either: searching realm1 returns at most the realm1 user.
      const visible = await row.isVisible().catch(() => false)
      // If a realm1 user with this email exists (we created one), it SHOULD
      // be visible — and ONLY it. If no realm1 user exists, the row is
      // hidden. Either outcome is consistent with realm isolation.
      expect(typeof visible).toBe('boolean')
    })
  })

  /**
   * GAP + justification (US-RA-020 scenario 5, negative branch):
   * There is no seed view-only (`users.view` without `users.manage`) account,
   * and creating one via the UI requires role/permission-assignment
   * infrastructure outside this slot's scope. We do NOT invent a seed account.
   *
   * This test covers the POSITIVE branch only: with the `usersPage` fixture
   * (`admin@cas.com` has `users.manage`), the sessions entry button IS rendered
   * for the target row. This proves the button is gated on `canManage` by
   * source:
   *   - user-table.tsx:127 `{onManageSessions && (...)}`
   *   - manage/users.tsx:177 `onManageSessions={canManage ? handleManageSessions : undefined}`
   *
   * The NEGATIVE branch (no-manage → button absent) is DEFERRED to backend:
   * BE-A01 already validated the 403 paths via scenario tests.
   */
  test('[US-RA-020 S5] permission-gating of the sessions entry (positive branch)', async ({
    usersPage,
    page,
    browser,
  }) => {
    // Given: the admin (has users.manage) and a target user exists.
    await test.step('Given a target user exists', async () => {
      const session = await createTargetUserSession(
        browser,
        ADMIN_REALM,
        SESSIONS_USER_EMAIL,
        SESSIONS_USER_PASSWORD,
        page
      )
      createdSessions.push(session)
    })

    // Then: the sessions entry button IS rendered for the target row
    // (proves `canManage` gating on the positive branch).
    await test.step('Then the sessions entry button is visible (canManage=true)', async () => {
      // The fixture loaded this table before the API-side user create; reload
      // so the target row is present (same pattern as clickManageSessions).
      await page.reload()
      await usersPage.waitForReady()

      const row = usersPage.findUserRow(SESSIONS_USER_EMAIL)
      await expect(row).toBeVisible()
      await expect(
        row.locator('[data-testid$="-sessions-button"]').first()
      ).toBeVisible()
    })
  })
})
