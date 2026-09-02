import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { AgreementLinks } from '../AgreementLinks'

vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    Link: ({
      to,
      params,
      children,
      ...props
    }: {
      to: string
      params?: Record<string, string>
      children?: React.ReactNode
    }) => {
      let href = to as string
      if (params) {
        Object.entries(params).forEach(([key, value]) => {
          href = href.replace(new RegExp(`\\$\\{${key}\\}|\\$${key}`, 'g'), value)
        })
      }
      return (
        <a href={href} {...props}>
          {children}
        </a>
      )
    },
  }
})

describe('AgreementLinks', () => {
  const realmId = 'test-realm'

  it('GIVEN a realmId WHEN rendering THEN shows ToS and Privacy Policy links with correct hrefs', async () => {
    renderLinks(<AgreementLinks realmId={realmId} beforeText="I agree to " />)

    const termsLink = screen.getByTestId('terms-of-service-link')
    const privacyLink = screen.getByTestId('privacy-policy-link')

    expect(termsLink).toHaveTextContent('Terms of Service')
    expect(termsLink).toHaveAttribute('href', `/${realmId}/legal/terms_of_service`)
    expect(privacyLink).toHaveTextContent('Privacy Policy')
    expect(privacyLink).toHaveAttribute('href', `/${realmId}/legal/privacy_policy`)
  })

  it('opens a configured external agreement safely in a new tab', async () => {
    renderLinks(
      <AgreementLinks
        realmId={realmId}
        agreementType="terms_of_service"
        agreements={[
          {
            agreement_type: 'terms_of_service',
            version_id: 'v1',
            version_no: 1,
            effective_at: new Date().toISOString(),
            mode: 'link',
            external_url: 'https://example.com/terms',
          },
        ]}
      />
    )
    const link = await screen.findByTestId('terms-of-service-link')
    expect(link).toHaveAttribute('href', 'https://example.com/terms')
    expect(link).toHaveAttribute('target', '_blank')
    expect(link).toHaveAttribute('rel', 'noopener noreferrer')
  })
})
function renderLinks(node: React.ReactNode) {
  return render(node)
}
