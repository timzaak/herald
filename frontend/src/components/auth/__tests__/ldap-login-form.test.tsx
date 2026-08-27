/**
 * LDAP (corporate-directory) login form component test.
 *
 * Covers the branches the Demo cannot cheaply reach:
 *   - field validation errors surface next to their inputs
 *   - Turnstile renders only when the Client App has it enabled
 *   - back button hands control back to the route (password form)
 *   - submit is blocked while pending / OAuth params incomplete
 *
 * The submit-boundary error mapping (`resolveLdapLoginError`, owned by
 * `auth-utils` and wired into the route's mutation onError) is exercised here
 * as a pure function: 503/429 must use dedicated localized keys while 401
 * passes the backend anti-enumeration message through verbatim.
 *
 * The full happy-path submission (JIT provisioning, 2FA, consent, redirect)
 * is covered by the Demo, not duplicated here. The component itself holds no
 * mutations — values travel up through `onSubmit`.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

vi.mock('@/paraglide/messages', () => ({
  m: new Proxy({}, { get: (_target: unknown, prop: string) => () => `[${prop}]` }),
}))

// auth-utils transitively pulls the auth-service/SDK chain; the mapping under
// test only needs error-utils + messages, so sever the heavy imports (same
// pattern as email-otp-handoff.test.ts).
vi.mock('@/lib/auth-service', () => ({
  performLogin: vi.fn(),
  fetchAuthData: vi.fn(),
  performLogout: vi.fn(),
  performPkceTokenExchange: vi.fn(),
  switchFirstPartyClient: vi.fn(),
  ClientSwitchError: class ClientSwitchError extends Error {
    constructor(public readonly status: number) {
      super('Client switch failed')
    }
  },
}))

vi.mock('@/stores/auth-store', () => ({
  useAuthStore: {
    getState: vi.fn(),
  },
  clearAuthStorage: vi.fn(),
}))

vi.mock('@/lib/herald-client', () => ({
  ensureHeraldClient: vi.fn(),
  getActiveHeraldClient: vi.fn(() => null),
  applyTokenSet: vi.fn(),
  bindHeraldClientId: vi.fn(),
  runTokenSwitch: vi.fn(),
}))

vi.mock('../turnstile-widget', () => ({
  TurnstileWidget: () => <div data-testid="turnstile-widget-mock" />,
}))

vi.mock('@/components/legal/AgreementLinks', () => ({
  AgreementLinks: () => <div data-testid="agreement-links-mock" />,
}))

import { LdapLoginForm } from '../ldap-login-form'
import { resolveLdapLoginError } from '@/lib/auth-utils'

const defaultProps = {
  realmId: 'corp',
  isPending: false,
  onSubmit: vi.fn(),
  onBack: vi.fn(),
}

describe('LdapLoginForm', () => {
  beforeEach(() => {
    defaultProps.onSubmit.mockClear()
    defaultProps.onBack.mockClear()
  })

  it('GIVEN a cleared username WHEN the field revalidates THEN the required error shows', async () => {
    const screen = render(<LdapLoginForm {...defaultProps} />)

    await userEvent.type(screen.getByTestId('ldap-username-input'), 'j')
    await userEvent.clear(screen.getByTestId('ldap-username-input'))

    expect(await screen.findByText('[auth.ldap.username_required]')).toBeInTheDocument()
  })

  it('GIVEN a cleared password WHEN the field revalidates THEN the required error shows', async () => {
    const screen = render(<LdapLoginForm {...defaultProps} />)

    await userEvent.type(screen.getByTestId('ldap-password-input'), 'p')
    await userEvent.clear(screen.getByTestId('ldap-password-input'))

    expect(await screen.findByText('[auth.ldap.password_required]')).toBeInTheDocument()
  })

  it('GIVEN the Client App enables Turnstile WHEN rendering THEN the widget is shown', () => {
    const screen = render(
      <LdapLoginForm {...defaultProps} turnstileStatus={{ enabled: true, siteKey: 'site-key' }} />
    )

    expect(screen.getByTestId('turnstile-widget-mock')).toBeInTheDocument()
  })

  it('GIVEN Turnstile is not enabled WHEN rendering THEN no widget is shown', () => {
    const screen = render(<LdapLoginForm {...defaultProps} turnstileStatus={{ enabled: false }} />)

    expect(screen.queryByTestId('turnstile-widget-mock')).not.toBeInTheDocument()
  })

  it('GIVEN the user taps back WHEN on the LDAP form THEN the route returns to the password form', async () => {
    const screen = render(<LdapLoginForm {...defaultProps} />)

    await userEvent.click(screen.getByTestId('ldap-back-button'))

    expect(defaultProps.onBack).toHaveBeenCalledTimes(1)
  })

  it('GIVEN a submission is in flight WHEN rendering THEN inputs and submit are disabled', () => {
    const screen = render(<LdapLoginForm {...defaultProps} isPending={true} />)

    expect(screen.getByTestId('ldap-username-input')).toBeDisabled()
    expect(screen.getByTestId('ldap-password-input')).toBeDisabled()
    expect(screen.getByTestId('ldap-submit-button')).toBeDisabled()
  })

  it('GIVEN incomplete OAuth params WHEN rendering THEN submit is blocked (same guard as the password form)', () => {
    const screen = render(<LdapLoginForm {...defaultProps} hasPartialOAuth={true} />)

    expect(screen.getByTestId('ldap-submit-button')).toBeDisabled()
  })
})

describe('resolveLdapLoginError (submit-boundary error mapping)', () => {
  it('maps a 503 directory outage to the dedicated unavailable key', () => {
    expect(
      resolveLdapLoginError({ status: 503, code: 'service_unavailable', message: 'boom' })
    ).toBe('[auth.ldap.unavailable]')
  })

  it('maps a 429 rate limit to the dedicated key', () => {
    expect(resolveLdapLoginError({ status: 429, message: 'Too many requests' })).toBe(
      '[auth.ldap.rate_limited]'
    )
  })

  it('passes the backend 401 message through verbatim (anti-enumeration copy)', () => {
    // The 401 body IS the user-facing text: identical to password login and
    // deliberately indistinguishable between "no such directory user" and
    // "wrong password" — the mapping must not reword or localize it.
    expect(resolveLdapLoginError({ status: 401, message: 'invalid credentials' })).toBe(
      'invalid credentials'
    )
  })

  it('passes plain Error instances through', () => {
    expect(resolveLdapLoginError(new Error('network down'))).toBe('network down')
  })
})
