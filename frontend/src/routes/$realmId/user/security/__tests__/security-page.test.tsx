import React from 'react'
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

const mockNavigate = vi.fn()

vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    createFileRoute: () => (config: Record<string, unknown>) => ({
      useParams: () => ({ realmId: 'test-realm' }),
      ...config,
    }),
    useNavigate: () => mockNavigate,
  }
})

vi.mock('@/components/profile/change-password-form', () => ({
  ChangePasswordForm: () => <div data-testid="change-password-form">Change Password Form</div>,
}))

vi.mock('@/components/profile/totp/totp-status-card', () => ({
  TotpStatusCard: () => <div data-testid="totp-status-card">TOTP Status Card</div>,
}))

vi.mock('@/components/profile/totp/totp-disable-form', () => ({
  TotpDisableForm: () => <div data-testid="totp-disable-form">TOTP Disable Form</div>,
}))

vi.mock('@/components/profile/totp/totp-regenerate-form', () => ({
  TotpRegenerateForm: () => <div data-testid="totp-regenerate-form">TOTP Regenerate Form</div>,
}))

vi.mock('@/components/profile/passkey/passkey-list', () => ({
  PasskeyList: () => <div data-testid="passkey-list">Passkey List</div>,
}))

vi.mock('@/components/profile/passkey/passkey-register-form', () => ({
  PasskeyRegisterForm: () => <div data-testid="passkey-register-form">Passkey Register Form</div>,
}))

vi.mock('@/components/ui/tabs', () => ({
  Tabs: ({ children }: { children: React.ReactNode }) => <div data-testid="tabs">{children}</div>,
  TabsList: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="tabs-list">{children}</div>
  ),
  TabsTrigger: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="tabs-trigger">{children}</div>
  ),
  TabsContent: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="tabs-content">{children}</div>
  ),
}))

vi.mock('@/lib/api-generated/sdk.gen', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-generated/sdk.gen')>()
  return {
    ...original,
    deleteAccount: vi.fn(),
  }
})

// Stub the user feature-availability query so the page never issues a real
// network call. Each test seeds its desired feature flags via the module
// override below.
let mockFeatureFlags = { passkeyEnabled: true, totpEnabled: true }
vi.mock('@/data/query-options', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/data/query-options')>()
  return {
    ...actual,
    userFeatureAvailabilityQueryOptions: {
      queryKey: ['user-feature-availability', 'test'],
      queryFn: async () => ({
        user: {
          ...mockFeatureFlags,
          pointsVisible: false,
          subscriptionVisible: false,
          invoicesVisible: false,
        },
        invoiceEligibility: {},
      }),
    },
  }
})

import { ProfileSecurity } from '../index'

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

// Feature flags default to enabled to preserve the pre-existing assertions
// that expect the passkey/totp tabs to be present.
function renderSecurityPage(flags: { passkeyEnabled?: boolean; totpEnabled?: boolean } = {}) {
  mockFeatureFlags = { passkeyEnabled: true, totpEnabled: true, ...flags }
  const queryClient = createTestQueryClient()
  return render(
    <QueryClientProvider client={queryClient}>
      <ProfileSecurity />
    </QueryClientProvider>
  )
}

describe('ProfileSecurity', () => {
  it('GIVEN passkey enabled for realm THEN renders the passkey management UI', async () => {
    renderSecurityPage({ passkeyEnabled: true })

    expect(await screen.findByTestId('passkey-list')).toBeInTheDocument()
  })

  it('GIVEN passkey disabled for realm THEN hides the passkey management UI', async () => {
    renderSecurityPage({ passkeyEnabled: false })
    // Wait for the query to settle so the gating has applied.
    await screen.findByTestId('tabs')

    expect(screen.queryByTestId('passkey-list')).not.toBeInTheDocument()
  })

  it('GIVEN totp enabled for realm THEN renders the totp status card', async () => {
    renderSecurityPage({ totpEnabled: true })

    expect(await screen.findByTestId('totp-status-card')).toBeInTheDocument()
  })

  it('GIVEN totp disabled for realm THEN hides the totp status card', async () => {
    renderSecurityPage({ totpEnabled: false })
    // Wait for the query to settle so the gating has applied.
    await screen.findByTestId('tabs')

    expect(screen.queryByTestId('totp-status-card')).not.toBeInTheDocument()
  })
})
