import { useQuery } from '@tanstack/react-query'
import { getErrorMessage } from '@/lib/error-utils'
import { useEffect, useState } from 'react'
import { z } from 'zod'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { useStore } from '@tanstack/react-form'
import { TextareaField } from '@/components/shared/form-fields/textarea-field'
import { TextField } from '@/components/shared/form-fields/text-field'
import { MarkdownContent } from '@/components/legal/MarkdownContent'
import {
  legalAdminAgreementsQueryOptions,
  legalDraftQueryOptions,
  legalVersionQueryOptions,
  queryKeys,
  publishFromDraftMutation,
  revertToDefaultAgreementMutation,
  saveDraftMutation,
  discardDraftMutation,
} from '@/data/query-options'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { m } from '@/paraglide/messages'
import { formatDate } from '@/lib/date-utils'
import { AlertCircle } from 'lucide-react'
import type { AdminAgreementView, AgreementType } from '@/lib/api-generated'

const publishSchema = z
  .object({
    versionLabel: z.string().max(100, {
      error: () => m['settings.legal.version_label_max_length'](),
    }),
    contentEn: z.string(),
    mode: z.enum(['full_text', 'link']),
    externalUrl: z.string().max(2048),
  })
  .refine((value) => value.mode !== 'full_text' || value.contentEn.trim().length > 0, {
    error: () => m['settings.legal.content_required'](),
    path: ['contentEn'],
  })
  .refine(
    (value) => {
      if (value.mode !== 'link') return true
      try {
        const url = new URL(value.externalUrl)
        return ['http:', 'https:'].includes(url.protocol)
      } catch {
        return false
      }
    },
    { error: () => m['settings.legal.external_url_required'](), path: ['externalUrl'] }
  )

const SUPPORTED_LOCALES = ['en'] as const

// Agreement bodies are locale-keyed objects (`{ en: '...' }`, per
// SUPPORTED_LOCALES); read the English text shared by draft seeding,
// platform-default prefill, and the past-version dialog.
function contentEnOf(content: unknown): string {
  return typeof content === 'object' && content !== null
    ? String((content as Record<string, unknown>).en ?? '')
    : ''
}

function getAgreementTitle(agreementType: AgreementType): string {
  if (agreementType === 'terms_of_service') {
    return m['legal.terms_of_service']()
  }
  if (agreementType === 'privacy_policy') {
    return m['legal.privacy_policy']()
  }
  return agreementType
}

function SourceBadge({ source }: { source: 'default' | 'custom' }) {
  const isCustom = source === 'custom'
  return (
    <Badge variant={isCustom ? 'secondary' : 'default'} data-testid={`source-badge-${source}`}>
      {isCustom ? m['settings.legal.source_custom']() : m['settings.legal.source_default']()}
    </Badge>
  )
}

function HistoryTable({
  view,
  onView,
}: {
  view: AdminAgreementView
  onView: (entry: AdminAgreementView['history'][number]) => void
}) {
  if (view.history.length === 0) {
    return (
      <p
        className="text-sm text-muted-foreground"
        data-testid={`legal-history-empty-${view.agreement_type}`}
      >
        {m['settings.legal.history_empty']()}
      </p>
    )
  }

  return (
    <Table data-testid={`legal-history-table-${view.agreement_type}`}>
      <TableHeader>
        <TableRow>
          <TableHead>{m['settings.legal.history_version']()}</TableHead>
          <TableHead>{m['settings.legal.version_label_label']()}</TableHead>
          <TableHead>{m['settings.legal.history_source']()}</TableHead>
          <TableHead>{m['settings.legal.history_effective_date']()}</TableHead>
          <TableHead className="sr-only">{m['settings.legal.history_view_button']()}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {view.history.map((entry) => (
          <TableRow
            key={entry.version_id}
            data-testid={`legal-history-row-${view.agreement_type}-${entry.version_id}`}
          >
            <TableCell>{entry.version_no}</TableCell>
            <TableCell>{entry.version_label ?? '-'}</TableCell>
            <TableCell>
              <SourceBadge source={entry.source} />
            </TableCell>
            <TableCell>{formatDate(entry.effective_at)}</TableCell>
            <TableCell>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => onView(entry)}
                data-testid={`legal-history-view-button-${view.agreement_type}-${entry.version_id}`}
              >
                {m['settings.legal.history_view_button']()}
              </Button>
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}

function AgreementCard({
  view,
  realmId,
  canManage,
}: {
  view: AdminAgreementView
  realmId: string
  canManage: boolean
}) {
  const [revertOpen, setRevertOpen] = useState(false)
  const [discardOpen, setDiscardOpen] = useState(false)
  const [previewOpen, setPreviewOpen] = useState(false)
  const [activeVersion, setActiveVersion] = useState<{
    versionId: string
    versionNo: number
  } | null>(null)
  const agreementType = view.agreement_type
  const title = getAgreementTitle(agreementType)

  // Draft state for this (realm, agreement_type). `null` means no draft staged
  // yet (a backend 404); a populated draft seeds the edit form so an admin can
  // resume an in-progress edit across sessions/browsers.
  const { data: draft } = useQuery(legalDraftQueryOptions(realmId, agreementType))

  // Platform-default prefill: a realm still on the platform template
  // (`source === 'default'`) has no realm-scoped rows, so without this the
  // editor would open blank and an admin would have to write a whole document
  // from scratch. Once no-draft is confirmed, fetch the effective version's
  // full body (the platform default row — `admin_get_version` admits
  // realm-less rows) and seed the editor from it; the admin tweaks the
  // template and publishes it as the realm's first custom version.
  // Custom-source realms are NOT prefilled: publishing mints a new immutable
  // version and triggers reconsent, so edits there must start deliberately.
  // Suppress the platform-default prefill after a publish on this mount.
  // Publishing deletes the draft and refetches both queries; the draft-null
  // resolution can land while `view.source` is still the stale 'default', and
  // without this latch that race would reseed the just-published editor with
  // the platform template.
  const [prefillSuppressed, setPrefillSuppressed] = useState(false)

  const shouldPrefillDefault =
    canManage && !prefillSuppressed && view.source === 'default' && draft === null
  const { data: defaultVersion } = useQuery({
    ...legalVersionQueryOptions(realmId, view.current_version.version_id),
    enabled: shouldPrefillDefault,
  })

  // Lazily fetch the full body of a past version when an admin opens it from the
  // history table. Gated on `activeVersion` so nothing is fetched until a row's
  // "View" button is clicked; the history list itself only carries summaries.
  const { data: activeVersionDetail, isLoading: isLoadingVersion } = useQuery({
    ...legalVersionQueryOptions(realmId, activeVersion?.versionId ?? ''),
    enabled: !!activeVersion,
  })

  // Save draft (upsert). Invalidating the draft key refreshes the seeded form.
  const saveDraft = useFormMutation({
    mutationFn: (data: {
      contentEn: string
      versionLabel: string
      mode: 'full_text' | 'link'
      externalUrl: string
    }) => {
      const content: Record<string, string> = {}
      if (data.contentEn.trim()) {
        content[SUPPORTED_LOCALES[0]] = data.contentEn.trim()
      }
      return saveDraftMutation(realmId, agreementType, {
        content,
        version_label: data.versionLabel.trim() || null,
        mode: data.mode,
        external_url: data.mode === 'link' ? data.externalUrl.trim() : null,
      })
    },
    getSuccessMessage: () => m['settings.legal.draft_saved_success'](),
    invalidateQueries: [queryKeys.legalDraft(realmId, agreementType)],
  })

  // Publish from draft. The backend reads the staged draft, creates a new
  // immutable version (advancing version_no, triggering reconsent), and clears
  // the draft. We pass the form's current version_label as an override so the
  // admin can adjust the label at publish time without a separate save.
  const publish = useFormMutation({
    mutationFn: (versionLabel: string | null) =>
      publishFromDraftMutation(realmId, agreementType, versionLabel),
    getSuccessMessage: () => m['settings.legal.publish_success'](),
    invalidateQueries: [
      queryKeys.legalAdminAgreements(realmId),
      queryKeys.legalAgreements(realmId),
      queryKeys.consentStatus(realmId),
      queryKeys.legalDraft(realmId, agreementType),
    ],
    onSuccess: () => {
      form.reset({ versionLabel: '', contentEn: '', mode: 'full_text', externalUrl: '' })
      setPrefillSuppressed(true)
    },
  })

  const revert = useFormMutation({
    mutationFn: () => revertToDefaultAgreementMutation(realmId, agreementType),
    getSuccessMessage: () => m['settings.legal.revert_success'](),
    invalidateQueries: [
      queryKeys.legalAdminAgreements(realmId),
      queryKeys.legalAgreements(realmId),
      queryKeys.consentStatus(realmId),
    ],
    onSuccess: () => setRevertOpen(false),
  })

  const discard = useFormMutation({
    mutationFn: () => discardDraftMutation(realmId, agreementType),
    getSuccessMessage: () => m['settings.legal.discard_draft_success'](),
    invalidateQueries: [queryKeys.legalDraft(realmId, agreementType)],
    onSuccess: () => {
      setDiscardOpen(false)
      form.reset({ versionLabel: '', contentEn: '', mode: 'full_text', externalUrl: '' })
    },
  })

  const form = useAppForm({
    schema: publishSchema,
    defaultValues: {
      versionLabel: '',
      contentEn: '',
      mode: 'full_text' as const,
      externalUrl: '',
    },
    onSubmit: async () => {
      // The form has no direct submit button: Publish/Save Draft each call
      // handleSubmit() then route to the right mutation. This no-op onSubmit
      // keeps the schema validation path used by form.handleSubmit intact.
    },
  })

  // Seed the form once the draft (or, failing that, the platform default
  // template) resolves. A draft is fetched per (realm, agreement_type); until
  // it resolves the form stays blank so an admin never edits a published
  // version's content by mistake. When a draft resolves, prefill both fields;
  // `form.reset` re-runs validation against the new values.
  //
  // Only seed while the form is still pristine. React Query hands back a fresh
  // `draft` reference on every background refetch / window focus / invalidation,
  // and a draft can also resolve *after* the admin has started editing (e.g. a
  // staged draft left by a previous session). Reseeding then would `form.reset`
  // and wipe the in-flight edit — most visibly reverting an admin's manual switch
  // of `mode` to link. Guarding on `isDirty` means an admin's edits always win
  // over a late/background draft resolution. Publish/discard still reset the
  // form explicitly in their `onSuccess` hooks (which clears `isDirty`, so the
  // next draft resolution can seed cleanly).
  const isFormDirty = useStore(form.store, (state) => state.isDirty)
  useEffect(() => {
    if (isFormDirty) return
    const seed = draft ?? (shouldPrefillDefault ? defaultVersion : undefined)
    if (seed) {
      form.reset({
        versionLabel: seed.version_label ?? '',
        contentEn: contentEnOf(seed.content),
        mode: seed.mode ?? view.current_version.mode ?? 'full_text',
        externalUrl: seed.external_url ?? '',
      })
    }
    // form is stable across renders; the seed derives from draft/defaultVersion.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draft, isFormDirty, defaultVersion, shouldPrefillDefault])

  // Publish = save the current edit as a draft, then publish that draft.
  async function handlePublish() {
    await form.handleSubmit()
    if (!form.state.isValid) return
    const { contentEn, versionLabel, mode, externalUrl } = form.state.values
    await saveDraft.mutate({ contentEn, versionLabel, mode, externalUrl })
    await publish.mutate(versionLabel.trim() || null)
  }

  async function handleSaveDraft() {
    await form.handleSubmit()
    if (!form.state.isValid) return
    const { contentEn, versionLabel, mode, externalUrl } = form.state.values
    await saveDraft.mutate({ contentEn, versionLabel, mode, externalUrl })
  }

  async function handleRevert() {
    await revert.mutate(undefined)
  }

  async function handleDiscard() {
    await discard.mutate(undefined)
  }

  return (
    <Card data-testid={`legal-agreement-card-${agreementType}`}>
      <CardHeader>
        <div className="flex items-center justify-between gap-4">
          <CardTitle data-testid={`legal-agreement-title-${agreementType}`}>{title}</CardTitle>
          <div className="flex gap-2">
            <Badge
              variant="outline"
              data-testid={`mode-badge-${view.current_version.mode ?? 'full_text'}`}
            >
              {view.current_version.mode === 'link'
                ? m['settings.legal.mode_link']()
                : m['settings.legal.mode_full_text']()}
            </Badge>
            <SourceBadge source={view.source} />
          </div>
        </div>
        <CardDescription data-testid={`legal-agreement-meta-${agreementType}`}>
          {m['settings.legal.version_no_label']()}: {view.current_version.version_no} •{' '}
          {m['settings.legal.effective_date_label']()}:{' '}
          {formatDate(view.current_version.effective_at)}
        </CardDescription>
      </CardHeader>

      <CardContent className="space-y-6">
        {canManage && shouldPrefillDefault && (
          <Alert data-testid={`legal-default-prefill-hint-${agreementType}`}>
            <AlertCircle className="h-4 w-4" />
            <AlertDescription>{m['settings.legal.default_prefill_hint']()}</AlertDescription>
          </Alert>
        )}
        {canManage ? (
          <AppForm>
            <form
              id={`legal-publish-form-${agreementType}`}
              onSubmit={(e) => {
                e.preventDefault()
                void handleSaveDraft()
              }}
              className="space-y-4"
            >
              <TextField
                form={form}
                name="versionLabel"
                label={m['settings.legal.version_label_label']()}
                inputId={`legal-version-label-${agreementType}`}
                dataTestId={`legal-version-label-input-${agreementType}`}
                disabled={saveDraft.isSubmitting || publish.isSubmitting}
                placeholder={m['settings.legal.version_label_placeholder']()}
              />
              <form.Field name="mode">
                {(field) => (
                  <label className="grid gap-2 text-sm font-medium">
                    {m['settings.legal.mode_label']()}
                    <select
                      value={field.state.value}
                      onChange={(event) =>
                        field.handleChange(event.target.value as 'full_text' | 'link')
                      }
                      className="h-9 rounded-md border bg-background px-3"
                      data-testid={`legal-mode-select-${agreementType}`}
                    >
                      <option value="full_text">{m['settings.legal.mode_full_text']()}</option>
                      <option value="link">{m['settings.legal.mode_link']()}</option>
                    </select>
                  </label>
                )}
              </form.Field>
              <form.Subscribe
                selector={(state) => state.values.mode}
                children={(mode) =>
                  mode === 'link' ? (
                    <TextField
                      form={form}
                      name="externalUrl"
                      label={m['settings.legal.external_url_label']()}
                      inputId={`legal-external-url-${agreementType}`}
                      dataTestId={`legal-external-url-input-${agreementType}`}
                      disabled={saveDraft.isSubmitting || publish.isSubmitting}
                      placeholder={m['settings.legal.external_url_placeholder']()}
                    />
                  ) : (
                    <TextareaField
                      form={form}
                      name="contentEn"
                      label={m['settings.legal.content_en_label']()}
                      inputId={`legal-content-en-${agreementType}`}
                      dataTestId={`legal-content-en-input-${agreementType}`}
                      disabled={saveDraft.isSubmitting || publish.isSubmitting}
                      rows={6}
                      helpText={m['settings.legal.content_help']()}
                    />
                  )
                }
              />
              <div className="flex flex-wrap gap-2">
                <Button
                  type="submit"
                  variant="outline"
                  disabled={saveDraft.isSubmitting || publish.isSubmitting}
                  data-testid={`legal-save-draft-button-${agreementType}`}
                >
                  {saveDraft.isSubmitting
                    ? m['settings.legal.saving_draft_button']()
                    : m['settings.legal.save_draft_button']()}
                </Button>
                <form.Subscribe
                  selector={(state) => ({
                    contentEn: state.values.contentEn,
                    mode: state.values.mode,
                  })}
                  children={({ contentEn, mode }) => (
                    <Button
                      type="button"
                      variant="outline"
                      onClick={() => setPreviewOpen(true)}
                      disabled={publish.isSubmitting || mode === 'link' || !contentEn?.trim()}
                      data-testid={`legal-preview-button-${agreementType}`}
                    >
                      {m['settings.legal.preview_button']()}
                    </Button>
                  )}
                />
                <Button
                  type="button"
                  onClick={() => void handlePublish()}
                  disabled={saveDraft.isSubmitting || publish.isSubmitting}
                  data-testid={`legal-publish-button-${agreementType}`}
                >
                  {publish.isSubmitting
                    ? m['settings.legal.publishing_button']()
                    : m['settings.legal.publish_from_draft_button']()}
                </Button>
                {draft && (
                  <Button
                    type="button"
                    variant="ghost"
                    onClick={() => setDiscardOpen(true)}
                    disabled={discard.isSubmitting}
                    data-testid={`legal-discard-draft-button-${agreementType}`}
                  >
                    {discard.isSubmitting
                      ? m['settings.legal.discarding_draft_button']()
                      : m['settings.legal.discard_draft_button']()}
                  </Button>
                )}
              </div>
            </form>
          </AppForm>
        ) : (
          <p
            className="text-sm text-muted-foreground"
            data-testid={`legal-agreement-view-only-${agreementType}`}
          >
            {m['settings.legal.view_only_notice']()}
          </p>
        )}

        {canManage && view.source === 'custom' && (
          <div>
            <Button
              variant="outline"
              onClick={() => setRevertOpen(true)}
              disabled={revert.isSubmitting}
              data-testid={`legal-revert-button-${agreementType}`}
            >
              {revert.isSubmitting
                ? m['settings.legal.reverting_button']()
                : m['settings.legal.revert_button']()}
            </Button>
          </div>
        )}

        <div>
          <h4
            className="text-sm font-medium mb-2"
            data-testid={`legal-history-title-${agreementType}`}
          >
            {m['settings.legal.history_title']()}
          </h4>
          <HistoryTable
            view={view}
            onView={(entry) =>
              setActiveVersion({ versionId: entry.version_id, versionNo: entry.version_no })
            }
          />
        </div>

        {/* Preview dialog: renders the current textarea value as Markdown using
            the same renderer the public agreement page uses. Pure client-side. */}
        <Dialog open={previewOpen} onOpenChange={setPreviewOpen}>
          <DialogContent
            className="max-w-3xl max-h-[85vh] overflow-y-auto"
            data-testid={`legal-preview-dialog-${agreementType}`}
          >
            <DialogHeader>
              <DialogTitle>{title}</DialogTitle>
            </DialogHeader>
            <MarkdownContent
              content={form.state.values.contentEn || m['settings.legal.preview_empty']()}
            />
          </DialogContent>
        </Dialog>

        {/* Past-version dialog: shows the full body of a selected history entry,
            fetched on demand (the history list only carries summaries). Renders
            with the same Markdown renderer as the preview dialog. */}
        <Dialog
          open={!!activeVersion}
          onOpenChange={(open) => {
            if (!open) setActiveVersion(null)
          }}
        >
          <DialogContent
            className="max-w-3xl max-h-[85vh] overflow-y-auto"
            data-testid={`legal-version-dialog-${agreementType}`}
          >
            <DialogHeader>
              <DialogTitle>
                {activeVersion
                  ? m['settings.legal.version_dialog_title']({ versionNo: activeVersion.versionNo })
                  : ''}
              </DialogTitle>
            </DialogHeader>
            {isLoadingVersion || !activeVersionDetail ? (
              <p className="text-sm text-muted-foreground">
                {m['settings.legal.version_dialog_loading']()}
              </p>
            ) : (
              <MarkdownContent content={contentEnOf(activeVersionDetail.content)} />
            )}
          </DialogContent>
        </Dialog>

        <AlertDialog open={revertOpen} onOpenChange={setRevertOpen}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle data-testid={`legal-revert-dialog-title-${agreementType}`}>
                {m['settings.legal.revert_dialog_title']()}
              </AlertDialogTitle>
              <AlertDialogDescription
                data-testid={`legal-revert-dialog-description-${agreementType}`}
              >
                {m['settings.legal.revert_dialog_description']()}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel
                onClick={() => setRevertOpen(false)}
                disabled={revert.isSubmitting}
                data-testid={`legal-revert-cancel-${agreementType}`}
              >
                {m['common.cancel']()}
              </AlertDialogCancel>
              <AlertDialogAction
                onClick={handleRevert}
                disabled={revert.isSubmitting}
                data-testid={`legal-revert-confirm-${agreementType}`}
              >
                {revert.isSubmitting
                  ? m['settings.legal.reverting_button']()
                  : m['settings.legal.revert_button']()}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>

        <AlertDialog open={discardOpen} onOpenChange={setDiscardOpen}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle data-testid={`legal-discard-draft-dialog-title-${agreementType}`}>
                {m['settings.legal.discard_draft_dialog_title']()}
              </AlertDialogTitle>
              <AlertDialogDescription
                data-testid={`legal-discard-draft-dialog-description-${agreementType}`}
              >
                {m['settings.legal.discard_draft_dialog_description']()}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel
                onClick={() => setDiscardOpen(false)}
                disabled={discard.isSubmitting}
                data-testid={`legal-discard-draft-cancel-${agreementType}`}
              >
                {m['common.cancel']()}
              </AlertDialogCancel>
              <AlertDialogAction
                onClick={handleDiscard}
                disabled={discard.isSubmitting}
                data-testid={`legal-discard-draft-confirm-${agreementType}`}
              >
                {discard.isSubmitting
                  ? m['settings.legal.discarding_draft_button']()
                  : m['settings.legal.discard_draft_button']()}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </CardContent>
    </Card>
  )
}

export function LegalAgreementTab({ realmId, canManage }: { realmId: string; canManage: boolean }) {
  const { data, isLoading, error, refetch } = useQuery(legalAdminAgreementsQueryOptions(realmId))

  if (isLoading) {
    return (
      <div className="text-sm text-muted-foreground" data-testid="legal-agreements-loading">
        {m['settings.legal.loading']()}
      </div>
    )
  }

  if (error) {
    return (
      <Alert variant="destructive" data-testid="legal-agreements-error">
        <AlertCircle className="h-4 w-4" />
        <AlertTitle>{m['settings.legal.error']()}</AlertTitle>
        <AlertDescription className="space-y-2">
          <p>{getErrorMessage(error)}</p>
          <Button
            variant="outline"
            size="sm"
            onClick={() => refetch()}
            data-testid="legal-agreements-retry"
          >
            {m['settings.legal.error_retry']()}
          </Button>
        </AlertDescription>
      </Alert>
    )
  }

  const agreements = data?.agreements ?? []

  if (agreements.length === 0) {
    return (
      <div className="text-sm text-muted-foreground" data-testid="legal-agreements-empty">
        {m['settings.legal.empty']()}
      </div>
    )
  }

  return (
    <div className="space-y-6" data-testid="legal-agreements-tab">
      {!canManage && (
        <Alert data-testid="legal-agreements-view-only">
          <AlertCircle className="h-4 w-4" />
          <AlertTitle>{m['settings.legal.view_only_title']()}</AlertTitle>
          <AlertDescription>{m['settings.legal.view_only_notice']()}</AlertDescription>
        </Alert>
      )}
      {agreements.map((view) => (
        <AgreementCard
          key={view.agreement_type}
          view={view}
          realmId={realmId}
          canManage={canManage}
        />
      ))}
    </div>
  )
}
