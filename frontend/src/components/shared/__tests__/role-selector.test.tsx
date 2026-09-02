import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { RoleSelector } from '../role-selector'

describe('RoleSelector', () => {
  const mockRoles = [
    { id: '1', name: 'Admin' },
    { id: '2', name: 'User' },
    { id: '3', name: 'Moderator' },
  ]

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('GIVEN roles array is provided WHEN rendering THEN should display all roles', async () => {
    const handleChange = vi.fn()
    const screen = render(
      <RoleSelector roles={mockRoles} selectedRoleIds={[]} onChange={handleChange} />
    )

    const trigger = screen.getByTestId('role-selector-trigger')
    expect(trigger).toBeInTheDocument()

    // Open dropdown
    await userEvent.click(trigger)

    // Check all roles are displayed
    expect(screen.getByTestId('role-selector-item-1')).toBeInTheDocument()
    expect(screen.getByTestId('role-selector-item-2')).toBeInTheDocument()
    expect(screen.getByTestId('role-selector-item-3')).toBeInTheDocument()
  })

  it('GIVEN role selector is rendered WHEN user clicks role THEN should call onChange with role ID', async () => {
    const handleChange = vi.fn()
    const screen = render(
      <RoleSelector roles={mockRoles} selectedRoleIds={[]} onChange={handleChange} />
    )

    // Open dropdown
    const trigger = screen.getByTestId('role-selector-trigger')
    await userEvent.click(trigger)

    // Click on a role
    const adminRole = screen.getByTestId('role-selector-item-1')
    await userEvent.click(adminRole)

    // Verify onChange was called with role ID
    expect(handleChange).toHaveBeenCalledTimes(1)
    expect(handleChange).toHaveBeenCalledWith(['1'])
  })

  it('GIVEN role is already selected WHEN user clicks again THEN should remove it', async () => {
    const handleChange = vi.fn()
    const screen = render(
      <RoleSelector roles={mockRoles} selectedRoleIds={['1']} onChange={handleChange} />
    )

    // Open dropdown
    const trigger = screen.getByTestId('role-selector-trigger')
    await userEvent.click(trigger)

    // Click on already selected role
    const adminRole = screen.getByTestId('role-selector-item-1')
    await userEvent.click(adminRole)

    // Verify onChange was called with empty array (removed)
    expect(handleChange).toHaveBeenCalledTimes(1)
    expect(handleChange).toHaveBeenCalledWith([])
  })

  it('GIVEN multiple roles are selected WHEN rendering THEN should display all selected roles', async () => {
    const handleChange = vi.fn()
    const screen = render(
      <RoleSelector roles={mockRoles} selectedRoleIds={['1', '2']} onChange={handleChange} />
    )

    const trigger = screen.getByTestId('role-selector-trigger')
    expect(trigger).toBeInTheDocument()

    // Check that badges are displayed for selected roles
    expect(screen.getAllByText('Admin', { exact: true })[0]).toBeInTheDocument()
    expect(screen.getAllByText('User', { exact: true })[0]).toBeInTheDocument()
  })

  it('GIVEN disabled prop is true WHEN rendering THEN should disable selector', async () => {
    const handleChange = vi.fn()
    const screen = render(
      <RoleSelector
        roles={mockRoles}
        selectedRoleIds={[]}
        onChange={handleChange}
        disabled={true}
      />
    )

    const trigger = screen.getByTestId('role-selector-trigger')
    expect(trigger).toBeDisabled()
  })

  it('GIVEN no roles are selected WHEN rendering THEN should display placeholder', async () => {
    const handleChange = vi.fn()
    const screen = render(
      <RoleSelector
        roles={mockRoles}
        selectedRoleIds={[]}
        onChange={handleChange}
        placeholder="Select roles..."
      />
    )

    const trigger = screen.getByTestId('role-selector-trigger')
    expect(trigger).toHaveTextContent('Select roles...')
  })
})
