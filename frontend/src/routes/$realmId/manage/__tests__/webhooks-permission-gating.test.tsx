/**
 * Webhooks list page permission gating test (US-WEP-001 scenario 4).
 *
 * "Entrypoint invisible, management denied" is the only user-facing surface
 * of the permission model: view gates the page (with the backend's 403 as
 * the second layer), manage gates destructive operations. Pins the frontend
 * half so a permission-string or gating regression is caught before the
 * backend layer ever sees the request.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { Suspense } from 'react'
import type { ReactNode } from 'react'

vi.mock('@/hooks/use-permission', () => ({
  usePermission: vi.fn(),
}))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => {
    const routeObj = {
      useParams: () => ({ realmId: 'test-realm' }),
      useSearch: () => ({ page: 0, pageSize: 20 }),
    }
    return (config: Record<string, unknown>) => ({ ...routeObj, ...config })
  },
  useNavigate: () => vi.fn(),
}))

vi.mock('@/data/query-options', () => ({
  webhooksQueryOptions: () => ({
    queryKey: ['webhooks', 'test-realm'],
    queryFn: () => Promise.resolve({ items: [], page: 0, pageSize: 20, total: 0 }),
  }),
  emailStatusQueryOptions: () => ({
    queryKey: ['email-status', 'test-realm'],
    queryFn: () => Promise.resolve({ configured: true }),
  }),
  queryKeys: {
    webhooksList: () => ['webhooks', 'test-realm'],
    webhook: (realmId: string, id: string) => ['webhook', realmId, id],
  },
}))

vi.mock('@/hooks/use-form-mutation', () => ({
  useFormMutation: () => ({
    mutate: vi.fn(() => Promise.resolve()),
    isSubmitting: false,
  }),
}))

vi.mock('@/hooks/use-dialog-state', () => ({
  useDialogManager: () => ({
    selectedItem: null,
    isOpen: false,
    open: vi.fn(),
    close: vi.fn(),
    onOpenChange: vi.fn(),
  }),
}))

vi.mock('@/components/webhook-subscription-table', () => ({
  WebhookSubscriptionTable: ({ canManage }: { canManage?: boolean }) => (
    <div data-testid="webhooks-table">
      <span data-testid="table-can-manage">{String(canManage ?? true)}</span>
    </div>
  ),
}))

vi.mock('@/components/ui/card', () => ({
  Card: ({ children }: { children: ReactNode }) => <div data-testid="card">{children}</div>,
  CardContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}))

vi.mock('@/components/shared', () => ({
  PageHeader: ({
    title,
    headingTestId,
    action,
  }: {
    title: string
    headingTestId?: string
    action?: { label: string; onClick: () => void; testId: string; icon?: ReactNode }
  }) => (
    <div data-testid="page-header">
      <h1 data-testid={headingTestId ?? 'heading'}>{title}</h1>
      {action && <button data-testid={action.testId}>{action.label}</button>}
    </div>
  ),
  ListPagination: () => <div data-testid="pagination" />,
  AccessDenied: ({ message }: { message?: string }) => (
    <div className="text-destructive">
      {message ?? 'Access denied: You do not have permission to view this page'}
    </div>
  ),
}))

import { usePermission } from '@/hooks/use-permission'
import { WebhooksPage } from '../webhooks.index'

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })
}

function renderPage() {
  const queryClient = createTestQueryClient()
  return render(
    <QueryClientProvider client={queryClient}>
      <Suspense fallback={<div data-testid="loading">Loading...</div>}>
        <WebhooksPage />
      </Suspense>
    </QueryClientProvider>
  )
}

function mockPermissions(permissions: string[]) {
  vi.mocked(usePermission).mockReturnValue({
    hasPermission: (permission: string) => permissions.includes(permission),
    hasAnyPermission: vi.fn(),
    hasAllPermissions: vi.fn(),
    hasRole: vi.fn(),
    hasAnyRole: vi.fn(),
    hasAdminPermission: false,
    permissions,
    roles: [],
    isLoading: false,
  })
}

describe('Webhooks page permission gating', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('shows access-denied when the operator lacks webhooks.view', async () => {
    mockPermissions([])

    renderPage()

    expect(await screen.findByText(/access denied/i)).toBeInTheDocument()
    expect(
      screen.getByText(/do not have permission to view webhook subscriptions/i)
    ).toBeInTheDocument()
    expect(screen.queryByTestId('webhooks-page')).not.toBeInTheDocument()
  })

  it('renders the page but withholds manage affordances with view only', async () => {
    mockPermissions(['webhooks.view'])

    renderPage()

    await waitFor(() => {
      expect(screen.getByTestId('webhooks-page')).toBeInTheDocument()
    })
    expect(screen.queryByText(/access denied/i)).not.toBeInTheDocument()
    expect(screen.queryByTestId('add-webhook-button')).not.toBeInTheDocument()
    expect(screen.getByTestId('table-can-manage')).toHaveTextContent('false')
  })

  it('renders the add button and enables row actions with view + manage', async () => {
    mockPermissions(['webhooks.view', 'webhooks.manage'])

    renderPage()

    await waitFor(() => {
      expect(screen.getByTestId('webhooks-page')).toBeInTheDocument()
    })
    expect(screen.getByTestId('add-webhook-button')).toBeInTheDocument()
    expect(screen.getByTestId('table-can-manage')).toHaveTextContent('true')
  })

  it('still denies the page with manage only (view is the page gate)', async () => {
    mockPermissions(['webhooks.manage'])

    renderPage()

    expect(await screen.findByText(/access denied/i)).toBeInTheDocument()
    expect(screen.queryByTestId('webhooks-page')).not.toBeInTheDocument()
  })
})
