import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { RoleTable } from '../role-table'
import type { RoleResponse } from '@/lib/api-generated'

// Mocks dialog components
vi.mock('../edit-role-dialog', () => ({
  EditRoleDialog: ({
    open,
    onOpenChange,
  }: {
    open: boolean
    onOpenChange: (open: boolean) => void
  }) => (
    <div data-testid="edit-role-dialog" style={{ display: open ? 'block' : 'none' }}>
      <button onClick={() => onOpenChange(false)}>Close</button>
    </div>
  ),
}))

vi.mock('../delete-role-dialog', () => ({
  DeleteRoleDialog: ({
    open,
    onOpenChange,
  }: {
    open: boolean
    onOpenChange: (open: boolean) => void
  }) => (
    <div data-testid="delete-role-dialog" style={{ display: open ? 'block' : 'none' }}>
      <button onClick={() => onOpenChange(false)}>Close</button>
    </div>
  ),
}))

vi.mock('../role-permissions-dialog', () => ({
  RolePermissionsDialog: ({
    open,
    onOpenChange,
  }: {
    open: boolean
    onOpenChange: (open: boolean) => void
  }) => (
    <div data-testid="role-permissions-dialog" style={{ display: open ? 'block' : 'none' }}>
      <button onClick={() => onOpenChange(false)}>Close</button>
    </div>
  ),
}))

vi.mock('@/hooks/use-realm-id', () => ({
  useRealmId: () => 'realm-1',
}))

vi.mock('@tanstack/react-query', () => ({
  useQueries: () => ({
    allPermissions: [],
    assignedPermissionIds: [],
  }),
  queryOptions: vi.fn((options) => options),
  useQuery: vi.fn(),
  useQueryClient: vi.fn(() => ({
    invalidateQueries: vi.fn(),
  })),
}))

vi.mock('@/data/query-options', () => ({
  adminPermissionsQueryOptions: vi.fn(() => ({ queryKey: ['permissions'] })),
  rolePermissionsQueryOptions: vi.fn(() => ({ queryKey: ['role-permissions'] })),
}))

describe('RoleTable', () => {
  const mockRoles: RoleResponse[] = [
    {
      id: '1',
      name: 'realm-admin',
      description: 'Realm administrator',
      realmId: 'realm-1',
      clientId: 'client-1',
      isBuiltin: true,
    },
    {
      id: '2',
      name: 'user-admin',
      description: 'User administrator',
      realmId: 'realm-1',
      clientId: 'client-1',
      isBuiltin: false,
    },
    {
      id: '3',
      name: 'user',
      description: 'Regular user',
      realmId: 'realm-1',
      clientId: 'client-1',
      isBuiltin: true,
    },
  ]

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('GIVEN roles array is provided WHEN rendering THEN should display all roles', async () => {
    const screen = render(
      <RoleTable roles={mockRoles} isLoading={false} error={null} canManage canManagePermissions />
    )

    expect(screen.getByTestId('role-table')).toBeInTheDocument()

    // Verify role names are displayed
    expect(screen.getByText('realm-admin')).toBeInTheDocument()
    expect(screen.getByText('user-admin')).toBeInTheDocument()

    // Verify we have delete buttons for non-built-in roles (only user-admin in mock data)
    const deleteButtons = screen.queryAllByTestId(/role-delete-button/)
    expect(deleteButtons.length).toBe(1)
  })

  it('GIVEN role table is rendered WHEN user clicks edit button THEN should open edit dialog', async () => {
    render(
      <RoleTable roles={mockRoles} isLoading={false} error={null} canManage canManagePermissions />
    )

    const editButton = screen.getByTestId('role-edit-button-1')
    await userEvent.click(editButton)

    const editDialog = screen.getByTestId('edit-role-dialog')
    expect(editDialog).toBeInTheDocument()
  })

  it('GIVEN custom role is displayed WHEN user clicks delete button THEN should open delete dialog', async () => {
    render(
      <RoleTable roles={mockRoles} isLoading={false} error={null} canManage canManagePermissions />
    )

    const deleteButton = screen.getByTestId('role-delete-button-2')
    await userEvent.click(deleteButton)

    const deleteDialog = screen.getByTestId('delete-role-dialog')
    expect(deleteDialog).toBeInTheDocument()
  })

  it('GIVEN role is builtin WHEN rendering THEN should not show delete button', async () => {
    render(
      <RoleTable roles={mockRoles} isLoading={false} error={null} canManage canManagePermissions />
    )

    // Builtin roles with id '1' and '3' should not have delete buttons
    expect(screen.queryByTestId('role-delete-button-1')).not.toBeInTheDocument()
    expect(screen.queryByTestId('role-delete-button-3')).not.toBeInTheDocument()
  })

  it('GIVEN user clicks manage permissions WHEN clicked THEN should open permissions dialog', async () => {
    render(
      <RoleTable roles={mockRoles} isLoading={false} error={null} canManage canManagePermissions />
    )

    const permissionsButton = screen.getByTestId('role-permissions-button-1')
    await userEvent.click(permissionsButton)

    const permissionsDialog = screen.getByTestId('role-permissions-dialog')
    expect(permissionsDialog).toBeInTheDocument()
  })

  it('GIVEN role has no description WHEN rendering THEN should display dash', async () => {
    const roleWithoutDescription: RoleResponse = {
      id: '4',
      name: 'custom-role',
      description: null,
      realmId: 'realm-1',
      clientId: 'client-1',
      isBuiltin: false,
    }

    const screen = render(
      <RoleTable
        roles={[roleWithoutDescription]}
        isLoading={false}
        error={null}
        canManage
        canManagePermissions
      />
    )

    // Should show - for null description
    const tableRows = screen.container.querySelectorAll('tbody tr')
    const hasDash = Array.from(tableRows).some((row) => row.textContent?.includes('-'))
    expect(hasDash).toBe(true)
  })

  describe('button-level permission gating (permissions.md §6/§4.2)', () => {
    it('GIVEN view-only roles access (canManage false) WHEN rendering THEN edit/delete stay visible but disabled', () => {
      // INTENT (permissions.md §6): "仅有 view 权限时管理按钮不可用" — the
      // roles list remains readable for delegated view-only admins while every
      // write affordance is inert; backend roles.manage enforcement is the
      // backstop, not the only gate.
      const screen = render(
        <RoleTable
          roles={mockRoles}
          isLoading={false}
          error={null}
          canManage={false}
          canManagePermissions
        />
      )

      const editable = mockRoles.find((r) => !r.isBuiltin)
      expect(screen.getByTestId(`role-edit-button-${editable.id}`)).toBeDisabled()
      expect(screen.getByTestId(`role-delete-button-${editable.id}`)).toBeDisabled()
    })

    it('GIVEN no policies.manage WHEN rendering THEN the role-permissions association button is disabled even with roles.manage', () => {
      // INTENT (permissions.md §4.2): role→permission association is a
      // policies.manage write surface — an admin holding roles.manage but not
      // policies.manage must not reach it from this table.
      const screen = render(
        <RoleTable
          roles={mockRoles}
          isLoading={false}
          error={null}
          canManage
          canManagePermissions={false}
        />
      )

      const anyRole = mockRoles[0]
      expect(screen.getByTestId(`role-permissions-button-${anyRole.id}`)).toBeDisabled()
    })

    it('GIVEN full manage access WHEN rendering THEN all action buttons are enabled', () => {
      const screen = render(
        <RoleTable
          roles={mockRoles}
          isLoading={false}
          error={null}
          canManage
          canManagePermissions
        />
      )

      const editable = mockRoles.find((r) => !r.isBuiltin)
      expect(screen.getByTestId(`role-edit-button-${editable.id}`)).toBeEnabled()
      expect(screen.getByTestId(`role-delete-button-${editable.id}`)).toBeEnabled()
      expect(screen.getByTestId(`role-permissions-button-${editable.id}`)).toBeEnabled()
    })
  })

  it('GIVEN isLoading is true WHEN rendering THEN should show loading state', async () => {
    const screen = render(
      <RoleTable roles={[]} isLoading={true} error={null} canManage canManagePermissions />
    )

    expect(screen.getByText('Loading roles...')).toBeInTheDocument()
  })

  it('GIVEN error is provided WHEN rendering THEN should show error state', async () => {
    const screen = render(
      <RoleTable roles={[]} isLoading={false} error={new Error('Failed to load')} />
    )

    expect(screen.getByText('Failed to load roles. Please try again later.')).toBeInTheDocument()
  })

  it('GIVEN roles array is empty WHEN rendering THEN should show empty state', async () => {
    const screen = render(
      <RoleTable roles={[]} isLoading={false} error={null} canManage canManagePermissions />
    )

    expect(
      screen.getByText('No roles found. Create your first role to get started.')
    ).toBeInTheDocument()
  })
})
