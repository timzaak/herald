import { describe, test, expect, it } from 'vitest'
import {
  parseEmailConfig,
  buildEmailConfigRequest,
  emptyCustomDomainConfig,
  normalizeCustomDomainConfig,
  toUpdateCustomDomainConfigRequest,
  parseLdapConfig,
  buildLdapConfigRequest,
} from '../realm-config-utils'
import type {
  RealmConfigResponse,
  UpdateCustomDomainConfigRequest,
  UpsertRealmConfigRequest,
} from '@/lib/api-generated'
import type { CustomDomainConfigForm, LdapConfigForm } from '@/lib/schemas/realm-config'

const makeConfig = (
  configType: string,
  configKey: string,
  configValue: string
): RealmConfigResponse =>
  ({
    configType,
    configKey,
    configValue,
    enabled: true,
    isSecret: false,
    id: 'test-id',
    realmId: 'test-realm',
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
  }) as RealmConfigResponse

describe('parseEmailConfig', () => {
  test('returns defaults when email configs are empty', () => {
    const result = parseEmailConfig([])

    expect(result).toEqual({
      provider: 'resend',
      fromAddress: '',
      resendApiKey: undefined,
      smtpHost: undefined,
      smtpPort: '587',
      smtpUsername: undefined,
      smtpPassword: undefined,
      smtpEncryption: 'starttls',
    })
  })

  test('parses full resend config', () => {
    const configs: RealmConfigResponse[] = [
      makeConfig('email', 'provider', 'resend'),
      makeConfig('email', 'from_address', 'noreply@example.com'),
      makeConfig('email', 'resend_api_key', 're_xxxxx'),
    ]

    const result = parseEmailConfig(configs)

    expect(result).toEqual({
      provider: 'resend',
      fromAddress: 'noreply@example.com',
      resendApiKey: 're_xxxxx',
      smtpHost: undefined,
      smtpPort: '587',
      smtpUsername: undefined,
      smtpPassword: undefined,
      smtpEncryption: 'starttls',
    })
  })

  test('parses full smtp config', () => {
    const configs: RealmConfigResponse[] = [
      makeConfig('email', 'provider', 'smtp'),
      makeConfig('email', 'from_address', 'admin@corp.com'),
      makeConfig('email', 'smtp_host', 'smtp.corp.com'),
      makeConfig('email', 'smtp_port', '465'),
      makeConfig('email', 'smtp_username', 'admin@corp.com'),
      makeConfig('email', 'smtp_password', 'secret-pass'),
      makeConfig('email', 'smtp_encryption', 'ssl'),
    ]

    const result = parseEmailConfig(configs)

    expect(result).toEqual({
      provider: 'smtp',
      fromAddress: 'admin@corp.com',
      resendApiKey: undefined,
      smtpHost: 'smtp.corp.com',
      smtpPort: '465',
      smtpUsername: 'admin@corp.com',
      smtpPassword: 'secret-pass',
      smtpEncryption: 'ssl',
    })
  })

  test('parses partial config with defaults', () => {
    const configs: RealmConfigResponse[] = [
      makeConfig('email', 'provider', 'smtp'),
      makeConfig('email', 'smtp_host', 'smtp.example.com'),
    ]

    const result = parseEmailConfig(configs)

    expect(result.provider).toBe('smtp')
    expect(result.fromAddress).toBe('')
    expect(result.smtpHost).toBe('smtp.example.com')
    expect(result.smtpPort).toBe('587')
    expect(result.smtpEncryption).toBe('starttls')
    expect(result.smtpPassword).toBeUndefined()
  })

  test('ignores non-email config types', () => {
    const configs: RealmConfigResponse[] = [
      makeConfig('totp', 'settings', '{"enabled":true}'),
      makeConfig('registration', 'enabled', 'true'),
      makeConfig('email', 'provider', 'resend'),
    ]

    const result = parseEmailConfig(configs)

    expect(result.provider).toBe('resend')
  })
})

describe('buildEmailConfigRequest', () => {
  test('builds request with all non-secret fields', () => {
    const config = {
      provider: 'smtp' as const,
      fromAddress: 'noreply@example.com',
      smtpHost: 'smtp.example.com',
      smtpPort: '465',
      smtpUsername: 'user@example.com',
      smtpEncryption: 'ssl' as const,
      resendApiKey: undefined,
      smtpPassword: 'my-new-password',
    }

    const result = buildEmailConfigRequest(config)

    const keys = result.map((r) => r.configKey)
    expect(keys).toContain('provider')
    expect(keys).toContain('from_address')
    expect(keys).toContain('smtp_host')
    expect(keys).toContain('smtp_port')
    expect(keys).toContain('smtp_username')
    expect(keys).toContain('smtp_encryption')
    expect(keys).toContain('smtp_password')
    expect(keys).not.toContain('resend_api_key')

    // All entries have configType 'email'
    expect(result.every((r) => r.configType === 'email')).toBe(true)
  })

  test('skips masked secret values', () => {
    const config = {
      provider: 'resend' as const,
      fromAddress: 'noreply@example.com',
      resendApiKey: '••••••••', // masked placeholder
      smtpHost: undefined,
      smtpPort: '587',
      smtpUsername: undefined,
      smtpPassword: '••••••••',
      smtpEncryption: 'starttls' as const,
    }

    const result = buildEmailConfigRequest(config)

    const keys = result.map((r) => r.configKey)
    expect(keys).not.toContain('resend_api_key')
    expect(keys).not.toContain('smtp_password')
  })

  test('includes new secret values when provided', () => {
    const config = {
      provider: 'resend' as const,
      fromAddress: 'noreply@example.com',
      resendApiKey: 're_new_key_123',
      smtpHost: undefined,
      smtpPort: '587',
      smtpUsername: undefined,
      smtpPassword: undefined,
      smtpEncryption: 'starttls' as const,
    }

    const result = buildEmailConfigRequest(config)

    const apiKeyEntry = result.find((r) => r.configKey === 'resend_api_key')
    expect(apiKeyEntry).toBeDefined()
    expect(apiKeyEntry!.configValue).toBe('re_new_key_123')
    expect(apiKeyEntry!.isSecret).toBe(true)
  })

  test('marks isSecret correctly for all fields', () => {
    const config = {
      provider: 'smtp' as const,
      fromAddress: 'noreply@example.com',
      resendApiKey: 're_key',
      smtpHost: 'smtp.example.com',
      smtpPort: '587',
      smtpUsername: 'user',
      smtpPassword: 'pass',
      smtpEncryption: 'starttls' as const,
    }

    const result = buildEmailConfigRequest(config)

    // Non-secret fields
    expect(result.find((r) => r.configKey === 'provider')!.isSecret).toBe(false)
    expect(result.find((r) => r.configKey === 'from_address')!.isSecret).toBe(false)
    expect(result.find((r) => r.configKey === 'smtp_host')!.isSecret).toBe(false)
    expect(result.find((r) => r.configKey === 'smtp_port')!.isSecret).toBe(false)
    expect(result.find((r) => r.configKey === 'smtp_username')!.isSecret).toBe(false)
    expect(result.find((r) => r.configKey === 'smtp_encryption')!.isSecret).toBe(false)

    // Secret fields
    expect(result.find((r) => r.configKey === 'resend_api_key')!.isSecret).toBe(true)
    expect(result.find((r) => r.configKey === 'smtp_password')!.isSecret).toBe(true)
  })

  test('does not include enabled field in request entries', () => {
    const config = {
      provider: 'resend' as const,
      fromAddress: 'noreply@example.com',
      smtpPort: '587',
      smtpEncryption: 'starttls' as const,
    }

    const result = buildEmailConfigRequest(config)

    // None of the entries should have an 'enabled' property
    for (const entry of result) {
      expect(entry).not.toHaveProperty('enabled')
    }
  })
})

// ==================== Custom-domain mapper pure functions ====================

describe('emptyCustomDomainConfig', () => {
  test('returns a config with a null hostname (no custom login domain configured)', () => {
    expect(emptyCustomDomainConfig()).toEqual({ hostname: null })
  })
})

describe('normalizeCustomDomainConfig', () => {
  test('passes a valid custom-domain config through unchanged', () => {
    expect(normalizeCustomDomainConfig({ hostname: 'login.acme.com' })).toEqual({
      hostname: 'login.acme.com',
    })
  })

  test('keeps an empty-string hostname as-is (trim happens in toUpdate, not normalize)', () => {
    // z.string() accepts '' — the schema intentionally does not coerce empties
    // to null, so a stored-but-empty hostname round-trips without being lost.
    expect(normalizeCustomDomainConfig({ hostname: '' })).toEqual({ hostname: '' })
  })

  // A malformed stored config must never crash the admin form: safeParse fails
  // and we fall back to the empty config so the editor renders a clean state.
  it.each([
    ['null', null],
    ['undefined', undefined],
    ['empty object', {}],
    ['non-object', 'login.acme.com'],
    ['hostname of wrong type', { hostname: 123 }],
    ['hostname as array', { hostname: ['login.acme.com'] }],
    ['extra junk object', { unrelated: 'x' }],
  ])('falls back to empty config when value is %s', (_label, value) => {
    expect(normalizeCustomDomainConfig(value)).toEqual({ hostname: null })
  })
})

describe('toUpdateCustomDomainConfigRequest', () => {
  const trimCases: Array<[string, string | null, string | null]> = [
    ['trims surrounding whitespace', '  login.acme.com  ', 'login.acme.com'],
    ['collapses a whitespace-only hostname to null', '   ', null],
    ['collapses an empty hostname to null', '', null],
    ['keeps an already-trimmed hostname unchanged', 'login.acme.com', 'login.acme.com'],
    ['keeps a null hostname as null', null, null],
  ]

  it.each(trimCases)('%s', (_label, hostname, expected) => {
    const form: CustomDomainConfigForm = { hostname }
    expect(toUpdateCustomDomainConfigRequest(form)).toEqual({ hostname: expected })
  })

  test('returns a value assignable to the generated UpdateCustomDomainConfigRequest shape', () => {
    // Shape guard: ensures the mapper keeps matching the wire contract even if
    // the schema gains fields later. `hostname` must be `string | null`.
    const result: UpdateCustomDomainConfigRequest = toUpdateCustomDomainConfigRequest({
      hostname: 'login.acme.com',
    })
    expect(result).toEqual({ hostname: 'login.acme.com' })
  })
})

// ==================== LDAP directory config ====================

/**
 * `bind_password` rows come back with `configValue: null` (server-side
 * masking); the string-typed factory above cannot express that.
 */
const makeLdapRow = (
  configKey: 'settings' | 'bind_password',
  configValue: string | null,
  enabled: boolean
): RealmConfigResponse =>
  ({
    configType: 'ldap',
    configKey,
    configValue,
    enabled,
    isSecret: configKey === 'bind_password',
    id: `ldap-${configKey}`,
    realmId: 'test-realm',
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
  }) as RealmConfigResponse

const LDAP_SETTINGS_JSON = JSON.stringify({
  enabled: true,
  url: 'ldaps://directory.corp.example.com:636',
  starttls: false,
  baseDn: 'dc=corp,dc=example,dc=com',
  bindDn: 'cn=herald,ou=services,dc=corp,dc=example,dc=com',
  userFilter: '(&(objectClass=user)(sAMAccountName={login}))',
  mailAttribute: 'mail',
})

describe('parseLdapConfig', () => {
  test('returns fail-closed defaults when no ldap rows exist', () => {
    const result = parseLdapConfig([])

    expect(result).toEqual({
      enabled: false,
      url: '',
      starttls: false,
      baseDn: '',
      bindDn: '',
      bindPassword: '',
      userFilter: '',
      mailAttribute: 'mail',
      hasBindPassword: false,
    })
  })

  test('masked bind_password row yields an empty password field but hasBindPassword=true', () => {
    // The stored service-account password must never surface in the admin
    // form: the server masks the value to null, and the parse maps that to an
    // empty field plus a row-existence signal.
    const result = parseLdapConfig([
      makeLdapRow('settings', LDAP_SETTINGS_JSON, true),
      makeLdapRow('bind_password', null, true),
    ])

    expect(result.enabled).toBe(true)
    expect(result.url).toBe('ldaps://directory.corp.example.com:636')
    expect(result.bindDn).toBe('cn=herald,ou=services,dc=corp,dc=example,dc=com')
    expect(result.userFilter).toBe('(&(objectClass=user)(sAMAccountName={login}))')
    expect(result.bindPassword).toBe('')
    expect(result.hasBindPassword).toBe(true)
  })

  test('settings row without a bind_password row marks hasBindPassword=false', () => {
    const result = parseLdapConfig([makeLdapRow('settings', LDAP_SETTINGS_JSON, true)])

    expect(result.hasBindPassword).toBe(false)
  })

  test('malformed settings JSON falls back to defaults while keeping the row signal', () => {
    const result = parseLdapConfig([
      makeLdapRow('settings', '{not json', true),
      makeLdapRow('bind_password', null, true),
    ])

    expect(result.enabled).toBe(false)
    expect(result.url).toBe('')
    expect(result.hasBindPassword).toBe(true)
  })

  test('fills mailAttribute default when the stored JSON omits it', () => {
    const partial = JSON.parse(LDAP_SETTINGS_JSON)
    delete partial.mailAttribute
    const result = parseLdapConfig([makeLdapRow('settings', JSON.stringify(partial), true)])

    expect(result.mailAttribute).toBe('mail')
  })
})

describe('buildLdapConfigRequest', () => {
  const baseForm: LdapConfigForm = {
    enabled: true,
    url: 'ldaps://directory.corp.example.com:636',
    starttls: false,
    baseDn: 'dc=corp,dc=example,dc=com',
    bindDn: 'cn=herald,ou=services,dc=corp,dc=example,dc=com',
    bindPassword: '',
    userFilter: '(&(objectClass=user)(sAMAccountName={login}))',
    mailAttribute: 'mail',
  }

  test('omits the bind_password row when the password is empty (keep-stored-value)', () => {
    // Empty-secret preservation: sending no row (rather than an empty secret
    // row) is what makes the backend keep the stored password.
    const rows = buildLdapConfigRequest(baseForm)

    expect(rows).toHaveLength(1)
    expect(rows[0].configKey).toBe('settings')
  })

  test('includes the bind_password secret row only when a new password is entered', () => {
    const rows = buildLdapConfigRequest({ ...baseForm, bindPassword: 'new-pass' })

    expect(rows).toHaveLength(2)
    const secretRow = rows.find((r) => r.configKey === 'bind_password')
    expect(secretRow?.configValue).toBe('new-pass')
    expect(secretRow?.isSecret).toBe(true)
  })

  test('settings row-level enabled mirrors the JSON enabled (single source of truth)', () => {
    const rows = buildLdapConfigRequest({ ...baseForm, enabled: false })

    expect(rows[0].enabled).toBe(false)
    expect(JSON.parse(rows[0].configValue).enabled).toBe(false)
  })

  test('settings JSON round-trips through parseLdapConfig with the same editable values', () => {
    const rows = buildLdapConfigRequest(baseForm)
    const parsed = parseLdapConfig(rows as RealmConfigResponse[])

    expect({ ...parsed, hasBindPassword: false }).toEqual({ ...baseForm, hasBindPassword: false })
  })

  test('normalizes an empty bindDn to null (anonymous search)', () => {
    const rows = buildLdapConfigRequest({ ...baseForm, bindDn: '' })

    expect(JSON.parse(rows[0].configValue).bindDn).toBeNull()
  })

  test('returns rows assignable to the generated UpsertRealmConfigRequest shape', () => {
    const rows: UpsertRealmConfigRequest[] = buildLdapConfigRequest(baseForm)
    expect(rows.every((r) => r.configType === 'ldap')).toBe(true)
  })
})
