/**
 * LDAP corporate-directory Realm setup helpers for Demo tests
 *
 * Flips the realm's LDAP configuration on/off for the "Given the realm has
 * corporate login enabled/disabled" steps shared by the US-LD-001/002
 * login demos and the US-LD-003 admin-config demo (including the
 * degradation assertion that needs LDAP off)
 * — docs/user-stories/auth/support-ldap.md.
 *
 * The directory is the REAL OpenLDAP container (`cas-demo-ldap`, started by
 * scripts/lib/demo_env.py): LDAPS on 127.0.0.1:6360, seeded by
 * backend/infra/tests/ldap-directory-assets/seed.ldif. Its certificate chains
 * to a private fixture CA, so the realm settings must carry `caCertPem` — the
 * same trust path a real private-CA deployment would use. The Settings UI does
 * NOT manage that field in v1 (and a UI save drops it), so it is seeded via
 * the admin configs API — the UI still drives every user-visible step, this
 * helper only provides the deployment-level trust anchor. Mirrors the
 * email-otp demos reading the OTP code from Redis because the demo env has no
 * mailbox: the flow is real, the out-of-band input is injected.
 *
 * Config rows are written through POST /api/configs/{realmId}/batch (the same
 * endpoint the Settings UI saves through) with the same row shapes the
 * frontend's buildLdapConfigRequest produces, plus `caCertPem`.
 *
 * These helpers do NOT edit seed data or SQL.
 *
 * @see ../pages/login-page.ts (`LoginPage.getAccessToken` — Bearer for the API)
 * @see ./auth.ts (`createBearerApiContext`, `REALM_ADMINS`, `clearSessionData`)
 * @see ./email-otp-setup.ts (the enable/disable/reset pattern this mirrors)
 */

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { Page, expect, type APIRequestContext } from '@playwright/test'
import type { UnifiedLogger } from './unified-logger'
import { clearSessionData, createBearerApiContext, REALM_ADMINS } from './auth'
import { LoginPage } from '../pages/login-page'

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000'

/**
 * The demo directory's well-known shape. Credentials and DNs are fixed by the
 * seed fixtures (backend/infra/tests/ldap-directory-assets) shared between the
 * test and demo environments; the container ACL denies anonymous entry reads,
 * so a service account is mandatory.
 */
export const DEMO_LDAP = {
  ldapsUrl: 'ldaps://127.0.0.1:6360',
  baseDn: 'dc=herald,dc=test',
  bindDn: 'cn=admin,dc=herald,dc=test',
  bindPassword: 'svc-password',
  userFilter: '(uid={login})',
  mailAttribute: 'mail',
  /** Directory users usable in demos (see seed.ldif). */
  users: {
    alice: { username: 'alice', password: 'alicepass', email: 'alice@example.com' },
  },
} as const

const CA_CERT_PATH = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
  '..',
  '..',
  'backend',
  'infra',
  'tests',
  'ldap-directory-assets',
  'certs',
  'ca.crt'
)

function readCaCertPem(): string {
  const pem = fs.readFileSync(CA_CERT_PATH, 'utf8')
  if (!pem.includes('BEGIN CERTIFICATE')) {
    throw new Error(`[Ldap Setup] Fixture CA at ${CA_CERT_PATH} does not look like PEM`)
  }
  return pem
}

interface LdapSettingsJson {
  enabled: boolean
  url: string
  starttls: boolean
  baseDn: string
  bindDn: string | null
  userFilter: string
  mailAttribute: string
  caCertPem: string
}

function buildSettings(enabled: boolean, url: string): LdapSettingsJson {
  return {
    enabled,
    url,
    // ldaps:// carries TLS in the scheme; StartTLS is the plaintext-ldap://
    // upgrade path and must stay off here (write-path validation enforces the
    // pairing, and the UI locks the switch for ldaps:// the same way).
    starttls: false,
    baseDn: DEMO_LDAP.baseDn,
    bindDn: DEMO_LDAP.bindDn,
    userFilter: DEMO_LDAP.userFilter,
    mailAttribute: DEMO_LDAP.mailAttribute,
    caCertPem: readCaCertPem(),
  }
}

/**
 * Log in as the realm admin via the login UI and return an APIRequestContext
 * authenticated with the post-login Bearer token (the same token the frontend
 * itself uses for admin API calls such as the configs batch upsert).
 */
async function createAdminApiContext(
  page: Page,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<APIRequestContext> {
  const credentials = REALM_ADMINS[realmId]
  if (!credentials) {
    throw new Error(`[Ldap Setup] No admin credentials registered for realm "${realmId}"`)
  }

  demoLogger.testCode.log(`[Ldap Setup] Logging in as admin of realm "${realmId}"...`)
  const loginPage = new LoginPage(page, demoLogger)
  await loginPage.loginAsAdmin(credentials.email, credentials.password, realmId)

  return createBearerApiContext(loginPage.getAccessToken())
}

async function putLdapConfig(
  api: APIRequestContext,
  realmId: string,
  settings: LdapSettingsJson,
  options: { bindPassword?: string } = {}
): Promise<void> {
  const configs: Array<Record<string, unknown>> = [
    {
      configType: 'ldap',
      configKey: 'settings',
      configValue: JSON.stringify(settings),
      isSecret: false,
      enabled: settings.enabled,
    },
  ]
  // Omitting the secret row on a disable keeps the stored password (the
  // backend's "empty secret preserves the stored value" rule); the enable
  // path always sends it because enabling with a bindDn requires one.
  if (settings.enabled || options.bindPassword) {
    configs.push({
      configType: 'ldap',
      configKey: 'bind_password',
      configValue: options.bindPassword ?? DEMO_LDAP.bindPassword,
      isSecret: true,
      enabled: settings.enabled,
    })
  }

  const response = await api.post(`${BASE_URL}/api/configs/${realmId}/batch`, {
    data: { configs },
  })
  if (!response.ok()) {
    const body = await response.text().catch(() => '')
    throw new Error(
      `[Ldap Setup] Failed to save LDAP config for realm "${realmId}": ` +
        `${response.status()} ${body}`
    )
  }
}

/**
 * Enable corporate-directory login for a realm (the "Given LDAP is enabled"
 * step). Seeds the full trusted configuration via the admin API and polls the
 * public status endpoint until it reports `enabled: true`, so the caller can
 * immediately assert login-page entry visibility.
 *
 * @param page        Playwright Page.
 * @param demoLogger  UnifiedLogger from the test fixture.
 * @param realmId     Target realm id.
 * @param options     `url` — override the directory URL (e.g. an unreachable
 *                    port to demo the directory-unavailable branch).
 */
export async function enableLdapForRealm(
  page: Page,
  demoLogger: UnifiedLogger,
  realmId: string,
  options: { url?: string } = {}
): Promise<void> {
  const url = options.url ?? DEMO_LDAP.ldapsUrl

  demoLogger.testCode.log(
    `[Ldap Setup] Enabling LDAP for realm "${realmId}" (url=${url})`
  )

  const api = await createAdminApiContext(page, demoLogger, realmId)
  try {
    await putLdapConfig(api, realmId, buildSettings(true, url))
    await expect
      .poll(
        async () => {
          const response = await api.get(`${BASE_URL}/api/auth/${realmId}/ldap/status`)
          return response.ok() ? (await response.json()).enabled : false
        },
        { timeout: 15000 }
      )
      .toBe(true)
  } finally {
    await api.dispose()
  }

  // Leave a clean unauthenticated state: the caller's next step is typically
  // `goto /realm/auth/login`, which the root loader redirects to /manage while
  // the admin session is still alive (login card never renders).
  await clearSessionData(page)

  demoLogger.testCode.log(`[Ldap Setup] LDAP enabled for realm "${realmId}"`)
}

/**
 * Disable corporate-directory login for a realm (degradation setup /
 * teardown). Config values are preserved — only `enabled` flips to false —
 * and the stored service-account password is kept (the secret row is omitted).
 * Best-effort: wrapped in try/catch so a teardown failure never hard-fails
 * the run.
 */
export async function disableLdapForRealm(
  page: Page,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<void> {
  try {
    demoLogger.testCode.log(`[Ldap Setup] Disabling LDAP for realm "${realmId}"`)

    const api = await createAdminApiContext(page, demoLogger, realmId)
    try {
      await putLdapConfig(api, realmId, buildSettings(false, DEMO_LDAP.ldapsUrl))
      await expect
        .poll(
          async () => {
            const response = await api.get(`${BASE_URL}/api/auth/${realmId}/ldap/status`)
            return response.ok() ? (await response.json()).enabled : true
          },
          { timeout: 15000 }
        )
        .toBe(false)
    } finally {
      await api.dispose()
    }

    await clearSessionData(page)

    demoLogger.testCode.log(`[Ldap Setup] LDAP disabled for realm "${realmId}"`)
  } catch (error) {
    // Teardown / degradation setup must never hard-fail the test run.
    console.warn(`[Ldap Setup] Failed to disable LDAP for realm "${realmId}":`, error)
  }
}

/**
 * Restore the "LDAP not configured" pristine state for the realm by deleting
 * both ldap config rows (`settings` + `bind_password`). Used by the
 * admin-config demo, whose initial-state and blocked-save-gate phases depend
 * on `hasBindPassword === false`. Best-effort (try/catch) when used as
 * teardown; 404 on a delete means the row is already gone.
 */
export async function resetLdapRowsForRealm(
  page: Page,
  demoLogger: UnifiedLogger,
  realmId: string
): Promise<void> {
  try {
    demoLogger.testCode.log(`[Ldap Setup] Resetting LDAP config rows for realm "${realmId}"`)

    const api = await createAdminApiContext(page, demoLogger, realmId)
    try {
      for (const key of ['settings', 'bind_password']) {
        const response = await api.delete(`${BASE_URL}/api/configs/${realmId}/ldap/${key}`)
        if (!response.ok() && response.status() !== 404) {
          const body = await response.text().catch(() => '')
          throw new Error(
            `[Ldap Setup] Failed to delete ldap config "${key}" for realm "${realmId}": ` +
              `${response.status()} ${body}`
          )
        }
      }
    } finally {
      await api.dispose()
    }

    await clearSessionData(page)

    demoLogger.testCode.log(`[Ldap Setup] LDAP config rows reset for realm "${realmId}"`)
  } catch (error) {
    // Teardown must never hard-fail the test run; an initial-state call that
    // fails loudly is preferable, but the admin login inside may legitimately
    // fail mid-teardown — log and continue.
    console.warn(`[Ldap Setup] Failed to reset LDAP rows for realm "${realmId}":`, error)
  }
}
