import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { TransactionFilters } from '../TransactionFilters'
import type { TransactionFilters as TransactionFiltersType } from '@/lib/schemas/points-forms'

describe('TransactionFilters', () => {
  afterEach(() => {
    vi.clearAllMocks()
  })

  const mockOnChange = vi.fn()
  const mockOnClear = vi.fn()
  const mockClientApps = [
    { id: 'app-1', name: 'App 1' },
    { id: 'app-2', name: 'App 2' },
  ]

  const defaultFilters: TransactionFiltersType = {
    transactionType: undefined,
    startTime: undefined,
    endTime: undefined,
    clientAppId: undefined,
  }

  describe('rendering', () => {
    it('GIVEN admin is true WHEN rendering THEN should display client app filter', () => {
      render(
        <TransactionFilters
          filters={defaultFilters}
          onChange={mockOnChange}
          onClear={mockOnClear}
          admin={true}
          clientApps={mockClientApps}
        />
      )

      expect(screen.getByLabelText('Client App')).toBeInTheDocument()
    })

    it('GIVEN admin is false WHEN rendering THEN should not display client app filter', () => {
      render(
        <TransactionFilters
          filters={defaultFilters}
          onChange={mockOnChange}
          onClear={mockOnClear}
          admin={false}
          clientApps={mockClientApps}
        />
      )

      expect(screen.queryByLabelText('Client App')).not.toBeInTheDocument()
    })
  })

  describe('filter interactions', () => {
    it('GIVEN user selects transaction type WHEN applying THEN should call onChange with correct type', async () => {
      const user = userEvent.setup({ delay: null })
      render(
        <TransactionFilters
          filters={defaultFilters}
          onChange={mockOnChange}
          onClear={mockOnClear}
        />
      )

      // Click on the select trigger (combobox)
      const typeSelectTrigger = screen.getByRole('combobox', { name: /transaction type/i })
      await user.click(typeSelectTrigger)

      // Select recharge option using role for better specificity
      const rechargeOption = screen.getByRole('option', { name: 'Recharge' })
      await user.click(rechargeOption)

      const applyButton = screen.getByTestId('apply-filters-button')
      await user.click(applyButton)

      expect(mockOnChange).toHaveBeenCalledWith(
        expect.objectContaining({
          transactionType: 'recharge',
        })
      )
    })

    it('GIVEN user sets date range WHEN applying THEN should call onChange with correct datetime strings', async () => {
      const user = userEvent.setup({ delay: null })
      render(
        <TransactionFilters
          filters={defaultFilters}
          onChange={mockOnChange}
          onClear={mockOnClear}
        />
      )

      const fromDateInput = screen.getByTestId('filter-from-date')
      await user.type(fromDateInput, '2025-01-01')

      const toDateInput = screen.getByTestId('filter-to-date')
      await user.type(toDateInput, '2025-03-15')

      const applyButton = screen.getByTestId('apply-filters-button')
      await user.click(applyButton)

      expect(mockOnChange).toHaveBeenCalledWith(
        expect.objectContaining({
          startTime: '2025-01-01T00:00:00.000Z',
          endTime: '2025-03-15T23:59:59.999Z',
        })
      )
    })

    it('GIVEN admin user selects client app WHEN applying THEN should call onChange with client app ID', async () => {
      const user = userEvent.setup({ delay: null })
      render(
        <TransactionFilters
          filters={defaultFilters}
          onChange={mockOnChange}
          onClear={mockOnClear}
          admin={true}
          clientApps={mockClientApps}
        />
      )

      // Click on the select trigger (combobox)
      const clientAppSelectTrigger = screen.getByRole('combobox', { name: /client app/i })
      await user.click(clientAppSelectTrigger)

      // Select app 1 option using role for better specificity
      const app1Option = screen.getByRole('option', { name: 'App 1' })
      await user.click(app1Option)

      const applyButton = screen.getByTestId('apply-filters-button')
      await user.click(applyButton)

      expect(mockOnChange).toHaveBeenCalledWith(
        expect.objectContaining({
          clientAppId: 'app-1',
        })
      )
    })

    it('GIVEN user selects "All types" WHEN applying THEN should call onChange with undefined type', async () => {
      const user = userEvent.setup({ delay: null })
      const filtersWithType = { ...defaultFilters, transactionType: 'recharge' as const }
      render(
        <TransactionFilters
          filters={filtersWithType}
          onChange={mockOnChange}
          onClear={mockOnClear}
        />
      )

      // Click on the select trigger (combobox)
      const typeSelectTrigger = screen.getByRole('combobox', { name: /transaction type/i })
      await user.click(typeSelectTrigger)

      // Select all types option using role for better specificity
      const allTypesOption = screen.getByRole('option', { name: 'All types' })
      await user.click(allTypesOption)

      const applyButton = screen.getByTestId('apply-filters-button')
      await user.click(applyButton)

      expect(mockOnChange).toHaveBeenCalledWith(
        expect.objectContaining({
          transactionType: undefined,
        })
      )
    })
  })

  describe('clear filters', () => {
    it('GIVEN filters are active WHEN clicking clear THEN should call onClear', async () => {
      const user = userEvent.setup({ delay: null })
      const activeFilters: TransactionFiltersType = {
        transactionType: 'recharge',
        startTime: '2025-01-01T00:00:00Z',
        endTime: '2025-03-15T23:59:59.999Z',
      }
      render(
        <TransactionFilters filters={activeFilters} onChange={mockOnChange} onClear={mockOnClear} />
      )

      const clearButton = screen.getByTestId('clear-filters-button')
      expect(clearButton).toBeInTheDocument()

      await user.click(clearButton)

      expect(mockOnClear).toHaveBeenCalled()
    })

    it('GIVEN filters are empty WHEN rendering THEN should not show clear button', () => {
      render(
        <TransactionFilters
          filters={defaultFilters}
          onChange={mockOnChange}
          onClear={mockOnClear}
        />
      )

      expect(screen.queryByTestId('clear-filters-button')).not.toBeInTheDocument()
    })
  })

  describe('date conversion', () => {
    it('GIVEN filter with ISO datetime string WHEN rendering THEN should display date part only', () => {
      const filtersWithDate: TransactionFiltersType = {
        ...defaultFilters,
        startTime: '2025-01-01T10:30:00Z',
        endTime: '2025-03-15T15:45:00Z',
      }
      render(
        <TransactionFilters
          filters={filtersWithDate}
          onChange={mockOnChange}
          onClear={mockOnClear}
        />
      )

      const fromDateInput = screen.getByTestId('filter-from-date') as HTMLInputElement
      const toDateInput = screen.getByTestId('filter-to-date') as HTMLInputElement

      expect(fromDateInput.value).toBe('2025-01-01')
      expect(toDateInput.value).toBe('2025-03-15')
    })
  })
})
