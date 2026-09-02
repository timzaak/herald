import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { AuthPageWrapper } from '../auth-page-wrapper'
import type { PublicWhiteLabelConfig } from '@/lib/api-generated/types.gen'

/**
 * jsdom's built-in `Image()` never fires `load`/`error`, which would leave the
 * background preload pending forever. We install a minimal fake that records the
 * handlers and lets each test drive them (or auto-fails) so we can assert both
 * the success and fallback paths deterministically.
 */
type FakeImage = {
  onload: (() => void) | null
  onerror: (() => void) | null
  src: string
}

type ImageMode = 'success' | 'error'

function installFakeImage(mode: ImageMode) {
  const instances: FakeImage[] = []
  // A real constructor (not an arrow fn) so `new Image()` works; vitest can spy
  // on its calls via `ctor.mock`.
  const Ctor = vi.fn(function (this: FakeImage) {
    let backingSrc = ''
    const img: FakeImage = { onload: null, onerror: null, src: '' }
    Object.defineProperty(this, 'onload', {
      configurable: true,
      get: () => img.onload,
      set: (v: (() => void) | null) => {
        img.onload = v
      },
    })
    Object.defineProperty(this, 'onerror', {
      configurable: true,
      get: () => img.onerror,
      set: (v: (() => void) | null) => {
        img.onerror = v
      },
    })
    Object.defineProperty(this, 'src', {
      configurable: true,
      get() {
        return backingSrc
      },
      set(value: string) {
        backingSrc = value
        // Mimic the native async decode + dispatch on the next macrotask.
        setTimeout(() => {
          if (mode === 'error') img.onerror?.()
          else img.onload?.()
        }, 0)
      },
    })
    instances.push(this)
  })
  vi.stubGlobal('Image', Ctor)
  return { ctor: Ctor, instances }
}

// The default (no white-label background) skin is the flat paper background.
// Tenant gradients/images are applied as inline backgroundImage styles, so the
// class itself stays on the root in every case; fallback tests pair it with
// the absence of `background-image` in the style attribute.
const DEFAULT_BG_CLASS = 'bg-background'

function rootEl(container: HTMLElement): HTMLElement {
  const el = container.querySelector('[data-testid="auth-page-root"]')
  if (!el) throw new Error('AuthPageWrapper root element not found')
  return el as HTMLElement
}

describe('AuthPageWrapper', () => {
  let originalImage: typeof globalThis.Image | undefined

  beforeEach(() => {
    originalImage = globalThis.Image
  })

  afterEach(() => {
    if (originalImage) {
      vi.stubGlobal('Image', originalImage)
    }
    vi.unstubAllGlobals()
  })

  describe('logo', () => {
    it('GIVEN logoUrl present WHEN rendering THEN shows the logo image, not the text fallback', async () => {
      const whiteLabel: PublicWhiteLabelConfig = { logoUrl: 'https://cdn.example.com/logo.svg' }
      const screen = render(
        <AuthPageWrapper whiteLabel={whiteLabel}>
          <div>child</div>
        </AuthPageWrapper>
      )
      const logo = screen.getByTestId('auth-brand-logo')
      expect(logo).toBeInTheDocument()
      expect(logo).toHaveAttribute('src', 'https://cdn.example.com/logo.svg')
      expect(screen.queryByTestId('auth-brand-text')).not.toBeInTheDocument()
    })

    it('GIVEN no logoUrl WHEN rendering THEN shows the Herald text fallback', async () => {
      const screen = render(
        <AuthPageWrapper whiteLabel={{}}>
          <div>child</div>
        </AuthPageWrapper>
      )
      const text = screen.getByTestId('auth-brand-text')
      expect(text).toBeInTheDocument()
      expect(text).toHaveTextContent('Herald')
      expect(screen.queryByTestId('auth-brand-logo')).not.toBeInTheDocument()
    })

    it('GIVEN no logo and a configured brand name WHEN rendering THEN uses the shared brand fallback', () => {
      render(
        <AuthPageWrapper whiteLabel={{ brandName: 'Acme Identity' }} realmName="Realm Name">
          <div>child</div>
        </AuthPageWrapper>
      )
      expect(screen.getByTestId('auth-brand-text')).toHaveTextContent('Acme Identity')
    })

    it('GIVEN no configured brand name WHEN rendering THEN falls back to the realm name', () => {
      render(
        <AuthPageWrapper whiteLabel={{}} realmName="Realm Name">
          <div>child</div>
        </AuthPageWrapper>
      )
      expect(screen.getByTestId('auth-brand-text')).toHaveTextContent('Realm Name')
    })

    it('GIVEN logo fails to load WHEN onError fires THEN switches to the Herald text fallback', async () => {
      const whiteLabel: PublicWhiteLabelConfig = { logoUrl: 'https://broken.example.com/x.png' }
      const screen = render(
        <AuthPageWrapper whiteLabel={whiteLabel}>
          <div>child</div>
        </AuthPageWrapper>
      )
      const logo = screen.getByTestId('auth-brand-logo')
      // React wires `onError` to the native error event; fire it like a broken load.
      fireEvent.error(logo)
      expect(screen.queryByTestId('auth-brand-logo')).not.toBeInTheDocument()
      const text = screen.getByTestId('auth-brand-text')
      expect(text).toBeInTheDocument()
      expect(text).toHaveTextContent('Herald')
    })
  })

  describe('document branding', () => {
    it('GIVEN a brand and favicon WHEN mounted and unmounted THEN applies and restores the document head', () => {
      document.title = 'Herald Admin'
      const originalIcon = document.createElement('link')
      originalIcon.rel = 'icon'
      originalIcon.href = '/default.ico'
      document.head.appendChild(originalIcon)

      const view = render(
        <AuthPageWrapper
          whiteLabel={{ brandName: 'Acme', faviconUrl: 'https://cdn.example.com/acme.ico' }}
        >
          <div>child</div>
        </AuthPageWrapper>
      )

      expect(document.title).toBe('Acme')
      expect(originalIcon.href).toBe('https://cdn.example.com/acme.ico')

      view.unmount()
      expect(document.title).toBe('Herald Admin')
      expect(originalIcon.href).toContain('/default.ico')
      originalIcon.remove()
    })

    it('GIVEN no existing favicon WHEN unmounted THEN removes the route-scoped icon', () => {
      document.querySelectorAll('link[rel~="icon"]').forEach((link) => link.remove())
      const view = render(
        <AuthPageWrapper whiteLabel={{ faviconUrl: 'https://cdn.example.com/acme.ico' }}>
          <div>child</div>
        </AuthPageWrapper>
      )
      expect(document.querySelector('link[rel~="icon"]')).not.toBeNull()
      view.unmount()
      expect(document.querySelector('link[rel~="icon"]')).toBeNull()
    })

    it('GIVEN a configured favicon fails to load THEN restores the previous favicon', () => {
      const originalIcon = document.createElement('link')
      originalIcon.rel = 'icon'
      originalIcon.href = '/default.ico'
      document.head.appendChild(originalIcon)
      render(
        <AuthPageWrapper whiteLabel={{ faviconUrl: 'https://broken.example.com/icon.ico' }}>
          <div>child</div>
        </AuthPageWrapper>
      )

      fireEvent.error(originalIcon)
      expect(originalIcon.href).toContain('/default.ico')
      originalIcon.remove()
    })
  })

  describe('accent color', () => {
    it('GIVEN a valid accentColor WHEN rendering THEN sets --primary and --ring on the root style, not className', async () => {
      const whiteLabel: PublicWhiteLabelConfig = { accentColor: '#2563eb' }
      const screen = render(
        <AuthPageWrapper whiteLabel={whiteLabel}>
          <div>child</div>
        </AuthPageWrapper>
      )
      const root = rootEl(screen.container)
      const style = root.getAttribute('style') ?? ''
      // Assert presence of the CSS variable overrides (never on className).
      expect(style).toContain('--primary: #2563eb')
      expect(style).toContain('--ring: #2563eb')
      expect(root.className).not.toContain('#2563eb')
    })

    it('GIVEN no accentColor WHEN rendering THEN leaves the CSS variables unset', async () => {
      const screen = render(
        <AuthPageWrapper whiteLabel={{}}>
          <div>child</div>
        </AuthPageWrapper>
      )
      const root = rootEl(screen.container)
      const style = root.getAttribute('style') ?? ''
      expect(style).not.toContain('--primary')
      expect(style).not.toContain('--ring')
    })
  })

  describe('footer', () => {
    it('GIVEN footerText present WHEN rendering THEN renders the footer', async () => {
      const screen = render(
        <AuthPageWrapper whiteLabel={{ footerText: 'Example Inc.' }}>
          <div>child</div>
        </AuthPageWrapper>
      )
      const footer = screen.getByTestId('auth-brand-footer')
      expect(footer).toBeInTheDocument()
      expect(footer).toHaveTextContent('Example Inc.')
    })

    it('GIVEN no footerText WHEN rendering THEN does not render the footer', async () => {
      const screen = render(
        <AuthPageWrapper whiteLabel={{}}>
          <div>child</div>
        </AuthPageWrapper>
      )
      expect(screen.queryByTestId('auth-brand-footer')).not.toBeInTheDocument()
    })
  })

  describe('background', () => {
    it('GIVEN an image background that loads WHEN rendering THEN applies backgroundImage via style', async () => {
      installFakeImage('success')
      const whiteLabel: PublicWhiteLabelConfig = {
        background: { type: 'image', value: 'https://cdn.example.com/bg.jpg' },
      }
      const screen = render(
        <AuthPageWrapper whiteLabel={whiteLabel}>
          <div>child</div>
        </AuthPageWrapper>
      )
      const root = rootEl(screen.container)
      await waitFor(() => {
        const style = root.getAttribute('style') ?? ''
        expect(style).toContain('background-image')
        expect(style).toContain('https://cdn.example.com/bg.jpg')
      })
    })

    it('GIVEN an image background that fails WHEN rendering THEN falls back to the default paper background (no backgroundImage)', async () => {
      installFakeImage('error')
      const whiteLabel: PublicWhiteLabelConfig = {
        background: { type: 'image', value: 'https://broken.example.com/bg.jpg' },
      }
      const screen = render(
        <AuthPageWrapper whiteLabel={whiteLabel}>
          <div>child</div>
        </AuthPageWrapper>
      )
      const root = rootEl(screen.container)
      await waitFor(() => {
        const style = root.getAttribute('style') ?? ''
        expect(style).not.toContain('background-image')
      })
      // Default gradient class remains intact as the fallback.
      expect(root.className).toContain(DEFAULT_BG_CLASS)
    })

    it('GIVEN a valid gradient background WHEN rendering THEN applies the gradient via style', async () => {
      const whiteLabel: PublicWhiteLabelConfig = {
        background: { type: 'gradient', value: 'linear-gradient(to right, #1e3a8a, #2563eb)' },
      }
      const screen = render(
        <AuthPageWrapper whiteLabel={whiteLabel}>
          <div>child</div>
        </AuthPageWrapper>
      )
      const root = rootEl(screen.container)
      const style = root.getAttribute('style') ?? ''
      // jsdom normalizes the hex stops to rgb(), so assert on the stable prefix.
      expect(style).toContain('background-image: linear-gradient(to right')
    })

    it('GIVEN an invalid gradient background WHEN rendering THEN falls back to the default paper background', async () => {
      const whiteLabel: PublicWhiteLabelConfig = {
        background: { type: 'gradient', value: 'url("https://evil.example.com/x.png")' },
      }
      const screen = render(
        <AuthPageWrapper whiteLabel={whiteLabel}>
          <div>child</div>
        </AuthPageWrapper>
      )
      const root = rootEl(screen.container)
      const style = root.getAttribute('style') ?? ''
      expect(style).not.toContain('background-image')
      expect(root.className).toContain(DEFAULT_BG_CLASS)
    })
  })

  describe('children', () => {
    it('GIVEN no whiteLabel at all WHEN rendering THEN renders Herald text and keeps the default paper background', async () => {
      const screen = render(<AuthPageWrapper>children</AuthPageWrapper>)
      expect(screen.getByTestId('auth-brand-text')).toHaveTextContent('Herald')
      const root = rootEl(screen.container)
      expect(root.className).toContain(DEFAULT_BG_CLASS)
    })
  })
})
