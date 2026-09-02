import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, act } from '@testing-library/react'
import { TurnstileWidget } from '../turnstile-widget'

// Mock Turnstile component before importing
vi.mock('@marsidev/react-turnstile', () => ({
  Turnstile: vi.fn(({ onSuccess, onError, onExpire }) => {
    // Store callbacks on window for test access
    if (typeof window !== 'undefined') {
      ;(window as any)._turnstileCallbacks = { onSuccess, onError, onExpire }
    }
    return <div data-testid="turnstile-mock" />
  }),
}))

// Type for accessing mock callbacks
type TurnstileCallbacks = {
  onSuccess?: (token: string) => void
  onError?: (error: string) => void
  onExpire?: () => void
}

describe('TurnstileWidget', () => {
  const mockOnTokenChange = vi.fn()
  const mockOnError = vi.fn()
  const mockSiteKey = 'test-site-key'

  beforeEach(() => {
    mockOnTokenChange.mockClear()
    mockOnError.mockClear()
    if (typeof window !== 'undefined') {
      ;(window as any)._turnstileCallbacks = undefined
    }
  })

  const renderWidget = async () =>
    render(
      <TurnstileWidget
        siteKey={mockSiteKey}
        onTokenChange={mockOnTokenChange}
        onError={mockOnError}
      />
    )

  const getCallbacks = (): TurnstileCallbacks =>
    (window as any)._turnstileCallbacks as TurnstileCallbacks

  describe('success callback', () => {
    it('GIVEN success callback triggers WHEN token is generated THEN calls onTokenChange and not onError', async () => {
      await renderWidget()
      const testToken = 'test-token-123'
      getCallbacks()?.onSuccess?.(testToken)

      expect(mockOnTokenChange).toHaveBeenCalledWith(testToken)
      expect(mockOnTokenChange).toHaveBeenCalledTimes(1)
      expect(mockOnError).not.toHaveBeenCalled()
    })
  })

  describe('error callback', () => {
    it('GIVEN error callback triggers WHEN error occurs THEN calls onError and displays error message', async () => {
      const screen = await renderWidget()
      const testError = 'timeout-error'

      await act(() => {
        getCallbacks()?.onError?.(testError)
      })

      expect(mockOnError).toHaveBeenCalledWith(testError)
      expect(mockOnError).toHaveBeenCalledTimes(1)
      expect(screen.getByText(testError)).toBeInTheDocument()
    })
  })

  describe('expiration', () => {
    it('GIVEN expire callback triggers WHEN token expires THEN calls onTokenChange with null and not onError', async () => {
      await renderWidget()
      getCallbacks()?.onExpire?.()

      expect(mockOnTokenChange).toHaveBeenCalledWith(null)
      expect(mockOnTokenChange).toHaveBeenCalledTimes(1)
      expect(mockOnError).not.toHaveBeenCalled()
    })
  })
})
