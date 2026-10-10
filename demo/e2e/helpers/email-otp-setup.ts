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
 * - The PUT endpoint enforces the same prerequisite server-side (write-path
 *   guard mirroring the registration-side one): enabling with
 *   `enabled=true` while the email channel is unconfigured is a 400.
 * - But this demo suite REQUIRES the email channel to stay UNCONFIGURED
 *   during the login flow: `EmailService::send_email`
 *   (backend/core/src/third/email.rs) silently skips delivery when the realm
 *   has no email config, while the OTP code is persisted to Redis BEFORE the
 *   send attempt — so the send endpoint returns 200 and the tests read the
 *   code from Redis via email-otp-redis-helper. With a provider configured
 *   (even with a fake Resend key) the backend really attempts delivery and
 *   the send endpoint 500s ("resend send failed: 401 Unauthorized").
 * - `enableEmailOtpForRealm` therefore satisfies both constraints in
 *   sequence: provision a minimal fake Resend config via POST
 *   /api/configs/batch (so the PUT passes the write-path guard), flip the
 *   OTP config on, then delete the `config_type='email'` rows to restore
 *   the unconfigured channel the send step depends on.
 *
 * `enableEmailOtpForRealm` additionally restores the unconfigured email
 * channel (deleting leftover `config_type='email'` rows) when a previous
 * run/demo left a provider configured — otherwise the OTP send step would
 * 500 as described above.
 *
 * These helpers do NOT edit seed data or SQL.
 *
 * @see ../pages/login-page.ts (`LoginPage.getAccessToken` — Bearer for the API)
 * @see ./auth.ts (`createAdminApiContext`, `clearSessionData`)
 */

import { Page, expect, type APIRequestContext } from '@playwright/test'
import type { UnifiedLogger } from './unified-logger'
import { clearSessionData, createAdminApiContext } from './auth'

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

const EMAIL_STATUS_POLL_TIMEOUT = 15000

interface EmailChannelStatus {
  configured: boolean
  provider?: string
}

/**
 * GET /api/configs/email/status, failing loud on transport/server errors —
 * shared by both ensure* helpers so the endpoint contract lives in one place.
 */
async function fetchEmailStatus(
  api: APIRequestContext,
  realmId: string
): Promise<EmailChannelStatus> {
  const response = await api.get(`${BASE_URL}/api/configs/email/status`)
  if (!response.ok()) {
    const body = await response.text().catch(() => '')
    throw new Error(
      `[EmailOtp Setup] Email status check failed for realm "${realmId}": ` +
        `${response.status()} ${body}`
    )
  }
  return await response.json()
}

/**
 * Poll the email status endpoint until `configured` reaches `expected`. The
 * poll body tolerates transient non-ok responses as "not configured" instead
 * of throwing (unlike fetchEmailStatus) so it can ride out restarts — EXCEPT
 * 401/403: an expired admin token never recovers mid-poll, and reporting it as
 * "not configured" would mask the real failure behind a poll timeout.
 */
async function pollEmailConfigured(
  api: APIRequestContext,
  expected: boolean
): Promise<void> {
  await expect
    .poll(
      async () => {
        const response = await api.get(`${BASE_URL}/api/configs/email/status`)
        if (response.ok()) {
          return (await response.json()).configured
        }
        if (response.status() === 401 || response.status() === 403) {
          const body = await response.text().catch(() => '')
          throw new Error(
            `[EmailOtp Setup] Email status poll got ${response.status()} (admin token rejected?) ${body}`
          )
        }
        return false
      },
      { timeout: EMAIL_STATUS_POLL_TIMEOUT }
    )
    .toBe(expected)
}

/**
 * Restore the "email channel not configured" state for the realm.
 *
 * Idempotent: when the status endpoint already reports `configured: false`
 * (the pristine seed state), nothing is deleted. Otherwise every known
 * `config_type='email'` key is deleted (404 = already gone) and the status is
 * polled back to `configured: false`.
 */
export async function ensureEmailChannelNotConfigured(
  api: APIRequestContext,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<void> {
  const status = await fetchEmailStatus(api, realmId)

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
  // The deletes hit distinct keys and each tolerates 404, so they run in
  // parallel; errors are then checked in key order for a deterministic
  // fail-loud message.
  const deleteResponses = await Promise.all(
    EMAIL_CONFIG_KEYS.map(async (key) => ({
      key,
      response: await api.delete(`${BASE_URL}/api/configs/email/${key}`),
    }))
  )
  for (const { key, response: deleteResponse } of deleteResponses) {
    // 404 = the key was never stored; everything else must fail loud.
    if (!deleteResponse.ok() && deleteResponse.status() !== 404) {
      const body = await deleteResponse.text().catch(() => '')
      throw new Error(
        `[EmailOtp Setup] Failed to delete email config "${key}" for realm "${realmId}": ` +
          `${deleteResponse.status()} ${body}`
      )
    }
  }

  await pollEmailConfigured(api, false)
}

/**
 * Provision a minimal (fake) Resend email configuration via the admin API.
 *
 * The PUT config/email-otp endpoint rejects `enabled: true` while the email
 * channel is unconfigured (write-path guard mirroring the registration-side
 * prerequisite), so the enable helper must make `configured` flip to true
 * first. Idempotent: when the status endpoint already reports
 * `configured: true`, nothing is written.
 *
 * The rows match the backend test helper `insert_resend_email_config_direct`
 * (provider / from_address / resend_api_key — the three fields
 * `EmailService::is_email_configured` requires for Resend). The key is
 * fake; it only needs to be non-empty because the helper deletes these rows
 * again before the demo's send step (see ensureEmailChannelNotConfigured).
 */
export async function ensureEmailChannelConfigured(
  api: APIRequestContext,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<void> {
  if ((await fetchEmailStatus(api, realmId)).configured) {
    demoLogger.testCode.log(
      `[EmailOtp Setup] Email channel already configured for realm "${realmId}"; nothing to provision`
    )
    return
  }

  demoLogger.testCode.log(
    `[EmailOtp Setup] Provisioning temporary Resend config for realm "${realmId}" ` +
      `(write-path guard requires a configured channel to enable OTP)`
  )
  const response = await api.post(`${BASE_URL}/api/configs/batch`, {
    data: {
      configs: [
        { configType: 'email', configKey: 'provider', configValue: 'resend', isSecret: false, enabled: true },
        {
          configType: 'email',
          configKey: 'from_address',
          configValue: 'noreply@example.com',
          isSecret: false,
          enabled: true,
        },
        {
          configType: 'email',
          configKey: 'resend_api_key',
          configValue: 're_demo_setup_key',
          isSecret: true,
          enabled: true,
        },
      ],
    },
  })
  if (!response.ok()) {
    const body = await response.text().catch(() => '')
    throw new Error(
      `[EmailOtp Setup] Failed to provision email config for realm "${realmId}": ` +
        `${response.status()} ${body}`
    )
  }

  await pollEmailConfigured(api, true)
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
 * Logs in as the realm admin, provisions a temporary minimal email config
 * (the PUT write-path guard rejects enabling without one — see
 * ensureEmailChannelConfigured), flips the OTP config on via the admin API,
 * then restores the unconfigured email channel the demo's send step depends
 * on (see ensureEmailChannelNotConfigured). Idempotent: the PUT re-writes
 * the same values when OTP is already on.
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
    // Order matters: the PUT write-path guard rejects enabling without a
    // configured email channel, so provision one first, enable, then restore
    // the unconfigured channel the demo's send step depends on (silent-skip
    // delivery + Redis-only codes).
    await ensureEmailChannelConfigured(api, demoLogger, realmId)
    await putEmailOtpConfig(api, realmId, true, autoRegister)
  } finally {
    // The restore must run even when the PUT fails: a leftover fake Resend
    // config breaks the "unconfigured ⇒ send silently skips" premise every
    // later OTP step (and the email-config demo's clean-state assertions)
    // relies on.
    await ensureEmailChannelNotConfigured(api, demoLogger, realmId)
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
