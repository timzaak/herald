import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import {
  createInvoice,
  updateInvoice,
  issueInvoice,
  voidInvoice,
  markPaid,
  upsertSellerConfig,
  applyInvoice,
  upsertRealmConfig,
  createCreditNote,
} from '@/lib/api-generated'
import type {
  CreateInvoiceRequest,
  UpdateInvoiceRequest,
  CreateCreditNoteRequest,
} from '@/lib/api-generated'
import type {
  InvoiceCreateFormData,
  InvoiceEditFormData,
  InvoiceSellerConfigFormData,
  ApplyInvoiceFormData,
  InvoicePolicyConfigFormData,
} from '@/lib/schemas/invoice-forms'
import type { RecordRefundFormData } from '@/lib/schemas/credit-note-forms'
import { displayPriceToCents } from '@/lib/invoice-utils'
import { getErrorMessage } from '@/lib/error-utils'
import { m } from '@/paraglide/messages'
import { invoiceKeys } from '@/data/invoice-query-options'
import { queryKeys } from '@/data/query-options'

export function useCreateInvoice(realmId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (values: InvoiceCreateFormData) => {
      const body: CreateInvoiceRequest = {
        accountId: values.accountId,
        billingName: values.billingName,
        billingEmail: values.billingEmail ?? undefined,
        billingAddress: values.billingAddress,
        billingPhone: values.billingPhone ?? undefined,
        billingTaxId: values.billingTaxId,
        sellerName: values.sellerName,
        sellerEmail: values.sellerEmail ?? undefined,
        sellerAddress: values.sellerAddress,
        sellerPhone: values.sellerPhone ?? undefined,
        sellerTaxId: values.sellerTaxId,
        currency: values.currency,
        lineItems: values.lineItems.map((item) => ({
          name: item.name,
          description: item.description ?? undefined,
          quantity: item.quantity,
          unitPrice: displayPriceToCents(item.unitPrice),
        })),
        discountMode: values.discountMode ?? undefined,
        discountValue: values.discountValue ?? undefined,
        taxMode: values.taxMode ?? undefined,
        taxValue: values.taxValue ?? undefined,
        shippingMode: values.shippingMode ?? undefined,
        shippingValue: values.shippingValue ?? undefined,
        dueDate: values.dueDate,
        paymentTerms: values.paymentTerms ?? undefined,
        notes: values.notes ?? undefined,
        subscriptionId: values.subscriptionId ?? undefined,
        paymentAttemptId: values.paymentAttemptId ?? undefined,
      }
      const response = await createInvoice({ path: { realmId }, body })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success('Invoice created')
      queryClient.invalidateQueries({ queryKey: invoiceKeys.all(realmId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.featureAvailability(realmId) })
    },
    onError: (error) => {
      const errorMessage = getErrorMessage(error)
      toast.error(`Failed to create invoice: ${errorMessage}`)
    },
  })
}

export function useUpdateInvoice(realmId: string, invoiceId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (values: InvoiceEditFormData) => {
      const body: UpdateInvoiceRequest = {
        billingName: values.billingName,
        billingEmail: values.billingEmail ?? undefined,
        billingAddress: values.billingAddress,
        billingPhone: values.billingPhone ?? undefined,
        billingTaxId: values.billingTaxId,
        sellerName: values.sellerName,
        sellerEmail: values.sellerEmail ?? undefined,
        sellerAddress: values.sellerAddress,
        sellerPhone: values.sellerPhone ?? undefined,
        sellerTaxId: values.sellerTaxId,
        lineItems: values.lineItems.map((item) => ({
          name: item.name,
          description: item.description ?? undefined,
          quantity: item.quantity,
          unitPrice: displayPriceToCents(item.unitPrice),
        })),
        discountMode: values.discountMode ?? undefined,
        discountValue: values.discountValue ?? undefined,
        taxMode: values.taxMode ?? undefined,
        taxValue: values.taxValue ?? undefined,
        shippingMode: values.shippingMode ?? undefined,
        shippingValue: values.shippingValue ?? undefined,
        dueDate: values.dueDate,
        paymentTerms: values.paymentTerms ?? undefined,
        notes: values.notes ?? undefined,
      }
      const response = await updateInvoice({
        path: { realmId, invoiceId },
        body,
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success('Invoice updated')
      queryClient.invalidateQueries({ queryKey: invoiceKeys.all(realmId) })
    },
    onError: (error) => {
      const errorMessage = getErrorMessage(error)
      toast.error(`Failed to update invoice: ${errorMessage}`)
    },
  })
}

export function useIssueInvoice(realmId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async ({ invoiceId, issueDate }: { invoiceId: string; issueDate?: string }) => {
      const response = await issueInvoice({
        path: { realmId, invoiceId },
        body: { issueDate: issueDate ?? undefined },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success('Invoice issued')
      queryClient.invalidateQueries({ queryKey: invoiceKeys.all(realmId) })
    },
    onError: (error) => {
      const errorMessage = getErrorMessage(error)
      toast.error(`Failed to issue invoice: ${errorMessage}`)
    },
  })
}

export function useVoidInvoice(realmId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async ({ invoiceId, voidReason }: { invoiceId: string; voidReason?: string }) => {
      const response = await voidInvoice({
        path: { realmId, invoiceId },
        body: { voidReason: voidReason ?? undefined },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success('Invoice voided')
      queryClient.invalidateQueries({ queryKey: invoiceKeys.all(realmId) })
    },
    onError: (error) => {
      const errorMessage = getErrorMessage(error)
      toast.error(`Failed to void invoice: ${errorMessage}`)
    },
  })
}

export function useMarkPaid(realmId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async ({ invoiceId }: { invoiceId: string }) => {
      const response = await markPaid({
        path: { realmId, invoiceId },
        body: {},
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success('Invoice marked as paid')
      queryClient.invalidateQueries({ queryKey: invoiceKeys.all(realmId) })
    },
    onError: (error) => {
      const errorMessage = getErrorMessage(error)
      toast.error(`Failed to mark invoice as paid: ${errorMessage}`)
    },
  })
}

export function useUpsertSellerConfig(realmId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (values: InvoiceSellerConfigFormData) => {
      const response = await upsertSellerConfig({
        path: { realmId },
        body: {
          sellerName: values.sellerName,
          sellerAddress: values.sellerAddress,
          sellerEmail: values.sellerEmail ?? undefined,
          sellerPhone: values.sellerPhone ?? undefined,
          sellerTaxId: values.sellerTaxId,
          defaultPaymentTerms: values.defaultPaymentTerms ?? undefined,
        },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success('Seller config saved')
      queryClient.invalidateQueries({ queryKey: invoiceKeys.sellerConfig(realmId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.featureAvailability(realmId) })
    },
    onError: (error) => {
      const errorMessage = getErrorMessage(error)
      toast.error(`Failed to save seller config: ${errorMessage}`)
    },
  })
}

export function useApplyInvoice(realmId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (values: ApplyInvoiceFormData) => {
      const response = await applyInvoice({
        body: {
          currency: values.currency,
          paymentAttemptId: values.paymentAttemptId ?? undefined,
          subscriptionId: values.subscriptionId ?? undefined,
          billingName: values.billingName,
          billingEmail: values.billingEmail ?? undefined,
          billingAddress: values.billingAddress,
          billingPhone: values.billingPhone ?? undefined,
          billingTaxId: values.billingTaxId,
          dueDate: values.dueDate,
          notes: values.notes ?? undefined,
        },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success('Invoice application submitted')
      queryClient.invalidateQueries({ queryKey: invoiceKeys.myAll(realmId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.featureAvailability(realmId) })
    },
    onError: (error) => {
      const errorMessage = getErrorMessage(error)
      toast.error(`Failed to apply for invoice: ${errorMessage}`)
    },
  })
}

export function useUpsertInvoicePolicy(realmId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (values: InvoicePolicyConfigFormData) => {
      const response = await upsertRealmConfig({
        path: { realmId },
        body: {
          configType: 'invoice_policy',
          configKey: 'settings',
          configValue: JSON.stringify({
            policy: values.policy,
            provider_capabilities: values.providerCapabilities,
          }),
          enabled: true,
        },
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success('Invoice policy saved')
      queryClient.invalidateQueries({ queryKey: invoiceKeys.policyConfig(realmId) })
    },
    onError: (error) => {
      const errorMessage = getErrorMessage(error)
      toast.error(`Failed to save invoice policy: ${errorMessage}`)
    },
  })
}

export function useCreateCreditNote(realmId: string, invoiceId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (values: RecordRefundFormData) => {
      const body: CreateCreditNoteRequest = {
        amount: displayPriceToCents(values.amount),
        memo: values.memo,
      }
      const response = await createCreditNote({
        path: { realmId, invoiceId },
        body,
      })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: () => {
      toast.success(m['billing.credit_note_created']())
      queryClient.invalidateQueries({ queryKey: invoiceKeys.detail(realmId, invoiceId) })
      queryClient.invalidateQueries({ queryKey: invoiceKeys.all(realmId) })
      queryClient.invalidateQueries({ queryKey: invoiceKeys.myDetail(realmId, invoiceId) })
    },
    // Intentionally no onError toast: Record Refund dialog shows inline errors
    // and must stay open on failure. Callers should catch with mutateAsync.
  })
}
