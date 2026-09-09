import { describe, it, expect, afterEach, vi } from 'vitest'
import { useState } from 'react'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { PermissionTransfer } from '../permission-transfer'
import type { PermissionResponse } from '@/lib/api-generated'

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
  {
    id: '4',
    name: 'billing.view',
    resource: 'billing',
    action: 'view',
    description: 'View billing',
    realmId: 'realm-1',
    isBuiltin: false,
  },
]

// Drives the transfer like the real dialog does: onTogglePermission feeds back
// into assignedPermissionIds so moved items actually change panes.
function TransferHarness({
  initialAssigned,
  isBuiltinRole = false,
  disabled = false,
  onToggle,
}: {
  initialAssigned: string[]
  isBuiltinRole?: boolean
  disabled?: boolean
  onToggle?: (permissionId: string, checked: boolean) => void
}) {
  const [assigned, setAssigned] = useState(initialAssigned)
  return (
    <PermissionTransfer
      permissions={mockPermissions}
      assignedPermissionIds={assigned}
      onTogglePermission={(id, checked) => {
        onToggle?.(id, checked)
        setAssigned((prev) => (checked ? [...prev, id] : prev.filter((x) => x !== id)))
      }}
      isBuiltinRole={isBuiltinRole}
      disabled={disabled}
    />
  )
}

describe('PermissionTransfer', () => {
  afterEach(() => {
    vi.clearAllMocks()
  })

  it('GIVEN some permissions are assigned WHEN rendering THEN assigned appear only on the right pane and unassigned only on the left', () => {
    render(<TransferHarness initialAssigned={['1']} />)
    const availablePane = screen.getByTestId('permission-available-pane')
    const selectedPane = screen.getByTestId('permission-selected-pane')

    // users.view is assigned: right pane only, never duplicated on the left
    expect(within(selectedPane).getByTestId('permission-item-1')).toBeInTheDocument()
    expect(within(availablePane).queryByTestId('permission-item-1')).not.toBeInTheDocument()

    // Unassigned permissions stay on the left pane
    expect(within(availablePane).getByTestId('permission-item-2')).toBeInTheDocument()
    expect(within(availablePane).getByTestId('permission-item-3')).toBeInTheDocument()
    expect(within(availablePane).getByTestId('permission-item-4')).toBeInTheDocument()
  })

  it('GIVEN many permissions WHEN searching on the left THEN only matching permissions remain and one can be found and moved without scrolling', async () => {
    const onToggle = vi.fn()
    render(<TransferHarness initialAssigned={[]} onToggle={onToggle} />)

    await userEvent.type(screen.getByTestId('permission-available-search'), 'billing')

    // The search narrows the catalog to the billing match only
    expect(screen.getByTestId('permission-item-4')).toBeInTheDocument()
    expect(screen.queryByTestId('permission-item-2')).not.toBeInTheDocument()
    expect(screen.queryByTestId('permission-item-3')).not.toBeInTheDocument()

    expect(screen.getByTestId('permission-move-right')).toBeDisabled()
    await userEvent.click(screen.getByTestId('permission-checkbox-4'))
    await userEvent.click(screen.getByTestId('permission-move-right'))

    expect(onToggle).toHaveBeenCalledTimes(1)
    expect(onToggle).toHaveBeenCalledWith('4', true)
    // The moved permission now lives on the right pane
    expect(
      within(screen.getByTestId('permission-selected-pane')).getByTestId('permission-item-4')
    ).toBeInTheDocument()
  })

  it('GIVEN search is active WHEN clicking select-all THEN only the visible matches get checked', async () => {
    const onToggle = vi.fn()
    render(<TransferHarness initialAssigned={['1']} onToggle={onToggle} />)

    await userEvent.type(screen.getByTestId('permission-available-search'), 'roles')
    await userEvent.click(screen.getByTestId('permission-available-select-all'))

    expect(screen.getByTestId('permission-checkbox-3')).toBeChecked()
    // users.manage matches neither the search nor the select-all click
    expect(screen.queryByTestId('permission-checkbox-2')).not.toBeInTheDocument()

    await userEvent.click(screen.getByTestId('permission-move-right'))
    expect(onToggle).toHaveBeenCalledTimes(1)
    expect(onToggle).toHaveBeenCalledWith('3', true)
  })

  it('GIVEN a checked item is hidden by a new search WHEN moving THEN the hidden item keeps its check and is still moved', async () => {
    const onToggle = vi.fn()
    render(<TransferHarness initialAssigned={[]} onToggle={onToggle} />)

    await userEvent.click(screen.getByTestId('permission-checkbox-2'))
    // Filtering users.manage out of view must not silently drop the pending selection
    await userEvent.type(screen.getByTestId('permission-available-search'), 'billing')

    expect(screen.queryByTestId('permission-checkbox-2')).not.toBeInTheDocument()
    expect(screen.getByTestId('permission-move-right')).toBeEnabled()

    await userEvent.click(screen.getByTestId('permission-move-right'))
    expect(onToggle).toHaveBeenCalledWith('2', true)
  })

  it('GIVEN a permission is assigned WHEN checking it on the right and moving left THEN it is unassigned and returns to the left pane', async () => {
    const onToggle = vi.fn()
    render(<TransferHarness initialAssigned={['1', '3']} onToggle={onToggle} />)

    expect(screen.getByTestId('permission-move-left')).toBeDisabled()
    await userEvent.click(screen.getByTestId('permission-checkbox-3'))
    await userEvent.click(screen.getByTestId('permission-move-left'))

    expect(onToggle).toHaveBeenCalledTimes(1)
    expect(onToggle).toHaveBeenCalledWith('3', false)
    // Back on the left pane, unchecked
    expect(screen.getByTestId('permission-checkbox-3')).not.toBeChecked()
  })

  it('GIVEN a builtin role with an assigned builtin permission WHEN rendering THEN the builtin permission is locked and cannot be moved out, while custom permissions can still be added', async () => {
    const onToggle = vi.fn()
    render(<TransferHarness initialAssigned={['1']} isBuiltinRole onToggle={onToggle} />)

    // Built-in permissions must not be removable from built-in roles
    expect(screen.getByTestId('permission-checkbox-1')).toBeDisabled()
    expect(screen.getByTestId('permission-selected-select-all')).toBeDisabled()
    expect(screen.getByTestId('permission-move-left')).toBeDisabled()

    // Adding custom permissions stays possible on a built-in role
    await userEvent.click(screen.getByTestId('permission-checkbox-3'))
    await userEvent.click(screen.getByTestId('permission-move-right'))
    expect(onToggle).toHaveBeenCalledWith('3', true)
  })

  it('GIVEN no permissions are assigned WHEN rendering THEN the right pane shows the empty-selection hint and a non-matching search shows the no-match hint', async () => {
    render(<TransferHarness initialAssigned={[]} />)

    expect(screen.getByText('No permissions selected yet')).toBeInTheDocument()

    await userEvent.type(screen.getByTestId('permission-available-search'), 'zzz')
    expect(screen.getByText('No matching permissions')).toBeInTheDocument()
  })

  it('GIVEN permissions are listed WHEN rendering THEN the left pane groups them by resource with counts', () => {
    render(<TransferHarness initialAssigned={[]} />)

    expect(screen.getByText('users', { exact: true })).toBeInTheDocument()
    expect(screen.getByText('roles', { exact: true })).toBeInTheDocument()
    expect(screen.getByText('billing', { exact: true })).toBeInTheDocument()
    expect(screen.getByText('(2 permissions)')).toBeInTheDocument()
    expect(screen.getAllByText('(1 permission)').length).toBe(2)
  })

  it('GIVEN the dialog is saving WHEN rendering THEN all checkboxes, search inputs and move buttons are disabled', () => {
    render(<TransferHarness initialAssigned={['1']} disabled />)

    expect(screen.getByTestId('permission-checkbox-2')).toBeDisabled()
    expect(screen.getByTestId('permission-checkbox-1')).toBeDisabled()
    expect(screen.getByTestId('permission-available-search')).toBeDisabled()
    expect(screen.getByTestId('permission-selected-search')).toBeDisabled()
    expect(screen.getByTestId('permission-move-right')).toBeDisabled()
    expect(screen.getByTestId('permission-move-left')).toBeDisabled()
  })

  it('GIVEN the permission catalog is empty WHEN rendering THEN the empty state replaces the panes', () => {
    render(
      <PermissionTransfer
        permissions={[]}
        assignedPermissionIds={[]}
        onTogglePermission={vi.fn()}
        isBuiltinRole={false}
      />
    )

    expect(screen.getByText('No permissions available')).toBeInTheDocument()
    expect(screen.queryByTestId('permission-move-right')).not.toBeInTheDocument()
  })
})
