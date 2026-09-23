import { useEffect } from 'react'
import { toast } from 'sonner'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { RedirectUrisInput, type UriItem } from '@/components/client-apps/redirect-uris-input'
import { getFieldErrorMessage } from '@/lib/form-utils'
import {
  createClientAppSchema,
  DEFAULT_BROWSER_REFRESH_TTL_SECONDS,
} from '@/lib/schemas/client-app-forms'
import type { CreateClientAppFormData } from '@/lib/schemas/client-app-forms'
import { createClientApp } from '@/lib/api-generated'
import { queryKeys } from '@/data/query-options'
import { getErrorMessage } from '@/lib/error-utils'
import { m } from '@/paraglide/messages'

interface CreateClientAppDialogProps {
  realmId: string
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Called with the created app's id so the caller can fold it into coverage. */
  onCreated: (clientAppId: string) => void
}

const defaults: CreateClientAppFormData = {
  clientId: '',
  name: '',
  description: '',
  redirectUris: [],
  iconUrl: '',
  enabled: true,
  browserRefreshAbsoluteTtlSeconds: DEFAULT_BROWSER_REFRESH_TTL_SECONDS,
  allowedOrigins: [],
  deviceCodeGrantEnabled: false,
  turnstileEnabled: false,
  turnstileSiteKey: '',
  turnstileSecretKey: '',
}

function toUriItems(uris: string[]): UriItem[] {
  return uris.map((value, index) => ({ id: `uri-${index}`, value, isValid: true }))
}

/**
 * Quick-create dialog for Client Apps, opened from the credit-bucket coverage
 * field. Coverage may only reference self-created apps, so when a realm has
 * none yet this dialog lets the user create one in place instead of leaving
 * for /manage/client-apps/new and losing the in-progress bucket form.
 *
 * Reuses `createClientAppSchema` (same validation as the full form page);
 * advanced settings keep their schema defaults and can be edited later on the
 * client-apps page.
 */
export function CreateClientAppDialog({
  realmId,
  open,
  onOpenChange,
  onCreated,
}: CreateClientAppDialogProps) {
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: async (data: CreateClientAppFormData) => {
      // turnstileSecretKey is write-only; omit when empty (see client-app-form-page).
      const { turnstileSecretKey: _secret, ...rest } = data
      const body = {
        ...rest,
        ...(data.turnstileSecretKey ? { turnstileSecretKey: data.turnstileSecretKey } : {}),
      }
      const response = await createClientApp({ body })
      if (response.error) throw response.error
      return response.data
    },
    onSuccess: (data) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.clientAppsList(realmId) })
      if (data?.clientSecret) {
        toast.success(m['client_apps.created_with_secret'](), {
          description: data.clientSecret,
          duration: 15000,
        })
      } else {
        toast.success(m['client_apps.created_success']())
      }
      onCreated(data?.id ?? '')
      onOpenChange(false)
    },
    onError: (error) => {
      toast.error(getErrorMessage(error))
    },
  })

  const form = useAppForm({
    schema: createClientAppSchema,
    defaultValues: defaults,
    onSubmit: async ({ value }) => {
      await mutation.mutateAsync(value)
    },
  })

  // Reset the form each time the dialog opens so a cancelled draft never
  // leaks into the next attempt.
  useEffect(() => {
    if (open) {
      form.reset(defaults)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reset only on open transition
  }, [open])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[525px]">
        <DialogHeader>
          <DialogTitle>{m['credit_buckets.coverage_create_title']()}</DialogTitle>
          <DialogDescription>{m['credit_buckets.coverage_create_description']()}</DialogDescription>
        </DialogHeader>

        <AppForm>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              e.stopPropagation()
              form.handleSubmit()
            }}
            className="space-y-4"
          >
            <form.Field name="clientId">
              {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
              {(field: any) => (
                <div className="space-y-2">
                  <Label htmlFor="bucket-create-client-id">
                    {m['client_apps.form_client_id_label']()}
                  </Label>
                  <Input
                    id="bucket-create-client-id"
                    placeholder={m['client_apps.form_client_id_placeholder']()}
                    value={field.state.value}
                    onChange={(e) => field.handleChange(e.target.value)}
                    data-testid="bucket-create-client-app-client-id"
                  />
                  {(field.state.meta.isTouched || form.state.isSubmitted) &&
                    field.state.meta.errors.length > 0 && (
                      <p className="text-sm text-destructive" role="alert">
                        {getFieldErrorMessage(field.state.meta)}
                      </p>
                    )}
                </div>
              )}
            </form.Field>

            <form.Field name="name">
              {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
              {(field: any) => (
                <div className="space-y-2">
                  <Label htmlFor="bucket-create-client-name">
                    {m['client_apps.form_name_label']()}
                  </Label>
                  <Input
                    id="bucket-create-client-name"
                    value={field.state.value}
                    onChange={(e) => field.handleChange(e.target.value)}
                    data-testid="bucket-create-client-app-name"
                  />
                  {(field.state.meta.isTouched || form.state.isSubmitted) &&
                    field.state.meta.errors.length > 0 && (
                      <p className="text-sm text-destructive" role="alert">
                        {getFieldErrorMessage(field.state.meta)}
                      </p>
                    )}
                </div>
              )}
            </form.Field>

            <form.Field name="redirectUris">
              {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
              {(field: any) => (
                <div className="space-y-2">
                  <RedirectUrisInput
                    value={toUriItems(field.state.value)}
                    onChange={(items) => field.handleChange(items.map((item) => item.value))}
                    label={m['client_apps.form_redirect_uris_label']()}
                    helpText={m['client_apps.form_redirect_uris_help']()}
                    required
                    dataTestId="bucket-create-client-app-redirect-uris"
                  />
                  {form.state.isSubmitted && field.state.meta.errors.length > 0 && (
                    <p className="text-sm text-destructive" role="alert">
                      {getFieldErrorMessage(field.state.meta)}
                    </p>
                  )}
                </div>
              )}
            </form.Field>

            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => onOpenChange(false)}
                disabled={mutation.isPending}
              >
                {m['common.cancel']()}
              </Button>
              <Button
                type="submit"
                disabled={mutation.isPending}
                data-testid="bucket-create-client-app-submit"
              >
                {mutation.isPending
                  ? m['client_apps.form_saving']()
                  : m['credit_buckets.coverage_create_button']()}
              </Button>
            </DialogFooter>
          </form>
        </AppForm>
      </DialogContent>
    </Dialog>
  )
}
