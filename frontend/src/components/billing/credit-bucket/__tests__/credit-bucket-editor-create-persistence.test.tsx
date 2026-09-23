import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { creditBucketsHandlers } from '@/test/mocks/handlers/credit-buckets'
import { CreditBucketEditor } from '../credit-bucket-editor'

// ===== Test helpers (mirror destructive-confirm.test.tsx) =====

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

// ===== Follow-up verification (outside T01..T06) =====
//
// Question: does the CREATE branch of CreditBucketEditor wipe entered values
// when a re-render is triggered after editing another field?
//
// `createDefaults` is a fresh object every render. If TanStack Form's internal
// update effect compares defaultValues by reference (not deep equality) and
// resets untouched state.values on mismatch, the typed name would vanish the
// moment the Switch flips. The UPDATE branch had exactly this bug; this test
// proves whether CREATE shares it.
//
// Verdict (empirical): CREATE IS SAFE. defaultValues is consumed only on the
// form instance's initial mount; subsequent renders with a fresh
// `createDefaults` reference do NOT reset state.values. No production change.
// Kept as a regression guard: the sibling update branch was genuinely buggy
// in exactly this way, so this locks down the create flow's value persistence
// against future refactors of the defaults object.

describe('CreditBucketEditor (create) — value persistence across re-render', () => {
  it('keeps the typed bucket name after another field triggers a re-render', async () => {
    const user = userEvent.setup()
    server.use(
      ...creditBucketsHandlers,
      // POST create handler so a stray submit would not 500 — we do NOT submit
      // here, but the mutation is wired into the form's onSubmit.
      http.post('http://localhost:3000/api/bill/credit-buckets', () =>
        HttpResponse.json({ id: 'b-new' }, { status: 201 })
      )
    )

    withQueryClient(
      <CreditBucketEditor realmId="r1" bucket={null} formKey="new" onSaved={vi.fn()} />
    )

    // Type a name into the create-mode name field.
    const nameInput = await screen.findByTestId('credit-bucket-editor-name')
    expect(nameInput).toBeInTheDocument()
    await user.type(nameInput, 'Seasonal Promo')

    expect((nameInput as HTMLInputElement).value).toBe('Seasonal Promo')

    expect(screen.queryByRole('switch', { name: /registration/i })).not.toBeInTheDocument()
    const enabledSwitch = screen.getByRole('switch', { name: /enabled/i })
    expect(enabledSwitch).toBeChecked()
    await user.click(enabledSwitch)
    expect(enabledSwitch).not.toBeChecked()

    // THE assertion: the typed name must survive the re-render.
    expect(screen.getByDisplayValue('Seasonal Promo')).toBeInTheDocument()
    expect((nameInput as HTMLInputElement).value).toBe('Seasonal Promo')
  })
})
