/**
 * Phone-Login Realm Setup Helpers for Demo Tests
 *
 * Convenience wrappers that flip the realm's "phone login" switch (and the
 * fake SMS channel it is gated on) for the US-PL-001/002 user-flow demos and
 * the US-PL-004 admin-config demo.
 *
 * The switch is flipped via the admin REST API (PUT
 * /api/realms/{realmId}/config/phone-login — the same endpoint the Settings
 * UI saves through), NOT through the Settings UI:
 *
 * - Enabling requires `sms_status.configured === true` (fail-closed write
 *   guard, DEC-phone-login-006), so a minimal FAKE Aliyun channel is
 *   provisioned first via POST /api/configs/batch. The credentials only need
 *   to be non-empty: `SmsService::is_sms_configured` checks presence, and the
 *   demos intercept the browser-side send requests anyway (`page.route`).
 * - Unlike the email-OTP setup, the fake channel is KEPT while the switch is
 *   on: the Security-page phone tab and the Settings switch gate both read
 *   `configured`, and the real send would fail loudly (502) through fake
 *   Aliyun credentials — interception happens in the browser, never here.
 * - Teardown order matters: the config KV delete guard rejects deleting the
 *   channel rows while the switch is on (`ensure_provider_config_deletable`),
 *   so the switch goes off FIRST, then the rows are deleted and the status is
 *   polled back to `configured: false` (the pristine state the config demo's
 *   initial assertions depend on).
 *
 * These helpers do NOT edit seed data or SQL (phone bindings on accounts are
 * seeded via `phone-db-helper.ts`).
 *
 * @see ../../../backend/core/src/third/sms.rs (`ALIYUN_REQUIRED_KEYS`, `SmsConfigStatus`)
 * @see ../../../backend/api/src/application/http/realm/phone_login_config.rs (fail-closed PUT)
 * @see ./auth.ts (`createAdminApiContext`, `clearSessionData`)
 */

import { Page, expect, type APIRequestContext } from '@playwright/test'
import type { UnifiedLogger } from './unified-logger'
import { clearSessionData, createAdminApiContext } from './auth'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

/**
 * The channel form also writes empty fields for the unselected provider.
 * Cleanup must remove those rows even when the channel is not configured.
 */
const SMS_CONFIG_KEYS = [
  'provider',
  'aliyun_access_key_id',
  'aliyun_access_key_secret',
  'aliyun_sign_name',
  'aliyun_template_code',
  'twilio_account_sid',
  'twilio_auth_token',
  'twilio_from',
  'twilio_content_sid',
] as const

const SMS_STATUS_POLL_TIMEOUT = 15000

interface SmsChannelStatus {
  configured: boolean
  provider?: string
  missingFields?: string[]
}

/** GET /api/configs/sms/status, failing loud on transport/server errors. */
async function fetchSmsStatus(api: APIRequestContext): Promise<SmsChannelStatus> {
  const response = await api.get(`${BASE_URL}/api/configs/sms/status`)
  if (!response.ok()) {
    const body = await response.text().catch(() => '')
    throw new Error(`[PhoneLogin Setup] SMS status check failed: ${response.status()} ${body}`)
  }
  return await response.json()
}

/**
 * A failed status request is not evidence that the channel is unconfigured.
 */
async function pollSmsConfigured(api: APIRequestContext, expected: boolean): Promise<void> {
  await expect
    .poll(
      async () => (await fetchSmsStatus(api)).configured,
      { timeout: SMS_STATUS_POLL_TIMEOUT }
    )
    .toBe(expected)
}

/**
 * PUT the realm's phone-login configuration via the admin API. Body keys are
 * camelCase (`enabled`, `defaultCountryCode`) per the backend request schema
 * (UpdateRealmPhoneLoginConfigRequest, serde rename_all = "camelCase").
 */
async function putPhoneLoginConfig(
  api: APIRequestContext,
  realmId: string,
  enabled: boolean
): Promise<void> {
  const response = await api.put(`${BASE_URL}/api/realms/${realmId}/config/phone-login`, {
    data: { enabled, defaultCountryCode: 'CN' },
  })
  if (!response.ok()) {
    const body = await response.text().catch(() => '')
    throw new Error(
      `[PhoneLogin Setup] Failed to ${enabled ? 'enable' : 'disable'} phone login for realm "${realmId}": ` +
        `${response.status()} ${body}`
    )
  }
  const data = await response.json()
  expect(data.enabled).toBe(enabled)
}

/**
 * Read the realm's phone-login config via the admin REST API (the same
 * endpoint the switch saves through) — authoritative post-save assertion
 * independent of the switch UI state.
 */
export async function getPhoneLoginConfig(
  api: APIRequestContext,
  realmId: string
): Promise<{ enabled: boolean; defaultCountryCode?: string }> {
  const response = await api.get(`${BASE_URL}/api/realms/${realmId}/config/phone-login`)
  if (!response.ok()) {
    throw new Error(`[PhoneLogin Setup] getPhoneLoginConfig failed: ${response.status()}`)
  }
  return await response.json()
}

/**
 * Provision a minimal (fake) Aliyun SMS channel via the admin API.
 *
 * Idempotent: when the status endpoint already reports `configured: true`,
 * nothing is written. The credential values only need to be non-empty —
 * see the file header for why the channel is safe to leave configured.
 */
export async function ensureSmsChannelConfigured(
  api: APIRequestContext,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<void> {
  if ((await fetchSmsStatus(api)).configured) {
    demoLogger.testCode.log(
      `[PhoneLogin Setup] SMS channel already configured for realm "${realmId}"; nothing to provision`
    )
    return
  }

  demoLogger.testCode.log(
    `[PhoneLogin Setup] Provisioning temporary Aliyun config for realm "${realmId}" ` +
      `(the phone-login switch's fail-closed write guard requires a configured channel)`
  )
  const response = await api.post(`${BASE_URL}/api/configs/batch`, {
    data: {
      configs: [
        { configType: 'sms', configKey: 'provider', configValue: 'aliyun', isSecret: false, enabled: true },
        {
          configType: 'sms',
          configKey: 'aliyun_access_key_id',
          configValue: 'LTAI_demo_setup_key',
          isSecret: false,
          enabled: true,
        },
        {
          configType: 'sms',
          configKey: 'aliyun_access_key_secret',
          configValue: 'aliyun-demo-setup-secret',
          isSecret: true,
          enabled: true,
        },
        {
          configType: 'sms',
          configKey: 'aliyun_sign_name',
          configValue: 'HeraldDemo',
          isSecret: false,
          enabled: true,
        },
        {
          configType: 'sms',
          configKey: 'aliyun_template_code',
          configValue: 'SMS_demo_setup_template',
          isSecret: false,
          enabled: true,
        },
      ],
    },
  })
  if (!response.ok()) {
    const body = await response.text().catch(() => '')
    throw new Error(
      `[PhoneLogin Setup] Failed to provision SMS config for realm "${realmId}": ${response.status()} ${body}`
    )
  }

  await pollSmsConfigured(api, true)
}

/**
 * Delete every SMS channel config row. 404 (never stored) is tolerated;
 * fails loud on anything else. Returns true when a delete was rejected by
 * the KV delete guard (400 — phone login still enabled).
 */
async function deleteAllSmsKeys(api: APIRequestContext, realmId: string): Promise<boolean> {
  let deleteGuarded = false
  for (const key of SMS_CONFIG_KEYS) {
    const response = await api.delete(`${BASE_URL}/api/configs/sms/${key}`)
    if (response.ok() || response.status() === 404) continue
    if (response.status() === 400) {
      deleteGuarded = true
      continue
    }
    const body = await response.text().catch(() => '')
    throw new Error(
      `[PhoneLogin Setup] Failed to delete SMS config "${key}" for realm "${realmId}": ` +
        `${response.status()} ${body}`
    )
  }
  return deleteGuarded
}

/**
 * Restore the "SMS channel not configured" pristine state for the realm.
 *
 * MUST run after the switch is off: the KV delete guard rejects deleting the
 * channel rows while phone login is enabled. When the delete answers 400
 * (dangling-enabled leftover from an interrupted run), the switch is turned
 * off once and the delete retried before failing loud.
 */
export async function ensureSmsChannelNotConfigured(
  api: APIRequestContext,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<void> {
  // No-op fast path: the authoritative list is the cleanup's own final
  // assertion — an empty list already IS the pristine state.
  const existing = await api.get(`${BASE_URL}/api/configs/sms`)
  if (!existing.ok()) {
    const body = await existing.text().catch(() => '')
    throw new Error(
      `[PhoneLogin Setup] SMS config read failed for realm "${realmId}": ${existing.status()} ${body}`
    )
  }
  if ((await existing.json()).length === 0) {
    demoLogger.testCode.log(
      `[PhoneLogin Setup] SMS channel already pristine for realm "${realmId}"; nothing to clean`
    )
    return
  }

  const status = await fetchSmsStatus(api)
  demoLogger.testCode.log(
    `[PhoneLogin Setup] SMS channel is configured (provider=${status.provider}); ` +
      `deleting config rows to restore the pristine unconfigured state`
  )

  const deleteGuarded = await deleteAllSmsKeys(api, realmId)
  if (deleteGuarded) {
    demoLogger.testCode.log(
      `[PhoneLogin Setup] Channel delete guarded by an enabled switch; disabling switch and retrying`
    )
    await putPhoneLoginConfig(api, realmId, false)
    const stillGuarded = await deleteAllSmsKeys(api, realmId)
    if (stillGuarded) {
      throw new Error(
        `[PhoneLogin Setup] SMS config rows for realm "${realmId}" still delete-guarded after disabling the switch`
      )
    }
  }

  await pollSmsConfigured(api, false)
  const remaining = await api.get(`${BASE_URL}/api/configs/sms`)
  expect(remaining.ok(), 'SMS cleanup requires a successful authoritative read').toBe(true)
  expect(await remaining.json(), 'All channel rows created by the demo must be removed').toEqual([])
}

/**
 * Enable phone login for a realm (the "Given Realm has phone login on" step).
 *
 * Logs in as the realm admin, provisions the temporary fake Aliyun channel
 * (fail-closed write guard requires one — see ensureSmsChannelConfigured),
 * then flips the switch on via the admin API. Idempotent: the PUT re-writes
 * the same values when the switch is already on.
 */
export async function enablePhoneLoginForRealm(
  page: Page,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<void> {
  demoLogger.testCode.log(`[PhoneLogin Setup] Enabling phone login for realm "${realmId}"`)

  const api = await createAdminApiContext(page, demoLogger, realmId)
  try {
    await ensureSmsChannelConfigured(api, demoLogger, realmId)
    await putPhoneLoginConfig(api, realmId, true)
  } finally {
    await api.dispose()
  }

  // Leave a clean unauthenticated state: the caller's next step is typically
  // `goto /realm/auth/login`, which the root loader redirects away from while
  // the admin session is still alive.
  await clearSessionData(page)

  demoLogger.testCode.log(`[PhoneLogin Setup] Phone login enabled for realm "${realmId}"`)
}

/**
 * Switch off before deleting channel rows: the backend guards active channels.
 * Cleanup failures propagate so polluted state cannot be accepted as a pass.
 */
export async function disablePhoneLoginAndClearChannel(
  page: Page,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<void> {
  const api = await createAdminApiContext(page, demoLogger, realmId)
  try {
    await putPhoneLoginConfig(api, realmId, false)
    await ensureSmsChannelNotConfigured(api, demoLogger, realmId)
    demoLogger.testCode.log(`[PhoneLogin Setup] Cleanup verified: phone login off; SMS unconfigured (${realmId})`)
  } finally {
    await api.dispose()
    await clearSessionData(page)
  }
}
