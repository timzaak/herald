import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import type { ReactNode } from 'react'
import { LegalAgreementPage } from '../$agreementType'

const realmId = 'test-realm'

vi.mock('@/components/shared/locale-provider', () => ({
  useLocale: () => ({ locale: 'en', switchLocale: vi.fn() }),
}))

vi.mock('@tanstack/react-router', async () => {
  const actual =
    await vi.importActual<typeof import('@tanstack/react-router')>('@tanstack/react-router')
  return {
    ...actual,
    createFileRoute: () => (config: Record<string, unknown>) => ({
      useParams: () => ({ realmId, agreementType: 'terms_of_service' }),
      ...config,
    }),
    Link: ({
      to,
      params,
      children,
      ...props
    }: {
      to: string
      params?: Record<string, string>
      children?: ReactNode
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

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
}

function renderAgreementPage() {
  const queryClient = createTestQueryClient()
  return render(
    <QueryClientProvider client={queryClient}>
      <LegalAgreementPage />
    </QueryClientProvider>
  )
}

function setupAgreementHandler(response: object, status = 200) {
  server.use(
    http.get('/api/legal/public/:realmId/agreements/:agreementType', () =>
      HttpResponse.json(response, { status })
    )
  )
}

describe('LegalAgreementPage', () => {
  it('shows an external-link action instead of rendering an empty body in link mode', async () => {
    setupAgreementHandler({
      agreement_type: 'terms_of_service',
      version_id: 'v1',
      version_no: 2,
      effective_at: new Date().toISOString(),
      content: {},
      mode: 'link',
      external_url: 'https://example.com/terms',
    })
    renderAgreementPage()
    const card = await screen.findByTestId('agreement-external-link')
    expect(card.querySelector('a')).toHaveAttribute('href', 'https://example.com/terms')
    expect(card.querySelector('a')).toHaveAttribute('target', '_blank')
  })
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders body content for a valid agreement', async () => {
    setupAgreementHandler({
      agreement_type: 'terms_of_service',
      version_id: 'tos-v2',
      version_no: 2,
      effective_at: '2026-06-30T00:00:00Z',
      content: 'These are the terms of service.',
    })

    renderAgreementPage()

    expect(await screen.findByTestId('agreement-card')).toBeInTheDocument()
    expect(screen.getByTestId('agreement-body')).toHaveTextContent(
      'These are the terms of service.'
    )
  })

  it('renders Markdown content as HTML', async () => {
    setupAgreementHandler({
      agreement_type: 'terms_of_service',
      version_id: 'tos-v2',
      version_no: 2,
      effective_at: '2026-06-30T00:00:00Z',
      content: '# Heading\n\nSome **bold** text.',
    })

    renderAgreementPage()

    const body = await screen.findByTestId('agreement-body')
    expect(body).toHaveTextContent('Heading')
    expect(body).toHaveTextContent('Some bold text.')
  })

  it('renders body as JSON when content is an object', async () => {
    setupAgreementHandler({
      agreement_type: 'terms_of_service',
      version_id: 'tos-v2',
      version_no: 2,
      effective_at: '2026-06-30T00:00:00Z',
      content: { en: 'English terms' },
    })

    renderAgreementPage()

    const body = await screen.findByTestId('agreement-body')
    expect(body).toHaveTextContent('English terms')
  })

  it('shows empty body message when content is null', async () => {
    setupAgreementHandler({
      agreement_type: 'terms_of_service',
      version_id: 'tos-v2',
      version_no: 2,
      effective_at: '2026-06-30T00:00:00Z',
      content: null,
    })

    renderAgreementPage()

    expect(await screen.findByTestId('agreement-empty-body')).toBeInTheDocument()
  })

  it('shows loading state while fetching', () => {
    setupAgreementHandler({
      agreement_type: 'terms_of_service',
      version_id: 'tos-v1',
      version_no: 1,
      effective_at: '2026-06-30T00:00:00Z',
      content: 'Terms',
    })

    renderAgreementPage()

    expect(screen.getByTestId('agreement-loading')).toBeInTheDocument()
  })

  it('shows error state when API fails', async () => {
    setupAgreementHandler({ message: 'Server error' }, 500)

    renderAgreementPage()

    expect(await screen.findByTestId('agreement-error', {}, { timeout: 3000 })).toBeInTheDocument()
    expect(screen.getByText('Server error')).toBeInTheDocument()
  })
})
