import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RolePermissionsDialog } from '../role-permissions-dialog'
import type { RoleResponse, PermissionResponse } from '@/lib/api-generated'

// Mock API
const mockAssignPermission = vi.fn().mockResolvedValue({ data: { success: true } })
const mockRemovePermission = vi.fn().mockResolvedValue({ data: { success: true } })

vi.mock('@/lib/api-generated', () => ({
  assignPermissionToRole: (...args: unknown[]) => mockAssignPermission(...args),
  removePermissionFromRole: (...args: unknown[]) => mockRemovePermission(...args),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

vi.mock('@/stores/auth-store', () => ({
  useRealmId: () => 'realm-1',
}))

function wrapper({ children }: { children: React.ReactNode }) {
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
}

describe('RolePermissionsDialog', () => {
  const mockRole: RoleResponse = {
    id: '1',
    name: 'realm-admin',
    description: 'Realm administrator',
    realmId: 'realm-1',
    clientId: 'client-1',
    isBuiltin: true,
  }

  const mockPermissions: PermissionResponse[] = [
    {
      id: '1',
      name: 'users.view',
      resource: 'users',
      action: 'view',
      description: 'View users',
      realmId: 'realm-1',
      isBuiltin: true,
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

  const assignedPermissionIds = ['1', '2']

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('GIVEN dialog is open WHEN rendering THEN should display permission summary stats', async () => {
    const result = render(
      <RolePermissionsDialog
        open={true}
        onOpenChange={vi.fn()}
        role={mockRole}
        realmId="realm-1"
        allPermissions={mockPermissions}
        assignedPermissionIds={assignedPermissionIds}
      />,
      { wrapper }
    )

    expect(result.getByText('2 / 3')).toBeInTheDocument()
    expect(result.getByText('permissions assigned')).toBeInTheDocument()
  })

  it('GIVEN role is builtin WHEN rendering THEN should show warning message', async () => {
    const result = render(
      <RolePermissionsDialog
        open={true}
        onOpenChange={vi.fn()}
        role={mockRole}
        realmId="realm-1"
        allPermissions={mockPermissions}
        assignedPermissionIds={assignedPermissionIds}
      />,
      { wrapper }
    )

    expect(result.getAllByText(/Built-in permissions cannot be removed/)[0]).toBeInTheDocument()
  })

  it('GIVEN role is not builtin WHEN rendering THEN should not show warning message', async () => {
    const customRole = { ...mockRole, isBuiltin: false }
    render(
      <RolePermissionsDialog
        open={true}
        onOpenChange={vi.fn()}
        role={customRole}
        realmId="realm-1"
        allPermissions={mockPermissions}
        assignedPermissionIds={assignedPermissionIds}
      />,
      { wrapper }
    )

    expect(screen.queryByTestId('builtin-permission-warning')).not.toBeInTheDocument()
  })

  it('GIVEN user clicks Cancel button WHEN clicked THEN should call onOpenChange with false', async () => {
    const handleOpenChange = vi.fn()
    render(
      <RolePermissionsDialog
        open={true}
        onOpenChange={handleOpenChange}
        role={mockRole}
        realmId="realm-1"
        allPermissions={mockPermissions}
        assignedPermissionIds={assignedPermissionIds}
      />,
      { wrapper }
    )

    const cancelButton = screen.getByTestId('role-permissions-cancel-button')
    await userEvent.click(cancelButton)

    expect(handleOpenChange).toHaveBeenCalledWith(false)
  })

  it('GIVEN permissions are provided WHEN rendering THEN should group by resource', async () => {
    const result = render(
      <RolePermissionsDialog
        open={true}
        onOpenChange={vi.fn()}
        role={mockRole}
        realmId="realm-1"
        allPermissions={mockPermissions}
        assignedPermissionIds={assignedPermissionIds}
      />,
      { wrapper }
    )

    expect(result.getAllByText('users', { exact: true })[0]).toBeInTheDocument()
    expect(result.getAllByText('roles', { exact: true })[0]).toBeInTheDocument()
  })

  it('GIVEN no changes WHEN rendering THEN Save button should be disabled', async () => {
    render(
      <RolePermissionsDialog
        open={true}
        onOpenChange={vi.fn()}
        role={mockRole}
        realmId="realm-1"
        allPermissions={mockPermissions}
        assignedPermissionIds={assignedPermissionIds}
      />,
      { wrapper }
    )

    const saveButton = screen.getByTestId('role-permissions-save-button')
    expect(saveButton).toBeDisabled()
  })

  it('GIVEN user checks an available permission and moves it right WHEN moved THEN Save button should be enabled', async () => {
    const customRole = { ...mockRole, isBuiltin: false }
    render(
      <RolePermissionsDialog
        open={true}
        onOpenChange={vi.fn()}
        role={customRole}
        realmId="realm-1"
        allPermissions={mockPermissions}
        assignedPermissionIds={['1']}
      />,
      { wrapper }
    )

    // permission 2 is not assigned, check it on the left pane and move it right
    await userEvent.click(screen.getByTestId('permission-checkbox-2'))
    const saveButton = screen.getByTestId('role-permissions-save-button')
    expect(saveButton).toBeDisabled()
    await userEvent.click(screen.getByTestId('permission-move-right'))

    expect(saveButton).toBeEnabled()
  })

  it('GIVEN user moves permissions between panes and saves WHEN Save clicked THEN should fire batch API calls', async () => {
    const customRole = { ...mockRole, isBuiltin: false }
    render(
      <RolePermissionsDialog
        open={true}
        onOpenChange={vi.fn()}
        role={customRole}
        realmId="realm-1"
        allPermissions={mockPermissions}
        assignedPermissionIds={['1']}
      />,
      { wrapper }
    )

    // Assign permission 2: check on the left pane, move right
    await userEvent.click(screen.getByTestId('permission-checkbox-2'))
    await userEvent.click(screen.getByTestId('permission-move-right'))
    // Remove permission 1: it lives on the right pane, check it and move left
    await userEvent.click(screen.getByTestId('permission-checkbox-1'))
    await userEvent.click(screen.getByTestId('permission-move-left'))

    const saveButton = screen.getByTestId('role-permissions-save-button')
    await userEvent.click(saveButton)

    expect(mockAssignPermission).toHaveBeenCalledWith(
      expect.objectContaining({
        body: { permissionId: '2' },
      })
    )
    expect(mockRemovePermission).toHaveBeenCalledWith(
      expect.objectContaining({
        path: expect.objectContaining({ permissionId: '1' }),
      })
    )
  })
})
