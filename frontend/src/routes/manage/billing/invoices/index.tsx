import { createFileRoute } from '@tanstack/react-router'
import {
  InvoiceAdminRoute,
  invoiceAdminSearchSchema,
} from '@/routes/$realmId/manage/billing/invoices/index'

export const Route = createFileRoute('/manage/billing/invoices/')({
  validateSearch: invoiceAdminSearchSchema,
  component: InvoiceAdminRoute,
})
