/**
 * Email-OTP Realm Setup Helpers for Demo Tests
 *
 * Convenience wrappers that flip the Email-OTP feature on/off for a realm.
 * These are the "Given Realm has OTP on / off" setup steps shared by the
 * US-EO-001/002 user-flow demos and the US-EO-003 admin-config demo
 * (including the degradation assertion that needs OTP off).
 *
 * OTP is flipped via the admin REST API (PUT /api/realms/{realmId}/
 * config/email-otp — the same endpoint the Settings UI saves through), NOT
 * through the Settings UI:
 *
 * - Frontend commit 364767b2 guards the UI switches/save button behind
 *   `emailStatus.configured` (email-config-form.tsx `emailOtpDisabled`), so
 *   clicking them requires a configured email channel.
 * - But this demo suite REQUIRES the email channel to stay UNCONFIGURED:
 *   `EmailService::send_email` (backend/core/src/third/email.rs) silently
 *   skips delivery when the realm has no email config, while the OTP code is
 *   persisted to Redis BEFORE the send attempt — so the send endpoint returns
 *   200 and the tests read the code from Redis via email-otp-redis-helper.
 *   With a provider configured (even with a fake Resend key) the backend
 *   really attempts delivery and the send endpoint 500s ("resend send
 *   failed: 401 Unauthorized").
 * - The API endpoint is not subject to the UI guard, so it is the only path
 *   that satisfies both constraints.
 *
 * `enableEmailOtpForRealm` additionally restores the unconfigured email
 * channel (deleting leftover `config_type='email'` rows) when a previous
 * run/demo left a provider configured — otherwise the OTP send step would
 * 500 as described above.
 *
 * These helpers do NOT edit seed data or SQL.
 *
 * @see ../pages/login-page.ts (`LoginPage.getAccessToken` — Bearer for the API)
 * @see ./auth.ts (`createBearerApiContext`, `REALM_ADMINS`, `clearSessionData`)
 */

import { Page, expect, type APIRequestContext } from '@playwright/test'
import type { UnifiedLogger } from './unified-logger'
import { clearSessionData, createBearerApiContext, REALM_ADMINS } from './auth'
import { LoginPage } from '../pages/login-page'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

/**
 * Every `realm_config` key the email channel can occupy (`config_type='email'`
 * — see `EmailService::read_email_config`). Deleting all of them restores the
 * "email not configured" state this demo suite (and the email-config-demo's
 * initial-state assertions) depends on.
 */
const EMAIL_CONFIG_KEYS = [
  'provider',
  'from_address',
  'resend_api_key',
  'smtp_host',
  'smtp_port',
  'smtp_username',
  'smtp_password',
  'smtp_encryption',
] as const

/**
 * Log in as the realm admin via the login UI and return an APIRequestContext
 * authenticated with the post-login Bearer token (the same token the frontend
 * itself uses for admin API calls such as PUT config/email-otp).
 */
async function createAdminApiContext(
  page: Page,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<APIRequestContext> {
  const credentials = REALM_ADMINS[realmId]
  if (!credentials) {
    throw new Error(`[EmailOtp Setup] No admin credentials registered for realm "${realmId}"`)
  }

  demoLogger.testCode.log(`[EmailOtp Setup] Logging in as admin of realm "${realmId}"...`)
  const loginPage = new LoginPage(page, demoLogger)
  await loginPage.loginAsAdmin(credentials.email, credentials.password, realmId)

  return createBearerApiContext(loginPage.getAccessToken())
}

/**
 * Restore the "email channel not configured" state for the realm.
 *
 * Idempotent: when the status endpoint already reports `configured: false`
 * (the pristine seed state), nothing is deleted. Otherwise every known
 * `config_type='email'` key is deleted (404 = already gone) and the status is
 * polled back to `configured: false`.
 */
async function ensureEmailChannelNotConfigured(
  api: APIRequestContext,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<void> {
  const statusUrl = `${BASE_URL}/api/configs/${realmId}/email/status`
  const statusResponse = await api.get(statusUrl)
  if (!statusResponse.ok()) {
    const body = await statusResponse.text().catch(() => '')
    throw new Error(
      `[EmailOtp Setup] Email status check failed for realm "${realmId}": ` +
        `${statusResponse.status()} ${body}`
    )
  }
  const status = await statusResponse.json()

  if (!status.configured) {
    demoLogger.testCode.log(
      `[EmailOtp Setup] Email channel already unconfigured for realm "${realmId}"; nothing to clean up`
    )
    return
  }

  demoLogger.testCode.log(
    `[EmailOtp Setup] Email channel is configured (provider=${status.provider}); ` +
      `deleting config rows so OTP send silently skips instead of 500ing`
  )
  for (const key of EMAIL_CONFIG_KEYS) {
    const deleteResponse = await api.delete(
      `${BASE_URL}/api/configs/${realmId}/email/${key}`
    )
    // 404 = the key was never stored; everything else must fail loud.
    if (!deleteResponse.ok() && deleteResponse.status() !== 404) {
      const body = await deleteResponse.text().catch(() => '')
      throw new Error(
        `[EmailOtp Setup] Failed to delete email config "${key}" for realm "${realmId}": ` +
          `${deleteResponse.status()} ${body}`
      )
    }
  }

  await expect.poll(
    async () => {
      const response = await api.get(statusUrl)
      return response.ok() ? (await response.json()).configured : false
    },
    { timeout: 15000 }
  ).toBe(false)
}

/**
 * PUT the realm's Email-OTP configuration via the admin API.
 *
 * Body keys are camelCase (`enabled`, `autoRegister`) per the backend request
 * schema (UpdateRealmEmailOtpConfigRequest, serde rename_all = "camelCase").
 */
async function putEmailOtpConfig(
  api: APIRequestContext,
  realmId: string,
  enabled: boolean,
  autoRegister: boolean
): Promise<void> {
  const response = await api.put(`${BASE_URL}/api/realms/${realmId}/config/email-otp`, {
    data: { enabled, autoRegister },
  })
  if (!response.ok()) {
    const body = await response.text().catch(() => '')
    throw new Error(
      `[EmailOtp Setup] Failed to ${enabled ? 'enable' : 'disable'} Email-OTP for realm "${realmId}": ` +
        `${response.status()} ${body}`
    )
  }
  const data = await response.json()
  expect(data.enabled).toBe(enabled)
  expect(data.autoRegister).toBe(autoRegister)
}

/**
 * Enable Email-OTP login for a realm (the "Given Realm has OTP on" step).
 *
 * Logs in as the realm admin, restores the unconfigured email channel when a
 * previous run/demo left a provider configured (see
 * ensureEmailChannelNotConfigured), then flips the OTP config on via the
 * admin API. Idempotent: the PUT re-writes the same values when OTP is
 * already on.
 *
 * @param page        Playwright Page.
 * @param demoLogger  UnifiedLogger from the test fixture.
 * @param realmId     Target realm id.
 * @param options     `autoRegister` — when true, auto-registration of
 *                    unregistered emails is enabled together with OTP.
 */
export async function enableEmailOtpForRealm(
  page: Page,
  demoLogger: UnifiedLogger,
  realmId: string,
  options: { autoRegister?: boolean } = {}
): Promise<void> {
  const { autoRegister = false } = options

  demoLogger.testCode.log(
    `[EmailOtp Setup] Enabling Email-OTP for realm "${realmId}" (autoRegister=${autoRegister})`
  )

  const api = await createAdminApiContext(page, demoLogger, realmId)
  try {
    await ensureEmailChannelNotConfigured(api, demoLogger, realmId)
    await putEmailOtpConfig(api, realmId, true, autoRegister)
  } finally {
    await api.dispose()
  }

  // Leave a clean unauthenticated state: the caller's next step is typically
  // `goto /realm/auth/login`, which the root loader redirects to /manage while
  // the admin session is still alive (login card never renders).
  await clearSessionData(page)

  demoLogger.testCode.log(`[EmailOtp Setup] Email-OTP enabled for realm "${realmId}"`)
}

/**
 * Disable Email-OTP login for a realm (best-effort teardown / degradation
 * setup).
 *
 * Logs in as the realm admin and flips the OTP config off (enabled=false,
 * autoRegister=false) via the admin API. Wrapped in try/catch so a teardown
 * failure never hard-fails the run — it logs and continues.
 *
 * @param page        Playwright Page.
 * @param demoLogger  UnifiedLogger from the test fixture.
 * @param realmId     Target realm id.
 */
export async function disableEmailOtpForRealm(
  page: Page,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<void> {
  try {
    demoLogger.testCode.log(`[EmailOtp Setup] Disabling Email-OTP for realm "${realmId}"`)

    const api = await createAdminApiContext(page, demoLogger, realmId)
    try {
      await putEmailOtpConfig(api, realmId, false, false)
    } finally {
      await api.dispose()
    }

    // Match enableEmailOtpForRealm: clear the admin session so teardown leaves
    // a clean state for the next test.
    await clearSessionData(page)

    demoLogger.testCode.log(`[EmailOtp Setup] Email-OTP disabled for realm "${realmId}"`)
  } catch (error) {
    // Teardown / degradation setup must never hard-fail the test run.
    console.warn(`[EmailOtp Setup] Failed to disable Email-OTP for realm "${realmId}":`, error)
  }
}
