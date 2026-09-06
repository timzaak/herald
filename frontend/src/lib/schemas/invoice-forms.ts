import { z } from 'zod'
import { parsePaymentTermsDays } from '@/lib/invoice-utils'
import { m } from '@/paraglide/messages'

export const invoiceLineItemSchema = z.object({
  name: z.string().min(1, { error: () => m['billing.invoice_validation_item_name_required']() }),
  description: z.string().max(500).optional().nullable(),
  quantity: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_quantity_required']() })
    .refine((val) => /^[0-9]+(\.[0-9]+)?$/.test(val) && parseFloat(val) > 0, {
      error: () => m['billing.invoice_validation_quantity_positive'](),
    }),
  unitPrice: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_unit_price_required']() })
    .refine((val) => /^[0-9]+(\.[0-9]{0,2})?$/.test(val) && parseFloat(val) >= 0, {
      error: () => m['billing.invoice_validation_unit_price_non_negative'](),
    }),
})

export type InvoiceLineItemFormData = z.infer<typeof invoiceLineItemSchema>

const discountModeSchema = z.enum(['fixed', 'percent']).nullable().optional()
const taxModeSchema = z.enum(['fixed', 'percent']).nullable().optional()
const shippingModeSchema = z.enum(['fixed']).nullable().optional()

const numericStringSchema = z
  .string()
  .refine(
    (val) => val === '' || val === null || val === undefined || /^[0-9]+(\.[0-9]+)?$/.test(val),
    { error: () => m['billing.invoice_validation_valid_number']() }
  )
  .nullable()
  .optional()

export const invoiceCreateFormSchema = z.object({
  accountId: z.string().min(1, { error: () => m['billing.invoice_validation_account_required']() }),
  billingName: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_billing_name_required']() }),
  billingEmail: z
    .string()
    .email({ error: () => m['billing.invoice_validation_billing_email_invalid']() })
    .optional()
    .nullable(),
  billingAddress: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_billing_address_required']() }),
  billingPhone: z.string().max(50).optional().nullable(),
  billingTaxId: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_billing_tax_id_required']() }),
  sellerName: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_seller_name_required']() }),
  sellerEmail: z
    .string()
    .email({ error: () => m['billing.invoice_validation_seller_email_invalid']() })
    .optional()
    .nullable(),
  sellerAddress: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_seller_address_required']() }),
  sellerPhone: z.string().max(50).optional().nullable(),
  sellerTaxId: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_seller_tax_id_required']() }),
  currency: z.string().min(3).max(3).default('CNY'),
  lineItems: z
    .array(invoiceLineItemSchema)
    .min(1, { error: () => m['billing.invoice_validation_min_line_items']() }),
  discountMode: discountModeSchema,
  discountValue: numericStringSchema,
  taxMode: taxModeSchema,
  taxValue: numericStringSchema,
  shippingMode: shippingModeSchema,
  shippingValue: numericStringSchema,
  dueDate: z.string().min(1, { error: () => m['billing.invoice_validation_due_date_required']() }),
  paymentTerms: z.string().max(200).optional().nullable(),
  notes: z.string().max(2000).optional().nullable(),
  subscriptionId: z.string().optional().nullable(),
  paymentAttemptId: z.string().optional().nullable(),
})

export type InvoiceCreateFormData = z.infer<typeof invoiceCreateFormSchema>

export const invoiceEditFormSchema = z.object({
  billingName: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_billing_name_required']() }),
  billingEmail: z
    .string()
    .email({ error: () => m['billing.invoice_validation_billing_email_invalid']() })
    .optional()
    .nullable(),
  billingAddress: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_billing_address_required']() }),
  billingPhone: z.string().max(50).optional().nullable(),
  billingTaxId: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_billing_tax_id_required']() }),
  sellerName: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_seller_name_required']() }),
  sellerEmail: z
    .string()
    .email({ error: () => m['billing.invoice_validation_seller_email_invalid']() })
    .optional()
    .nullable(),
  sellerAddress: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_seller_address_required']() }),
  sellerPhone: z.string().max(50).optional().nullable(),
  sellerTaxId: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_seller_tax_id_required']() }),
  lineItems: z
    .array(invoiceLineItemSchema)
    .min(1, { error: () => m['billing.invoice_validation_min_line_items']() }),
  discountMode: discountModeSchema,
  discountValue: numericStringSchema,
  taxMode: taxModeSchema,
  taxValue: numericStringSchema,
  shippingMode: shippingModeSchema,
  shippingValue: numericStringSchema,
  dueDate: z.string().min(1, { error: () => m['billing.invoice_validation_due_date_required']() }),
  paymentTerms: z.string().max(200).optional().nullable(),
  notes: z.string().max(2000).optional().nullable(),
})

export type InvoiceEditFormData = z.infer<typeof invoiceEditFormSchema>

export const invoiceSellerConfigSchema = z.object({
  sellerName: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_seller_config_name_required']() }),
  sellerAddress: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_seller_config_address_required']() }),
  sellerEmail: z
    .string()
    .email({ error: () => m['billing.invoice_validation_billing_email_invalid']() })
    .optional()
    .nullable(),
  sellerPhone: z.string().max(50).optional().nullable(),
  sellerTaxId: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_seller_config_tax_id_required']() }),
  defaultPaymentTerms: z.string().max(200).optional().nullable(),
})

export type InvoiceSellerConfigFormData = z.infer<typeof invoiceSellerConfigSchema>

// The apply form is only reachable with a pre-filled resource reference
// (paymentAttemptId or subscriptionId) supplied by the route's search params;
// there is no manual-ID-entry path. See P1-3 in `.ai/future/invoice_ux.md`.
export const applyInvoiceSchema = z.object({
  currency: z.string().min(3).max(3).default('CNY'),
  paymentAttemptId: z.string().optional().nullable(),
  subscriptionId: z.string().optional().nullable(),
  billingName: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_billing_name_required']() }),
  billingEmail: z
    .string()
    .email({ error: () => m['billing.invoice_validation_billing_email_invalid']() })
    .optional()
    .nullable(),
  billingAddress: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_billing_address_required']() }),
  billingPhone: z.string().max(50).optional().nullable(),
  billingTaxId: z
    .string()
    .min(1, { error: () => m['billing.invoice_validation_billing_tax_id_required']() }),
  dueDate: z.string().min(1, { error: () => m['billing.invoice_validation_due_date_required']() }),
  notes: z.string().max(2000).optional().nullable(),
})

export type ApplyInvoiceFormData = z.infer<typeof applyInvoiceSchema>

export type PrefilledInvoiceReference =
  | { type: 'paymentAttempt'; id: string }
  | { type: 'subscription'; id: string }

export const voidInvoiceSchema = z.object({
  voidReason: z.string().max(500).optional().nullable(),
})

export type VoidInvoiceFormData = z.infer<typeof voidInvoiceSchema>

export const invoicePolicyConfigSchema = z.object({
  policy: z.enum(['provider_first', 'manual_only', 'none']),
  providerCapabilities: z
    .record(
      z.string(),
      z.object({
        externalInvoiceEnabled: z.boolean(),
      })
    )
    .optional()
    .default({}),
})

export function parseInvoicePolicyConfig(configValue: string): InvoicePolicyConfigFormData {
  const parsed = JSON.parse(configValue)
  const capabilities = parsed.provider_capabilities ?? parsed.providerCapabilities ?? {}
  return invoicePolicyConfigSchema.parse({
    policy: parsed.policy,
    providerCapabilities: Object.fromEntries(
      Object.entries(capabilities).map(([provider, value]) => {
        const capability = value as Record<string, unknown>
        return [
          provider,
          {
            externalInvoiceEnabled:
              capability.externalInvoiceEnabled ?? capability.external_invoice_enabled,
          },
        ]
      })
    ),
  })
}

export type InvoicePolicyConfigFormData = z.infer<typeof invoicePolicyConfigSchema>

export function getInvoicePolicyDefaults(): InvoicePolicyConfigFormData {
  return {
    policy: 'provider_first',
    providerCapabilities: {
      stripe: { externalInvoiceEnabled: true },
      creem: { externalInvoiceEnabled: true },
    },
  }
}

function computeDueDate(terms: string | null | undefined): string {
  const days = parsePaymentTermsDays(terms)
  if (days === undefined) return ''
  return new Date(Date.now() + days * 86400000).toISOString().slice(0, 10)
}

export function getInvoiceFormDefaults(
  sellerConfig?: {
    sellerName?: string
    sellerAddress?: string | null
    sellerEmail?: string | null
    sellerPhone?: string | null
    sellerTaxId?: string
    defaultPaymentTerms?: string | null
  } | null
): InvoiceCreateFormData {
  return {
    accountId: '',
    billingName: '',
    billingEmail: null,
    billingAddress: '',
    billingPhone: null,
    billingTaxId: '',
    sellerName: sellerConfig?.sellerName ?? '',
    sellerEmail: sellerConfig?.sellerEmail ?? null,
    sellerAddress: sellerConfig?.sellerAddress ?? '',
    sellerPhone: sellerConfig?.sellerPhone ?? null,
    sellerTaxId: sellerConfig?.sellerTaxId ?? '',
    currency: 'CNY',
    lineItems: [{ name: '', description: null, quantity: '1', unitPrice: '0.00' }],
    discountMode: null,
    discountValue: null,
    taxMode: null,
    taxValue: null,
    shippingMode: null,
    shippingValue: null,
    dueDate: computeDueDate(sellerConfig?.defaultPaymentTerms),
    paymentTerms: null,
    notes: null,
    subscriptionId: null,
    paymentAttemptId: null,
  }
}

export function getApplyFormDefaults(
  prefilledReference?: PrefilledInvoiceReference
): ApplyInvoiceFormData {
  return {
    currency: 'CNY',
    paymentAttemptId: prefilledReference?.type === 'paymentAttempt' ? prefilledReference.id : null,
    subscriptionId: prefilledReference?.type === 'subscription' ? prefilledReference.id : null,
    billingName: '',
    billingEmail: null,
    billingAddress: '',
    billingPhone: null,
    billingTaxId: '',
    dueDate: new Date(Date.now() + 30 * 86400000).toISOString().slice(0, 10),
    notes: null,
  }
}
