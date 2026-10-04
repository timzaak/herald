import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { TransactionHistoryTable } from '../TransactionHistoryTable'
import {
  mockRechargeTransaction,
  mockConsumeTransaction,
} from '@/test/fixtures/points-transaction.fixture'
import type { TransactionFilters } from '@/lib/schemas/points-forms'

describe('TransactionHistoryTable', () => {
  afterEach(() => {
    vi.clearAllMocks()
  })

  const mockOnFiltersChange = vi.fn()
  const mockFilters: TransactionFilters = {
    transaction_type: undefined,
    start_time: undefined,
    end_time: undefined,
    client_app_id: undefined,
  }
  const mockClientApps = [{ id: 'app-123', name: 'Test App' }]

  describe('empty state', () => {
    it('GIVEN no transactions WHEN rendering THEN should show empty state', () => {
      render(<TransactionHistoryTable transactions={[]} filters={mockFilters} />)

      expect(screen.getByTestId('no-transactions')).toBeInTheDocument()
      expect(screen.getByText('No transactions found matching your criteria')).toBeInTheDocument()
    })

    it('GIVEN no transactions with active filters WHEN rendering THEN should show filter hint', () => {
      const activeFilters: TransactionFilters = {
        transactionType: 'recharge',
      }
      render(<TransactionHistoryTable transactions={[]} filters={activeFilters} />)

      expect(screen.getByText('Try adjusting your filters')).toBeInTheDocument()
    })
  })

  describe('rendering transactions', () => {
    it('GIVEN recharge transaction WHEN rendering THEN should display with green color and plus sign', () => {
      render(
        <TransactionHistoryTable transactions={[mockRechargeTransaction]} filters={mockFilters} />
      )

      const amount = screen.getByTestId('transaction-amount-0')
      expect(amount).toBeInTheDocument()
      expect(amount).toHaveTextContent('+1,000')
      expect(amount).toHaveClass(/text-success/)
    })

    it('GIVEN consume transaction WHEN rendering THEN should display with red color and minus sign', () => {
      render(
        <TransactionHistoryTable transactions={[mockConsumeTransaction]} filters={mockFilters} />
      )

      const amount = screen.getByTestId('transaction-amount-0')
      expect(amount).toBeInTheDocument()
      expect(amount).toHaveTextContent('-500')
      expect(amount).toHaveClass(/text-destructive/)
    })

    it('GIVEN transaction without description WHEN rendering THEN should display dash', () => {
      const transactionWithoutDesc = {
        ...mockRechargeTransaction,
        description: null,
      }
      render(
        <TransactionHistoryTable transactions={[transactionWithoutDesc]} filters={mockFilters} />
      )

      const description = screen.getByTestId('transaction-description-0')
      expect(description).toHaveTextContent('-')
    })
  })

  describe('admin view', () => {
    it('GIVEN admin view WHEN rendering THEN should display client app column', () => {
      render(
        <TransactionHistoryTable
          transactions={[mockRechargeTransaction]}
          filters={mockFilters}
          admin={true}
          clientApps={mockClientApps}
        />
      )

      const client = screen.getByTestId('transaction-client-0')
      expect(client).toBeInTheDocument()
      expect(client).toHaveTextContent('Test App')
    })

    it('GIVEN admin view with unknown client app WHEN rendering THEN should display client app ID', () => {
      render(
        <TransactionHistoryTable
          transactions={[mockRechargeTransaction]}
          filters={mockFilters}
          admin={true}
          clientApps={[]}
        />
      )

      const client = screen.getByTestId('transaction-client-0')
      expect(client).toBeInTheDocument()
      expect(client).toHaveTextContent('app-123')
    })

    it('GIVEN admin view with no client app WHEN rendering THEN should display dash', () => {
      const transactionWithoutClient = {
        ...mockRechargeTransaction,
        clientAppId: null,
      }
      render(
        <TransactionHistoryTable
          transactions={[transactionWithoutClient]}
          filters={mockFilters}
          admin={true}
          clientApps={mockClientApps}
        />
      )

      const client = screen.getByTestId('transaction-client-0')
      expect(client).toHaveTextContent('-')
    })

    it('GIVEN non-admin view WHEN rendering THEN should not display client app column', () => {
      render(
        <TransactionHistoryTable
          transactions={[mockRechargeTransaction]}
          filters={mockFilters}
          admin={false}
          clientApps={mockClientApps}
        />
      )

      expect(screen.queryByTestId('transaction-client-0')).not.toBeInTheDocument()
    })
  })

  describe('loading state', () => {
    it('GIVEN loading is true WHEN rendering THEN should show loading skeleton', () => {
      render(<TransactionHistoryTable transactions={[]} loading={true} filters={mockFilters} />)

      expect(screen.queryByTestId('no-transactions')).not.toBeInTheDocument()
      expect(screen.queryByTestId('transaction-row-0')).not.toBeInTheDocument()
    })
  })

  describe('bucket column', () => {
    // Locks the fix for the TanStack Table v8 column-id / getValue mismatch:
    // the bucket column declares `id: 'bucket'` + `accessorKey: 'bucketId'`,
    // so the cell must NOT call `row.getValue('bucketId')` (which resolves to
    // no column and logs "Column with id 'bucketId' does not exist").
    const bucketId = 'bucket-aaaaaaaa1111'
    const txnWithBucket = {
      ...mockRechargeTransaction,
      bucketId,
    }
    const buckets = [{ id: bucketId, name: 'Primary Pool' }]

    it('GIVEN buckets provided WHEN a row matches a bucket THEN renders the bucket NAME', () => {
      const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {})
      render(
        <TransactionHistoryTable
          transactions={[txnWithBucket]}
          filters={mockFilters}
          buckets={buckets}
        />
      )

      const bucketCell = screen.getByTestId('transaction-bucket-0')
      expect(bucketCell).toHaveTextContent('Primary Pool')
      // Must NOT fall back to the UUID prefix when a name is resolvable.
      expect(bucketCell).not.toHaveTextContent(bucketId.slice(0, 8))
      // The v8 column-id mismatch surfaces as a console.error; assert silence.
      expect(consoleError).not.toHaveBeenCalled()
      const offending = consoleError.mock.calls.find((args) =>
        String(args[0] ?? '').includes("Column with id 'bucketId' does not exist")
      )
      expect(offending).toBeUndefined()
      consoleError.mockRestore()
    })

    it('GIVEN buckets provided WHEN a row has no matching bucket THEN falls back to bucketId prefix', () => {
      const orphanTxn = {
        ...mockRechargeTransaction,
        bucketId: 'orphan-bucket-1234',
      }
      render(
        <TransactionHistoryTable
          transactions={[orphanTxn]}
          filters={mockFilters}
          buckets={buckets}
        />
      )

      // Fallback mirrors the client-app fallback: first 8 chars of the id.
      expect(screen.getByTestId('transaction-bucket-0')).toHaveTextContent('orphan-b')
    })
  })
})
