import {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  Fragment,
  type ReactNode,
} from 'react'
import { setLocale, baseLocale, locales, type Locale } from '@/paraglide/runtime'

type LocaleContextValue = {
  locale: Locale
  switchLocale: (newLocale: Locale) => void
}

const LocaleContext = createContext<LocaleContextValue | null>(null)

/**
 * Detect the user's preferred locale from browser settings.
 * - zh prefix -> 'zh-CN'
 * - everything else -> 'en' (baseLocale)
 */
function detectBrowserLocale(): string {
  const browserLang = navigator.language
  if (browserLang.startsWith('zh')) {
    return 'zh-CN'
  }
  return baseLocale
}

/**
 * Resolve the initial locale: localStorage > browser detection > baseLocale.
 * Returns a locale that is in the project's supported locales list.
 */
function resolveInitialLocale(): Locale {
  const stored = localStorage.getItem('herald-locale')
  if (stored && locales.includes(stored as Locale)) {
    return stored as Locale
  }
  const detected = detectBrowserLocale()
  if (locales.includes(detected as Locale)) {
    return detected as Locale
  }
  return baseLocale
}

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocalState] = useState<Locale>(resolveInitialLocale)

  // Initialize Paraglide runtime on mount
  useEffect(() => {
    setLocale(locale, { reload: false })
  }, [locale])

  const switchLocale = useCallback(
    (newLocale: Locale) => {
      if (!locales.includes(newLocale)) return
      if (newLocale === locale) return
      // Flip the paraglide runtime locale without a page reload (PRD i18n §6:
      // switching must not reload or re-navigate). Remounting the tree below
      // via `key={locale}` re-evaluates every `m.*` call site in the new
      // locale — only context consumers would re-render otherwise.
      setLocale(newLocale, { reload: false })
      setLocalState(newLocale)
    },
    [locale]
  )

  return (
    <LocaleContext.Provider value={{ locale, switchLocale }}>
      <Fragment key={locale}>{children}</Fragment>
    </LocaleContext.Provider>
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export function useLocale(): LocaleContextValue {
  const ctx = useContext(LocaleContext)
  if (!ctx) {
    throw new Error('useLocale must be used within a LocaleProvider')
  }
  return ctx
}
