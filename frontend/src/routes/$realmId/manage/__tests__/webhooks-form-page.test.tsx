/**
 * WebhookSubscriptionFormPage pre-validation test (US-WEP-001 scenario 2).
 *
 * The private/loopback inline rejection is the PRD's only named inline form
 * constraint, and it must fire BEFORE any network round-trip: the frontend
 * check is an early-reject optimization, so the matrix here also pins the
 * boundary in the other direction — a hostname that is not a literal private
 * address (DNS-resolvable private IPs are invisible to the browser) goes to
 * the server, which stays authoritative.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { WebhookSubscriptionFormPage } from '@/components/webhook-subscription-form-page'
import type { WebhookSubscriptionItem } from '@/lib/api-generated'
import { makeWebhookSubscription } from '@/test/fixtures/webhook-subscriptions'

const createWebhookSubscriptionMock = vi.fn()
const updateWebhookSubscriptionMock = vi.fn()

vi.mock('@/lib/api-generated', () => ({
  createWebhookSubscription: (...args: unknown[]) => createWebhookSubscriptionMock(...args),
  updateWebhookSubscription: (...args: unknown[]) => updateWebhookSubscriptionMock(...args),
}))

const navigateMock = vi.fn()

vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    useNavigate: () => navigateMock,
  }
})

vi.mock('@/lib/realm-routing', () => ({
  realmPath: (_ctx: unknown, path: string) => path,
  useResolvedRealmContext: () => ({ realmId: 'test-realm', mode: 'default' }),
}))

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

const REALM_ID = 'test-realm'

function renderForm({
  mode,
  webhook,
}: {
  mode: 'create' | 'edit'
  webhook?: WebhookSubscriptionItem
}) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <WebhookSubscriptionFormPage mode={mode} realmId={REALM_ID} webhook={webhook} />
    </QueryClientProvider>
  )
}

async function fillEndpointUrl(user: ReturnType<typeof userEvent.setup>, value: string) {
  // user-event treats [ and { as key-descriptor openers and escapes them by
  // doubling — http://[[::1]/ types the literal http://[::1]/.
  const escaped = value.replace(/[[{]/g, (c) => c + c)
  await user.type(screen.getByTestId('webhook-endpoint-url-input'), escaped)
}

async function checkCategory(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByTestId('webhook-category-checkbox-user-lifecycle'))
}

async function submit(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByTestId('webhook-save-button'))
}

describe('WebhookSubscriptionFormPage endpoint URL pre-validation', () => {
  const user = userEvent.setup({ delay: null })

  beforeEach(() => {
    createWebhookSubscriptionMock.mockReset()
    updateWebhookSubscriptionMock.mockReset()
    navigateMock.mockReset()
    createWebhookSubscriptionMock.mockResolvedValue({
      data: { id: 'new-sub', secret: 'whsec_newsecret', endpointUrl: 'https://example.com/cb' },
      error: undefined,
    })
    updateWebhookSubscriptionMock.mockResolvedValue({
      data: makeWebhookSubscription(),
      error: undefined,
    })
  })

  it.each([
    ['localhost'],
    ['http://127.0.0.1/webhooks'],
    ['http://192.168.1.1/webhooks'],
    ['http://10.0.0.1/webhooks'],
    ['http://172.16.0.1/webhooks'],
    ['http://169.254.169.254/webhooks'],
    ['http://[::1]/webhooks'],
    // ff00::/8 multicast: the backend rejects it at registration time, so the
    // inline check must mirror that or the operator only sees the 400 toast.
    ['http://[ff02::1]/webhooks'],
    ['ftp://example.com/webhooks'],
  ])('rejects %s inline without any request', async (url) => {
    renderForm({ mode: 'create' })

    await fillEndpointUrl(user, url)
    await checkCategory(user)
    await submit(user)

    expect(await screen.findByTestId('webhook-endpoint-url-error')).toBeInTheDocument()
    expect(screen.getByTestId('webhook-endpoint-url-error')).toHaveTextContent(
      /publicly reachable address/i
    )
    expect(createWebhookSubscriptionMock).not.toHaveBeenCalled()
  })

  it('submits a public https URL and forwards the exact payload', async () => {
    renderForm({ mode: 'create' })

    await fillEndpointUrl(user, 'https://example.com/webhooks/herald')
    await checkCategory(user)
    await submit(user)

    await waitFor(() => {
      expect(createWebhookSubscriptionMock).toHaveBeenCalledTimes(1)
    })
    expect(createWebhookSubscriptionMock).toHaveBeenCalledWith({
      body: {
        endpointUrl: 'https://example.com/webhooks/herald',
        eventCategories: ['user_lifecycle'],
      },
    })
  })

  it('lets a non-literal hostname through to the server (authoritative check)', async () => {
    renderForm({ mode: 'create' })

    await fillEndpointUrl(user, 'https://intranet.example.com/webhooks')
    await checkCategory(user)
    await submit(user)

    await waitFor(() => {
      expect(createWebhookSubscriptionMock).toHaveBeenCalledTimes(1)
    })
    expect(screen.queryByTestId('webhook-endpoint-url-error')).not.toBeInTheDocument()
  })

  it('rejects a zero-category selection inline without any request', async () => {
    renderForm({ mode: 'create' })

    await fillEndpointUrl(user, 'https://example.com/webhooks/herald')
    await submit(user)

    expect(await screen.findByTestId('webhook-categories-error')).toBeInTheDocument()
    expect(createWebhookSubscriptionMock).not.toHaveBeenCalled()
  })

  it('navigates to the reveal page carrying the one-time secret after create', async () => {
    renderForm({ mode: 'create' })

    await fillEndpointUrl(user, 'https://example.com/webhooks/herald')
    await checkCategory(user)
    await submit(user)

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledTimes(1)
    })
    const call = navigateMock.mock.calls[0][0]
    expect(call.to).toBe('/manage/webhooks/reveal')
    expect(call.state.secretData.secret).toBe('whsec_newsecret')
  })

  it('pre-fills endpoint URL and categories in edit mode and PUTs the replacement', async () => {
    renderForm({ mode: 'edit', webhook: makeWebhookSubscription() })

    const input = screen.getByTestId('webhook-endpoint-url-input') as HTMLInputElement
    expect(input).toHaveValue('https://example.com/webhooks/herald')
    expect(screen.getByTestId('webhook-category-checkbox-role-events')).toBeChecked()
    expect(screen.getByTestId('webhook-category-checkbox-user-lifecycle')).not.toBeChecked()

    await checkCategory(user)
    await submit(user)

    await waitFor(() => {
      expect(updateWebhookSubscriptionMock).toHaveBeenCalledTimes(1)
    })
    expect(updateWebhookSubscriptionMock).toHaveBeenCalledWith({
      path: { subscriptionId: 'sub-uuid-1' },
      body: {
        endpointUrl: 'https://example.com/webhooks/herald',
        eventCategories: ['role_events', 'user_lifecycle'],
      },
    })
  })
})
