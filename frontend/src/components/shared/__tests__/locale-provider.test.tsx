import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, act } from '@testing-library/react'
import { LocaleProvider, useLocale } from '../locale-provider'

/**
 * Helper component that reads locale context and exposes it + switchLocale for testing.
 */
function LocaleConsumer({ onLocale }: { onLocale?: (locale: string) => void }) {
  const { locale, switchLocale } = useLocale()
  onLocale?.(locale)
  return (
    <div data-testid="locale-display">
      <span data-testid="current-locale">{locale}</span>
      <button data-testid="switch-zh-cn" onClick={() => switchLocale('zh-CN')} />
      <button data-testid="switch-en" onClick={() => switchLocale('en')} />
    </div>
  )
}

describe('LocaleProvider', () => {
  // Track calls to the paraglide setLocale to verify the runtime is initialized.
  let setLocaleCalls: Array<{ locale: string; options: { reload: boolean } }>

  beforeEach(() => {
    setLocaleCalls = []

    // Reset localStorage and navigator between tests
    localStorage.clear()

    // Default navigator.language to English
    Object.defineProperty(navigator, 'language', {
      value: 'en-US',
      configurable: true,
      writable: true,
    })
  })

  /**
   * Render the LocaleProvider with a consumer that captures the resolved locale.
   * Returns helpers for inspecting locale state.
   */
  function renderWithLocale() {
    const capturedLocales: string[] = []
    const result = render(
      <LocaleProvider>
        <LocaleConsumer
          onLocale={(locale) => {
            capturedLocales.push(locale)
          }}
        />
      </LocaleProvider>
    )
    return { result, capturedLocales }
  }

  describe('first visit — browser language detection', () => {
    it('detects zh-CN when browser language is zh-CN and no stored preference', () => {
      Object.defineProperty(navigator, 'language', {
        value: 'zh-CN',
        configurable: true,
        writable: true,
      })

      const { result } = renderWithLocale()

      expect(result.getByTestId('current-locale').textContent).toBe('zh-CN')
    })

    it('detects zh-CN when browser language is zh-Hans-CN (zh prefix match)', () => {
      Object.defineProperty(navigator, 'language', {
        value: 'zh-Hans-CN',
        configurable: true,
        writable: true,
      })

      const { result } = renderWithLocale()

      expect(result.getByTestId('current-locale').textContent).toBe('zh-CN')
    })

    it('detects en when browser language is en-US', () => {
      Object.defineProperty(navigator, 'language', {
        value: 'en-US',
        configurable: true,
        writable: true,
      })

      const { result } = renderWithLocale()

      expect(result.getByTestId('current-locale').textContent).toBe('en')
    })

    it('detects en when browser language is en-GB', () => {
      Object.defineProperty(navigator, 'language', {
        value: 'en-GB',
        configurable: true,
        writable: true,
      })

      const { result } = renderWithLocale()

      expect(result.getByTestId('current-locale').textContent).toBe('en')
    })

    it('falls back to en for unsupported browser language (e.g. ja)', () => {
      Object.defineProperty(navigator, 'language', {
        value: 'ja',
        configurable: true,
        writable: true,
      })

      const { result } = renderWithLocale()

      expect(result.getByTestId('current-locale').textContent).toBe('en')
    })
  })

  describe('returning visit — stored preference', () => {
    it('reads stored zh-CN from localStorage, ignoring browser language', () => {
      localStorage.setItem('herald-locale', 'zh-CN')
      // Browser is English, but stored preference should win
      Object.defineProperty(navigator, 'language', {
        value: 'en-US',
        configurable: true,
        writable: true,
      })

      const { result } = renderWithLocale()

      expect(result.getByTestId('current-locale').textContent).toBe('zh-CN')
    })

    it('reads stored en from localStorage', () => {
      localStorage.setItem('herald-locale', 'en')
      // Browser is Chinese, but stored preference should win
      Object.defineProperty(navigator, 'language', {
        value: 'zh-CN',
        configurable: true,
        writable: true,
      })

      const { result } = renderWithLocale()

      expect(result.getByTestId('current-locale').textContent).toBe('en')
    })
  })

  describe('invalid stored locale — fallback to browser detection', () => {
    it('falls back to browser detection when localStorage has unsupported locale', () => {
      localStorage.setItem('herald-locale', 'fr')
      Object.defineProperty(navigator, 'language', {
        value: 'zh-CN',
        configurable: true,
        writable: true,
      })

      const { result } = renderWithLocale()

      // 'fr' is not in supported locales, so browser detection kicks in -> zh-CN
      expect(result.getByTestId('current-locale').textContent).toBe('zh-CN')
    })

    it('falls back to en when both stored locale and browser language are unsupported', () => {
      localStorage.setItem('herald-locale', 'fr')
      Object.defineProperty(navigator, 'language', {
        value: 'ja',
        configurable: true,
        writable: true,
      })

      const { result } = renderWithLocale()

      // 'fr' is unsupported, 'ja' doesn't match 'zh' prefix -> en (baseLocale)
      expect(result.getByTestId('current-locale').textContent).toBe('en')
    })
  })

  describe('switchLocale', () => {
    it('switches from en to zh-CN: updates React state', async () => {
      localStorage.setItem('herald-locale', 'en')
      const { result } = renderWithLocale()

      expect(result.getByTestId('current-locale').textContent).toBe('en')

      await act(async () => {
        result.getByTestId('switch-zh-cn').click()
      })

      expect(result.getByTestId('current-locale').textContent).toBe('zh-CN')
    })

    it('switching to zh-CN persists to localStorage via paraglide runtime', async () => {
      localStorage.setItem('herald-locale', 'en')
      const { result } = renderWithLocale()

      await act(async () => {
        result.getByTestId('switch-zh-cn').click()
      })

      // The paraglide runtime's setLocale with localStorage strategy writes to localStorage
      expect(localStorage.getItem('herald-locale')).toBe('zh-CN')
    })

    it('switches from zh-CN to en: updates React state and localStorage', async () => {
      localStorage.setItem('herald-locale', 'zh-CN')
      const { result } = renderWithLocale()

      expect(result.getByTestId('current-locale').textContent).toBe('zh-CN')

      await act(async () => {
        result.getByTestId('switch-en').click()
      })

      expect(result.getByTestId('current-locale').textContent).toBe('en')
      expect(localStorage.getItem('herald-locale')).toBe('en')
    })
  })

  describe('useLocale hook guard', () => {
    it('throws when useLocale is called outside LocaleProvider', () => {
      // Suppress console.error from React for the expected thrown error
      const spy = vi.spyOn(console, 'error').mockImplementation(() => {})

      expect(() => render(<LocaleConsumer />)).toThrow(
        'useLocale must be used within a LocaleProvider'
      )

      spy.mockRestore()
    })
  })
})
