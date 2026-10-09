import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ApiKeyTable } from '../api-key-table'
import type { ApiKeyListItem } from '@/lib/api-generated'

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

function makeKey(overrides: Partial<ApiKeyListItem> & { id: string }): ApiKeyListItem {
  return {
    createdAt: '2026-01-01T00:00:00Z',
    enabled: true,
    expiresAt: null,
    lastUsedAt: null,
    name: 'Test Key',
    realmId: 'realm-1',
    ...overrides,
  }
}

// ---------------------------------------------------------------------------
// Scenario 1: Badge count at boundaries (0, 1, 2)
// ---------------------------------------------------------------------------

describe('ApiKeyTable roles column', () => {
  it('shows the bound Client App name when present', () => {
    const key = makeKey({
      id: 'k-client-app',
      name: 'Scoped Key',
      clientAppId: '018f6f3a-7f25-7c00-9a2f-000000000001',
      clientAppName: 'Mobile App',
    })

    render(<ApiKeyTable data={[key]} />)

    expect(screen.getByTestId('api-key-client-app')).toHaveTextContent('Mobile App')
  })

  // WHY: a key whose bound app is disabled cannot authenticate (the app is
  // the kill switch). Without the warning the admin sees a healthy-looking
  // key and hunts for the cause in the wrong place.
  it('warns when the bound Client App is disabled', () => {
    const key = makeKey({
      id: 'k-client-app-disabled',
      name: 'Dead Gate Key',
      clientAppId: '018f6f3a-7f25-7c00-9a2f-000000000002',
      clientAppName: 'Mobile App',
      clientAppEnabled: false,
    })

    render(<ApiKeyTable data={[key]} />)

    expect(screen.getByTestId('api-key-client-app-disabled-warning')).toBeInTheDocument()
  })

  it('does not warn when the bound Client App is enabled', () => {
    const key = makeKey({
      id: 'k-client-app-enabled',
      name: 'Live Gate Key',
      clientAppId: '018f6f3a-7f25-7c00-9a2f-000000000003',
      clientAppName: 'Mobile App',
      clientAppEnabled: true,
    })

    render(<ApiKeyTable data={[key]} />)

    expect(screen.queryByTestId('api-key-client-app-disabled-warning')).not.toBeInTheDocument()
  })

  describe('shows all badges when 2 or fewer roles', () => {
    const cases = [
      {
        label: '0 roles',
        roles: [] as { id: string; name: string }[],
        expectEmDash: true,
      },
      {
        label: '1 role',
        roles: [{ id: 'r1', name: 'Admin' }],
        expectEmDash: false,
      },
      {
        label: '2 roles',
        roles: [
          { id: 'r1', name: 'Admin' },
          { id: 'r2', name: 'User' },
        ],
        expectEmDash: false,
      },
    ]

    it.each(cases)(
      'with $label: renders correct badges and no overflow',
      ({ roles, expectEmDash }) => {
        const key = makeKey({ id: 'k1', name: 'Key 1', roles })

        render(<ApiKeyTable data={[key]} onManageRoles={vi.fn()} canManageRoles={true} />)

        const cell = screen.getByTestId('api-key-roles-cell')

        if (expectEmDash) {
          // Em-dash for empty roles
          expect(cell).toHaveTextContent('—')
        } else {
          // Each role name should appear as badge text
          for (const role of roles) {
            expect(cell).toHaveTextContent(role.name)
          }
        }

        // No overflow badge should exist
        expect(screen.queryByTestId('api-key-roles-overflow')).not.toBeInTheDocument()
      }
    )

    it('with undefined roles: renders em-dash (absent field treated as empty)', () => {
      const key = makeKey({ id: 'k-undef', name: 'Key Undefined' })
      // Explicitly remove the roles field to test the undefined path
      delete (key as Record<string, unknown>).roles

      render(<ApiKeyTable data={[key]} onManageRoles={vi.fn()} canManageRoles={true} />)

      const cell = screen.getByTestId('api-key-roles-cell')
      expect(cell).toHaveTextContent('—')
      expect(screen.queryByTestId('api-key-roles-overflow')).not.toBeInTheDocument()
    })
  })

  // -------------------------------------------------------------------------
  // Scenario 2: Overflow badge when more than 2 roles
  // -------------------------------------------------------------------------

  describe('shows overflow badge when more than 2 roles', () => {
    it('renders first 2 badges and "+N more" overflow', () => {
      const roles = [
        { id: 'r1', name: 'Admin' },
        { id: 'r2', name: 'User' },
        { id: 'r3', name: 'Editor' },
        { id: 'r4', name: 'Viewer' },
        { id: 'r5', name: 'Custom' },
      ]
      const key = makeKey({ id: 'k5', name: 'Key 5', roles })

      render(<ApiKeyTable data={[key]} onManageRoles={vi.fn()} canManageRoles={true} />)

      const cell = screen.getByTestId('api-key-roles-cell')

      // First 2 role names should appear
      expect(cell).toHaveTextContent('Admin')
      expect(cell).toHaveTextContent('User')

      // Remaining role names should NOT appear
      expect(cell).not.toHaveTextContent('Editor')
      expect(cell).not.toHaveTextContent('Viewer')
      expect(cell).not.toHaveTextContent('Custom')

      // Overflow badge: +3 more
      const overflow = screen.getByTestId('api-key-roles-overflow')
      expect(overflow).toHaveTextContent('+3 more')
    })
  })

  // -------------------------------------------------------------------------
  // Scenario 3: Roles button hidden when canManageRoles is false
  // -------------------------------------------------------------------------

  describe('roles button permission gating', () => {
    it('hides Roles button when canManageRoles is false', () => {
      const key = makeKey({ id: 'k1', name: 'Key 1' })

      render(<ApiKeyTable data={[key]} onManageRoles={vi.fn()} canManageRoles={false} />)

      expect(screen.queryByTestId('manage-api-key-roles-button')).not.toBeInTheDocument()
    })

    // -----------------------------------------------------------------------
    // Scenario 4: Roles button visible and functional when canManageRoles is true
    // -----------------------------------------------------------------------

    it('shows Roles button and calls onManageRoles when clicked', async () => {
      const onManageRoles = vi.fn()
      const key = makeKey({ id: 'k1', name: 'Key 1' })

      render(<ApiKeyTable data={[key]} onManageRoles={onManageRoles} canManageRoles={true} />)

      const rolesButton = screen.getByTestId('manage-api-key-roles-button')
      expect(rolesButton).toBeInTheDocument()

      await userEvent.click(rolesButton)

      expect(onManageRoles).toHaveBeenCalledWith(key)
    })
  })
})
