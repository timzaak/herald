import { vi, beforeAll, afterEach, afterAll, beforeEach } from 'vitest'
import { server } from './mocks/server'

/**
 * Global setup file for Vitest tests.
 *
 * This file runs before each test file and configures:
 * - Global test settings
 * - Mock configurations (if needed)
 * - MSW setup (if needed for API mocking)
 */

// Configure API client with baseUrl for testing
// This ensures all API calls use a full URL that MSW can intercept
import { client } from '@/lib/api-generated/client.gen'
client.setConfig({
  baseUrl: 'http://localhost:3000',
})

// Point the Herald SDK client (login-family calls + token transport) at the
// same MSW-intercepted origin, and drop its persisted refresh token between
// tests so token state never leaks across test cases.
import { setHeraldBaseUrlOverride, HERALD_REFRESH_TOKEN_STORAGE_KEY } from '@/lib/herald-client'
setHeraldBaseUrlOverride('http://localhost:3000')
beforeEach(() => {
  window.localStorage.removeItem(HERALD_REFRESH_TOKEN_STORAGE_KEY)
})

// Set fixed English locale for all tests to prevent translation functions
// from causing test instability
import { setLocale } from '@/paraglide/runtime'
setLocale('en', { reload: false })

// Keep enough per-test budget for the full suite under parallel JSDOM load.
vi.setConfig({ testTimeout: 15000 })

// waitFor/findBy default to a 1s budget, which starves when the whole suite
// runs in parallel on a loaded machine (debounced queries and React state
// updates exceed it while other workers saturate the CPU). Align the
// async-util budget with the testTimeout headroom above.
import { configure } from '@testing-library/react'
configure({ asyncUtilTimeout: 5000 })

// Start/stop MSW once per test session and reset handlers between tests
beforeAll(() => {
  server.listen({ onUnhandledRequest: 'warn' })
})

afterEach(() => {
  server.resetHandlers()
})

afterAll(() => {
  server.close()
})

// Clear mocks before each test to ensure isolation
beforeEach(() => {
  vi.clearAllMocks()
})

// Add custom matchers from jest-dom
import '@testing-library/jest-dom'

// Mock ResizeObserver for Radix UI components
global.ResizeObserver = class ResizeObserver {
  constructor(_callback: ResizeObserverCallback) {}
  disconnect() {}
  observe(_target: Element, _options?: ResizeObserverOptions) {}
  unobserve(_target: Element) {}
}

// Mock IntersectionObserver
global.IntersectionObserver = class IntersectionObserver {
  constructor(_callback: IntersectionObserverCallback, _options?: IntersectionObserverInit) {}
  disconnect() {}
  observe(_target: Element) {}
  takeRecords(): IntersectionObserverEntry[] {
    return []
  }
  unobserve(_target: Element) {}
}

// Mock HTMLFormElement.requestSubmit for TanStack Form
HTMLFormElement.prototype.requestSubmit = function (this: HTMLFormElement) {
  const event = new Event('submit', { bubbles: true, cancelable: true })
  this.dispatchEvent(event)
}

// Mock Element.scrollIntoView for cmdk library
Element.prototype.scrollIntoView = function () {}
Element.prototype.hasPointerCapture = function () {
  return false
}
Element.prototype.setPointerCapture = function () {}
Element.prototype.releasePointerCapture = function () {}

Object.defineProperty(Element.prototype, 'dataset', {
  get: function () {
    return this._dataset || {}
  },
  set: function (value) {
    this._dataset = value
  },
  configurable: true,
})

Element.prototype.getBoundingClientRect = function () {
  return {
    top: 0,
    left: 0,
    bottom: 0,
    right: 0,
    width: 0,
    height: 0,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  }
}

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    promise: vi.fn(),
    dismiss: vi.fn(),
  },
}))

// Vaul library accesses DOM properties that don't exist in test environment, causing warnings.
// This mock eliminates warnings and supports our component usage patterns without simulating
// vaul's internal behavior (portal behavior, gesture handling), which is vaul's responsibility.
vi.mock('vaul', () => {
  /* eslint-disable react-hooks/refs */
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const React = require('react')
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const ReactDOM = require('react-dom')
  const { forwardRef, useContext, createContext, useState, useEffect, useRef } = React

  // Create context to share drawer state between components
  const DrawerContext = createContext<{
    open: boolean
    onOpenChange: ((open: boolean) => void) | undefined
  }>({
    open: false,
    onOpenChange: undefined,
  })

  // Create a shared portal container for all drawer portals
  let portalContainer: HTMLDivElement | null = null

  const getPortalContainer = () => {
    if (!portalContainer) {
      portalContainer = document.createElement('div')
      portalContainer.setAttribute('data-vaul-portal-container', 'true')
      document.body.appendChild(portalContainer)
    }
    return portalContainer
  }

  // Clean up portal container after each test to prevent memory leaks
  afterEach(() => {
    if (portalContainer && portalContainer.parentNode) {
      portalContainer.parentNode.removeChild(portalContainer)
      portalContainer = null
    }
  })

  // Create mock components that accept ref where needed
  const DrawerRoot = forwardRef<any, any>(
    ({ open: controlledOpen, onOpenChange, children, ...props }: any, ref) => {
      // Use controlled state if provided, otherwise use local state
      const [internalOpen, setInternalOpen] = useState(controlledOpen || false)
      const isOpen = controlledOpen !== undefined ? controlledOpen : internalOpen

      // Update internal state when controlled prop changes
      useEffect(() => {
        if (controlledOpen !== undefined) {
          setInternalOpen(controlledOpen)
        }
      }, [controlledOpen])

      const handleOpenChange = (newOpen: boolean) => {
        if (onOpenChange) {
          onOpenChange(newOpen)
        } else {
          setInternalOpen(newOpen)
        }
      }

      return React.createElement(
        DrawerContext.Provider,
        {
          value: {
            open: isOpen,
            onOpenChange: handleOpenChange,
          },
        },
        React.createElement(
          'div',
          {
            ref,
            'data-state': isOpen ? 'open' : 'closed',
            'data-slot': 'drawer',
            ...props,
          },
          children
        )
      )
    }
  )
  DrawerRoot.displayName = 'Drawer.Root'

  const DrawerTrigger = forwardRef<any, any>(({ children, onClick, ...props }: any, ref) => {
    const { onOpenChange } = useContext(DrawerContext)
    return React.createElement(
      'button',
      {
        ref,
        onClick: (e: any) => {
          if (onClick) onClick(e)
          if (onOpenChange) onOpenChange(true)
        },
        type: 'button',
        'data-testid': 'drawer-trigger',
        ...props,
      },
      children
    )
  })
  DrawerTrigger.displayName = 'Drawer.Trigger'

  const DrawerPortal = ({ children, ...props }: any) => {
    const containerRef = useRef<HTMLDivElement | null>(null)
    const [container, setContainer] = useState<HTMLDivElement | null>(null)

    useEffect(() => {
      // Create a new container for this portal instance
      const portalContainer = document.createElement('div')
      portalContainer.setAttribute('data-slot', 'drawer-portal')
      getPortalContainer().appendChild(portalContainer)
      containerRef.current = portalContainer
      setContainer(portalContainer)

      return () => {
        // Clean up portal container on unmount
        if (portalContainer.parentNode) {
          portalContainer.parentNode.removeChild(portalContainer)
        }
      }
    }, [])

    if (!container) {
      return null
    }

    // Render children into the portal container using React Portal
    return ReactDOM.createPortal(
      React.createElement(
        'div',
        {
          'data-slot': 'drawer-portal',
          ...props,
        },
        children
      ),
      container
    )
  }
  DrawerPortal.displayName = 'Drawer.Portal'

  const DrawerOverlay = forwardRef<any, any>(({ onClick, ...props }: any, ref) => {
    const { open, onOpenChange } = useContext(DrawerContext)

    // Only render overlay when drawer is open
    if (!open) {
      return null
    }

    return React.createElement('div', {
      ref,
      onClick: (e: any) => {
        if (onClick) onClick(e)
        if (onOpenChange) onOpenChange(false)
      },
      'data-slot': 'drawer-overlay',
      'data-state': 'open',
      ...props,
    })
  })
  DrawerOverlay.displayName = 'Drawer.Overlay'

  const DrawerClose = forwardRef<any, any>(({ children, onClick, ...props }: any, ref) => {
    const { onOpenChange } = useContext(DrawerContext)
    return React.createElement(
      'button',
      {
        ref,
        onClick: (e: any) => {
          if (onClick) onClick(e)
          if (onOpenChange) onOpenChange(false)
        },
        type: 'button',
        'data-testid': 'drawer-close',
        ...props,
      },
      children
    )
  })
  DrawerClose.displayName = 'Drawer.Close'

  const DrawerContent = forwardRef<any, any>(({ children, ...props }: any, ref) => {
    const { open } = useContext(DrawerContext)

    // Don't render content when drawer is closed
    if (!open) {
      return null
    }

    return React.createElement(
      'div',
      {
        ref,
        'data-slot': 'drawer-content',
        'data-vaul-drawer-direction': 'bottom',
        'data-state': 'open',
        ...props,
      },
      children
    )
  })
  DrawerContent.displayName = 'Drawer.Content'

  const DrawerTitle = forwardRef<any, any>(({ children, ...props }: any, ref) => {
    // Always render title (parent DrawerContent will handle visibility)
    return React.createElement(
      'div',
      {
        ref,
        'data-slot': 'drawer-title',
        ...props,
      },
      children
    )
  })
  DrawerTitle.displayName = 'Drawer.Title'

  const DrawerDescription = forwardRef<any, any>(({ children, ...props }: any, ref) => {
    // Always render description (parent DrawerContent will handle visibility)
    return React.createElement(
      'div',
      {
        ref,
        'data-slot': 'drawer-description',
        ...props,
      },
      children
    )
  })
  DrawerDescription.displayName = 'Drawer.Description'

  return {
    Drawer: {
      Root: DrawerRoot,
      Trigger: DrawerTrigger,
      Portal: DrawerPortal,
      Overlay: DrawerOverlay,
      Close: DrawerClose,
      Content: DrawerContent,
      Title: DrawerTitle,
      Description: DrawerDescription,
    },
  }
  /* eslint-enable react-hooks/refs */
})

// Mock window.matchMedia for vaul library
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(), // Deprecated
    removeListener: vi.fn(), // Deprecated
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
})

// Mock getComputedStyle for vaul library compatibility
const mockComputedStyle = {
  getPropertyValue: (property: string) => {
    // Return appropriate values for common CSS properties that vaul might access
    if (
      property === 'transform' ||
      property === '-webkit-transform' ||
      property === '-moz-transform' ||
      property === '-o-transform'
    ) {
      return 'matrix(1, 0, 0, 1, 0, 0)'
    }
    if (property === 'width' || property === 'height') {
      return '0px'
    }
    if (
      property === 'top' ||
      property === 'left' ||
      property === 'right' ||
      property === 'bottom'
    ) {
      return '0px'
    }
    if (property === 'opacity') {
      return '1'
    }
    if (property === 'display') {
      return 'block'
    }
    return ''
  },
  // Add string-like properties for compatibility
  toString: () => '[object CSSStyleDeclaration]',
}

// Mock getComputedStyle to return our mock object
vi.stubGlobal('getComputedStyle', () => mockComputedStyle)
