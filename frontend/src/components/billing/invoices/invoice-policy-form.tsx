import { useEffect, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Label } from '@/components/ui/label'
import { AppForm, useAppForm } from '@/components/ui/tanstack-form'
import { BaseFormDialog } from '@/components/shared/form-dialog'
import {
  invoicePolicyConfigSchema,
  type InvoicePolicyConfigFormData,
  getInvoicePolicyDefaults,
  parseInvoicePolicyConfig,
} from '@/lib/schemas/invoice-forms'
import { getProviderLabel } from '@/lib/invoice-utils'
import { invoicePolicyConfigQueryOptions } from '@/data/invoice-query-options'
import { useUpsertInvoicePolicy } from '@/data/invoice-mutations'
import { m } from '@/paraglide/messages'

interface InvoicePolicyFormProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  realmId: string
}

function parsePolicyConfig(
  configs: { configKey: string; configValue?: string | null }[]
): InvoicePolicyConfigFormData {
  const settings = configs.find((c) => c.configKey === 'settings')
  if (settings?.configValue) {
    try {
      return parseInvoicePolicyConfig(settings.configValue)
    } catch {
      return getInvoicePolicyDefaults()
    }
  }
  return getInvoicePolicyDefaults()
}

export function InvoicePolicyForm({ open, onOpenChange, realmId }: InvoicePolicyFormProps) {
  const { data: configs = [] } = useQuery({
    ...invoicePolicyConfigQueryOptions(realmId),
    enabled: open,
  })

  const { mutate: upsertPolicy, isPending: isSubmitting } = useUpsertInvoicePolicy(realmId)

  const defaultValues = useMemo<InvoicePolicyConfigFormData>(
    () => parsePolicyConfig(configs),
    [configs]
  )

  const form = useAppForm({
    schema: invoicePolicyConfigSchema,
    defaultValues,
    onSubmit: async ({ value }) => {
      upsertPolicy(value, {
        onSuccess: () => {
          onOpenChange(false)
        },
      })
    },
  })

  useEffect(() => {
    if (open) {
      form.reset(defaultValues)
    }
  }, [defaultValues, form, open])

  return (
    <BaseFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={m['billing.invoice_policy_title']()}
      data-testid="invoice-policy-form-dialog"
      footer={
        <>
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
            data-testid="invoice-policy-cancel-button"
          >
            {m['common.cancel']()}
          </Button>
          <Button
            type="submit"
            form="invoice-policy-form"
            disabled={isSubmitting}
            data-testid="invoice-policy-save-button"
          >
            {isSubmitting ? m['shared.saving']() : m['common.save']()}
          </Button>
        </>
      }
    >
      <form
        id="invoice-policy-form"
        onSubmit={(e) => {
          e.preventDefault()
          e.stopPropagation()
          form.handleSubmit()
        }}
        data-testid="invoice-policy-form-container"
      >
        <AppForm>
          <div className="space-y-4">
            <form.Field
              name="policy"
              children={(field) => (
                <div className="space-y-2">
                  <Label htmlFor="invoice-policy-select">
                    {m['billing.invoice_policy_title']()}
                  </Label>
                  <Select
                    value={field.state.value}
                    onValueChange={(value) =>
                      field.handleChange(value as InvoicePolicyConfigFormData['policy'])
                    }
                  >
                    <SelectTrigger data-testid="invoice-policy-select">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="provider_first">
                        {m['billing.invoice_policy_provider_first']()}
                      </SelectItem>
                      <SelectItem value="manual_only">
                        {m['billing.invoice_policy_manual_only']()}
                      </SelectItem>
                      <SelectItem value="none">{m['billing.invoice_policy_none']()}</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              )}
            />

            <form.Field
              name="providerCapabilities"
              children={(field) => (
                <div className="space-y-3">
                  <Label>{m['billing.invoice_policy_external_invoice_enabled']()}</Label>
                  {Object.entries(field.state.value ?? {}).map(([provider, caps]) => (
                    <div key={provider} className="flex items-center justify-between">
                      <Label>{getProviderLabel(provider)}</Label>
                      <Switch
                        checked={caps.externalInvoiceEnabled}
                        onCheckedChange={(checked) => {
                          const current = field.state.value ?? {}
                          field.handleChange({
                            ...current,
                            [provider]: { externalInvoiceEnabled: checked },
                          })
                        }}
                        data-testid={`invoice-policy-${provider}-switch`}
                      />
                    </div>
                  ))}
                </div>
              )}
            />
          </div>
        </AppForm>
      </form>
    </BaseFormDialog>
  )
}
