/**
 * @vitest-environment jsdom
 */

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { ProfileMobileHeader } from '../profile-mobile-header'
import type { ReactNode } from 'react'
import { LocaleProvider } from '@/components/shared/locale-provider'

let featureData:
  | { user: { pointsVisible: boolean; subscriptionVisible: boolean; invoicesVisible: boolean } }
  | undefined = undefined
let pathname = '/test-realm/user/points'

vi.mock('@tanstack/react-query', () => ({
  useQuery: () => ({ data: featureData }),
}))

vi.mock('@tanstack/react-router', () => ({
  Link: ({ to, children, ...props }: { to: string; children: ReactNode }) => (
    <a href={to} {...props}>
      {children}
    </a>
  ),
  useLocation: () => ({ pathname }),
}))

vi.mock('@/stores/auth-store', () => ({
  useRealmId: () => 'test-realm',
}))

vi.mock('@/data/query-options', () => ({
  userFeatureAvailabilityQueryOptions: { queryKey: ['user-feature-availability'] },
}))

const renderHeader = () =>
  render(
    <LocaleProvider>
      <ProfileMobileHeader />
    </LocaleProvider>
  )

describe('ProfileMobileHeader', () => {
  beforeEach(() => {
    featureData = undefined
    pathname = '/test-realm/user/points'
  })

  it('renders the back-to-hub header on a feature-gated section page while the feature query is unresolved', () => {
    // The header is a gated section page's only mobile chrome: if its
    // existence depended on feature visibility, a loading or failed
    // availability query would leave the page with no navigation at all.
    renderHeader()

    const header = screen.getByTestId('profile-mobile-header')
    const back = screen.getByTestId('profile-mobile-back')
    expect(back).toHaveAttribute('href', '/test-realm/user')
    expect(screen.getByTestId('profile-mobile-title')).toHaveTextContent('Points')
    expect(header).toBeInTheDocument()
  })

  it('also survives an entitlement flip that hides the section from the menu', () => {
    // Admin disables the points capability while the user sits on /user/points:
    // the route does not redirect, so the back header must not unmount.
    featureData = {
      user: { pointsVisible: false, subscriptionVisible: false, invoicesVisible: false },
    }

    renderHeader()

    expect(screen.getByTestId('profile-mobile-back')).toHaveAttribute('href', '/test-realm/user')
  })

  it('stays hidden on sub-flow pages and on the hub landing itself', () => {
    // Sub-flows ship their own in-page back affordances, and the hub page has
    // its own chrome — a second header there would stack two back buttons.
    pathname = '/test-realm/user/security/totp-setup'
    const { unmount } = renderHeader()
    expect(screen.queryByTestId('profile-mobile-header')).not.toBeInTheDocument()
    unmount()

    pathname = '/test-realm/user'
    renderHeader()
    expect(screen.queryByTestId('profile-mobile-header')).not.toBeInTheDocument()
  })
})
