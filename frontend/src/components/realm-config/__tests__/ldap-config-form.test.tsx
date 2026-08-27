/**
 * LDAP directory config form test.
 *
 * Covers the three admin-facing interaction constraints:
 *   - a plaintext directory address is rejected before any request fires
 *     (ldaps-with-StartTLS is rejected the same way — backend rules mirrored)
 *   - enabling with a service-account DN but no stored/entered password is
 *     blocked with an actionable message (the masked value is unreadable, so
 *     row existence is the only signal)
 *   - without `settings.manage` the whole form is disabled
 *
 * Plus the starttls lock for `ldaps://` URLs and the composed save payload.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

vi.mock('@/paraglide/messages', () => ({
  m: new Proxy({}, { get: (_target: unknown, prop: string) => () => `[${prop}]` }),
}))

import { LdapConfigForm } from '../ldap-config-form'
import type { LdapConfigState } from '@/lib/schemas/realm-config'

function makeInitialConfig(overrides: Partial<LdapConfigState> = {}): LdapConfigState {
  return {
    enabled: true,
    url: 'ldaps://directory.corp.example.com:636',
    starttls: false,
    baseDn: 'dc=corp,dc=example,dc=com',
    bindDn: 'cn=herald,ou=services,dc=corp,dc=example,dc=com',
    bindPassword: '',
    userFilter: '(&(objectClass=user)(sAMAccountName={login}))',
    mailAttribute: 'mail',
    hasBindPassword: true,
    ...overrides,
  }
}

const mockOnSave = vi.fn()

function renderForm(props: Partial<Parameters<typeof LdapConfigForm>[0]> = {}) {
  return render(
    <LdapConfigForm
      initialConfig={makeInitialConfig()}
      hasBindPassword={true}
      onSave={mockOnSave}
      {...props}
    />
  )
}

describe('LdapConfigForm', () => {
  beforeEach(() => {
    mockOnSave.mockReset()
    mockOnSave.mockResolvedValue(undefined)
  })

  it('GIVEN a complete valid config WHEN saving THEN onSave receives the composed form values', async () => {
    renderForm()

    await userEvent.click(screen.getByTestId('ldap-save-button'))

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith(
        expect.objectContaining({
          enabled: true,
          url: 'ldaps://directory.corp.example.com:636',
          starttls: false,
          bindDn: 'cn=herald,ou=services,dc=corp,dc=example,dc=com',
          userFilter: '(&(objectClass=user)(sAMAccountName={login}))',
        })
      )
    })
  })

  it.each([
    [
      'plaintext ldap:// without StartTLS',
      { url: 'ldap://directory.corp.example.com', starttls: false },
    ],
    [
      'ldaps:// with redundant StartTLS',
      { url: 'ldaps://directory.corp.example.com', starttls: true },
    ],
  ])(
    'GIVEN %s WHEN saving THEN the request is blocked with an inline error',
    async (_name, overrides) => {
      renderForm({ initialConfig: makeInitialConfig(overrides) })

      await userEvent.click(screen.getByTestId('ldap-save-button'))

      expect(await screen.findByTestId('ldap-starttls-error')).toHaveTextContent(
        '[settings.ldap.error_encryption_required]'
      )
      expect(mockOnSave).not.toHaveBeenCalled()
    }
  )

  it('GIVEN enabling with a bindDn but no stored or entered password WHEN saving THEN the save is blocked with guidance', async () => {
    renderForm({
      initialConfig: makeInitialConfig({ hasBindPassword: false }),
      hasBindPassword: false,
    })

    await userEvent.click(screen.getByTestId('ldap-save-button'))

    expect(await screen.findByTestId('ldap-bind-password-error')).toBeInTheDocument()
    expect(mockOnSave).not.toHaveBeenCalled()
  })

  it('GIVEN a stored password exists (row present) WHEN saving with an empty field THEN the save proceeds (keep-stored-value)', async () => {
    renderForm({ initialConfig: makeInitialConfig({ hasBindPassword: true }) })

    await userEvent.click(screen.getByTestId('ldap-save-button'))

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalled()
    })
  })

  it('GIVEN no manage permission WHEN rendering THEN every control and the save button are disabled', () => {
    renderForm({ disabled: true })

    expect(screen.getByTestId('ldap-enabled-switch')).toBeDisabled()
    expect(screen.getByTestId('ldap-url-input')).toBeDisabled()
    expect(screen.getByTestId('ldap-starttls-switch')).toBeDisabled()
    expect(screen.getByTestId('ldap-basedn-input')).toBeDisabled()
    expect(screen.getByTestId('ldap-binddn-input')).toBeDisabled()
    expect(screen.getByTestId('ldap-bind-password-input')).toBeDisabled()
    expect(screen.getByTestId('ldap-user-filter-input')).toBeDisabled()
    expect(screen.getByTestId('ldap-mail-attribute-input')).toBeDisabled()
    expect(screen.getByTestId('ldap-save-button')).toBeDisabled()
  })

  it('GIVEN the URL is switched to ldaps:// WHEN StartTLS was on THEN the switch locks to off', async () => {
    renderForm({
      initialConfig: makeInitialConfig({
        url: 'ldap://directory.corp.example.com',
        starttls: true,
      }),
    })

    const urlInput = screen.getByTestId('ldap-url-input')
    await userEvent.clear(urlInput)
    await userEvent.type(urlInput, 'ldaps://directory.corp.example.com')

    const starttlsSwitch = screen.getByTestId('ldap-starttls-switch')
    expect(starttlsSwitch).toBeDisabled()
    expect(starttlsSwitch).not.toBeChecked()
  })
})
