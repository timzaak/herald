import { useEffect, useState } from 'react'
import type { CSSProperties } from 'react'
import type { PublicWhiteLabelConfig } from '@/lib/api-generated/types.gen'
import { resolveBrandName } from '@/lib/white-label-brand'

interface AuthPageWrapperProps {
  children: React.ReactNode
  realmName?: string | null
  /**
   * Public white-label configuration read from `GET /api/public-config/{realmId}`.
   * The wrapper is a pure component: it never issues its own query, callers
   * (auth routes) pass the resolved value in.
   */
  whiteLabel?: PublicWhiteLabelConfig | null
}

/**
 * A gradient string is only applied when it begins with a safe, supported
 * prefix. Anything else silently falls back to the default Herald gradient so a
 * malformed/malicious value cannot inject arbitrary CSS.
 */
function isSafeGradient(value: string | null | undefined): value is string {
  if (!value) return false
  const trimmed = value.trim()
  return trimmed.startsWith('linear-gradient(') || trimmed.startsWith('radial-gradient(')
}

/**
 * Accept hex only (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`). The set is
 * intentionally narrower than what CSS allows: the admin form's WCAG contrast
 * warning (`getContrastRatio` in white-label-contrast.ts) only parses hex, so
 * restricting application to hex keeps the warning coverage identical to the
 * applied-color set. A non-hex value (e.g. `rgb(...)`, a stray string) is
 * ignored so it cannot collapse Tailwind v4's theme (which composes
 * `oklch(var(--primary))`).
 */
const SAFE_COLOR_RE = /^#([0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/

function isValidAccentColor(value: string | null | undefined): value is string {
  return Boolean(value && SAFE_COLOR_RE.test(value.trim()))
}

/**
 * Preload a background image URL via a hidden `Image()` so a broken/inaccessible
 * URL never produces a broken-paint. Resolves `true` once the image decodes,
 * `false` on any error. Cleanup runs on both outcomes so repeated prop changes
 * do not leak listeners.
 */
function preloadImage(url: string): { promise: Promise<boolean>; cancel: () => void } {
  const img = new Image()
  let settled = false
  let resolveFn: (ok: boolean) => void = () => {}

  const promise = new Promise<boolean>((resolve) => {
    resolveFn = resolve
    img.onload = () => {
      if (settled) return
      settled = true
      resolve(true)
    }
    img.onerror = () => {
      if (settled) return
      settled = true
      resolve(false)
    }
    img.src = url
  })

  const cancel = () => {
    if (settled) return
    settled = true
    // Reassign handlers so a late native callback becomes a no-op, and clear
    // `src` to abort the in-flight request where the platform supports it.
    img.onload = null
    img.onerror = null
    img.src = ''
    resolveFn(false)
  }

  return { promise, cancel }
}

export function AuthPageWrapper({ children, realmName, whiteLabel }: AuthPageWrapperProps) {
  const brandName = resolveBrandName(whiteLabel, realmName)
  const logoUrl = whiteLabel?.logoUrl?.trim() || null
  const faviconUrl = whiteLabel?.faviconUrl?.trim() || null
  const accentColor = whiteLabel?.accentColor?.trim() || null
  const background = whiteLabel?.background ?? null
  const footerText = whiteLabel?.footerText?.trim() || null

  const [logoFailed, setLogoFailed] = useState(false)

  useEffect(() => {
    const previousTitle = document.title
    document.title = brandName

    const existing = document.querySelector<HTMLLinkElement>('link[rel~="icon"]')
    const icon = existing ?? document.createElement('link')
    const previousRel = icon.getAttribute('rel')
    const previousHref = icon.getAttribute('href')
    const created = existing === null
    const restoreIcon = () => {
      if (created) {
        icon.remove()
      } else {
        if (previousRel === null) icon.removeAttribute('rel')
        else icon.setAttribute('rel', previousRel)
        if (previousHref === null) icon.removeAttribute('href')
        else icon.setAttribute('href', previousHref)
      }
    }

    if (faviconUrl) {
      icon.rel = 'icon'
      icon.href = faviconUrl
      icon.addEventListener('error', restoreIcon, { once: true })
      if (created) document.head.appendChild(icon)
    }

    return () => {
      document.title = previousTitle
      if (!faviconUrl) return
      icon.removeEventListener('error', restoreIcon)
      restoreIcon()
    }
  }, [brandName, faviconUrl])

  // Background image: preload before painting so a broken URL silently keeps
  // the default gradient.
  const [bgImageUrl, setBgImageUrl] = useState<string | null>(null)

  useEffect(() => {
    const url = background?.type === 'image' ? background.value?.trim() || null : null
    if (!url) {
      // No image to preload: defer the reset so we never call setState
      // synchronously inside the effect body (avoids cascading renders).
      const id = window.setTimeout(() => setBgImageUrl(null), 0)
      return () => window.clearTimeout(id)
    }
    const { promise, cancel } = preloadImage(url)
    promise.then((ok) => {
      if (ok) setBgImageUrl(url)
      else setBgImageUrl(null)
    })
    return cancel
  }, [background])

  // Root style: accent color overrides the `--primary` / `--ring` CSS custom
  // properties only. Never concatenated into className. Invalid CSS color values
  // are ignored by the browser when assigned here, which is the safe fallback.
  const rootStyle: CSSProperties = {}
  if (isValidAccentColor(accentColor)) {
    // Custom properties are not part of React's typed `CSSProperties`; assigning
    // via a record cast is the documented pattern for shadcn/Tailwind v4 theming.
    ;(rootStyle as Record<string, string>)['--primary'] = accentColor
    ;(rootStyle as Record<string, string>)['--ring'] = accentColor
  }

  // Background style: prefer a validated gradient string, else a successfully
  // preloaded image. Anything else leaves the default gradient class intact.
  if (background?.type === 'gradient' && isSafeGradient(background.value)) {
    ;(rootStyle as Record<string, string>)['backgroundImage'] = background.value.trim()
  } else if (bgImageUrl) {
    ;(rootStyle as Record<string, string>)['backgroundImage'] = `url("${bgImageUrl}")`
    ;(rootStyle as Record<string, string>)['backgroundSize'] = 'cover'
    ;(rootStyle as Record<string, string>)['backgroundPosition'] = 'center'
  }

  const showLogoImg = Boolean(logoUrl) && !logoFailed

  return (
    <div
      data-testid="auth-page-root"
      className="flex min-h-screen flex-col items-center justify-center bg-background px-4 py-8 md:py-12"
      style={rootStyle}
    >
      <div className="w-full max-w-md">
        <div className="mb-8">
          {showLogoImg ? (
            <img
              data-testid="auth-brand-logo"
              src={logoUrl ?? undefined}
              alt=""
              className="h-10 w-auto object-contain"
              onError={() => setLogoFailed(true)}
            />
          ) : (
            <div data-testid="auth-brand-text" className="text-2xl font-semibold tracking-tight">
              {brandName}
            </div>
          )}
          {realmName && realmName !== brandName && (
            <div className="mt-1 font-mono text-xs text-muted-foreground">{realmName}</div>
          )}
        </div>
        <div aria-hidden="true" className="border-t border-border" />
        {children}
        {footerText ? (
          <div
            data-testid="auth-brand-footer"
            className="mt-8 border-t border-border pt-4 text-xs text-muted-foreground"
          >
            {footerText}
          </div>
        ) : null}
      </div>
    </div>
  )
}
