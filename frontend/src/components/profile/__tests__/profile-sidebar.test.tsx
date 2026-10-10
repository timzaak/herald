/**
 * @vitest-environment jsdom
 */

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { ProfileSidebar } from '../profile-sidebar'
import type { ReactNode } from 'react'
import { LocaleProvider } from '@/components/shared/locale-provider'

let featureData = {
  user: {
    pointsVisible: false,
    subscriptionVisible: false,
    invoicesVisible: false,
  },
}
// Default to an unconfigured realm so existing entry-visibility assertions
// stay about the feature flags; the change-email tests override it.
let publicConfigData = { emailChannelConfigured: false }
let permissions: string[] = []

vi.mock('@tanstack/react-query', () => ({
  useQuery: (options: { queryKey: unknown[] }) =>
    options.queryKey[0] === 'public-config' ? { data: publicConfigData } : { data: featureData },
}))

vi.mock('@tanstack/react-router', () => ({
  Link: ({ to, children, ...props }: { to: string; children: ReactNode }) => (
    <a href={to} {...props}>
      {children}
    </a>
  ),
  useLocation: () => ({ pathname: '/test-realm/user/profile' }),
}))

vi.mock('@/stores/auth-store', () => ({
  useRealmId: () => 'test-realm',
  usePermissions: () => permissions,
}))

vi.mock('@/lib/auth-utils', () => ({
  logoutFlow: vi.fn(),
}))

vi.mock('@/data/query-options', () => ({
  userFeatureAvailabilityQueryOptions: {
    queryKey: ['user-feature-availability'],
  },
  publicConfigQueryOptions: (realmId: string) => ({
    queryKey: ['public-config', realmId],
  }),
}))

describe('ProfileSidebar', () => {
  beforeEach(() => {
    permissions = []
  })

  it('shows the explicit admin-console entry only to an eligible administrator', () => {
    permissions = ['dashboard.view']

    render(
      <LocaleProvider>
        <ProfileSidebar />
      </LocaleProvider>
    )

    expect(screen.getByTestId('profile-admin-console-link')).toHaveAttribute('href', '/manage')
  })

  it('does not expose the admin dashboard entry without an admin permission', () => {
    render(
      <LocaleProvider>
        <ProfileSidebar />
      </LocaleProvider>
    )

    expect(screen.queryByTestId('profile-admin-console-link')).not.toBeInTheDocument()
  })

  it('shows points and purchase records together when points area is available', () => {
    // After the gate merge, `pointsVisible` drives both the Points and the
    // PurchaseRecords entries — they belong to the same points area and no
    // longer gate independently.
    featureData = {
      user: {
        pointsVisible: true,
        subscriptionVisible: false,
        invoicesVisible: false,
      },
    }

    render(
      <LocaleProvider>
        <ProfileSidebar />
      </LocaleProvider>
    )

    expect(screen.getByTestId('profile-menu-profile')).toBeInTheDocument()
    expect(screen.getByTestId('profile-menu-security')).toBeInTheDocument()
    expect(screen.getByTestId('profile-menu-points')).toBeInTheDocument()
    expect(screen.getByTestId('profile-menu-purchaserecords')).toBeInTheDocument()
    expect(screen.queryByTestId('profile-menu-subscription')).not.toBeInTheDocument()
    expect(screen.queryByTestId('profile-menu-invoices')).not.toBeInTheDocument()
  })

  it('shows the subscription entry when the realm has subscription capability', () => {
    // US-BI-009 scenario 0: with an enabled entitlement mapping the sidebar
    // must expose the my-subscriptions entry — without it the page is only
    // reachable by URL guessing.
    featureData = {
      user: {
        pointsVisible: false,
        subscriptionVisible: true,
        invoicesVisible: false,
      },
    }

    render(
      <LocaleProvider>
        <ProfileSidebar />
      </LocaleProvider>
    )

    const entry = screen.getByTestId('profile-menu-subscription')
    expect(entry).toBeInTheDocument()
    expect(entry).toHaveAttribute('href', '/subscription/my-subscriptions')
    expect(entry).toHaveTextContent('Subscription')
  })

  it('shows invoices when invoice features are available', () => {
    featureData = {
      user: {
        pointsVisible: false,
        subscriptionVisible: false,
        invoicesVisible: true,
      },
    }

    render(
      <LocaleProvider>
        <ProfileSidebar />
      </LocaleProvider>
    )

    expect(screen.getByTestId('profile-menu-invoices')).toBeInTheDocument()
  })

  it('does not show points/purchase records when points area is hidden (e.g. subscription-only realm with no enabled mappings on the points axis)', () => {
    featureData = {
      user: {
        pointsVisible: false,
        subscriptionVisible: true,
        invoicesVisible: false,
      },
    }

    render(
      <LocaleProvider>
        <ProfileSidebar />
      </LocaleProvider>
    )

    expect(screen.queryByTestId('profile-menu-purchaserecords')).not.toBeInTheDocument()
    expect(screen.queryByTestId('profile-menu-points')).not.toBeInTheDocument()
    // The subscription area stays reachable in a subscription-only realm.
    expect(screen.getByTestId('profile-menu-subscription')).toBeInTheDocument()
  })

  it('shows the change-email entry only when the realm has a usable email channel', () => {
    // emailChannelConfigured is the entry's visibility source: without a mail
    // channel the confirmation mail could never be sent, so the user must not
    // be led into a flow that dead-ends after reauth.
    publicConfigData = { emailChannelConfigured: true }

    render(
      <LocaleProvider>
        <ProfileSidebar />
      </LocaleProvider>
    )

    const entry = screen.getByTestId('profile-menu-changeemail')
    // /user/** targets collapse to the session-scoped form (no realm prefix),
    // matching how the Subscription entry renders.
    expect(entry).toHaveAttribute('href', '/user/change-email')
    expect(entry).toHaveTextContent('Change Email')
  })

  it('hides the change-email entry when the realm has no email channel', () => {
    publicConfigData = { emailChannelConfigured: false }

    render(
      <LocaleProvider>
        <ProfileSidebar />
      </LocaleProvider>
    )

    expect(screen.queryByTestId('profile-menu-changeemail')).not.toBeInTheDocument()
  })
})
