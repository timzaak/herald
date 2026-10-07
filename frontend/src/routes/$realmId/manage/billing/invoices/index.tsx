import { useState, useCallback, useEffect, useRef } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { z } from 'zod'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
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
import { InvoiceAdminPage } from '@/components/billing/invoices/invoice-admin-page'
import { InvoiceDetailDialog } from '@/components/billing/invoices/invoice-detail-dialog'
import { InvoiceSellerConfigForm } from '@/components/billing/invoices/invoice-seller-config-form'
import { InvoicePolicyForm } from '@/components/billing/invoices/invoice-policy-form'
import { useIssueInvoice, useVoidInvoice, useMarkPaid } from '@/data/invoice-mutations'
import type { InvoiceResponse } from '@/lib/api-generated'
import { m } from '@/paraglide/messages'
import { realmPath, useCurrentSearch, useResolvedRealmContext } from '@/lib/realm-routing'

// Search param that lets Billing hub deep-link to seller config or policy config dialog.
// Mirrors the zod-schema-on-validateSearch convention used by sibling billing routes
// (subscriptions.tsx, entitlement-mappings.tsx).
// eslint-disable-next-line react-refresh/only-export-components
export const invoiceAdminSearchSchema = z.object({
  open: z.enum(['seller', 'policy']).optional(),
})

export const Route = createFileRoute('/$realmId/manage/billing/invoices/')({
  component: InvoiceAdminRoute,
  validateSearch: invoiceAdminSearchSchema,
})

export function InvoiceAdminRoute() {
  const realmContext = useResolvedRealmContext()
  const realmId = realmContext.realmId
  const navigate = useNavigate()
  const search = useCurrentSearch<z.infer<typeof invoiceAdminSearchSchema>>()

  const [detailInvoiceId, setDetailInvoiceId] = useState<string | null>(null)

  const [sellerConfigOpen, setSellerConfigOpen] = useState(false)
  const [policyConfigOpen, setPolicyConfigOpen] = useState(false)

  // Deep-link support: the Billing hub navigates here with `?open=seller|policy`
  // to surface the invoice settings dialog. Open the dialog once per inbound value,
  // then strip the param (replace, no history entry) so a refresh doesn't re-open it.
  // The handledRef guard prevents the clear-navigate from re-triggering this effect.
  const handledOpen = useRef<string | undefined>(undefined)
  useEffect(() => {
    const requested = search.open
    if (!requested || handledOpen.current === requested) return
    handledOpen.current = requested
    if (requested === 'seller') {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- open dialog once per inbound deep-link value
      setSellerConfigOpen(true)
    } else if (requested === 'policy') {
      setPolicyConfigOpen(true)
    }
    navigate({
      to: realmPath(realmContext, '/manage/billing/invoices'),
      search: { open: undefined },
      replace: true,
    })
  }, [search.open, navigate, realmContext])

  const [issueTarget, setIssueTarget] = useState<InvoiceResponse | null>(null)
  const [issueDate, setIssueDate] = useState('')

  const [voidTarget, setVoidTarget] = useState<InvoiceResponse | null>(null)
  const [voidReason, setVoidReason] = useState('')

  const [markPaidTarget, setMarkPaidTarget] = useState<InvoiceResponse | null>(null)

  const issueMutation = useIssueInvoice(realmId)
  const voidMutation = useVoidInvoice(realmId)
  const markPaidMutation = useMarkPaid(realmId)

  const handleCreateInvoice = useCallback(() => {
    navigate({
      to: realmPath(realmContext, '/manage/billing/invoices/new'),
    })
  }, [navigate, realmContext])

  const handleEditInvoice = useCallback(
    (invoice: InvoiceResponse) => {
      navigate({
        to: realmPath(realmContext, `/manage/billing/invoices/${invoice.id}/edit`),
      })
    },
    [navigate, realmContext]
  )

  const handleViewInvoice = useCallback((invoice: InvoiceResponse) => {
    setDetailInvoiceId(invoice.id)
  }, [])

  const handleOpenSellerConfig = useCallback(() => {
    setSellerConfigOpen(true)
  }, [])

  const handleOpenPolicyConfig = useCallback(() => {
    setPolicyConfigOpen(true)
  }, [])

  const handleIssueInvoice = useCallback((invoice: InvoiceResponse) => {
    setIssueTarget(invoice)
    setIssueDate('')
  }, [])

  const handleVoidInvoice = useCallback((invoice: InvoiceResponse) => {
    setVoidTarget(invoice)
    setVoidReason('')
  }, [])

  const handleMarkPaidInvoice = useCallback((invoice: InvoiceResponse) => {
    setMarkPaidTarget(invoice)
  }, [])

  return (
    <>
      <InvoiceAdminPage
        realmId={realmId}
        onCreateInvoice={handleCreateInvoice}
        onEditInvoice={handleEditInvoice}
        onViewInvoice={handleViewInvoice}
        onOpenSellerConfig={handleOpenSellerConfig}
        onOpenPolicyConfig={handleOpenPolicyConfig}
        onIssueInvoice={handleIssueInvoice}
        onVoidInvoice={handleVoidInvoice}
        onMarkPaidInvoice={handleMarkPaidInvoice}
      />

      <InvoiceDetailDialog
        open={!!detailInvoiceId}
        onOpenChange={(open) => {
          if (!open) setDetailInvoiceId(null)
        }}
        realmId={realmId}
        invoiceId={detailInvoiceId}
      />

      <InvoiceSellerConfigForm
        open={sellerConfigOpen}
        onOpenChange={setSellerConfigOpen}
        realmId={realmId}
      />

      <InvoicePolicyForm
        open={policyConfigOpen}
        onOpenChange={setPolicyConfigOpen}
        realmId={realmId}
      />

      <AlertDialog
        open={!!issueTarget}
        onOpenChange={(open) => {
          if (!open) setIssueTarget(null)
        }}
      >
        <AlertDialogContent data-testid="issue-confirm-dialog">
          <AlertDialogHeader>
            <AlertDialogTitle>{m['billing.invoice_issue_confirm_title']()}</AlertDialogTitle>
            <AlertDialogDescription>
              {m['billing.invoice_issue_confirm_description']({
                number: issueTarget?.invoiceNumber ?? '',
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="space-y-2">
            <Label htmlFor="issue-date" className="text-sm">
              {m['billing.invoice_issue_date_label']()}
            </Label>
            <Input
              id="issue-date"
              type="date"
              value={issueDate}
              onChange={(e) => setIssueDate(e.target.value)}
              data-testid="issue-date-input"
            />
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={issueMutation.isPending} data-testid="issue-cancel-button">
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                issueMutation.mutate(
                  { invoiceId: issueTarget!.id, issueDate: issueDate || undefined },
                  { onSettled: () => setIssueTarget(null) }
                )
              }}
              disabled={issueMutation.isPending}
              data-testid="issue-confirm-button"
            >
              {issueMutation.isPending
                ? m['billing.invoice_issue_issuing']()
                : m['billing.invoice_issue_button']()}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        open={!!voidTarget}
        onOpenChange={(open) => {
          if (!open) setVoidTarget(null)
        }}
      >
        <AlertDialogContent data-testid="void-confirm-dialog">
          <AlertDialogHeader>
            <AlertDialogTitle>{m['billing.invoice_void_confirm_title']()}</AlertDialogTitle>
            <AlertDialogDescription>
              {m['billing.invoice_void_confirm_description']({
                number: voidTarget?.invoiceNumber ?? '',
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="space-y-2">
            <Label htmlFor="void-reason" className="text-sm">
              {m['billing.invoice_void_reason_label']()}
            </Label>
            <Input
              id="void-reason"
              value={voidReason}
              onChange={(e) => setVoidReason(e.target.value)}
              placeholder={m['billing.invoice_void_reason_placeholder']()}
              data-testid="void-reason-input"
            />
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={voidMutation.isPending} data-testid="void-cancel-button">
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                voidMutation.mutate(
                  { invoiceId: voidTarget!.id, voidReason: voidReason || undefined },
                  { onSettled: () => setVoidTarget(null) }
                )
              }}
              disabled={voidMutation.isPending}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              data-testid="void-confirm-button"
            >
              {voidMutation.isPending
                ? m['billing.invoice_voiding']()
                : m['billing.invoice_void']()}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        open={!!markPaidTarget}
        onOpenChange={(open) => {
          if (!open) setMarkPaidTarget(null)
        }}
      >
        <AlertDialogContent data-testid="mark-paid-confirm-dialog">
          <AlertDialogHeader>
            <AlertDialogTitle>{m['billing.invoice_mark_paid_confirm_title']()}</AlertDialogTitle>
            <AlertDialogDescription>
              {m['billing.invoice_mark_paid_confirm_description']({
                number: markPaidTarget?.invoiceNumber ?? '',
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel
              disabled={markPaidMutation.isPending}
              data-testid="mark-paid-cancel-button"
            >
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                markPaidMutation.mutate(
                  { invoiceId: markPaidTarget!.id },
                  { onSettled: () => setMarkPaidTarget(null) }
                )
              }}
              disabled={markPaidMutation.isPending}
              data-testid="mark-paid-confirm-button"
            >
              {markPaidMutation.isPending
                ? m['billing.invoice_marking']()
                : m['billing.invoice_mark_as_paid']()}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
