import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

vi.mock('@tanstack/react-router', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@tanstack/react-router')>()
  return {
    ...actual,
    createFileRoute: () => (config: Record<string, unknown>) => ({
      ...config,
      useParams: () => ({ realmId: 'realm-1' }),
    }),
  }
})

const { updateRules, permissions } = vi.hoisted(() => ({
  updateRules: vi.fn(),
  permissions: { current: ['points.view', 'points.manage'] as string[] },
}))

vi.mock('@/hooks/use-auth', () => ({
  useAuth: () => ({ permissions: permissions.current }),
}))

vi.mock('@/data/query-options', () => ({
  queryKeys: {
    registrationRules: (realmId: string) => ['registration-rules', realmId],
  },
  registrationRulesQueryOptions: (realmId: string) => ({
    queryKey: ['registration-rules', realmId],
    queryFn: async () => ({
      realmId,
      rules: [
        {
          id: 'registration-rule',
          bucketId: 'bucket-a',
          triggerSources: ['registration'],
          grantMode: 'fixed',
          pointsAmount: 50,
          validityDays: 0,
          enabled: true,
          displayOrder: 0,
        },
        {
          id: 'periodic-rule',
          bucketId: 'bucket-b',
          triggerSources: ['free_periodic_grant'],
          grantMode: 'quota',
          quotaWindows: [{ windowSeconds: 3600, limit: 10 }],
          enabled: true,
          displayOrder: 1,
        },
        {
          id: 'combined-rule',
          bucketId: 'bucket-a',
          triggerSources: ['registration', 'free_periodic_grant'],
          grantMode: 'fixed',
          pointsAmount: 10,
          validityDays: 0,
          grantPeriodType: 'monthly',
          enabled: true,
          displayOrder: 2,
        },
      ],
    }),
  }),
  creditBucketsListQueryOptions: (realmId: string) => ({
    queryKey: ['credit-buckets', realmId],
    queryFn: async () => [
      {
        id: 'bucket-a',
        name: 'General',
        bucketKey: 'general',
        displayOrder: 0,
        enabled: true,
        coveredClientAppCount: 1,
        ruleReferenceCount: 1,
      },
      {
        id: 'bucket-b',
        name: 'Images',
        bucketKey: 'images',
        displayOrder: 1,
        enabled: true,
        coveredClientAppCount: 1,
        ruleReferenceCount: 1,
      },
    ],
  }),
  updateRegistrationRulesMutation: updateRules,
}))

import { RegistrationRulesPage } from '@/routes/$realmId/manage/points/registration-rules'

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <RegistrationRulesPage />
    </QueryClientProvider>
  )
}

describe('registration distribution rules', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    permissions.current = ['points.view', 'points.manage']
    updateRules.mockResolvedValue({ realmId: 'realm-1', rules: [] })
  })

  it('renders registration fixed grants and free-periodic quota as separate rules', async () => {
    renderPage()
    expect(await screen.findByTestId('point-rule-registration-rule')).toBeInTheDocument()
    expect(screen.getByTestId('point-rule-periodic-rule')).toBeInTheDocument()
    expect(screen.getByTestId('point-rule-quota-periodic-rule-editor')).toBeInTheDocument()
    // Combined rules are not editable here — they must not leak into a card.
    expect(screen.queryByTestId('point-rule-combined-rule')).not.toBeInTheDocument()
  })

  it('submits the complete rule set without legacy scalar config fields', async () => {
    const user = userEvent.setup()
    renderPage()
    await user.click(await screen.findByTestId('registration-rules-save'))

    await waitFor(() => expect(updateRules).toHaveBeenCalledOnce())
    const [, body] = updateRules.mock.calls[0] ?? []
    expect(body.rules).toHaveLength(3)
    expect(body.rules).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ id: 'registration-rule', triggerSources: ['registration'] }),
        expect.objectContaining({
          id: 'periodic-rule',
          triggerSources: ['free_periodic_grant'],
          grantMode: 'quota',
        }),
      ])
    )
    expect(body).not.toHaveProperty('registrationBonusPoints')
  })

  it('keeps combined rules untouched when editing one trigger group', async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByTestId('point-rule-registration-rule')

    // A combined rule exists (both triggers); the page flags it instead of
    // silently rewriting it, and editing one group must not alter it.
    expect(screen.getByTestId('registration-rules-combined-notice')).toBeInTheDocument()

    await user.click(screen.getByTestId('point-rule-enabled-registration-rule'))
    await user.click(screen.getByTestId('registration-rules-save'))

    await waitFor(() => expect(updateRules).toHaveBeenCalledOnce())
    const [, body] = updateRules.mock.calls[0] ?? []
    const combined = body.rules.filter((rule: { id?: string }) => rule.id === 'combined-rule')
    expect(combined).toHaveLength(1)
    expect(combined[0]).toEqual(
      expect.objectContaining({
        triggerSources: ['registration', 'free_periodic_grant'],
        grantMode: 'fixed',
        grantPeriodType: 'monthly',
      })
    )
    expect(body.rules.find((rule: { id?: string }) => rule.id === 'registration-rule')).toEqual(
      expect.objectContaining({ enabled: false })
    )
  })

  it('allows points.view users to inspect rules without edit controls', async () => {
    permissions.current = ['points.view']
    renderPage()
    expect(await screen.findByTestId('point-rule-registration-rule')).toBeInTheDocument()
    expect(screen.queryByTestId('registration-rules-save')).not.toBeInTheDocument()
    expect(screen.getAllByTestId('point-rule-add')[0]).toBeDisabled()
  })
})
