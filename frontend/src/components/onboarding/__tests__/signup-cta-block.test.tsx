import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { renderWithProviders } from '@/test/utils/render'
import { SignupCtaBlock } from '../signup-cta-block'

// The block renders a TanStack Router Link; outside a real router only the
// anchor contract matters (href = the target the component computed). Keep the
// rest of the router package intact for transitive importers.
vi.mock('@tanstack/react-router', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@tanstack/react-router')>()
  return {
    ...actual,
    Link: ({
      to,
      children,
      ...rest
    }: {
      to?: string
      children?: React.ReactNode
    } & Record<string, unknown>) => (
      <a href={to} {...rest}>
        {children}
      </a>
    ),
  }
})

const ADMIN_REALM_CONTEXT = { realmId: 'admin', isCustomDomain: false }

// The public platform-signup status and the resolved realm are the only
// inputs to the block's visibility, and both are fail-closed gates: loading,
// a disabled switch, a failed query and a tenant realm must all keep the
// block — and any error detail — off the highest-traffic login page.
describe('SignupCtaBlock visibility (fail-closed)', () => {
  let statusRequests: string[] = []

  beforeEach(() => {
    statusRequests = []
  })

  it('GIVEN the platform signup switch is enabled WHEN the block mounts THEN it renders and links into the admin realm signup page', async () => {
    server.use(
      http.get('/api/auth/:realmId/signup/status', ({ params }) => {
        statusRequests.push(String(params.realmId))
        return HttpResponse.json({ enabled: true })
      })
    )

    renderWithProviders(<SignupCtaBlock realmContext={ADMIN_REALM_CONTEXT} />)

    const block = await screen.findByTestId('signup-cta-block')
    expect(block).toBeInTheDocument()
    // The entry always targets the admin realm's self-service signup page.
    expect(screen.getByTestId('signup-cta-link')).toHaveAttribute('href', '/admin/auth/signup')
    // The status query is fixed to the admin realm, the switch's only host.
    expect(statusRequests).toEqual(['admin'])
  })

  it('GIVEN the platform switch is enabled but the resolved realm is a tenant realm WHEN the status settles THEN nothing renders', async () => {
    // The audience gate lives in the component, not in call sites: a future
    // mount on a tenant login page must not leak the platform-branded copy.
    let hits = 0
    server.use(
      http.get('/api/auth/:realmId/signup/status', () => {
        hits += 1
        return HttpResponse.json({ enabled: true })
      })
    )

    const { container } = renderWithProviders(
      <SignupCtaBlock realmContext={{ realmId: 'acme', isCustomDomain: false }} />
    )

    await waitFor(() => {
      expect(hits).toBe(1)
    })

    expect(container.textContent).toBe('')
  })

  it.each([
    ['a disabled switch', () => HttpResponse.json({ enabled: false })],
    ['a failed status query', () => new HttpResponse(null, { status: 500 })],
  ])(
    'GIVEN %s WHEN the status settles THEN nothing renders and no error detail leaks',
    async (_label, respond) => {
      let hits = 0
      server.use(
        http.get('/api/auth/:realmId/signup/status', () => {
          hits += 1
          return respond()
        })
      )

      const { container } = renderWithProviders(
        <SignupCtaBlock realmContext={ADMIN_REALM_CONTEXT} />
      )

      // Wait until the status actually resolved, so the absence below is the
      // settled decision and not just the loading state.
      await waitFor(() => {
        expect(hits).toBe(1)
      })

      expect(screen.queryByTestId('signup-cta-block')).not.toBeInTheDocument()
      expect(screen.queryByTestId('signup-cta-link')).not.toBeInTheDocument()
      // Fail-closed means literally nothing: no block, no error copy.
      expect(container.textContent).toBe('')
    }
  )
})
