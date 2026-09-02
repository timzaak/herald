import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { toast } from 'sonner'

// --- Mocks ----------------------------------------------------------------
//
// Mirrors entitlement-mappings-page.test.tsx: the whole mutations module is
// mocked so individual tests drive `mockCreateMutate` to decide
// mock's onError controller just delegates to the form's caller-supplied
// onError — the form owns the duplicate vs config-error classification.

// Permission hook: default to a fully-privileged admin (billing.manage +
// points.manage both pass) so the credit-strategy fields render.
vi.mock('@/hooks/use-permission', () => ({
  usePermission: vi.fn(() => ({
    hasPermission: (_p: string) => true,
  })),
}))

// The form uses TanStack Router's `useNavigate` (back / cancel / post-create
// return to the mappings list). Mock it with a spy so the component renders
// without a router provider and success navigation is assertable (mirrors the
// client-app-form-page test).
const { navigateMock } = vi.hoisted(() => ({ navigateMock: vi.fn() }))
vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    useNavigate: () => navigateMock,
  }
})

// The form derives its return path from `realmPath` +
// `useResolvedRealmContext`. Mock both to avoid depending on the URL/router.
vi.mock('@/lib/realm-routing', () => ({
  realmPath: (_ctx: unknown, path: string) => path,
  useResolvedRealmContext: () => ({ realmId: 'realm-1', mode: 'default' }),
}))

// Query options: canned bucket list + roles so the form renders without a
// real query. The bucket list must be non-empty for the bucket Select.
const { bucketsHolder } = vi.hoisted(() => ({
  bucketsHolder: {
    current: [] as Array<{
      id: string
      name: string
      bucketKey: string
      displayOrder: number
      enabled: boolean
      coveredClientAppCount: number
      ruleReferenceCount: number
    }>,
  },
}))

vi.mock('@/data/query-options', () => ({
  // The real cache key is ['entitlement-mappings', realmId, {}]; the mock keeps
  // the prefix shape so invalidateQueries(queryKey) calls can be observed.
  queryKeys: {
    entitlementMappings: (realmId: string, _filters: Record<string, unknown>) => [
      'entitlement-mappings',
      realmId,
      {},
    ],
  },
  entitlementMappingsQueryOptions: () => ({
    queryKey: ['entitlement-mappings', 'realm-1'],
    queryFn: async () => ({ items: [], total: 0 }),
  }),
  creditBucketsListQueryOptions: () => ({
    queryKey: ['credit-buckets', 'realm-1'],
    queryFn: async () => bucketsHolder.current,
  }),
  adminRolesQueryOptions: () => ({
    queryKey: ['roles', 'realm-1'],
    queryFn: async () => [],
  }),
}))

const { mockCreateMutate, mockIsCreateMappingDuplicateError, mockIsCreateMappingConfigError } =
  vi.hoisted(() => {
    const mockCreateMutate = vi.fn()
    // Mirror the real helper contract from entitlement-mapping-mutations.ts so
    // the form's branch logic (not the helper itself — that is unit-tested
    // elsewhere) drives the observed toast. 409 → duplicate; 23514 code or
    // status >= 500 → config error.
    const mockIsCreateMappingDuplicateError = (e: unknown) =>
      !!e && typeof e === 'object' && (e as { status?: unknown }).status === 409
    const mockIsCreateMappingConfigError = (e: unknown) => {
      if (!e || typeof e !== 'object') return false
      const obj = e as { code?: unknown; status?: unknown }
      if (obj.code === '23514') return true
      return typeof obj.status === 'number' && obj.status >= 500
    }
    return {
      mockCreateMutate,
      mockIsCreateMappingDuplicateError,
      mockIsCreateMappingConfigError,
    }
  })

vi.mock('@/data/entitlement-mapping-mutations', () => ({
  useCreateEntitlementMapping: () => ({
    mutate: (
      req: unknown,
      opts: { onSuccess?: () => void; onError?: (error: unknown) => void }
    ) => {
      // Delegate to the per-test controller; tests drive success/failure via
      // mockImplementation that invokes opts.onSuccess?.() / opts.onError?(err).
      mockCreateMutate(req, opts)
    },
    isPending: false,
  }),
  isCreateMappingDuplicateError: mockIsCreateMappingDuplicateError,
  isCreateMappingConfigError: mockIsCreateMappingConfigError,
}))

// Role selector: stub (the granted-roles field is exercised by the page test).
vi.mock('@/components/shared/role-selector', () => ({
  RoleSelector: () => <div data-testid="role-selector-stub" />,
}))

import { CreateEntitlementMappingPage } from '../create-entitlement-mapping-page'
import { m } from '@/paraglide/messages'

// --- Fixtures --------------------------------------------------------------

const BUCKETS = [
  {
    id: 'bucket-1',
    name: 'Default Bucket',
    bucketKey: 'default',
    displayOrder: 0,
    enabled: true,
    coveredClientAppCount: 1,
    ruleReferenceCount: 0,
  },
]

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
}

function Wrapper({ client, children }: { client: QueryClient; children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

/**
 * Fill the create-mapping form's required fields. Radix Selects are driven by
 * clicking the testid trigger then the option (the SelectItem renders with
 * `role="option"` in a portal). Defaults to an `apple` recurring mapping with a
 * monthly period.
 */
async function fillCreateForm(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByTestId('create-mapping-provider-select'))
  await user.click(await screen.findByRole('option', { name: 'App Store' }))

  // Text fields are set with single change events — keystroke-by-keystroke
  // typing drops characters on CPU-starved workers and corrupts the payload.
  fireEvent.change(screen.getByTestId('create-mapping-external-product-id-input'), {
    target: { value: 'com.example.app.premium' },
  })
  fireEvent.change(screen.getByTestId('create-mapping-entitlement-key-input'), {
    target: { value: 'premium' },
  })

  // Billing Type = recurring (so billingPeriod becomes visible + required)
  await user.click(screen.getByTestId('create-mapping-billing-type-select'))
  await user.click(await screen.findByRole('option', { name: /recurring/i }))

  // Billing Period (required because recurring)
  await user.click(screen.getByTestId('create-mapping-billing-period-select'))
  await user.click(await screen.findByRole('option', { name: /month/i }))

  await user.click(screen.getByTestId('point-rule-add'))
  await user.click(screen.getByTestId('point-rule-bucket'))
  await user.click(await screen.findByRole('option', { name: 'Default Bucket' }))
}

function renderPage() {
  const client = makeQueryClient()
  const view = render(
    <Wrapper client={client}>
      <CreateEntitlementMappingPage realmId="realm-1" canManagePoints={true} />
    </Wrapper>
  )
  return { client, ...view }
}

/**
 * Fill the create-mapping form for a non-renewing mapping. Mirrors
 * {@link fillCreateForm} but selects `non_renewing` as the billing type and
 * fills the conditional `create-mapping-service-duration-days-input`. Billing
 * period is intentionally left unset (non-renewing + billingPeriod are mutually
 */
async function fillCreateFormNonRenewing(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByTestId('create-mapping-provider-select'))
  await user.click(await screen.findByRole('option', { name: 'App Store' }))

  fireEvent.change(screen.getByTestId('create-mapping-external-product-id-input'), {
    target: { value: 'com.example.app.premium' },
  })
  fireEvent.change(screen.getByTestId('create-mapping-entitlement-key-input'), {
    target: { value: 'premium' },
  })

  // Billing Type = non_renewing (option label = billing.billing_type_non_renewing).
  await user.click(screen.getByTestId('create-mapping-billing-type-select'))
  await user.click(await screen.findByRole('option', { name: /non-renewing/i }))

  // Conditional non-renewing duration field.
  fireEvent.change(screen.getByTestId('create-mapping-service-duration-days-input'), {
    target: { value: '30' },
  })

  await user.click(screen.getByTestId('point-rule-add'))
  await user.click(screen.getByTestId('point-rule-bucket'))
  await user.click(await screen.findByRole('option', { name: 'Default Bucket' }))
}

beforeEach(() => {
  vi.clearAllMocks()
  bucketsHolder.current = BUCKETS
})

// --- Tests -----------------------------------------------------------------

describe('CreateEntitlementMappingPage — submit success', () => {
  it('submits a valid form, toasts success, and returns to the mappings list', async () => {
    mockCreateMutate.mockImplementation(
      (_req: unknown, opts: { onSuccess?: () => void; onError?: (error: unknown) => void }) => {
        opts.onSuccess?.()
      }
    )

    const user = userEvent.setup()
    renderPage()

    await fillCreateForm(user)
    await user.click(screen.getByTestId('create-mapping-submit-button'))

    await waitFor(() => {
      expect(mockCreateMutate).toHaveBeenCalledTimes(1)
    })
    const body = mockCreateMutate.mock.calls[0]?.[0] as Record<string, unknown>
    expect(body.paymentProvider).toBe('apple')
    expect(body.externalProductId).toBe('com.example.app.premium')
    expect(body.entitlementKey).toBe('premium')
    expect(body.pointRules).toEqual([
      expect.objectContaining({
        bucketId: 'bucket-1',
        triggerSources: ['subscription_initial'],
      }),
    ])
    expect(body.billingType).toBe('recurring')
    expect(body.billingPeriod).toBe('monthly')

    await waitFor(() => {
      expect(toast.success).toHaveBeenCalledWith(m['billing.create_mapping_success']())
    })
    expect(navigateMock).toHaveBeenCalledWith({ to: '/manage/billing/entitlement-mappings' })
  })
})

describe('CreateEntitlementMappingPage — error classification (§4.2.2)', () => {
  // 409 Conflict → duplicate. Distinct from 23514/non-4xx — must NOT be
  // conflated. The form surfaces `billing.create_mapping_duplicate`.
  it("shows the 'product id already exists' message on a 409 duplicate", async () => {
    mockCreateMutate.mockImplementation(
      (_req: unknown, opts: { onSuccess?: () => void; onError?: (error: unknown) => void }) => {
        opts.onError?.({ status: 409, code: 'mapping_already_exists' })
      }
    )

    const user = userEvent.setup()
    renderPage()

    await fillCreateForm(user)
    await user.click(screen.getByTestId('create-mapping-submit-button'))

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(m['billing.create_mapping_duplicate']())
    })

    // The duplicate branch also surfaces inline (the only fix is editing the
    // provider/product inputs, not retrying).
    await waitFor(() => {
      expect(screen.getByTestId('create-mapping-submit-error')).toHaveTextContent(
        String(m['billing.create_mapping_duplicate']())
      )
    })

    // A 409 is NOT a config error — assert the other branch did not fire.
    expect(toast.error).not.toHaveBeenCalledWith(m['billing.create_mapping_config_error']())
    // And the form stays on the page (the admin edits the inputs).
    expect(navigateMock).not.toHaveBeenCalled()
  })

  // 23514 / non-4xx → configuration error (DB CHECK / server defense). Two
  // representative triggers: a 23514-tagged body and a 500 server failure.
  it.each([
    ['a 23514-tagged CHECK body', { status: 422, code: '23514' }],
    ['a 500 server failure', { status: 500, message: 'internal error' }],
  ])('shows the configuration-error message on %s', async (_label, error) => {
    mockCreateMutate.mockImplementation(
      (_req: unknown, opts: { onSuccess?: () => void; onError?: (error: unknown) => void }) => {
        opts.onError?.(error)
      }
    )

    const user = userEvent.setup()
    renderPage()

    await fillCreateForm(user)
    await user.click(screen.getByTestId('create-mapping-submit-button'))

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(m['billing.create_mapping_config_error']())
    })

    // Config error must NOT surface as a duplicate (distinct branches, §4.2.2).
    expect(toast.error).not.toHaveBeenCalledWith(m['billing.create_mapping_duplicate']())
  })

  // 400 validation falls through to the generic `billing.create_mapping_failed`
  // branch — it is neither a duplicate (409) nor a config error (23514/>=500).
  it('falls back to the generic failure message on a 400 validation error', async () => {
    mockCreateMutate.mockImplementation(
      (_req: unknown, opts: { onSuccess?: () => void; onError?: (error: unknown) => void }) => {
        opts.onError?.({ status: 400, code: 'bad_request', message: 'invalid' })
      }
    )

    const user = userEvent.setup()
    renderPage()

    await fillCreateForm(user)
    await user.click(screen.getByTestId('create-mapping-submit-button'))

    await waitFor(() => {
      expect(screen.getByTestId('create-mapping-submit-error')).toHaveTextContent(
        String(m['billing.create_mapping_failed']())
      )
    })

    // 400 is neither the duplicate nor the config-error branch.
    expect(toast.error).not.toHaveBeenCalledWith(m['billing.create_mapping_duplicate']())
    expect(toast.error).not.toHaveBeenCalledWith(m['billing.create_mapping_config_error']())
  })
})

describe('CreateEntitlementMappingPage — client-side validation', () => {
  // The schema's recurring ⇒ billingPeriod refinement (support-iap §4.4.2) is
  // Demo-unreachable (the form hides submit until the field is filled). This
  // Vitest is the only coverage of the safeParse gate blocking submit.
  it('blocks submit and shows a billingPeriod field error when recurring has no period', async () => {
    const user = userEvent.setup()
    renderPage()

    // Fill everything except billingPeriod.
    await user.click(screen.getByTestId('create-mapping-provider-select'))
    await user.click(await screen.findByRole('option', { name: 'App Store' }))
    await user.type(
      screen.getByTestId('create-mapping-external-product-id-input'),
      'com.example.app.premium'
    )
    await user.type(screen.getByTestId('create-mapping-entitlement-key-input'), 'premium')
    await user.click(screen.getByTestId('create-mapping-billing-type-select'))
    await user.click(await screen.findByRole('option', { name: /recurring/i }))
    // billingPeriod select intentionally left empty.

    await user.click(screen.getByTestId('create-mapping-submit-button'))

    expect(mockCreateMutate).not.toHaveBeenCalled()
    expect(toast.success).not.toHaveBeenCalled()

    await waitFor(() => {
      const alerts = screen.getAllByRole('alert')
      // At least one field error rendered. The form renders a <p role="alert">
      // per failed field; recurring-without-period fails billingPeriod.
      expect(alerts.length).toBeGreaterThan(0)
    })
  })

  it('blocks submit when required text fields are empty', async () => {
    const user = userEvent.setup()
    renderPage()

    await user.click(screen.getByTestId('create-mapping-submit-button'))

    expect(mockCreateMutate).not.toHaveBeenCalled()
    await waitFor(() => {
      expect(screen.getAllByRole('alert').length).toBeGreaterThan(0)
    })
  })
})

//
// The form renders the serviceDurationDays input only for non_renewing and
// trims the submitted payload by billing type (non-renewing sends the integer,
// other types send null). These branches are Demo-unreachable as isolated
// payload assertions, so Vitest is the coverage.

describe('CreateEntitlementMappingPage — non-renewing interaction', () => {
  it('renders the service-duration-days input only when billingType is non_renewing', async () => {
    const user = userEvent.setup()
    renderPage()

    // Initially (no billing type selected) the non-renewing duration input is
    // absent.
    expect(screen.queryByTestId('create-mapping-service-duration-days-input')).toBeNull()

    // Selecting non_renewing reveals the conditional duration input.
    await user.click(screen.getByTestId('create-mapping-billing-type-select'))
    await user.click(await screen.findByRole('option', { name: /non-renewing/i }))

    expect(screen.getByTestId('create-mapping-service-duration-days-input')).toBeInTheDocument()
  })

  it('submits a non_renewing mapping and trims serviceDurationDays into the payload', async () => {
    mockCreateMutate.mockImplementation(
      (_req: unknown, opts: { onSuccess?: () => void; onError?: (error: unknown) => void }) => {
        opts.onSuccess?.()
      }
    )

    const user = userEvent.setup()
    renderPage()

    await fillCreateFormNonRenewing(user)
    await user.click(screen.getByTestId('create-mapping-submit-button'))

    await waitFor(() => {
      expect(mockCreateMutate).toHaveBeenCalledTimes(1)
    })
    const body = mockCreateMutate.mock.calls[0]?.[0] as Record<string, unknown>
    // The non-renewing branch keeps the duration and drops the billing period.
    expect(body.billingType).toBe('non_renewing')
    expect(body.serviceDurationDays).toBe(30)
    expect(body.billingPeriod).toBeNull()
  })

  it('sends serviceDurationDays: null in the payload for non-non_renewing types', async () => {
    // Verifies the per-type payload trim does NOT leak a serviceDurationDays
    mockCreateMutate.mockImplementation(
      (_req: unknown, opts: { onSuccess?: () => void; onError?: (error: unknown) => void }) => {
        opts.onSuccess?.()
      }
    )

    const user = userEvent.setup()
    renderPage()

    await fillCreateForm(user)
    await user.click(screen.getByTestId('create-mapping-submit-button'))

    await waitFor(() => {
      expect(mockCreateMutate).toHaveBeenCalledTimes(1)
    })
    const body = mockCreateMutate.mock.calls[0]?.[0] as Record<string, unknown>
    expect(body.billingType).toBe('recurring')
    expect(body.serviceDurationDays).toBeNull()
  })
})

//
// WeChat has no hosted catalog: the form prices the mapping by hand, hides
// the catalog-only external price id, and cannot offer recurring (WeChat has
// no auto-renewal — the backend rejects it, so the schema must stop it
// first). These branches are Demo-unreachable, so Vitest is the coverage.

async function fillCreateFormWechat(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByTestId('create-mapping-provider-select'))
  await user.click(await screen.findByRole('option', { name: 'WeChat Pay' }))

  await user.type(screen.getByTestId('create-mapping-external-product-id-input'), 'wx_prod_1')
  await user.type(screen.getByTestId('create-mapping-entitlement-key-input'), 'wechat-pro')

  await user.click(screen.getByTestId('create-mapping-billing-type-select'))
  await user.click(await screen.findByRole('option', { name: /non-renewing/i }))
  await user.type(screen.getByTestId('create-mapping-service-duration-days-input'), '30')

  // Currency is prefilled CNY by the defaults; type the manual price.
  await user.type(screen.getByTestId('create-mapping-price-input'), '19.9')
}

describe('CreateEntitlementMappingPage — WeChat provider-aware form', () => {
  it('shows manual price/currency + notice and hides the catalog price id for WeChat', async () => {
    const user = userEvent.setup()
    renderPage()

    await user.click(screen.getByTestId('create-mapping-provider-select'))
    await user.click(await screen.findByRole('option', { name: 'WeChat Pay' }))

    // Manual price fields + the scenes notice appear.
    expect(screen.getByTestId('create-mapping-price-input')).toBeInTheDocument()
    expect(screen.getByTestId('create-mapping-currency-input')).toBeInTheDocument()
    expect(screen.getByTestId('create-mapping-currency-input')).toHaveValue('CNY')
    expect(screen.getByTestId('create-mapping-wechat-notice')).toBeInTheDocument()

    // No hosted catalog ⇒ no external price id field.
    expect(screen.queryByTestId('create-mapping-external-price-id-input')).toBeNull()

    // No auto-renewal in scope ⇒ recurring must not be offered at all.
    await user.click(screen.getByTestId('create-mapping-billing-type-select'))
    expect(screen.queryByRole('option', { name: /^recurring$/i })).toBeNull()
    expect(await screen.findByRole('option', { name: /non-renewing/i })).toBeInTheDocument()
  })

  it('keeps the external price id for catalog providers (stripe regression)', async () => {
    const user = userEvent.setup()
    renderPage()

    await user.click(screen.getByTestId('create-mapping-provider-select'))
    await user.click(await screen.findByRole('option', { name: 'Stripe' }))

    expect(screen.getByTestId('create-mapping-external-price-id-input')).toBeInTheDocument()
    expect(screen.queryByTestId('create-mapping-price-input')).toBeNull()
    expect(screen.queryByTestId('create-mapping-wechat-notice')).toBeNull()
  })

  it('converts the major-unit price to integer minor units in the payload', async () => {
    mockCreateMutate.mockImplementation(
      (_req: unknown, opts: { onSuccess?: () => void; onError?: (error: unknown) => void }) => {
        opts.onSuccess?.()
      }
    )

    const user = userEvent.setup()
    renderPage()

    await fillCreateFormWechat(user)
    await user.click(screen.getByTestId('create-mapping-submit-button'))

    await waitFor(() => {
      expect(mockCreateMutate).toHaveBeenCalledTimes(1)
    })
    const body = mockCreateMutate.mock.calls[0]?.[0] as Record<string, unknown>
    // 19.9 yuan → 1990 fen (string-split parsing, no float drift).
    expect(body.price).toBe(1990)
    expect(body.currency).toBe('CNY')
    expect(body.paymentProvider).toBe('wechat')
  })

  it('blocks submit when the manual price is missing', async () => {
    const user = userEvent.setup()
    renderPage()

    await fillCreateFormWechat(user)
    await user.clear(screen.getByTestId('create-mapping-price-input'))
    await user.click(screen.getByTestId('create-mapping-submit-button'))

    expect(mockCreateMutate).not.toHaveBeenCalled()
    await waitFor(() => {
      expect(screen.getAllByRole('alert').length).toBeGreaterThan(0)
    })
  })

  it('resets a recurring billing type when switching the provider to WeChat', async () => {
    const user = userEvent.setup()
    renderPage()

    // Stripe + recurring first (a fully legal catalog combination).
    await user.click(screen.getByTestId('create-mapping-provider-select'))
    await user.click(await screen.findByRole('option', { name: 'Stripe' }))
    await user.click(screen.getByTestId('create-mapping-billing-type-select'))
    await user.click(await screen.findByRole('option', { name: /^recurring$/i }))

    // Switching to WeChat must drop the now-unreachable recurring selection.
    await user.click(screen.getByTestId('create-mapping-provider-select'))
    await user.click(await screen.findByRole('option', { name: 'WeChat Pay' }))

    const trigger = screen.getByTestId('create-mapping-billing-type-select')
    await waitFor(() => {
      expect(trigger).toHaveTextContent(
        String(m['billing.create_mapping_billing_type_placeholder']())
      )
    })
  })
})
