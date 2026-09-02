/**
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { toast } from 'sonner'
import { server } from '@/test/mocks/server'
import {
  createGrantPointsErrorHandler,
  createUserSearchEmptyHandler,
  userSearchHandler,
} from '@/test/mocks/handlers/points'
import { GrantPointsDialog } from '../grant-points-dialog'

// Mock usePermission -- dialog requires points.manage for full UI
vi.mock('@/hooks/use-permission', () => ({
  usePermission: vi.fn(),
}))

import { usePermission } from '@/hooks/use-permission'

// ---------- Helpers ----------

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  })
}

function renderDialog(
  overrides: {
    open?: boolean
    onOpenChange?: (open: boolean) => void
    realmId?: string
    queryClient?: QueryClient
  } = {}
) {
  const onOpenChange = overrides.onOpenChange ?? vi.fn()
  const qc = overrides.queryClient ?? createTestQueryClient()

  const result = render(
    <QueryClientProvider client={qc}>
      <GrantPointsDialog
        open={overrides.open ?? true}
        onOpenChange={onOpenChange}
        realmId={overrides.realmId ?? 'test-realm'}
      />
    </QueryClientProvider>
  )

  return { ...result, onOpenChange, queryClient: qc }
}

/** Mock usePermission to grant points.manage by default */
function mockPermissionGranted() {
  vi.mocked(usePermission).mockReturnValue({
    hasPermission: (p: string) => p === 'points.manage',
    hasAnyPermission: vi.fn(() => true),
    hasAllPermissions: vi.fn(() => true),
    hasRole: vi.fn(() => false),
    hasAnyRole: vi.fn(() => false),
    hasAdminPermission: false,
    permissions: ['points.manage'],
    roles: [],
    isLoading: false,
  })
}

/** Mock usePermission to deny points.manage */
function mockPermissionDenied() {
  vi.mocked(usePermission).mockReturnValue({
    hasPermission: () => false,
    hasAnyPermission: vi.fn(() => false),
    hasAllPermissions: vi.fn(() => false),
    hasRole: vi.fn(() => false),
    hasAnyRole: vi.fn(() => false),
    hasAdminPermission: false,
    permissions: [],
    roles: [],
    isLoading: false,
  })
}

/** Helper to fill a valid form for submission tests */
async function fillValidForm() {
  const user = userEvent.setup()

  // Type in the user search to trigger search
  const searchInput = screen.getByTestId('users-search-input')
  await user.type(searchInput, 'alice@example.com')

  // Wait for search results and select user
  const aliceButton = await screen.findByRole('button', { name: /alice@example.com/i })
  await user.click(aliceButton)

  // Select the required target bucket (no default; options from useEnabledBuckets)
  await user.click(screen.getByTestId('grant-points-bucket-select'))
  const bucketOption = await screen.findByRole('option', { name: /default bucket/i })
  await user.click(bucketOption)

  // Amount defaults to 1 which is valid -- no need to change it

  // Fill reason
  const reasonInput = screen.getByTestId('grant-points-reason-input')
  await user.type(reasonInput, 'Promotional grant')

  return user
}

// ---------- Tests ----------
// grantPointsSchema is covered in src/lib/schemas/__tests__/points-forms.test.ts
// (with intent docs: bucketId required business error, UUID-grammar boundary).

describe('GrantPointsDialog', () => {
  beforeEach(() => {
    mockPermissionGranted()
    server.use(userSearchHandler)
    // Provide an enabled credit bucket so the mandatory Target bucket Select
    // has a selectable option. The list endpoint returns a bare BucketResponse[]
    // (creditBucketsListQueryOptions casts response.data as BucketResponse[]).
    server.use(
      http.get('http://localhost:3000/api/realms/:realmId/billing/credit-buckets', () =>
        HttpResponse.json([
          {
            id: 'bucket-1',
            name: 'Default Bucket',
            bucketKey: 'default',
            enabled: true,
            coveredClientAppCount: 0,
            displayOrder: 0,
            entitlementMappingCount: 0,
            ruleReferenceCount: 0,
          },
        ])
      )
    )
  })

  describe('form validation', () => {
    it('shows validation errors when submitting empty form', async () => {
      renderDialog()

      const submitButton = screen.getByTestId('grant-points-submit-button')
      await userEvent.click(submitButton)

      // Form should not proceed to confirmation without required fields
      await waitFor(() => {
        expect(screen.queryByTestId('grant-points-confirm-dialog')).not.toBeInTheDocument()
      })
    })
  })

  describe('permanent validity toggle', () => {
    it('starts with permanent toggle checked and validity days input disabled', () => {
      renderDialog()

      const permanentToggle = screen.getByTestId('grant-points-permanent-toggle')
      expect(permanentToggle).toBeChecked()

      const validityInput = screen.getByTestId('grant-points-validity-days-input')
      expect(validityInput).toBeDisabled()
    })
  })

  describe('user search', () => {
    it('shows loading state while searching', async () => {
      const user = userEvent.setup()
      // Use a delayed handler to ensure loading state is visible
      server.use(
        http.get('http://localhost:3000/api/users/:realmId', async () => {
          await new Promise((resolve) => setTimeout(resolve, 200))
          return HttpResponse.json({
            items: [
              {
                id: 'user-1',
                email: 'alice@example.com',
                nickname: 'Alice',
                realmId: 'test-realm',
                status: 1,
                createdAt: '2026-01-01T00:00:00Z',
              },
            ],
            page: 0,
            pageSize: 20,
            total: 1,
          })
        })
      )

      renderDialog()

      const searchInput = screen.getByTestId('users-search-input')
      await user.type(searchInput, 'alice')

      // Should show loading indicator
      expect(await screen.findByText(/searching/i)).toBeInTheDocument()
    })

    it('displays search results', async () => {
      renderDialog()
      const user = userEvent.setup()

      const searchInput = screen.getByTestId('users-search-input')
      await user.type(searchInput, 'alice')

      expect(
        await screen.findByText(/alice@example.com/i, {}, { timeout: 3000 })
      ).toBeInTheDocument()
    })

    it('shows "No users found" when search returns empty', async () => {
      server.use(createUserSearchEmptyHandler())
      renderDialog()
      const user = userEvent.setup()

      const searchInput = screen.getByTestId('users-search-input')
      await user.type(searchInput, 'nonexistent')

      expect(await screen.findByText(/no users found/i)).toBeInTheDocument()
    })

    it('selects a user from search results', async () => {
      renderDialog()
      const user = userEvent.setup()

      const searchInput = screen.getByTestId('users-search-input')
      await user.type(searchInput, 'alice')

      const aliceButton = await screen.findByRole('button', { name: /alice@example.com/i })
      await user.click(aliceButton)

      // Selected user shown with email and Change button
      expect(screen.getByText('alice@example.com')).toBeInTheDocument()
      expect(screen.getByRole('button', { name: /change/i })).toBeInTheDocument()
    })

    it('does not fire query when input is empty', async () => {
      let queryMade = false
      server.use(
        http.get('http://localhost:3000/api/users/:realmId', () => {
          queryMade = true
          return HttpResponse.json({ items: [], page: 0, pageSize: 20, total: 0 })
        })
      )

      renderDialog()

      // Wait a moment -- no query should fire for empty input
      await new Promise((resolve) => setTimeout(resolve, 100))
      expect(queryMade).toBe(false)
    })
  })

  describe('confirmation flow', () => {
    it('opens confirmation dialog when "Review Grant" is clicked with valid form', async () => {
      renderDialog()
      await fillValidForm()

      const submitButton = screen.getByTestId('grant-points-submit-button')
      await userEvent.click(submitButton)

      const confirmDialog = await screen.findByTestId('grant-points-confirm-dialog')
      expect(confirmDialog).toBeInTheDocument()
      // Confirm dialog contains the title
      expect(confirmDialog).toHaveTextContent('Confirm Grant')
      // Summary shows user email
      expect(confirmDialog).toHaveTextContent('alice@example.com')
      // Summary shows reason
      expect(confirmDialog).toHaveTextContent('Promotional grant')
    })

    it('triggers mutation when confirm is clicked', async () => {
      renderDialog()
      const user = await fillValidForm()

      const submitButton = screen.getByTestId('grant-points-submit-button')
      await user.click(submitButton)

      const confirmButton = await screen.findByTestId('grant-points-confirm-button')
      await user.click(confirmButton)

      // Mutation should fire -- verify toast is called on success
      await waitFor(() => {
        expect(toast.success).toHaveBeenCalled()
      })
    })

    it('returns to form when cancel is clicked in confirmation dialog', async () => {
      renderDialog()
      const user = await fillValidForm()

      const submitButton = screen.getByTestId('grant-points-submit-button')
      await user.click(submitButton)

      // Confirmation dialog is open
      expect(screen.getByTestId('grant-points-confirm-dialog')).toBeInTheDocument()

      // Click Cancel in confirmation dialog (outline button before Confirm button)
      const cancelButtons = screen.getAllByRole('button', { name: /cancel/i })
      // The confirmation dialog cancel button
      const confirmDialogCancelButton = cancelButtons[cancelButtons.length - 1]
      await user.click(confirmDialogCancelButton)

      // Confirmation dialog closes, form dialog stays open
      await waitFor(() => {
        expect(screen.queryByTestId('grant-points-confirm-dialog')).not.toBeInTheDocument()
      })
      expect(screen.getByTestId('grant-points-form-dialog')).toBeInTheDocument()
    })
  })

  describe('successful submission', () => {
    it('calls toast.success and onOpenChange(false)', async () => {
      const onOpenChange = vi.fn()
      renderDialog({ onOpenChange })
      const user = await fillValidForm()

      const submitButton = screen.getByTestId('grant-points-submit-button')
      await user.click(submitButton)

      const confirmButton = await screen.findByTestId('grant-points-confirm-button')
      await user.click(confirmButton)

      await waitFor(() => {
        expect(toast.success).toHaveBeenCalledWith(expect.stringContaining('Successfully granted'))
      })
      expect(onOpenChange).toHaveBeenCalledWith(false)
    })
  })

  describe('server error handling', () => {
    it.each([
      { status: 403, message: 'Insufficient permissions' },
      { status: 500, message: 'Internal server error' },
    ])(
      'shows inline error alert for $status and keeps dialog open',
      async ({ status, message }) => {
        server.use(createGrantPointsErrorHandler(status, message))
        renderDialog()
        const user = await fillValidForm()

        const submitButton = screen.getByTestId('grant-points-submit-button')
        await user.click(submitButton)

        const confirmButton = await screen.findByTestId('grant-points-confirm-button')
        await user.click(confirmButton)

        // Error alert should appear in the form dialog
        const errorAlert = await screen.findByTestId('grant-points-error-message')
        expect(errorAlert).toBeInTheDocument()

        // Dialog stays open
        expect(screen.getByTestId('grant-points-form-dialog')).toBeInTheDocument()
      }
    )
  })

  describe('permission gating', () => {
    it('shows permission denied message when user lacks points.manage', () => {
      mockPermissionDenied()
      renderDialog()

      expect(screen.getByText(/do not have permission to grant points/i)).toBeInTheDocument()
    })
  })
})
