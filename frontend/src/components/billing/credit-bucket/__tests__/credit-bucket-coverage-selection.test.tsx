import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { creditBucketsHandlers } from '@/test/mocks/handlers/credit-buckets'
import { useAuthStore } from '@/stores/auth-store'
import { CreditBucketEditor } from '../credit-bucket-editor'

// ===== Test helpers (mirror credit-bucket-editor-create-persistence.test.tsx) =====

const API_BASE_URL = 'http://localhost:3000'

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  })
}

function withQueryClient(ui: React.ReactNode) {
  const client = makeQueryClient()
  return render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>)
}

/** Minimal ClientAppItem shape the editor consumes (id / name / clientId). */
function app(id: string, clientId: string, name: string) {
  return { id, clientId, name }
}

// The three clients every realm is seeded with (see realm::services).
const BUILT_IN_APPS = [
  app('id-console', 'admin-web-console', 'Admin Web Console'),
  app('id-account-center', 'user-account-center', 'User Account Center'),
  app('id-admin-api', 'admin-api-client', 'API Key Client'),
]

/**
 * Stateful client-apps store: GET returns the current list, POST appends and
 * echoes the created app (with a one-time clientSecret, like the backend).
 * NOTE: in `server.use(...)` the EARLIER handler wins on path conflicts, so
 * this must be registered before `creditBucketsHandlers` (whose default
 * client-apps GET returns an empty list).
 */
function useClientAppsHandler(initial: ReturnType<typeof app>[]) {
  const items = [...initial]
  return {
    handler: [
      http.get(`${API_BASE_URL}/api/client`, () =>
        HttpResponse.json({ items, page: 0, pageSize: 100, total: items.length })
      ),
      http.post(`${API_BASE_URL}/api/client`, async ({ request }) => {
        const body = (await request.json()) as { clientId: string; name: string }
        const created = {
          id: 'id-created',
          clientId: body.clientId,
          name: body.name,
          clientSecret: 'secret-123',
        }
        items.push(created)
        return HttpResponse.json(created, { status: 201 })
      }),
    ],
  }
}

// ===== Intent =====
//
// Coverage decides where a credit bucket's points are spendable. The built-in
// clients (admin-web-console / user-account-center / admin-api-client) are
// realm infrastructure, not customer apps — covering them would silently
// misroute billing. Coverage must therefore only ever reference self-created
// client apps, and a realm with none must be able to create one in place
// instead of leaving the bucket form.

describe('CreditBucketEditor — coverage client app selection', () => {
  beforeEach(() => {
    useAuthStore.setState({ permissions: [], roles: [] })
  })

  it('hides built-in client apps; only self-created apps are selectable', async () => {
    const user = userEvent.setup()
    server.use(
      ...useClientAppsHandler([...BUILT_IN_APPS, app('id-store', 'web-store', 'Web Store')])
        .handler,
      ...creditBucketsHandlers
    )

    withQueryClient(
      <CreditBucketEditor realmId="r1" bucket={null} formKey="new" onSaved={vi.fn()} />
    )

    await screen.findByTestId('credit-bucket-editor-name')

    // Without clients.manage the inline create entry is hidden.
    expect(screen.queryByTestId('bucket-coverage-create-client-app')).not.toBeInTheDocument()

    // The multiselect renders once the client-apps query resolves.
    const trigger = await screen.findByTestId('bucket-coverage-multiselect')
    await user.click(trigger)

    expect(
      await screen.findByTestId('bucket-coverage-multiselect-item-id-store')
    ).toBeInTheDocument()
    for (const builtIn of BUILT_IN_APPS) {
      expect(
        screen.queryByTestId(`bucket-coverage-multiselect-item-${builtIn.id}`)
      ).not.toBeInTheDocument()
    }

    await user.click(screen.getByTestId('bucket-coverage-multiselect-item-id-store'))
    expect(screen.getByTestId('bucket-coverage-multiselect').textContent).toContain('Web Store')
  })

  it('create entry creates a client app inline and auto-selects it for coverage', async () => {
    useAuthStore.setState({ permissions: ['clients.manage'] })
    const user = userEvent.setup()
    server.use(
      // Realm has ONLY built-in apps: nothing selectable without creating one.
      ...useClientAppsHandler([...BUILT_IN_APPS]).handler,
      ...creditBucketsHandlers,
      http.post(`${API_BASE_URL}/api/bill/credit-buckets`, () =>
        HttpResponse.json({ id: 'b-new' }, { status: 201 })
      )
    )

    withQueryClient(
      <CreditBucketEditor realmId="r1" bucket={null} formKey="new" onSaved={vi.fn()} />
    )

    // Progress on the bucket form first — the inline create must not lose it.
    const nameInput = await screen.findByTestId('credit-bucket-editor-name')
    await user.type(nameInput, 'Seasonal Promo')

    await user.click(screen.getByTestId('bucket-coverage-create-client-app'))

    await user.type(await screen.findByTestId('bucket-create-client-app-client-id'), 'storefront')
    await user.type(screen.getByTestId('bucket-create-client-app-name'), 'Storefront')
    await user.type(
      screen.getByTestId('bucket-create-client-app-redirect-uris-field'),
      'https://app.example.com/callback'
    )
    await user.click(screen.getByTestId('bucket-create-client-app-redirect-uris-add-button'))
    await user.click(screen.getByTestId('bucket-create-client-app-submit'))

    // The created app lands in the coverage selection, resolved by label —
    // proof the invalidation refetch picked it up and auto-selection ran.
    await waitFor(() => {
      expect(screen.getByTestId('bucket-coverage-multiselect').textContent).toContain('Storefront')
    })

    // Bucket form state survived the inline creation (no navigation happened).
    expect((nameInput as HTMLInputElement).value).toBe('Seasonal Promo')
  })
})
