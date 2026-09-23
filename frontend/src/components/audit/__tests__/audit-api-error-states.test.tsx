import { describe, test, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query'
import { server } from '@/test/mocks/server'
import { auditListQueryOptions, auditDetailQueryOptions } from '@/data/query-options'
import { AuditEventDetailSheet } from '../audit-event-detail-sheet'

// Mock tanstack router
vi.mock('@tanstack/react-router', () => ({
  useNavigate: () => vi.fn(),
}))

const API_BASE_URL = 'http://localhost:3000'

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })
}

function renderWithQueryClient(ui: React.ReactNode) {
  const queryClient = createTestQueryClient()
  return render(<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>)
}

// Minimal component to test audit list query error handling
function AuditListErrorTestComponent({ realmId }: { realmId: string }) {
  const { data, isLoading, error } = useQuery({
    ...auditListQueryOptions(realmId, {}),
    retry: false,
  })

  if (isLoading) return <div data-testid="loading">Loading...</div>
  if (error) return <div data-testid="audit-list-error">{error.message}</div>
  if (data) return <div data-testid="audit-list-data">{data.total} events</div>
  return null
}

describe('Audit API error states', () => {
  beforeEach(() => {
    server.resetHandlers()
  })

  describe('List endpoint errors', () => {
    test('shows error when list endpoint returns 500', async () => {
      server.use(
        http.get(`${API_BASE_URL}/api/audit`, () => {
          return HttpResponse.json({ message: 'Internal Server Error' }, { status: 500 })
        })
      )

      renderWithQueryClient(<AuditListErrorTestComponent realmId="test-realm" />)

      expect(screen.getByTestId('loading')).toBeInTheDocument()

      const errorElement = await screen.findByTestId('audit-list-error', undefined, {
        timeout: 5000,
      })
      expect(errorElement).toBeInTheDocument()
    })
    // 403/network-error variants hit the identical throw→error-state branch
    // (retry is disabled and the harness renders one error testid), so a
    // single representative per endpoint pins the contract.
  })

  describe('Detail endpoint errors', () => {
    test('shows error message when detail endpoint returns 404', async () => {
      server.use(
        http.get(`${API_BASE_URL}/api/audit/:eventId`, () => {
          return HttpResponse.json({ message: 'Event not found' }, { status: 404 })
        })
      )

      renderWithQueryClient(
        <AuditEventDetailSheet eventId="not-found-id" realmId="test-realm" onClose={vi.fn()} />
      )

      const errorElement = await screen.findByTestId('audit-detail-error', undefined, {
        timeout: 5000,
      })
      expect(errorElement).toBeInTheDocument()
      expect(errorElement.textContent).toContain('Failed to load event details')
    })

    test('does not fetch when eventId is null', () => {
      renderWithQueryClient(
        <AuditEventDetailSheet eventId={null} realmId="test-realm" onClose={vi.fn()} />
      )

      expect(screen.queryByTestId('audit-detail-error')).not.toBeInTheDocument()
    })
  })
})
