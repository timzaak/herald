import { useMemo } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { ArrowLeft } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { AppForm, useAppForm } from '@/components/ui/tanstack-form'
import { TextField, TextareaField } from '@/components/shared/form-fields'
import {
  applyInvoiceSchema,
  getApplyFormDefaults,
  type PrefilledInvoiceReference,
} from '@/lib/schemas/invoice-forms'
import { useApplyInvoice } from '@/data/invoice-mutations'
import { m } from '@/paraglide/messages'
import { resolveApiError } from '@/lib/error-utils'

interface ApplyInvoiceFormPageProps {
  realmId: string
  prefilledReference?: PrefilledInvoiceReference
  returnTo?: string
}

export function ApplyInvoiceFormPage({
  realmId,
  prefilledReference,
  returnTo,
}: ApplyInvoiceFormPageProps) {
  const navigate = useNavigate()
  const applyMutation = useApplyInvoice(realmId)
  const { mutate: apply, isPending: isSubmitting } = applyMutation
  // The legacy `creem_merchant_of_record` code is kept for the rename deploy
  // window (an older backend may still reject with it); drop it once no
  // pre-rename backend can serve this page.
  const rejectionCode = resolveApiError(applyMutation.error).code
  const isMorRejection =
    rejectionCode === 'mor_provider_invoice_blocked' || rejectionCode === 'creem_merchant_of_record'
  const defaultValues = useMemo(
    () => getApplyFormDefaults(prefilledReference),
    [prefilledReference]
  )

  const form = useAppForm({
    schema: applyInvoiceSchema,
    defaultValues,
    onSubmit: async ({ value }) => {
      apply(value, {
        onSuccess: () => {
          navigate({
            to: '/$realmId/user/invoices',
            params: { realmId },
          })
        },
      })
    },
  })

  const handleCancel = () => {
    if (returnTo === `/${realmId}/user/points`) {
      navigate({
        to: '/$realmId/user/points',
        params: { realmId },
      })
      return
    }

    if (returnTo === `/${realmId}/user/subscription-history`) {
      navigate({
        to: '/$realmId/user/subscription-history',
        params: { realmId },
      })
      return
    }

    navigate({
      to: '/$realmId/user/invoices',
      params: { realmId },
    })
  }

  return (
    <div className="max-w-2xl mx-auto py-6 px-6 space-y-6" data-testid="apply-form-page">
      <div className="flex items-center gap-4">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={handleCancel}
          data-testid="apply-invoice-back-button"
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <h1 className="text-2xl font-bold" data-testid="apply-form-title">
          {m['billing.invoice_apply_title']()}
        </h1>
      </div>

      {isMorRejection && (
        <div
          className="rounded-md border border-warning/20 bg-warning/10 p-3 text-sm text-warning"
          data-testid="apply-invoice-mor-rejection"
        >
          {m['billing.invoice_apply_creem_rejection']()}
        </div>
      )}

      <form
        onSubmit={(e) => {
          e.preventDefault()
          e.stopPropagation()
          form.handleSubmit()
        }}
        className="space-y-6"
      >
        <AppForm>
          <div className="space-y-6">
            <Card data-testid="apply-form-reference-section">
              <CardHeader>
                <CardTitle>{m['billing.invoice_apply_reference']()}</CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                {prefilledReference && (
                  <div
                    className="rounded-md border border-muted bg-muted/40 px-3 py-2"
                    data-testid="apply-prefilled-reference"
                  >
                    <p className="text-sm font-medium">
                      {prefilledReference.type === 'paymentAttempt'
                        ? m['billing.invoice_apply_points_package']()
                        : m['billing.invoice_apply_subscription']()}
                    </p>
                    <p className="mt-1 text-xs text-muted-foreground">
                      {m['billing.invoice_apply_ref_hint']()}
                    </p>
                  </div>
                )}
              </CardContent>
            </Card>

            <Card data-testid="apply-form-billing-section">
              <CardHeader>
                <CardTitle>{m['billing.invoice_apply_billing_info']()}</CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <TextField
                  form={form}
                  name="billingName"
                  label={m['billing.invoice_apply_billing_name']()}
                  dataTestId="apply-billing-name-input"
                  placeholder={m['billing.invoice_apply_billing_name_placeholder']()}
                  required
                />
                <TextField
                  form={form}
                  name="billingEmail"
                  label={m['billing.invoice_apply_billing_email']()}
                  dataTestId="apply-billing-email-input"
                  type="email"
                  placeholder="billing@example.com"
                />
                <TextField
                  form={form}
                  name="billingAddress"
                  label={m['billing.invoice_apply_billing_address']()}
                  dataTestId="apply-billing-address-input"
                  placeholder={m['billing.invoice_placeholder_address']()}
                  required
                />
                <TextField
                  form={form}
                  name="billingPhone"
                  label={m['billing.invoice_apply_billing_phone']()}
                  dataTestId="apply-billing-phone-input"
                  placeholder="+1 234 567 8900"
                />
                <TextField
                  form={form}
                  name="billingTaxId"
                  label={m['billing.invoice_apply_billing_tax_id']()}
                  dataTestId="apply-billing-tax-id-input"
                  placeholder={m['billing.invoice_apply_billing_tax_id_placeholder']()}
                  required
                />
              </CardContent>
            </Card>

            <Card data-testid="apply-form-details-section">
              <CardHeader>
                <CardTitle>{m['billing.invoice_apply_invoice_details']()}</CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <TextField
                  form={form}
                  name="dueDate"
                  label={m['billing.invoice_due_date_label']()}
                  dataTestId="apply-due-date-input"
                  type="date"
                  required
                />
                <TextareaField
                  form={form}
                  name="notes"
                  label={m['billing.invoice_apply_notes']()}
                  dataTestId="apply-notes-input"
                  placeholder={m['billing.invoice_apply_notes_placeholder']()}
                  rows={3}
                />
                <div className="rounded-md border border-muted bg-muted/40 px-3 py-2">
                  <p className="text-xs text-muted-foreground">
                    {m['billing.invoice_apply_seller_auto']()}
                  </p>
                </div>
              </CardContent>
            </Card>
          </div>
        </AppForm>

        <div className="flex items-center gap-3 pt-4 border-t">
          <Button
            type="button"
            variant="outline"
            onClick={handleCancel}
            data-testid="apply-invoice-cancel-button"
          >
            {m['common.cancel']()}
          </Button>
          <Button type="submit" disabled={isSubmitting} data-testid="apply-invoice-submit-button">
            {isSubmitting ? m['billing.invoice_submitting']() : m['billing.invoice_submit']()}
          </Button>
        </div>
      </form>
    </div>
  )
}
