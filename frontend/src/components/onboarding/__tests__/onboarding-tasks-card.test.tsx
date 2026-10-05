import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act } from 'react'
import { render, screen } from '@testing-library/react'
import { useAuthStore } from '@/stores/auth-store'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { OnboardingTasksCard } from '../onboarding-tasks-card'

// The card renders TanStack Router Links; outside a real router only the
// anchor contract matters. Keep the rest of the router package intact for
// transitive importers.
vi.mock('@tanstack/react-router', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@tanstack/react-router')>()
  return {
    ...actual,
    Link: ({
      to,
      children,
      ...rest
    }: {
      to?: string
      children?: React.ReactNode
    } & Record<string, unknown>) => (
      <a href={to} {...rest}>
        {children}
      </a>
    ),
  }
})

function setPermissions(permissions: string[]) {
  act(() => {
    useAuthStore.setState({ permissions })
  })
}

// The starter checklist must obey the same view permissions as its QuickNav
// counterparts: a restricted admin must never be pointed at a console page
// they cannot enter.
describe('OnboardingTasksCard permission gating', () => {
  beforeEach(() => {
    setPermissions([])
  })

  it('GIVEN the admin holds every task permission WHEN the card renders THEN all three starter tasks and the replay entry show', () => {
    setPermissions([PERMISSION.USERS_VIEW, PERMISSION.CLIENTS_VIEW, PERMISSION.SETTINGS_VIEW])
    render(<OnboardingTasksCard />)

    expect(screen.getByTestId('onboarding-task-users')).toBeInTheDocument()
    expect(screen.getByTestId('onboarding-task-clients')).toBeInTheDocument()
    expect(screen.getByTestId('onboarding-task-settings')).toBeInTheDocument()
    expect(screen.getByTestId('onboarding-tour-restart-button')).toBeInTheDocument()
  })

  it('GIVEN a restricted admin lacks clients.view and settings.view WHEN the card renders THEN those tasks are hidden and the rest stays', () => {
    setPermissions([PERMISSION.USERS_VIEW])
    render(<OnboardingTasksCard />)

    expect(screen.getByTestId('onboarding-task-users')).toBeInTheDocument()
    expect(screen.queryByTestId('onboarding-task-clients')).not.toBeInTheDocument()
    expect(screen.queryByTestId('onboarding-task-settings')).not.toBeInTheDocument()
    expect(screen.getByTestId('onboarding-tour-restart-button')).toBeInTheDocument()
  })
})
