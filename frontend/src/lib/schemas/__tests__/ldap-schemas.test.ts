import { describe, it, expect, vi } from 'vitest'

vi.mock('@/paraglide/messages', () => ({
  m: new Proxy({}, { get: (_target: unknown, prop: string) => () => `[${prop}]` }),
}))

import { ldapLoginSchema } from '../common'
import { ldapConfigSchema } from '../realm-config'

/**
 * LDAP login + directory-config schema boundaries.
 *
 * The login schema deliberately does NOT apply the local 8..36 password
 * policy: directory credentials are owned by the enterprise directory, and
 * only non-empty caps apply. The config schema mirrors the backend's
 * save-time validation minimum (encrypted channel, exactly one {login}
 * placeholder, balanced parens) so the admin gets an immediate red-field
 * rejection instead of a server 400 — the two rule sets must not drift.
 */

function makeLogin(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    username: 'jdoe',
    password: 'directory-password',
    ...overrides,
  }
}

describe('ldapLoginSchema', () => {
  it.each([
    ['1-char directory password', { password: 'x' }],
    ['password longer than the local 36-char cap', { password: 'a'.repeat(100) }],
    ['username wider than the local 3-char minimum', { username: 'a' }],
  ])('accepts %s (directory policy, not local policy)', (_name, overrides) => {
    const result = ldapLoginSchema.safeParse(makeLogin(overrides))

    expect(result.success).toBe(true)
  })

  it.each([
    ['empty username', { username: '' }, 'auth.ldap.username_required'],
    ['username over 254 chars', { username: 'a'.repeat(255) }, 'auth.ldap.username_max_length'],
    ['empty password', { password: '' }, 'auth.ldap.password_required'],
    ['password over 512 chars', { password: 'a'.repeat(513) }, 'auth.ldap.password_max_length'],
  ])('rejects %s', (_name, overrides, expectedKey) => {
    const result = ldapLoginSchema.safeParse(makeLogin(overrides))

    expect(result.success).toBe(false)
    if (!result.success) {
      expect(result.error.issues[0].message).toBe(`[${expectedKey}]`)
    }
  })
})

function makeConfig(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    enabled: true,
    url: 'ldaps://directory.corp.example.com:636',
    starttls: false,
    baseDn: 'dc=corp,dc=example,dc=com',
    bindDn: 'cn=herald,ou=services,dc=corp,dc=example,dc=com',
    bindPassword: 'service-pass',
    userFilter: '(&(objectClass=user)(sAMAccountName={login}))',
    mailAttribute: 'mail',
    ...overrides,
  }
}

describe('ldapConfigSchema credential-channel encryption', () => {
  it('accepts ldaps:// with StartTLS off', () => {
    expect(ldapConfigSchema.safeParse(makeConfig()).success).toBe(true)
  })

  it('accepts ldap:// with StartTLS on', () => {
    const result = ldapConfigSchema.safeParse(
      makeConfig({ url: 'ldap://directory.corp.example.com', starttls: true })
    )

    expect(result.success).toBe(true)
  })

  it.each([
    [
      'plaintext ldap:// without StartTLS',
      { url: 'ldap://directory.corp.example.com', starttls: false },
    ],
    ['ldaps:// with redundant StartTLS', { starttls: true }],
  ])('rejects %s (hard rule: credentials only over an encrypted channel)', (_name, overrides) => {
    const result = ldapConfigSchema.safeParse(makeConfig(overrides))

    expect(result.success).toBe(false)
    if (!result.success) {
      const issue = result.error.issues.find((i) => i.path[0] === 'starttls')
      expect(issue?.message).toBe('[settings.ldap.error_encryption_required]')
    }
  })
})

describe('ldapConfigSchema field rules', () => {
  it.each([
    ['url with a non-LDAP scheme', { url: 'https://directory.corp.example.com' }],
    ['url with no host', { url: 'ldaps://' }],
    ['url over 512 chars', { url: `ldaps://${'a'.repeat(510)}.com` }],
    ['empty baseDn', { baseDn: '' }],
    ['baseDn over 512 chars', { baseDn: 'a'.repeat(513) }],
    ['bindDn over 512 chars', { bindDn: 'a'.repeat(513) }],
    ['empty userFilter', { userFilter: '' }],
    ['userFilter over 512 chars', { userFilter: 'a'.repeat(513) }],
  ])('rejects %s', (_name, overrides) => {
    expect(ldapConfigSchema.safeParse(makeConfig(overrides)).success).toBe(false)
  })

  it('accepts an empty bindDn (anonymous search)', () => {
    expect(ldapConfigSchema.safeParse(makeConfig({ bindDn: '' })).success).toBe(true)
  })

  it.each([
    ['missing placeholder', { userFilter: '(&(objectClass=user)(sAMAccountName=jd))' }],
    ['two placeholders', { userFilter: '(|(uid={login})(cn={login}))' }],
  ])('rejects userFilter with %s', (_name, overrides) => {
    const result = ldapConfigSchema.safeParse(makeConfig(overrides))

    expect(result.success).toBe(false)
    if (!result.success) {
      const issue = result.error.issues.find((i) => i.path[0] === 'userFilter')
      expect(issue?.message).toBe('[settings.ldap.error_login_placeholder]')
    }
  })

  it('rejects userFilter with unbalanced parentheses', () => {
    const result = ldapConfigSchema.safeParse(
      makeConfig({ userFilter: '(&(objectClass=user)(uid={login})' })
    )

    expect(result.success).toBe(false)
    if (!result.success) {
      const issue = result.error.issues.find((i) => i.path[0] === 'userFilter')
      expect(issue?.message).toBe('[settings.ldap.error_parens_unbalanced]')
    }
  })

  it.each([
    ['empty', ''],
    ['over 64 chars', 'a'.repeat(65)],
    ['invalid characters (underscore)', 'user_mail'],
  ])('rejects mailAttribute that is %s', (_name, mailAttribute) => {
    expect(ldapConfigSchema.safeParse(makeConfig({ mailAttribute })).success).toBe(false)
  })

  it('accepts a mixed-case mailAttribute', () => {
    expect(
      ldapConfigSchema.safeParse(makeConfig({ mailAttribute: 'proxyAddresses' })).success
    ).toBe(true)
  })
})
