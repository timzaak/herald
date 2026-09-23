import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { server } from '@/test/mocks/server'
import { renderWithProviders } from '@/test/utils/render'
import { LegalAgreementTab } from '../LegalAgreementTab'
import type { AdminAgreementView } from '@/lib/api-generated'

const realmId = 'test-realm'

function makeVersionSummary(overrides?: Record<string, unknown>) {
  return {
    version_id: 'version-001',
    version_no: 1,
    effective_at: '2026-06-30T00:00:00Z',
    source: 'default' as const,
    version_label: null,
    ...overrides,
  }
}

function makeAgreementView(overrides?: Partial<AdminAgreementView>): AdminAgreementView {
  return {
    agreement_type: 'terms_of_service',
    source: 'default',
    current_version: makeVersionSummary({
      agreement_type: 'terms_of_service',
      version_id: 'tos-v1',
      version_no: 1,
    }) as AdminAgreementView['current_version'],
    history: [],
    ...overrides,
  }
}

function setupAdminAgreementsHandler(response: { agreements: AdminAgreementView[] }, status = 200) {
  server.use(
    http.get(`http://localhost:3000/api/legal/admin/agreements`, () =>
      HttpResponse.json(response, { status })
    )
  )
}

// Stub the per-(realm,type) draft GET to return a 404 ("no draft") by default.
// Individual tests override with a 200 when they need a staged draft. The 404
// path is the normal "no draft yet" state the query option collapses to null.
function setupDraftNotFound() {
  server.use(
    http.get(
      `http://localhost:3000/api/legal/admin/agreements/terms_of_service/draft`,
      () => new HttpResponse(null, { status: 404 })
    ),
    http.get(
      `http://localhost:3000/api/legal/admin/agreements/privacy_policy/draft`,
      () => new HttpResponse(null, { status: 404 })
    )
  )
}

describe('LegalAgreementTab', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    setupDraftNotFound()
  })

  it('shows loading state while fetching agreements', () => {
    setupAdminAgreementsHandler({ agreements: [] })
    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage={false} />)

    expect(screen.getByTestId('legal-agreements-loading')).toBeInTheDocument()
  })

  it('shows error state and allows retry', async () => {
    setupAdminAgreementsHandler({ agreements: [] }, 500)
    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage={false} />)

    const errorAlert = await screen.findByTestId('legal-agreements-error', {}, { timeout: 3000 })
    expect(errorAlert).toBeInTheDocument()

    const retryButton = screen.getByTestId('legal-agreements-retry')
    expect(retryButton).toBeInTheDocument()
  })

  it('shows empty state when no agreements exist', async () => {
    setupAdminAgreementsHandler({ agreements: [] })
    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage={false} />)

    expect(await screen.findByTestId('legal-agreements-empty')).toBeInTheDocument()
  })

  it('renders view-only notice and hides controls without manage permission', async () => {
    setupAdminAgreementsHandler({
      agreements: [
        makeAgreementView({
          agreement_type: 'terms_of_service',
          source: 'custom',
        }),
        makeAgreementView({
          agreement_type: 'privacy_policy',
          source: 'default',
          current_version: makeVersionSummary({
            agreement_type: 'privacy_policy',
            version_id: 'privacy-v1',
            version_no: 1,
          }) as AdminAgreementView['current_version'],
        }),
      ],
    })

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage={false} />)

    expect(await screen.findByTestId('legal-agreements-view-only')).toBeInTheDocument()
    expect(screen.getByTestId('legal-agreement-view-only-terms_of_service')).toBeInTheDocument()
    expect(screen.queryByTestId('legal-publish-button-terms_of_service')).not.toBeInTheDocument()
    expect(screen.queryByTestId('legal-revert-button-terms_of_service')).not.toBeInTheDocument()
  })

  it('renders draft + publish + preview controls and hides revert for default source with manage permission', async () => {
    setupAdminAgreementsHandler({
      agreements: [
        makeAgreementView({
          agreement_type: 'terms_of_service',
          source: 'default',
        }),
      ],
    })

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage />)

    expect(await screen.findByTestId('legal-publish-button-terms_of_service')).toBeInTheDocument()
    expect(screen.getByTestId('legal-save-draft-button-terms_of_service')).toBeInTheDocument()
    expect(screen.getByTestId('legal-preview-button-terms_of_service')).toBeInTheDocument()
    expect(screen.queryByTestId('legal-revert-button-terms_of_service')).not.toBeInTheDocument()
    expect(screen.getByTestId('legal-history-empty-terms_of_service')).toBeInTheDocument()
  })

  it('renders version history entries with source badges', async () => {
    setupAdminAgreementsHandler({
      agreements: [
        makeAgreementView({
          agreement_type: 'terms_of_service',
          source: 'custom',
          history: [
            makeVersionSummary({
              version_id: 'tos-v1',
              version_no: 1,
              source: 'default',
            }),
            makeVersionSummary({
              version_id: 'tos-v2',
              version_no: 2,
              source: 'custom',
              version_label: 'June update',
            }),
          ],
        }),
      ],
    })

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage={false} />)

    await screen.findByTestId('legal-history-table-terms_of_service')
    expect(screen.getByTestId('legal-history-row-terms_of_service-tos-v1')).toBeInTheDocument()
    expect(screen.getByTestId('legal-history-row-terms_of_service-tos-v2')).toBeInTheDocument()
    expect(screen.getAllByTestId('source-badge-default')).toHaveLength(1)
    expect(screen.getAllByTestId('source-badge-custom')).toHaveLength(2)
  })

  it('validates that at least one locale content is provided before publishing', async () => {
    setupAdminAgreementsHandler({
      agreements: [makeAgreementView()],
    })

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage />)
    const user = userEvent.setup()

    const publishButton = await screen.findByTestId('legal-publish-button-terms_of_service')
    await user.click(publishButton)

    expect(await screen.findByText(/English content is required/i)).toBeInTheDocument()
  })

  it('publishes from a draft: saves draft then publishes, invalidating the admin agreements query', async () => {
    let listCalls = 0
    let saveDraftBody: unknown
    let publishBody: unknown
    let publishCalls = 0

    server.use(
      http.get(`http://localhost:3000/api/legal/admin/agreements`, () => {
        listCalls += 1
        return HttpResponse.json({
          agreements: [
            makeAgreementView({
              agreement_type: 'terms_of_service',
              source: 'default',
            }),
          ],
        })
      }),
      http.put(
        `http://localhost:3000/api/legal/admin/agreements/terms_of_service/draft`,
        async ({ request }) => {
          saveDraftBody = await request.json()
          return HttpResponse.json({
            agreement_type: 'terms_of_service',
            content: { en: 'Updated terms' },
            version_label: 'Summer update',
            updated_at: '2026-07-01T00:00:00Z',
          })
        }
      ),
      http.post(
        `http://localhost:3000/api/legal/admin/agreements/terms_of_service/publish`,
        async ({ request }) => {
          publishCalls += 1
          publishBody = await request.json().catch(() => null)
          return HttpResponse.json({
            version_id: 'tos-v2',
            version_no: 2,
            effective_at: '2026-07-01T00:00:00Z',
          })
        }
      )
    )

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage />)
    const user = userEvent.setup()

    await screen.findByTestId('legal-publish-button-terms_of_service')

    await user.type(
      screen.getByTestId('legal-version-label-input-terms_of_service'),
      'Summer update'
    )
    await user.type(screen.getByTestId('legal-content-en-input-terms_of_service'), 'Updated terms')
    await user.click(screen.getByTestId('legal-publish-button-terms_of_service'))

    // Publish saves the draft first (PUT .../draft), then publishes (POST .../publish).
    await waitFor(() => {
      expect(saveDraftBody).toEqual({
        content: { en: 'Updated terms' },
        version_label: 'Summer update',
        mode: 'full_text',
        external_url: null,
      })
    })
    await waitFor(() => {
      expect(publishCalls).toBe(1)
    })
    // The publish body carries the version_label override (the form's current label).
    expect(publishBody).toEqual({ version_label: 'Summer update' })

    await waitFor(() => {
      expect(listCalls).toBeGreaterThanOrEqual(2)
    })
  })

  it('saves a draft without publishing when Save Draft is clicked', async () => {
    let saveDraftBody: unknown
    let saveCalls = 0
    let publishCalls = 0

    server.use(
      http.get(`http://localhost:3000/api/legal/admin/agreements`, () =>
        HttpResponse.json({
          agreements: [makeAgreementView({ source: 'default' })],
        })
      ),
      http.put(
        `http://localhost:3000/api/legal/admin/agreements/terms_of_service/draft`,
        async ({ request }) => {
          saveCalls += 1
          saveDraftBody = await request.json()
          return HttpResponse.json({
            agreement_type: 'terms_of_service',
            content: { en: 'work in progress' },
            version_label: 'wip',
            updated_at: '2026-07-01T00:00:00Z',
          })
        }
      ),
      http.post(`http://localhost:3000/api/legal/admin/agreements/terms_of_service/publish`, () => {
        publishCalls += 1
        return HttpResponse.json({
          version_id: 'tos-v2',
          version_no: 2,
          effective_at: '2026-07-01T00:00:00Z',
        })
      })
    )

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage />)
    const user = userEvent.setup()

    await screen.findByTestId('legal-save-draft-button-terms_of_service')
    await user.type(
      screen.getByTestId('legal-content-en-input-terms_of_service'),
      'work in progress'
    )
    await user.type(screen.getByTestId('legal-version-label-input-terms_of_service'), 'wip')
    await user.click(screen.getByTestId('legal-save-draft-button-terms_of_service'))

    await waitFor(() => {
      expect(saveCalls).toBe(1)
    })
    expect(saveDraftBody).toEqual({
      content: { en: 'work in progress' },
      version_label: 'wip',
      mode: 'full_text',
      external_url: null,
    })
    // Saving a draft must NOT publish.
    expect(publishCalls).toBe(0)
  })

  it('opens a Markdown preview dialog rendering the current content', async () => {
    setupAdminAgreementsHandler({
      agreements: [makeAgreementView({ source: 'default' })],
    })

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage />)
    const user = userEvent.setup()

    await screen.findByTestId('legal-preview-button-terms_of_service')
    await user.type(
      screen.getByTestId('legal-content-en-input-terms_of_service'),
      '# Heading{enter}{enter}**bold** text'
    )
    await user.click(screen.getByTestId('legal-preview-button-terms_of_service'))

    const dialog = await screen.findByTestId('legal-preview-dialog-terms_of_service')
    expect(dialog).toBeInTheDocument()
    // The Markdown renderer turns "# Heading" into an <h1> and "**bold**" into <strong>.
    expect(dialog.querySelector('h1')).not.toBeNull()
    expect(dialog.querySelector('strong')).not.toBeNull()
  })

  it('seeds the edit form from an existing draft', async () => {
    setupAdminAgreementsHandler({
      agreements: [makeAgreementView({ source: 'default' })],
    })
    server.use(
      http.get(`http://localhost:3000/api/legal/admin/agreements/terms_of_service/draft`, () =>
        HttpResponse.json({
          agreement_type: 'terms_of_service',
          content: { en: 'resumed draft body' },
          version_label: 'resumed label',
          updated_at: '2026-07-01T00:00:00Z',
        })
      )
    )

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage />)

    // Wait for the draft to load and seed the form. The discard button only
    // renders when a draft is present, so it doubles as a load gate.
    await screen.findByTestId('legal-discard-draft-button-terms_of_service')
    expect(
      (screen.getByTestId('legal-version-label-input-terms_of_service') as HTMLInputElement).value
    ).toBe('resumed label')
    expect(
      (screen.getByTestId('legal-content-en-input-terms_of_service') as HTMLTextAreaElement).value
    ).toBe('resumed draft body')
  })

  it('does not reset the mode select when a staged draft resolves after the admin edits the form', async () => {
    // Regression: the seed effect used to re-run whenever the React Query
    // `draft` reference changed. In the demo suite a prior run left a staged
    // draft, so this run's draft GET resolved AFTER the admin had already
    // switched `mode` to link — the effect then `form.reset({ mode: 'full_text' })`
    // and clobbered the manual selection, so the external URL field vanished.
    // Reproduce by holding the draft GET (a real staged draft) pending until
    // after the admin picks link.
    let resolveDraft!: () => void
    const draftGate = new Promise<void>((resolve) => {
      resolveDraft = resolve
    })
    setupAdminAgreementsHandler({
      agreements: [makeAgreementView({ source: 'default' })],
    })
    server.use(
      http.get(
        `http://localhost:3000/api/legal/admin/agreements/terms_of_service/draft`,
        async () => {
          await draftGate
          return HttpResponse.json({
            agreement_type: 'terms_of_service',
            content: { en: 'leftover draft body' },
            version_label: 'leftover',
            updated_at: '2026-07-01T00:00:00Z',
            mode: 'full_text',
            pending_version_no: 2,
          })
        }
      )
    )

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage />)
    const user = userEvent.setup()

    // Draft GET is still pending, so the form is blank. The admin switches
    // hosting mode to link; the external URL field mounts.
    const card = await screen.findByTestId('legal-agreement-card-terms_of_service')
    await user.selectOptions(within(card).getByTestId('legal-mode-select-terms_of_service'), 'link')
    await screen.findByTestId('legal-external-url-input-terms_of_service')

    // The staged draft now resolves (mode=full_text). On the buggy code the seed
    // effect re-runs asynchronously and `form.reset({ mode: 'full_text' })`
    // flips the select back to full_text, unmounting the external URL field.
    // Let React flush the post-resolution reseed before asserting, so the test
    // actually observes the regression rather than racing past it.
    resolveDraft()
    await new Promise((resolve) => setTimeout(resolve, 300))

    expect(
      (screen.getByTestId('legal-mode-select-terms_of_service') as HTMLSelectElement).value
    ).toBe('link')
    expect(screen.getByTestId('legal-external-url-input-terms_of_service')).toBeInTheDocument()
  })

  it('discards a draft after confirmation', async () => {
    let discardCalled = false

    server.use(
      http.get(`http://localhost:3000/api/legal/admin/agreements`, () =>
        HttpResponse.json({
          agreements: [makeAgreementView({ source: 'default' })],
        })
      ),
      http.get(`http://localhost:3000/api/legal/admin/agreements/terms_of_service/draft`, () =>
        HttpResponse.json({
          agreement_type: 'terms_of_service',
          content: { en: 'doomed draft' },
          version_label: null,
          updated_at: '2026-07-01T00:00:00Z',
        })
      ),
      http.delete(`http://localhost:3000/api/legal/admin/agreements/terms_of_service/draft`, () => {
        discardCalled = true
        return new HttpResponse(null, { status: 204 })
      })
    )

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage />)
    const user = userEvent.setup()

    await screen.findByTestId('legal-discard-draft-button-terms_of_service')
    await user.click(screen.getByTestId('legal-discard-draft-button-terms_of_service'))

    expect(
      await screen.findByTestId('legal-discard-draft-dialog-title-terms_of_service')
    ).toBeInTheDocument()
    await user.click(screen.getByTestId('legal-discard-draft-confirm-terms_of_service'))

    await waitFor(() => {
      expect(discardCalled).toBe(true)
    })
  })

  it('reverts a custom agreement to platform default after confirmation', async () => {
    let listCalls = 0
    let revertCalled = false

    server.use(
      http.get(`http://localhost:3000/api/legal/admin/agreements`, () => {
        listCalls += 1
        return HttpResponse.json({
          agreements: [
            makeAgreementView({
              agreement_type: 'terms_of_service',
              source: 'custom',
            }),
          ],
        })
      }),
      http.delete(
        `http://localhost:3000/api/legal/admin/agreements/terms_of_service/custom`,
        () => {
          revertCalled = true
          return HttpResponse.json({
            version_id: 'tos-v3',
            version_no: 3,
            effective_at: '2026-07-02T00:00:00Z',
          })
        }
      )
    )

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage />)
    const user = userEvent.setup()

    await screen.findByTestId('legal-revert-button-terms_of_service')
    await user.click(screen.getByTestId('legal-revert-button-terms_of_service'))

    expect(
      await screen.findByTestId('legal-revert-dialog-title-terms_of_service')
    ).toBeInTheDocument()

    await user.click(screen.getByTestId('legal-revert-confirm-terms_of_service'))

    await waitFor(() => {
      expect(revertCalled).toBe(true)
    })

    await waitFor(() => {
      expect(listCalls).toBeGreaterThanOrEqual(2)
    })
  })

  it('disables the preview button when the content textarea is empty', async () => {
    setupAdminAgreementsHandler({
      agreements: [makeAgreementView({ source: 'default' })],
    })

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage />)

    const previewButton = await screen.findByTestId('legal-preview-button-terms_of_service')
    // No content typed yet → preview is disabled (the dialog would only show
    // "Nothing to preview yet." otherwise).
    expect(previewButton).toBeDisabled()
  })

  it('opens a version detail dialog with the past body when a history row View is clicked', async () => {
    let versionCalls = 0

    setupAdminAgreementsHandler({
      agreements: [
        makeAgreementView({
          agreement_type: 'terms_of_service',
          source: 'custom',
          history: [
            makeVersionSummary({
              version_id: 'tos-v1',
              version_no: 1,
              source: 'default',
            }),
          ],
        }),
      ],
    })
    server.use(
      http.get(`http://localhost:3000/api/legal/admin/agreements/versions/tos-v1`, () => {
        versionCalls += 1
        return HttpResponse.json({
          agreement_type: 'terms_of_service',
          version_no: 1,
          version_label: null,
          content: { en: '# Old heading{enter}{enter}legacy body' },
          effective_at: '2026-06-30T00:00:00Z',
        })
      })
    )

    renderWithProviders(<LegalAgreementTab realmId={realmId} canManage={false} />)
    const user = userEvent.setup()

    await screen.findByTestId('legal-history-row-terms_of_service-tos-v1')
    await user.click(screen.getByTestId('legal-history-view-button-terms_of_service-tos-v1'))

    // The version body is fetched on demand and rendered as Markdown.
    const dialog = await screen.findByTestId('legal-version-dialog-terms_of_service')
    expect(dialog).toBeInTheDocument()
    await waitFor(() => {
      expect(dialog.querySelector('h1')).not.toBeNull()
    })
    expect(versionCalls).toBe(1)
  })
})
