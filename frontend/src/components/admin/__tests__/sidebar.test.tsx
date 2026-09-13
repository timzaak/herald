/**
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Sidebar } from '../sidebar'
import { useAuthStore } from '@/stores/auth-store'
import { LocaleProvider } from '@/components/shared/locale-provider'
import { APP_VERSION, GITHUB_REPO_URL } from '@/lib/constants'

let currentPath = '/manage/billing?page=0&pageSize=20&status=all'

vi.mock('@tanstack/react-query', () => ({
  useQuery: () => ({ data: { id: 'admin', name: 'Admin' } }),
}))

vi.mock('@/data/query-options', () => ({
  realmQueryOptions: () => ({ queryKey: ['realm', 'admin'] }),
  featureAvailabilityQueryOptions: () => ({ queryKey: ['feature-availability', 'admin'] }),
}))

vi.mock('@tanstack/react-router', () => ({
  useLocation: () => ({ pathname: currentPath }),
  Link: ({
    to,
    className,
    activeProps,
    activeOptions,
    children,
    ...props
  }: {
    to: string
    className?: string
    activeProps?: { className?: string }
    activeOptions?: { exact?: boolean }
    children: React.ReactNode
  }) => {
    let isActive: boolean
    if (activeOptions?.exact) {
      const pathWithoutQuery = currentPath.split('?')[0]
      isActive = pathWithoutQuery === to
    } else {
      isActive =
        currentPath === to || currentPath.startsWith(`${to}/`) || currentPath.startsWith(`${to}?`)
    }

    const resolvedClassName = [className, isActive ? activeProps?.className : undefined]
      .filter(Boolean)
      .join(' ')

    return (
      <a href={to} className={resolvedClassName} {...props}>
        {children}
      </a>
    )
  },
}))

describe('Sidebar navigation', () => {
  beforeEach(() => {
    useAuthStore.setState({
      isAuthenticated: true,
      isLoading: false,
      realmId: 'admin',
      user: null,
      permissions: ['billing.view'],
      roles: [],
    })
  })

  afterEach(() => {
    cleanup()
    useAuthStore.getState().reset()
  })

  it('highlights entitlement mappings on the entitlement-mappings page', async () => {
    currentPath = '/manage/billing/entitlement-mappings'
    const user = userEvent.setup()
    render(
      <LocaleProvider>
        <Sidebar />
      </LocaleProvider>
    )

    await user.click(screen.getByTestId('sidebar-menu-products-&-payments'))

    const entitlementMappingsLink = screen.getByTestId('sidebar-menu-entitlement-mappings')
    const paymentProvidersLink = screen.getByTestId('sidebar-menu-payment-providers')

    expect(entitlementMappingsLink).toHaveClass('font-semibold')
    expect(paymentProvidersLink).not.toHaveClass('font-semibold')
  })

  it('highlights invoices on the invoices page (under Transactions)', async () => {
    currentPath = '/manage/billing/invoices'
    const user = userEvent.setup()
    render(
      <LocaleProvider>
        <Sidebar />
      </LocaleProvider>
    )

    await user.click(screen.getByTestId('sidebar-menu-transactions'))

    const invoicesLink = screen.getByTestId('sidebar-menu-invoices')
    const subscriptionHistoryLink = screen.getByTestId('sidebar-menu-subscription-history')

    expect(invoicesLink).toHaveClass('font-semibold')
    expect(subscriptionHistoryLink).not.toHaveClass('font-semibold')
  })

  it('highlights only payment providers on the payment providers page (under Products & Payments)', async () => {
    currentPath = '/manage/billing/payment-providers'
    const user = userEvent.setup()
    render(
      <LocaleProvider>
        <Sidebar />
      </LocaleProvider>
    )

    await user.click(screen.getByTestId('sidebar-menu-products-&-payments'))

    const providersLink = screen.getByTestId('sidebar-menu-payment-providers')
    const entitlementMappingsLink = screen.getByTestId('sidebar-menu-entitlement-mappings')

    expect(providersLink).toHaveClass('font-semibold')
    expect(entitlementMappingsLink).not.toHaveClass('font-semibold')
  })

  it('keeps sidebar navigation in its own scroll container when group expands', async () => {
    currentPath = '/manage/billing?page=0&pageSize=20&status=all'
    const user = userEvent.setup()
    render(
      <LocaleProvider>
        <Sidebar />
      </LocaleProvider>
    )

    await user.click(screen.getByTestId('sidebar-menu-products-&-payments'))

    const sidebar = screen.getByTestId('admin-sidebar')
    const nav = screen.getByTestId('sidebar-nav')

    expect(sidebar).toHaveClass('h-full', 'min-h-0', 'flex', 'flex-col')
    expect(nav).toHaveClass('min-h-0', 'flex-1', 'overflow-y-auto')
  })

  it('shows the deployed version and a link to the upstream GitHub repository', () => {
    // Operators use the footer version to compare their deployment against
    // upstream releases, so it must track package.json (what release.py bumps)
    // rather than a hand-maintained string.
    render(
      <LocaleProvider>
        <Sidebar />
      </LocaleProvider>
    )

    expect(screen.getByTestId('sidebar-version')).toHaveTextContent(`v${APP_VERSION}`)
    // Guard against a broken package.json import silently rendering "vundefined".
    expect(APP_VERSION).toMatch(/^\d+\.\d+\.\d+$/)

    const githubLink = screen.getByTestId('sidebar-github-link')
    expect(githubLink).toHaveAttribute('href', GITHUB_REPO_URL)
    // The link leaves the console, so it must open a new tab instead of
    // navigating the admin session away.
    expect(githubLink).toHaveAttribute('target', '_blank')
    expect(githubLink).toHaveAttribute('rel', 'noopener noreferrer')
  })
})
