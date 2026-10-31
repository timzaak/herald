/**
 * @vitest-environment jsdom
 */

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, screen, waitFor } from '@testing-library/react'
import { RecentUsersTable } from '../recent-users-table'
import { RecentAuditEventsTable } from '../recent-audit-events-table'
import { RecentPointsTransactionsTable } from '../recent-points-transactions-table'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { renderWithProviders } from '@/test/utils/render'

// The dashboard's recent tables exist so an admin lands on actionable data.
// Why these tests matter: every card is a window into a permission-scoped
// list, so a card shown without its permission (or with points disabled)
// either 403s or — worse, for points — silently shows the caller's own
// personal transactions masquerading as realm-wide activity.

const { permissions, featureFlags, usersFailure } = vi.hoisted(() => ({
  permissions: { current: [] as string[] },
  featureFlags: { current: { pointsVisible: true } },
  usersFailure: { current: false },
}))

vi.mock('@/hooks/use-permission', () => ({
  usePermission: () => ({
    hasPermission: (permission: string) => permissions.current.includes(permission),
  }),
}))

vi.mock('@/data/query-options', () => ({
  usersQueryOptions: (realmId: string, filters: { pageSize?: number }) => ({
    queryKey: ['users', realmId, filters],
    queryFn: async () => {
      if (usersFailure.current) throw new Error('users list unavailable')
      return {
        items: [
          {
            id: 'user-1',
            email: 'newest@example.com',
            nickname: 'Newest',
            status: 1,
            createdAt: '2026-10-31T08:00:00Z',
            realmId,
          },
          {
            id: 'user-2',
            email: 'older@example.com',
            nickname: null,
            status: 0,
            createdAt: '2026-10-30T08:00:00Z',
            realmId,
          },
        ],
        total: 2,
        page: 0,
        pageSize: filters.pageSize ?? 20,
      }
    },
  }),
  auditListQueryOptions: (realmId: string, filters: { pageSize?: number }) => ({
    queryKey: ['audit', realmId, filters],
    queryFn: async () => ({
      items: [
        {
          id: 'event-1',
          createdAt: '2026-10-31T09:00:00Z',
          actorName: 'admin@example.com',
          category: 'user_management',
          action: 'user_created',
          result: 'success',
        },
      ],
      total: 1,
      page: 0,
      pageSize: filters.pageSize ?? 20,
    }),
  }),
  pointsTransactionsQueryOptions: (realmId: string, filters: { pageSize?: number }) => ({
    queryKey: ['points-transactions', realmId, filters],
    queryFn: async () => ({
      transactions: [
        {
          id: 'tx-1',
          amount: 100,
          balanceAfter: 100,
          bucketId: 'bucket-0001',
          createdAt: '2026-10-31T10:00:00Z',
          description: 'sign-up grant',
        },
        {
          id: 'tx-2',
          amount: -30,
          balanceAfter: 70,
          bucketId: 'bucket-0001',
          createdAt: '2026-10-31T11:00:00Z',
          description: 'API call',
        },
      ],
      total: 2,
      page: 0,
      pageSize: filters.pageSize ?? 20,
    }),
  }),
  featureAvailabilityQueryOptions: (realmId: string) => ({
    queryKey: ['feature-availability', realmId],
    queryFn: async () => ({ admin: { pointsVisible: featureFlags.current.pointsVisible } }),
  }),
}))

vi.mock('@tanstack/react-router', () => ({
  Link: ({ to, children, ...props }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...props}>
      {children}
    </a>
  ),
}))

beforeEach(() => {
  cleanup()
  permissions.current = []
  featureFlags.current = { pointsVisible: true }
  usersFailure.current = false
})

describe('RecentUsersTable', () => {
  it('renders the newest users for admins holding users.view', async () => {
    permissions.current = [PERMISSION.USERS_VIEW]

    renderWithProviders(<RecentUsersTable realmId="admin" />)

    expect(await screen.findByText('newest@example.com')).toBeInTheDocument()
    expect(screen.getByText('older@example.com')).toBeInTheDocument()
    expect(
      screen
        .getByTestId('dashboard-recent-users-card')
        .querySelector('a[href="/$realmId/manage/users"]')
    ).toBeInTheDocument()
  })

  it('renders nothing without users.view — the card must not 403', () => {
    permissions.current = []

    renderWithProviders(<RecentUsersTable realmId="admin" />)

    expect(screen.queryByTestId('dashboard-recent-users-card')).not.toBeInTheDocument()
  })

  it('surfaces a failed list query instead of an empty state — "no users" must not mask an error', async () => {
    permissions.current = [PERMISSION.USERS_VIEW]
    usersFailure.current = true

    renderWithProviders(<RecentUsersTable realmId="admin" />)

    expect(await screen.findByText('Error: users list unavailable')).toBeInTheDocument()
    expect(screen.queryByText('No users yet')).not.toBeInTheDocument()
  })
})

describe('RecentAuditEventsTable', () => {
  it('renders the latest audit events for admins holding audit.view', async () => {
    permissions.current = [PERMISSION.AUDIT_VIEW]

    renderWithProviders(<RecentAuditEventsTable realmId="admin" />)

    expect(await screen.findByText('admin@example.com')).toBeInTheDocument()
    // Unmapped actions fall back to the raw string — 'user_created' is not in
    // AUDIT_ACTION_LABELS, so it must render verbatim.
    expect(screen.getByText('user_created')).toBeInTheDocument()
  })

  it('renders nothing without audit.view', () => {
    permissions.current = []

    renderWithProviders(<RecentAuditEventsTable realmId="admin" />)

    expect(screen.queryByTestId('dashboard-recent-audit-card')).not.toBeInTheDocument()
  })
})

describe('RecentPointsTransactionsTable', () => {
  it('renders realm-wide transactions for points managers when points is enabled', async () => {
    permissions.current = [PERMISSION.POINTS_MANAGE]

    renderWithProviders(<RecentPointsTransactionsTable realmId="admin" />)

    expect(await screen.findByText('+100')).toBeInTheDocument()
    expect(screen.getByText('-30')).toBeInTheDocument()
  })

  it('renders nothing when the realm has points disabled', async () => {
    permissions.current = [PERMISSION.POINTS_MANAGE]
    featureFlags.current = { pointsVisible: false }

    renderWithProviders(<RecentPointsTransactionsTable realmId="admin" />)

    // Give the feature-availability query a beat to resolve before asserting absence.
    await waitFor(() =>
      expect(screen.queryByTestId('dashboard-recent-points-card')).not.toBeInTheDocument()
    )
  })

  it('renders nothing for points.view-only admins — the API would silently scope the list to their own wallet', async () => {
    permissions.current = [PERMISSION.POINTS_VIEW]

    renderWithProviders(<RecentPointsTransactionsTable realmId="admin" />)

    await waitFor(() =>
      expect(screen.queryByTestId('dashboard-recent-points-card')).not.toBeInTheDocument()
    )
  })
})
