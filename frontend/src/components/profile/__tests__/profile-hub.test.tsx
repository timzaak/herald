/**
 * @vitest-environment jsdom
 */

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { ProfileHub } from '../profile-hub'
import type { ReactNode } from 'react'
import { LocaleProvider } from '@/components/shared/locale-provider'

const allFeaturesHidden = {
  user: {
    pointsVisible: false,
    subscriptionVisible: false,
    invoicesVisible: false,
  },
}

let featureData = allFeaturesHidden
const initialProfileData = {
  email: 'user@example.com',
  nickname: 'Alice',
}

let profileData: typeof initialProfileData | undefined = initialProfileData
let permissions: string[] = []

vi.mock('@tanstack/react-query', () => ({
  useQuery: ({ queryKey }: { queryKey: readonly unknown[] }) => {
    const key = String(queryKey[0])
    if (key === 'profile') return { data: profileData, isLoading: false }
    if (key === 'public-config') return { data: { realmName: 'Test Realm' } }
    return { data: featureData }
  },
}))

vi.mock('@tanstack/react-router', () => ({
  Link: ({ to, children, ...props }: { to: string; children: ReactNode }) => (
    <a href={to} {...props}>
      {children}
    </a>
  ),
  useLocation: () => ({ pathname: '/test-realm/user' }),
}))

vi.mock('@/stores/auth-store', () => ({
  useRealmId: () => 'test-realm',
  usePermissions: () => permissions,
}))

vi.mock('@/lib/auth-utils', () => ({
  logoutFlow: vi.fn(),
}))

vi.mock('@/data/query-options', () => ({
  userFeatureAvailabilityQueryOptions: { queryKey: ['user-feature-availability'] },
  profileQueryOptions: { queryKey: ['profile'] },
  publicConfigQueryOptions: (realmId: string) => ({ queryKey: ['public-config', realmId] }),
}))

const renderHub = () =>
  render(
    <LocaleProvider>
      <ProfileHub />
    </LocaleProvider>
  )

describe('ProfileHub', () => {
  beforeEach(() => {
    permissions = []
    featureData = allFeaturesHidden
    profileData = initialProfileData
  })

  it('identifies the signed-in user on the identity card and links to the profile section', () => {
    // The hub is the mobile landing page: without the identity card a user
    // cannot tell which account (and realm) they are acting in.
    renderHub()

    const identity = screen.getByTestId('profile-hub-identity')
    expect(identity).toHaveAttribute('href', '/test-realm/user/profile')
    expect(screen.getByTestId('profile-hub-identity-name')).toHaveTextContent('Alice')
    expect(identity).toHaveTextContent('user@example.com')
  })

  it('always exposes the account group even when every billing feature is off', () => {
    // Profile/Security are core account management and must never depend on
    // the realm's billing capabilities.
    renderHub()

    expect(screen.getByTestId('profile-hub-group-account')).toBeInTheDocument()
    expect(screen.getByTestId('profile-hub-menu-profile')).toBeInTheDocument()
    expect(screen.getByTestId('profile-hub-menu-security')).toBeInTheDocument()
    expect(screen.queryByTestId('profile-hub-group-billing')).not.toBeInTheDocument()
  })

  it('gates Points and Purchase Records together on the points-area visibility', () => {
    // Same merged gate as the desktop sidebar: the two entries belong to one
    // points area and must not drift apart between the two navigation surfaces.
    featureData = {
      user: {
        pointsVisible: true,
        subscriptionVisible: false,
        invoicesVisible: false,
      },
    }

    renderHub()

    expect(screen.getByTestId('profile-hub-menu-points')).toBeInTheDocument()
    expect(screen.getByTestId('profile-hub-menu-purchaserecords')).toBeInTheDocument()
    expect(screen.queryByTestId('profile-hub-menu-subscription')).not.toBeInTheDocument()
    expect(screen.queryByTestId('profile-hub-menu-invoices')).not.toBeInTheDocument()
  })

  it('shows subscription and invoices entries only when their capabilities exist', () => {
    // US-BI-009 scenario 0: without the entitlement-mapping gate the entry
    // would advertise a page that only errors for this realm's users.
    featureData = {
      user: {
        pointsVisible: false,
        subscriptionVisible: true,
        invoicesVisible: true,
      },
    }

    renderHub()

    expect(screen.getByTestId('profile-hub-menu-subscription')).toHaveAttribute(
      'href',
      '/test-realm/user/my-subscriptions'
    )
    expect(screen.getByTestId('profile-hub-menu-invoices')).toBeInTheDocument()
    expect(screen.queryByTestId('profile-hub-menu-points')).not.toBeInTheDocument()
  })

  it('shows a load-failure notice instead of an empty name when the profile query fails', () => {
    // The identity line must not degrade to an empty string on failure —
    // the user would lose the only account identification the hub gives.
    profileData = undefined

    renderHub()

    expect(screen.getByTestId('profile-hub-identity-error')).toHaveTextContent(
      'Failed to load profile'
    )
    expect(screen.queryByTestId('profile-hub-identity-name')).not.toBeInTheDocument()
  })

  it('keeps logout isolated in the footer, away from the menu rows', () => {
    // Logout is destructive: it must live in its own footer card so a user
    // browsing menu rows cannot hit it by accident.
    renderHub()

    const logout = screen.getByTestId('profile-hub-logout')
    expect(logout.closest('[data-testid="profile-hub-footer"]')).not.toBeNull()
    expect(logout.closest('[data-testid="profile-hub-group-account"]')).toBeNull()
  })

  it('shows the admin console row only to an eligible administrator', () => {
    permissions = ['dashboard.view']

    renderHub()

    const entry = screen.getByTestId('profile-hub-admin-console')
    expect(entry).toHaveAttribute('href', '/test-realm/manage')
    expect(entry).toHaveTextContent('Admin Console')
  })
})
