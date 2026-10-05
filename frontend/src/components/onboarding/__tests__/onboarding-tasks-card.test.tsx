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

let featureData: { admin?: Record<string, boolean> } | undefined

vi.mock('@tanstack/react-query', () => ({
  useQuery: () => ({ data: featureData }),
}))

vi.mock('@/data/query-options', () => ({
  featureAvailabilityQueryOptions: () => ({ queryKey: ['feature-availability', 'admin'] }),
}))

function setPermissions(permissions: string[]) {
  act(() => {
    useAuthStore.setState({ permissions })
  })
}

// The capability checklist must obey the same permission and
// feature-availability gating as its sidebar counterparts: a restricted
// admin must never be pointed at a page they cannot enter, and a realm with
// a capability disabled must not advertise it.
describe('OnboardingTasksCard gating', () => {
  beforeEach(() => {
    setPermissions([])
    featureData = { admin: { pointsVisible: true, billingConfigVisible: true } }
  })

  it('GIVEN the admin holds every task permission WHEN the card renders THEN all five capability tasks and the replay entry show', () => {
    setPermissions([
      PERMISSION.PERMISSIONS_VIEW,
      PERMISSION.BILLING_VIEW,
      PERMISSION.POINTS_VIEW,
      PERMISSION.SETTINGS_VIEW,
    ])
    render(<OnboardingTasksCard realmId="admin" />)

    expect(screen.getByTestId('onboarding-task-permissions')).toBeInTheDocument()
    expect(screen.getByTestId('onboarding-task-payment-providers')).toBeInTheDocument()
    expect(screen.getByTestId('onboarding-task-points')).toBeInTheDocument()
    expect(screen.getByTestId('onboarding-task-wallets')).toBeInTheDocument()
    expect(screen.getByTestId('onboarding-task-integrations')).toBeInTheDocument()
    expect(screen.getByTestId('onboarding-tour-restart-button')).toBeInTheDocument()
  })

  it('GIVEN a restricted admin lacks billing.view and points.view WHEN the card renders THEN those tasks are hidden and the rest stays', () => {
    setPermissions([PERMISSION.PERMISSIONS_VIEW, PERMISSION.SETTINGS_VIEW])
    render(<OnboardingTasksCard realmId="admin" />)

    expect(screen.getByTestId('onboarding-task-permissions')).toBeInTheDocument()
    expect(screen.getByTestId('onboarding-task-integrations')).toBeInTheDocument()
    expect(screen.queryByTestId('onboarding-task-payment-providers')).not.toBeInTheDocument()
    expect(screen.queryByTestId('onboarding-task-points')).not.toBeInTheDocument()
    expect(screen.queryByTestId('onboarding-task-wallets')).not.toBeInTheDocument()
    expect(screen.getByTestId('onboarding-tour-restart-button')).toBeInTheDocument()
  })

  it('GIVEN the realm has points and billing disabled WHEN the card renders THEN those tasks hide even with the permissions held', () => {
    featureData = { admin: { pointsVisible: false, billingConfigVisible: false } }
    setPermissions([
      PERMISSION.PERMISSIONS_VIEW,
      PERMISSION.BILLING_VIEW,
      PERMISSION.POINTS_VIEW,
      PERMISSION.SETTINGS_VIEW,
    ])
    render(<OnboardingTasksCard realmId="acme" />)

    expect(screen.getByTestId('onboarding-task-permissions')).toBeInTheDocument()
    expect(screen.getByTestId('onboarding-task-integrations')).toBeInTheDocument()
    expect(screen.queryByTestId('onboarding-task-payment-providers')).not.toBeInTheDocument()
    expect(screen.queryByTestId('onboarding-task-points')).not.toBeInTheDocument()
    expect(screen.queryByTestId('onboarding-task-wallets')).not.toBeInTheDocument()
  })
})
