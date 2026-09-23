import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  toAuthConsentAgreements,
  toRecordConsentRequest,
  legalAgreementsQueryOptions,
  legalAgreementQueryOptions,
  consentStatusQueryOptions,
  legalAdminAgreementsQueryOptions,
  legalVersionQueryOptions,
  legalDraftQueryOptions,
  recordConsentMutation,
  deleteAccountMutation,
  publishCustomAgreementMutation,
  revertToDefaultAgreementMutation,
  saveDraftMutation,
  discardDraftMutation,
  publishFromDraftMutation,
} from '@/data/query-options'
import type { LegalAgreementSummary } from '@/lib/api-generated'

vi.mock('@/lib/api-generated/sdk.gen', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/api-generated/sdk.gen')>()
  return {
    ...original,
    listAgreements: vi.fn(),
    getAgreement: vi.fn(),
    getConsentStatus: vi.fn(),
    recordConsent: vi.fn(),
    deleteAccount: vi.fn(),
    handleBeginReauth: vi.fn(),
    handleVerifyReauth: vi.fn(),
    adminListAgreements: vi.fn(),
    adminGetVersion: vi.fn(),
    adminPublishCustom: vi.fn(),
    adminRevertToDefault: vi.fn(),
    adminGetDraft: vi.fn(),
    adminSaveDraft: vi.fn(),
    adminPublishFromDraft: vi.fn(),
    adminDiscardDraft: vi.fn(),
  }
})

import {
  listAgreements,
  getAgreement,
  getConsentStatus,
  recordConsent,
  deleteAccount,
  handleBeginReauth,
  handleVerifyReauth,
  adminListAgreements,
  adminGetVersion,
  adminPublishCustom,
  adminRevertToDefault,
  adminGetDraft,
  adminSaveDraft,
  adminPublishFromDraft,
  adminDiscardDraft,
} from '@/lib/api-generated/sdk.gen'

function makeAgreementSummary(overrides?: Partial<LegalAgreementSummary>): LegalAgreementSummary {
  return {
    agreement_type: 'terms_of_service',
    version_id: 'version-001',
    version_no: 1,
    effective_at: '2026-06-30T00:00:00Z',
    title: 'Terms of Service',
    summary: null,
    ...overrides,
  }
}

describe('toAuthConsentAgreements', () => {
  it('maps snake_case summary fields to camelCase auth retry shape', () => {
    const summaries = [
      makeAgreementSummary({
        agreement_type: 'terms_of_service',
        version_id: 'tos-v2',
      }),
      makeAgreementSummary({
        agreement_type: 'privacy_policy',
        version_id: 'privacy-v3',
      }),
    ]

    const result = toAuthConsentAgreements(summaries)

    expect(result).toEqual([
      { agreementType: 'terms_of_service', versionId: 'tos-v2' },
      { agreementType: 'privacy_policy', versionId: 'privacy-v3' },
    ])
  })

  it('returns empty array for empty input', () => {
    expect(toAuthConsentAgreements([])).toEqual([])
  })
})

describe('toRecordConsentRequest', () => {
  it('maps snake_case summary fields to record consent request body', () => {
    const summaries = [
      makeAgreementSummary({
        agreement_type: 'terms_of_service',
        version_id: 'tos-v2',
      }),
    ]

    const result = toRecordConsentRequest(summaries)

    expect(result).toEqual({
      agreements: [{ agreement_type: 'terms_of_service', version_id: 'tos-v2' }],
    })
  })
})

describe('legalAgreementsQueryOptions', () => {
  beforeEach(() => {
    vi.mocked(listAgreements).mockResolvedValue({
      data: { agreements: [makeAgreementSummary()] },
      error: undefined,
    })
  })

  it('calls listAgreements with correct path params', async () => {
    const options = legalAgreementsQueryOptions('realm-1')
    await options.queryFn()

    expect(listAgreements).toHaveBeenCalledWith({ path: { realmId: 'realm-1' } })
  })

  it('returns agreements array', async () => {
    const options = legalAgreementsQueryOptions('realm-1')
    const result = await options.queryFn()

    expect(result).toEqual({ agreements: [makeAgreementSummary()] })
  })

  it('throws when API returns error', async () => {
    vi.mocked(listAgreements).mockResolvedValue({
      data: undefined,
      error: { message: 'Not found', status: 404 },
    })

    const options = legalAgreementsQueryOptions('realm-1')
    await expect(options.queryFn()).rejects.toThrow('Not found')
  })
})

describe('legalAgreementQueryOptions', () => {
  const detailResponse = {
    ...makeAgreementSummary(),
    content: { en: 'Terms body' },
  }

  beforeEach(() => {
    vi.mocked(getAgreement).mockResolvedValue({
      data: detailResponse,
      error: undefined,
    })
  })

  it('calls getAgreement with correct path params', async () => {
    const options = legalAgreementQueryOptions('realm-1', 'terms_of_service')
    await options.queryFn()

    expect(getAgreement).toHaveBeenCalledWith({
      path: { realmId: 'realm-1', agreementType: 'terms_of_service' },
    })
  })

  it('returns agreement detail', async () => {
    const options = legalAgreementQueryOptions('realm-1', 'terms_of_service')
    const result = await options.queryFn()

    expect(result).toEqual(detailResponse)
  })
})

describe('consentStatusQueryOptions', () => {
  const statusResponse = {
    items: [
      {
        agreement_type: 'terms_of_service' as const,
        current_version_id: 'version-001',
        consented_version_id: 'version-001',
        needs_reconsent: false,
      },
    ],
  }

  beforeEach(() => {
    vi.mocked(getConsentStatus).mockResolvedValue({
      data: statusResponse,
      error: undefined,
    })
  })

  it('calls getConsentStatus with correct path params', async () => {
    const options = consentStatusQueryOptions('realm-1')
    await options.queryFn()

    expect(getConsentStatus).toHaveBeenCalledWith({})
  })

  it('returns consent status response', async () => {
    const options = consentStatusQueryOptions('realm-1')
    const result = await options.queryFn()

    expect(result).toEqual(statusResponse)
  })
})

describe('legalAdminAgreementsQueryOptions', () => {
  const adminResponse = {
    agreements: [
      {
        agreement_type: 'terms_of_service' as const,
        source: 'default' as const,
        current_version: makeAgreementSummary(),
        history: [],
      },
    ],
  }

  beforeEach(() => {
    vi.mocked(adminListAgreements).mockResolvedValue({
      data: adminResponse,
      error: undefined,
    })
  })

  it('calls adminListAgreements with correct path params', async () => {
    const options = legalAdminAgreementsQueryOptions('realm-1')
    await options.queryFn()

    expect(adminListAgreements).toHaveBeenCalledWith({})
  })

  it('returns admin agreements response', async () => {
    const options = legalAdminAgreementsQueryOptions('realm-1')
    const result = await options.queryFn()

    expect(result).toEqual(adminResponse)
  })
})

describe('legalVersionQueryOptions', () => {
  const versionResponse = {
    agreement_type: 'terms_of_service' as const,
    version_no: 2,
    version_label: 'Summer update',
    content: { en: '# Terms body' },
    effective_at: '2026-07-01T00:00:00Z',
  }

  beforeEach(() => {
    vi.mocked(adminGetVersion).mockResolvedValue({
      data: versionResponse,
      error: undefined,
    })
  })

  it('calls adminGetVersion with correct path params', async () => {
    const options = legalVersionQueryOptions('realm-1', 'tos-v2')
    await options.queryFn()

    expect(adminGetVersion).toHaveBeenCalledWith({
      path: { versionId: 'tos-v2' },
    })
  })

  it('returns the version detail with full body', async () => {
    const options = legalVersionQueryOptions('realm-1', 'tos-v2')
    const result = await options.queryFn()

    expect(result).toEqual(versionResponse)
  })

  it('rethrows when the SDK returns an error', async () => {
    vi.mocked(adminGetVersion).mockResolvedValue({
      data: undefined,
      error: { status: 404, message: 'Agreement version not found' } as never,
    })

    const options = legalVersionQueryOptions('realm-1', 'missing')
    await expect(options.queryFn()).rejects.toBeDefined()
  })
})

describe('recordConsentMutation', () => {
  beforeEach(() => {
    vi.mocked(recordConsent).mockResolvedValue({
      data: undefined,
      error: undefined,
    })
  })

  it('calls recordConsent with realm and request body', async () => {
    const request = toRecordConsentRequest([makeAgreementSummary()])
    await recordConsentMutation('realm-1', request)

    expect(recordConsent).toHaveBeenCalledWith({
      body: request,
    })
  })

  it('throws when API returns error', async () => {
    vi.mocked(recordConsent).mockResolvedValue({
      data: undefined,
      error: { message: 'Conflict', status: 409 },
    })

    const request = toRecordConsentRequest([makeAgreementSummary()])
    await expect(recordConsentMutation('realm-1', request)).rejects.toEqual({
      message: 'Conflict',
      status: 409,
    })
  })
})

describe('deleteAccountMutation', () => {
  beforeEach(() => {
    vi.mocked(deleteAccount).mockResolvedValue({
      data: undefined,
      error: undefined,
    })
    // Reauth flow (delete_account): begin → verify → single-use ticket.
    vi.mocked(handleBeginReauth).mockResolvedValue({
      data: { availableFactors: ['password'] },
      error: undefined,
    })
    vi.mocked(handleVerifyReauth).mockResolvedValue({
      data: { reauthToken: 'reauth-token-123', expiresIn: 120 },
      error: undefined,
    })
  })

  it('obtains a reauth ticket then calls deleteAccount with reauthToken body', async () => {
    await deleteAccountMutation('secret')

    expect(handleBeginReauth).toHaveBeenCalledWith({ body: { targetOperation: 'delete_account' } })
    expect(handleVerifyReauth).toHaveBeenCalledWith({
      body: { targetOperation: 'delete_account', factor: 'password', password: 'secret' },
    })
    expect(deleteAccount).toHaveBeenCalledWith({ body: { reauthToken: 'reauth-token-123' } })
  })

  it('throws when API returns error', async () => {
    vi.mocked(deleteAccount).mockResolvedValue({
      data: undefined,
      error: { message: 'Unauthorized', status: 401 },
    })

    await expect(deleteAccountMutation('wrong')).rejects.toEqual({
      message: 'Unauthorized',
      status: 401,
    })
  })
})

describe('publishCustomAgreementMutation', () => {
  const publishResponse = {
    version_id: 'version-new',
    version_no: 2,
    effective_at: '2026-06-30T00:00:00Z',
  }

  beforeEach(() => {
    vi.mocked(adminPublishCustom).mockResolvedValue({
      data: publishResponse,
      error: undefined,
    })
  })

  it('calls adminPublishCustom with path and body', async () => {
    const body = { content: { en: 'Custom terms' } }
    const result = await publishCustomAgreementMutation('realm-1', 'terms_of_service', body)

    expect(adminPublishCustom).toHaveBeenCalledWith({
      path: { agreementType: 'terms_of_service' },
      body,
    })
    expect(result).toEqual(publishResponse)
  })
})

describe('revertToDefaultAgreementMutation', () => {
  const revertResponse = {
    version_id: 'version-reverted',
    version_no: 3,
    effective_at: '2026-06-30T00:00:00Z',
  }

  beforeEach(() => {
    vi.mocked(adminRevertToDefault).mockResolvedValue({
      data: revertResponse,
      error: undefined,
    })
  })

  it('calls adminRevertToDefault with path params', async () => {
    const result = await revertToDefaultAgreementMutation('realm-1', 'terms_of_service')

    expect(adminRevertToDefault).toHaveBeenCalledWith({
      path: { agreementType: 'terms_of_service' },
    })
    expect(result).toEqual(revertResponse)
  })
})

describe('legalDraftQueryOptions', () => {
  const draftResponse = {
    agreement_type: 'terms_of_service' as const,
    content: { en: 'draft body' },
    version_label: 'wip',
    updated_at: '2026-07-01T00:00:00Z',
  }

  it('calls adminGetDraft and returns the staged draft', async () => {
    vi.mocked(adminGetDraft).mockResolvedValue({ data: draftResponse, error: undefined })
    const options = legalDraftQueryOptions('realm-1', 'terms_of_service')
    const result = await options.queryFn()

    expect(adminGetDraft).toHaveBeenCalledWith({
      path: { agreementType: 'terms_of_service' },
    })
    expect(result).toEqual(draftResponse)
  })

  it('collapses a 404 (no draft) to null instead of erroring', async () => {
    vi.mocked(adminGetDraft).mockResolvedValue({
      data: undefined,
      error: { message: 'Not found', status: 404 },
    })
    const options = legalDraftQueryOptions('realm-1', 'terms_of_service')
    await expect(options.queryFn()).resolves.toBeNull()
  })

  it('retries transient (non-404) errors so a flake never blanks the form', async () => {
    // WHY: a 500/network flake must not be misread as "no draft" — that would
    // silently discard an admin's in-progress edit. The retry function must
    // return true for such errors so react-query gets a second shot.
    vi.mocked(adminGetDraft).mockResolvedValue({
      data: undefined,
      error: { message: 'Server error', status: 500 },
    })
    const options = legalDraftQueryOptions('realm-1', 'terms_of_service')
    const retry = options.retry as (failureCount: number, error: unknown) => boolean
    expect(retry(0, new Error('Server error'))).toBe(true)
  })

  it('rethrows non-404 errors', async () => {
    vi.mocked(adminGetDraft).mockResolvedValue({
      data: undefined,
      error: { message: 'Forbidden', status: 403 },
    })
    const options = legalDraftQueryOptions('realm-1', 'terms_of_service')
    await expect(options.queryFn()).rejects.toThrow('Forbidden')
  })
})

describe('saveDraftMutation', () => {
  const draftResponse = {
    agreement_type: 'terms_of_service' as const,
    content: { en: 'draft body' },
    version_label: 'wip',
    updated_at: '2026-07-01T00:00:00Z',
  }

  beforeEach(() => {
    vi.mocked(adminSaveDraft).mockResolvedValue({ data: draftResponse, error: undefined })
  })

  it('calls adminSaveDraft with path and body', async () => {
    const body = { content: { en: 'draft body' }, version_label: 'wip' }
    const result = await saveDraftMutation('realm-1', 'terms_of_service', body)

    expect(adminSaveDraft).toHaveBeenCalledWith({
      path: { agreementType: 'terms_of_service' },
      body,
    })
    expect(result).toEqual(draftResponse)
  })

  it('throws when API returns error', async () => {
    vi.mocked(adminSaveDraft).mockResolvedValue({
      data: undefined,
      error: { message: 'Forbidden', status: 403 },
    })
    await expect(
      saveDraftMutation('realm-1', 'terms_of_service', { content: { en: 'x' } })
    ).rejects.toEqual({ message: 'Forbidden', status: 403 })
  })
})

describe('discardDraftMutation', () => {
  beforeEach(() => {
    vi.mocked(adminDiscardDraft).mockResolvedValue({ data: undefined, error: undefined })
  })

  it('calls adminDiscardDraft with path params', async () => {
    await discardDraftMutation('realm-1', 'terms_of_service')

    expect(adminDiscardDraft).toHaveBeenCalledWith({
      path: { agreementType: 'terms_of_service' },
    })
  })

  it('throws when API returns error', async () => {
    vi.mocked(adminDiscardDraft).mockResolvedValue({
      data: undefined,
      error: { message: 'Forbidden', status: 403 },
    })
    await expect(discardDraftMutation('realm-1', 'terms_of_service')).rejects.toEqual({
      message: 'Forbidden',
      status: 403,
    })
  })
})

describe('publishFromDraftMutation', () => {
  const publishResponse = {
    version_id: 'version-new',
    version_no: 2,
    effective_at: '2026-07-01T00:00:00Z',
  }

  beforeEach(() => {
    vi.mocked(adminPublishFromDraft).mockResolvedValue({ data: publishResponse, error: undefined })
  })

  it('calls adminPublishFromDraft with empty body when no override given', async () => {
    const result = await publishFromDraftMutation('realm-1', 'terms_of_service')

    expect(adminPublishFromDraft).toHaveBeenCalledWith({
      path: { agreementType: 'terms_of_service' },
      body: {},
    })
    expect(result).toEqual(publishResponse)
  })

  it('passes version_label override in the body when provided', async () => {
    await publishFromDraftMutation('realm-1', 'terms_of_service', 'final label')

    expect(adminPublishFromDraft).toHaveBeenCalledWith({
      path: { agreementType: 'terms_of_service' },
      body: { version_label: 'final label' },
    })
  })

  it('throws when API returns error', async () => {
    vi.mocked(adminPublishFromDraft).mockResolvedValue({
      data: undefined,
      error: { message: 'No draft', status: 404 },
    })
    await expect(publishFromDraftMutation('realm-1', 'terms_of_service')).rejects.toEqual({
      message: 'No draft',
      status: 404,
    })
  })
})
