/**
 * ClientAppFormPage Turnstile fields component test (FE-D03, design §4.3.2 /
 * §5.2 / §8 D-PROTECT-01).
 *
 * Mirrors `passkey-config-form.test.tsx` / `email-otp-login-form.test.tsx`:
 * the generated SDK functions `createClientApp` / `updateClientApp` are mocked
 * with `vi.mock`. Covers the branches the route/Demo cannot cheaply reach:
 *   - create mode renders the three Turnstile fields with defaults off/empty
 *   - toggling `turnstileEnabled` reveals site/secret inputs; filling them
 *     composes the correct submit payload (secret included when non-empty)
 *   - create mode with an empty secret OMITS `turnstileSecretKey`
 *   - edit mode pre-fills `turnstileEnabled` + `turnstileSiteKey` from the
 *     `clientApp` prop but NEVER pre-fills the secret (write-only)
 *   - edit mode with an empty secret OMITS `turnstileSecretKey` (leaves the
 *     stored secret untouched)
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ClientAppFormPage } from '../client-app-form-page'
import type { ClientAppItem } from '@/lib/api-generated'

// --- Mocks ---------------------------------------------------------------

// `createClientApp` / `updateClientApp` are the only generated SDK functions
// the form invokes. Each returns the hey-api response shape `{ data, error }`.
const createClientAppMock = vi.fn()
const updateClientAppMock = vi.fn()

vi.mock('@/lib/api-generated', () => ({
  createClientApp: (...args: unknown[]) => createClientAppMock(...args),
  updateClientApp: (...args: unknown[]) => updateClientAppMock(...args),
}))

// The form uses TanStack Router's `useNavigate`. Mock it with a no-op so the
// component renders without a router provider (mirrors the email-otp test).
vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    useNavigate: () => vi.fn(),
  }
})

// The form derives its cancel/return path from `realmPath` +
// `useResolvedRealmContext`. Mock both to avoid depending on the URL/router.
vi.mock('@/lib/realm-routing', () => ({
  realmPath: (_ctx: unknown, path: string) => path,
  useResolvedRealmContext: () => ({ realmId: 'test-realm', mode: 'default' }),
}))

// --- Helpers -------------------------------------------------------------

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

const REALM_ID = 'test-realm'

function renderForm({ mode, clientApp }: { mode: 'create' | 'edit'; clientApp?: ClientAppItem }) {
  const queryClient = createTestQueryClient()
  return render(
    <QueryClientProvider client={queryClient}>
      <ClientAppFormPage mode={mode} realmId={REALM_ID} clientApp={clientApp} />
    </QueryClientProvider>
  )
}

// Minimal valid `ClientAppItem` for edit mode, override Turnstile fields per test.
function makeClientApp(overrides: Partial<ClientAppItem> = {}): ClientAppItem {
  return {
    id: 'app-uuid-1',
    realmId: REALM_ID,
    clientId: 'existing-client',
    name: 'Existing App',
    description: null,
    redirectUris: ['https://app.example.com/callback'],
    iconUrl: null,
    enabled: true,
    browserRefreshAbsoluteTtlSeconds: 2592000,
    allowedOrigins: [],
    deviceCodeGrantEnabled: false,
    isFirstParty: true,
    turnstileEnabled: false,
    turnstileSiteKey: null,
    ...overrides,
  }
}

// Fill the minimal required fields so the Zod schema accepts the submit.
// The redirect URIs live on their own tab (`redirect-uris`), so we switch to
// it, add one URI via the RedirectUrisInput (input testid
// `redirect-uris-input-field`, add button `redirect-uris-input-add-button`),
// then return to the Basic tab context.
async function fillRequiredCreateFields(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByTestId('client-id-input'), 'turnstile-app')
  await user.type(screen.getByTestId('client-app-name-input'), 'Turnstile App')
  await user.click(screen.getByTestId('tab-redirect-uris'))
  await user.type(
    screen.getByTestId('redirect-uris-input-field') as HTMLInputElement,
    'https://app.example.com/callback'
  )
  await user.click(screen.getByTestId('redirect-uris-input-add-button'))
}

// Switch to the Security tab to reach the Turnstile controls.
async function openSecurityTab(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByTestId('tab-security'))
}

describe('ClientAppFormPage — Turnstile fields (FE-D03)', () => {
  const user = userEvent.setup({ delay: null })

  beforeEach(() => {
    createClientAppMock.mockReset()
    updateClientAppMock.mockReset()
    createClientAppMock.mockResolvedValue({ data: { id: 'new-id' }, error: undefined })
    updateClientAppMock.mockResolvedValue({ data: { id: 'app-uuid-1' }, error: undefined })
  })

  it('create mode renders the Turnstile switch off by default and hides site/secret inputs', async () => {
    renderForm({ mode: 'create' })
    await openSecurityTab(user)

    const enabledSwitch = screen.getByTestId('client-app-turnstile-enabled-switch')
    expect(enabledSwitch).not.toBeChecked()
    // Site/secret inputs are only rendered when Turnstile is enabled.
    expect(screen.queryByTestId('client-app-turnstile-site-key-input')).not.toBeInTheDocument()
    expect(screen.queryByTestId('client-app-turnstile-secret-key-input')).not.toBeInTheDocument()
  })

  it('create mode: enabling Turnstile + filling site/secret composes a payload with all three fields', async () => {
    renderForm({ mode: 'create' })

    await fillRequiredCreateFields(user)
    await openSecurityTab(user)

    await user.click(screen.getByTestId('client-app-turnstile-enabled-switch'))
    // Revealing is driven by form state via form.Subscribe.
    const siteInput = (await screen.findByTestId(
      'client-app-turnstile-site-key-input'
    )) as HTMLInputElement
    const secretInput = (await screen.findByTestId(
      'client-app-turnstile-secret-key-input'
    )) as HTMLInputElement
    await user.type(siteInput, 'site-key-abc')
    await user.type(secretInput, 'secret-key-xyz')

    await user.click(screen.getByTestId('submit-button'))

    await waitFor(() => {
      expect(createClientAppMock).toHaveBeenCalledTimes(1)
    })
    const call = createClientAppMock.mock.calls[0][0] as {
      body: Record<string, unknown>
    }
    expect(call.body.turnstileEnabled).toBe(true)
    expect(call.body.turnstileSiteKey).toBe('site-key-abc')
    expect(call.body.turnstileSecretKey).toBe('secret-key-xyz')
  })

  it('create mode: an empty secret OMITS turnstileSecretKey from the payload', async () => {
    renderForm({ mode: 'create' })

    await fillRequiredCreateFields(user)
    await openSecurityTab(user)

    // Enable Turnstile but leave the secret blank, set a site key only.
    await user.click(screen.getByTestId('client-app-turnstile-enabled-switch'))
    const siteInput = (await screen.findByTestId(
      'client-app-turnstile-site-key-input'
    )) as HTMLInputElement
    await user.type(siteInput, 'site-key-abc')
    // Leave the secret input empty (its default).

    await user.click(screen.getByTestId('submit-button'))

    await waitFor(() => {
      expect(createClientAppMock).toHaveBeenCalledTimes(1)
    })
    const body = (createClientAppMock.mock.calls[0][0] as { body: Record<string, unknown> }).body
    expect(body.turnstileEnabled).toBe(true)
    expect(body.turnstileSiteKey).toBe('site-key-abc')
    // Secret must not be sent when empty (write-only: empty ⇒ not set).
    expect(body).not.toHaveProperty('turnstileSecretKey')
  })

  it('edit mode: pre-fills turnstileEnabled + turnstileSiteKey from the clientApp prop but NEVER pre-fills the secret', async () => {
    const clientApp = makeClientApp({
      turnstileEnabled: true,
      turnstileSiteKey: 'stored-site-key',
    })
    renderForm({ mode: 'edit', clientApp })
    await openSecurityTab(user)

    const enabledSwitch = await screen.findByTestId('client-app-turnstile-enabled-switch')
    expect(enabledSwitch).toBeChecked()
    const siteInput = (await screen.findByTestId(
      'client-app-turnstile-site-key-input'
    )) as HTMLInputElement
    expect(siteInput.value).toBe('stored-site-key')
    // The secret input is empty by default — ClientAppItem omits the secret
    // (write-only), so the form must never echo a stored secret back.
    const secretInput = screen.getByTestId(
      'client-app-turnstile-secret-key-input'
    ) as HTMLInputElement
    expect(secretInput.value).toBe('')
  })

  it('edit mode: an empty secret OMITS turnstileSecretKey so the stored secret is left untouched', async () => {
    const clientApp = makeClientApp({
      turnstileEnabled: true,
      turnstileSiteKey: 'stored-site-key',
    })
    renderForm({ mode: 'edit', clientApp })
    await openSecurityTab(user)

    // Turnstile is enabled (from prop); leave the secret input empty.
    await screen.findByTestId('client-app-turnstile-secret-key-input')
    await user.click(screen.getByTestId('submit-button'))

    await waitFor(() => {
      expect(updateClientAppMock).toHaveBeenCalledTimes(1)
    })
    const call = updateClientAppMock.mock.calls[0][0] as {
      path: { clientAppId?: string }
      body: Record<string, unknown>
    }
    expect(call.path.clientAppId).toBe('app-uuid-1')
    expect(call.body.turnstileEnabled).toBe(true)
    expect(call.body.turnstileSiteKey).toBe('stored-site-key')
    // Empty secret ⇒ omit, so the server leaves the stored secret untouched.
    expect(call.body).not.toHaveProperty('turnstileSecretKey')
  })

  it('edit mode: typing a new secret INCLUDES turnstileSecretKey to replace the stored one', async () => {
    const clientApp = makeClientApp({
      turnstileEnabled: true,
      turnstileSiteKey: 'stored-site-key',
    })
    renderForm({ mode: 'edit', clientApp })
    await openSecurityTab(user)

    const secretInput = (await screen.findByTestId(
      'client-app-turnstile-secret-key-input'
    )) as HTMLInputElement
    await user.type(secretInput, 'new-secret-123')
    await user.click(screen.getByTestId('submit-button'))

    await waitFor(() => {
      expect(updateClientAppMock).toHaveBeenCalledTimes(1)
    })
    const body = (updateClientAppMock.mock.calls[0][0] as { body: Record<string, unknown> }).body
    expect(body.turnstileSecretKey).toBe('new-secret-123')
  })
})
