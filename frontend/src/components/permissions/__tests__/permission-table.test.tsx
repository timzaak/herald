import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { PermissionTable } from '../permission-table'
import type { PermissionResponse } from '@/lib/api-generated'

// Mocks dialog components
vi.mock('../edit-permission-dialog', () => ({
  EditPermissionDialog: ({
    open,
    onOpenChange,
  }: {
    open: boolean
    onOpenChange: (open: boolean) => void
  }) => (
    <div data-testid="edit-permission-dialog" style={{ display: open ? 'block' : 'none' }}>
      <button onClick={() => onOpenChange(false)}>Close</button>
    </div>
  ),
}))

vi.mock('../delete-permission-dialog', () => ({
  DeletePermissionDialog: ({
    open,
    onOpenChange,
  }: {
    open: boolean
    onOpenChange: (open: boolean) => void
  }) => (
    <div data-testid="delete-permission-dialog" style={{ display: open ? 'block' : 'none' }}>
      <button onClick={() => onOpenChange(false)}>Close</button>
    </div>
  ),
}))

describe('PermissionTable', () => {
  const mockPermissions: PermissionResponse[] = [
    {
      id: '1',
      name: 'users.view',
      resource: 'users',
      action: 'view',
      description: 'View users',
      realmId: 'realm-1',
      isBuiltin: false,
    },
    {
      id: '2',
      name: 'users.manage',
      resource: 'users',
      action: 'manage',
      description: 'Manage users',
      realmId: 'realm-1',
      isBuiltin: true,
    },
    {
      id: '3',
      name: 'roles.view',
      resource: 'roles',
      action: 'view',
      description: 'View roles',
      realmId: 'realm-1',
      isBuiltin: false,
    },
  ]

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('GIVEN permission table is rendered WHEN user clicks edit button THEN should open edit dialog', async () => {
    render(
      <PermissionTable permissions={mockPermissions} isLoading={false} error={null} canManage />
    )

    const editButton = document.querySelector(
      '[data-testid="permission-edit-button-1"]'
    ) as HTMLElement
    await userEvent.click(editButton)

    const editDialog = document.querySelector('[data-testid="edit-permission-dialog"]')
    expect(editDialog).toBeInTheDocument()
  })

  it('GIVEN custom permission is displayed WHEN user clicks delete button THEN should open delete dialog', async () => {
    render(
      <PermissionTable
        permissions={mockPermissions.filter((p) => !p.isBuiltin)}
        isLoading={false}
        error={null}
        canManage
      />
    )

    const deleteButton = document.querySelector(
      '[data-testid="permission-delete-button-1"]'
    ) as HTMLElement
    await userEvent.click(deleteButton)

    const deleteDialog = document.querySelector('[data-testid="delete-permission-dialog"]')
    expect(deleteDialog).toBeInTheDocument()
  })

  it('GIVEN permission is builtin WHEN rendering THEN should not show delete button', async () => {
    render(
      <PermissionTable permissions={mockPermissions} isLoading={false} error={null} canManage />
    )

    // Builtin permission with id '2' should not have delete button
    const deleteButton = document.querySelector('[data-testid="permission-delete-button-2"]')
    expect(deleteButton).toBeNull()
  })

  it('GIVEN builtin permission WHEN rendering THEN should not show edit button', async () => {
    const screen = render(
      <PermissionTable permissions={mockPermissions} isLoading={false} error={null} canManage />
    )

    expect(screen.queryByTestId('permission-edit-button-2')).toBeNull()
  })

  it('GIVEN custom permission WHEN rendering THEN should enable edit button', async () => {
    const screen = render(
      <PermissionTable
        permissions={mockPermissions.filter((p) => !p.isBuiltin)}
        isLoading={false}
        error={null}
        canManage
      />
    )

    const editButton = screen.getByTestId('permission-edit-button-1')
    expect(editButton).toBeEnabled()
  })

  describe('button-level permission gating (permissions.md §6)', () => {
    it('GIVEN view-only access (canManage false) WHEN rendering THEN edit and delete buttons render but are disabled', () => {
      // INTENT (permissions.md §6): "仅有 view 权限时管理按钮不可用" — a
      // delegated admin with only permissions.view must still see the list,
      // but every write affordance must be inert; the backend 403 is the
      // backstop, not the only line of defense.
      const screen = render(
        <PermissionTable
          permissions={mockPermissions.filter((p) => !p.isBuiltin)}
          isLoading={false}
          error={null}
          canManage={false}
        />
      )

      expect(screen.getByTestId('permission-edit-button-1')).toBeDisabled()
      expect(screen.getByTestId('permission-delete-button-1')).toBeDisabled()
    })

    it('GIVEN manage access WHEN rendering THEN edit and delete buttons are enabled', () => {
      const screen = render(
        <PermissionTable
          permissions={mockPermissions.filter((p) => !p.isBuiltin)}
          isLoading={false}
          error={null}
          canManage
        />
      )

      expect(screen.getByTestId('permission-edit-button-1')).toBeEnabled()
      expect(screen.getByTestId('permission-delete-button-1')).toBeEnabled()
    })
  })

  it('GIVEN isLoading is true WHEN rendering THEN should show loading state', async () => {
    const screen = render(
      <PermissionTable permissions={[]} isLoading={true} error={null} canManage />
    )

    expect(screen.getByText('Loading permissions...')).toBeInTheDocument()
  })

  it('GIVEN error is provided WHEN rendering THEN should show error state', async () => {
    const screen = render(
      <PermissionTable
        permissions={[]}
        isLoading={false}
        error={new Error('Failed to load')}
        canManage
      />
    )

    expect(
      screen.getByText('Failed to load permissions. Please try again later.')
    ).toBeInTheDocument()
  })

  it('GIVEN permissions array is empty WHEN rendering THEN should show empty state', async () => {
    const screen = render(
      <PermissionTable permissions={[]} isLoading={false} error={null} canManage />
    )

    expect(
      screen.getByText('No permissions found. Create your first permission to get started.')
    ).toBeInTheDocument()
  })

  it('GIVEN permission has no description WHEN rendering THEN should display dash', async () => {
    const permissionWithoutDescription: PermissionResponse = {
      id: '4',
      name: 'test.view',
      resource: 'test',
      action: 'view',
      description: null,
      realmId: 'realm-1',
      isBuiltin: false,
    }

    const screen = render(
      <PermissionTable
        permissions={[permissionWithoutDescription]}
        isLoading={false}
        error={null}
        canManage
      />
    )

    // Should show - for null description
    const tableRows = document.querySelectorAll('tbody tr')
    const hasDash = Array.from(tableRows).some((row) => row.textContent?.includes('-'))
    expect(hasDash).toBe(true)
  })
})
