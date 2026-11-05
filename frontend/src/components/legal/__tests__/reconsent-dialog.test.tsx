import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ReconsentDialog } from '../ReconsentDialog'
import type { ConsentStatusItem } from '@/lib/api-generated'

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

describe('ReconsentDialog', () => {
  const realmId = 'test-realm'

  const pendingItems: ConsentStatusItem[] = [
    {
      agreement_type: 'terms_of_service',
      current_version_id: 'tos-v2',
      consented_version_id: 'tos-v1',
      needs_reconsent: true,
    },
    {
      agreement_type: 'privacy_policy',
      current_version_id: 'pp-v2',
      consented_version_id: 'pp-v1',
      needs_reconsent: true,
    },
  ]

  it('GIVEN pending agreements WHEN rendered THEN shows title, description and agreement links', () => {
    render(
      <ReconsentDialog
        realmId={realmId}
        open={true}
        items={pendingItems}
        isPending={false}
        onAgree={vi.fn()}
        onLogout={vi.fn()}
      />
    )

    expect(screen.getByTestId('reconsent-dialog-title')).toHaveTextContent('Updated Agreements')
    expect(screen.getByTestId('reconsent-dialog-description')).toHaveTextContent(
      'Please review and agree to the updated agreements to continue.'
    )
    expect(screen.getByTestId('reconsent-agreement-terms_of_service')).toBeInTheDocument()
    expect(screen.getByTestId('reconsent-agreement-privacy_policy')).toBeInTheDocument()
    expect(screen.getByTestId('terms-of-service-link')).toHaveAttribute(
      'href',
      `/${realmId}/legal/terms_of_service`
    )
    expect(screen.getByTestId('privacy-policy-link')).toHaveAttribute(
      'href',
      `/${realmId}/legal/privacy_policy`
    )
  })

  it('GIVEN only one pending agreement THEN renders only the pending item', () => {
    const singlePending: ConsentStatusItem[] = [
      {
        agreement_type: 'terms_of_service',
        current_version_id: 'tos-v2',
        consented_version_id: 'tos-v1',
        needs_reconsent: true,
      },
      {
        agreement_type: 'privacy_policy',
        current_version_id: 'pp-v2',
        consented_version_id: 'pp-v2',
        needs_reconsent: false,
      },
    ]

    render(
      <ReconsentDialog
        realmId={realmId}
        open={true}
        items={singlePending}
        isPending={false}
        onAgree={vi.fn()}
        onLogout={vi.fn()}
      />
    )

    expect(screen.getByTestId('reconsent-agreement-terms_of_service')).toBeInTheDocument()
    expect(screen.queryByTestId('reconsent-agreement-privacy_policy')).not.toBeInTheDocument()
  })

  it('WHEN agree button is clicked THEN calls onAgree', async () => {
    const onAgree = vi.fn()
    const user = userEvent.setup()

    render(
      <ReconsentDialog
        realmId={realmId}
        open={true}
        items={pendingItems}
        isPending={false}
        onAgree={onAgree}
        onLogout={vi.fn()}
      />
    )

    await user.click(screen.getByTestId('reconsent-agree-button'))

    expect(onAgree).toHaveBeenCalledTimes(1)
  })

  it('WHEN logout button is clicked THEN calls onLogout', async () => {
    const onLogout = vi.fn()
    const user = userEvent.setup()

    render(
      <ReconsentDialog
        realmId={realmId}
        open={true}
        items={pendingItems}
        isPending={false}
        onAgree={vi.fn()}
        onLogout={onLogout}
      />
    )

    await user.click(screen.getByTestId('reconsent-logout-button'))

    expect(onLogout).toHaveBeenCalledTimes(1)
  })
})
