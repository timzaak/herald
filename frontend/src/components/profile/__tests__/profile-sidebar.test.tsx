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
let permissions: string[] = []
let pathname = '/test-realm/user/profile'

vi.mock('@tanstack/react-query', () => ({
  useQuery: () => ({ data: featureData }),
}))

vi.mock('@tanstack/react-router', () => ({
  Link: ({ to, children, ...props }: { to: string; children: ReactNode }) => (
    <a href={to} {...props}>
      {children}
    </a>
  ),
  useLocation: () => ({ pathname }),
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
}))

describe('ProfileSidebar', () => {
  beforeEach(() => {
    permissions = []
    pathname = '/test-realm/user/profile'
  })

  it('highlights the Profile item on the /user landing, which renders that section inline', () => {
    // The landing page shows the Profile section's content; without the
    // highlight the same content would show different navigation states
    // depending on which URL it was reached from (mirrors /manage, whose
    // index highlights Dashboard).
    pathname = '/test-realm/user/'

    render(
      <LocaleProvider>
        <ProfileSidebar />
      </LocaleProvider>
    )

    expect(screen.getByTestId('profile-menu-profile')).toHaveClass('font-semibold')
    expect(screen.getByTestId('profile-menu-security')).not.toHaveClass('font-semibold')
  })

  it('shows the explicit admin-console entry only to an eligible administrator', () => {
    permissions = ['dashboard.view']

    render(
      <LocaleProvider>
        <ProfileSidebar />
      </LocaleProvider>
    )

    const entry = screen.getByTestId('profile-admin-console-link')
    expect(entry).toHaveAttribute('href', '/test-realm/manage')
    // The entry leads to the whole /manage console, so its label must say
    // "Admin Console" — not "Dashboard", which names the console's landing
    // page and misreads here as user-facing analytics.
    expect(entry).toHaveTextContent('Admin Console')
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
    expect(entry).toHaveAttribute('href', '/test-realm/user/my-subscriptions')
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
})
