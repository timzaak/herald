import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  webhookSubscriptionSchema,
  WEBHOOK_EVENT_CATEGORIES,
  WEBHOOK_CATEGORY_LABEL,
  WEBHOOK_CATEGORY_DESCRIPTION,
  type WebhookSubscriptionFormData,
} from '@/lib/schemas/webhook-forms'
import { createWebhookSubscription, updateWebhookSubscription } from '@/lib/api-generated'
import type {
  CreateWebhookSubscriptionResponse,
  WebhookSubscriptionItem,
} from '@/lib/api-generated'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { useNavigate } from '@tanstack/react-router'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Checkbox } from '@/components/ui/checkbox'
import { TextField } from '@/components/shared/form-fields'
import { queryKeys } from '@/data/query-options'
import { ArrowLeft } from 'lucide-react'
import { Card, CardContent } from '@/components/ui/card'
import { m } from '@/paraglide/messages'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'
import { WebhookStatusBadge } from '@/components/webhook-status-badge'
import { WebhookPausedReasonPopover } from '@/components/webhook-paused-reason-popover'

type MutationResult = CreateWebhookSubscriptionResponse | WebhookSubscriptionItem

interface WebhookSubscriptionFormPageProps {
  mode: 'create' | 'edit'
  realmId: string
  webhook?: WebhookSubscriptionItem
}

export function WebhookSubscriptionFormPage({
  mode,
  realmId,
  webhook,
}: WebhookSubscriptionFormPageProps) {
  const isCreate = mode === 'create'
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const webhooksPath = realmPath({ ...realmContext, realmId }, '/manage/webhooks')

  const goToList = () => navigate({ to: webhooksPath })

  const { isSubmitting, mutate } = useFormMutation<MutationResult, WebhookSubscriptionFormData>({
    mutationFn: (data) => {
      if (isCreate) {
        return createWebhookSubscription({
          body: { endpointUrl: data.endpointUrl, eventCategories: data.eventCategories },
        }).then((response) => {
          if (response.error) throw response.error
          return response.data as CreateWebhookSubscriptionResponse
        })
      }
      return updateWebhookSubscription({
        path: { subscriptionId: webhook!.id },
        body: { endpointUrl: data.endpointUrl, eventCategories: data.eventCategories },
      }).then((response) => {
        if (response.error) throw response.error
        return response.data as WebhookSubscriptionItem
      })
    },
    invalidateQueries: [
      queryKeys.webhooksList(realmId),
      ...(isCreate ? [] : [queryKeys.webhook(realmId, webhook!.id)]),
    ],
    getSuccessMessage: () =>
      isCreate ? m['webhooks.created_success']() : m['webhooks.updated_success'](),
    onSuccess: async (data) => {
      if (isCreate) {
        // The plaintext secret exists only in this response; carry it to the
        // reveal page via router state (never URL/cache/store).
        void navigate({
          to: realmPath({ ...realmContext, realmId }, '/manage/webhooks/reveal'),
          state: { secretData: data } as unknown as Parameters<typeof navigate>[0]['state'],
        })
      } else {
        void goToList()
      }
    },
  })

  const form = useAppForm({
    schema: webhookSubscriptionSchema,
    defaultValues: {
      endpointUrl: webhook?.endpointUrl ?? '',
      eventCategories: webhook?.eventCategories ?? [],
    } as WebhookSubscriptionFormData,
    onSubmit: async ({ value }) => {
      try {
        await mutate(value)
      } catch (error) {
        // useFormMutation already toasted the server reason; a vanished
        // subscription (404) leaves nothing to edit, so return to the list.
        if ((error as { status?: number })?.status === 404) {
          void goToList()
        }
      }
    },
  })

  return (
    <div className="space-y-6" data-testid="webhook-form-page">
      <div className="flex items-center gap-4">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => goToList()}
          data-testid="webhook-form-back-button"
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div>
          <h1 className="text-2xl font-bold" data-testid="page-title">
            {isCreate ? m['webhooks.form_title_new']() : m['webhooks.form_title_edit']()}
          </h1>
          <p className="text-muted-foreground text-sm">
            {isCreate
              ? m['webhooks.form_description_new']()
              : m['webhooks.form_description_edit']()}
          </p>
        </div>
      </div>

      {!isCreate && webhook && (
        <Card>
          <CardContent className="flex flex-wrap items-center gap-3 pt-6">
            <WebhookStatusBadge status={webhook.status} />
            {webhook.status === 'paused' && <WebhookPausedReasonPopover subscription={webhook} />}
            <span className="text-sm text-muted-foreground" data-testid="webhook-secret-hint">
              {m['webhooks.secret_hint_label']()}:{' '}
              <span className="font-mono">{webhook.secretHint}</span>
            </span>
          </CardContent>
        </Card>
      )}

      <AppForm>
        <form
          onSubmit={(e) => {
            e.preventDefault()
            e.stopPropagation()
            form.handleSubmit()
          }}
          className="max-w-lg space-y-6"
        >
          <TextField
            form={form}
            name="endpointUrl"
            label={m['webhooks.endpoint_url_label']()}
            inputId="webhook-endpoint-url"
            dataTestId="webhook-endpoint-url-input"
            placeholder="https://example.com/webhooks/herald"
            disabled={isSubmitting}
            helpText={m['webhooks.endpoint_url_hint']()}
            required
            errorTestId="webhook-endpoint-url-error"
          />

          <form.Field
            name="eventCategories"
            children={(field) => {
              const toggleCategory = (
                category: (typeof WEBHOOK_EVENT_CATEGORIES)[number],
                checked: boolean
              ) => {
                const next = checked
                  ? [...field.state.value, category]
                  : field.state.value.filter((c) => c !== category)
                field.handleChange(next)
              }
              return (
                <div className="space-y-2">
                  <Label>{m['webhooks.table_categories']()} *</Label>
                  <div className="space-y-3">
                    {WEBHOOK_EVENT_CATEGORIES.map((category) => (
                      <label
                        key={category}
                        className="flex cursor-pointer items-start gap-3 rounded-lg border p-3"
                        data-testid={`webhook-category-option-${category}`}
                      >
                        <Checkbox
                          checked={field.state.value.includes(category)}
                          onCheckedChange={(checked) => toggleCategory(category, checked === true)}
                          disabled={isSubmitting}
                          data-testid={`webhook-category-checkbox-${category.replace(/_/g, '-')}`}
                        />
                        <span className="space-y-0.5">
                          <span className="block text-sm font-medium">
                            {WEBHOOK_CATEGORY_LABEL[category]()}
                          </span>
                          <span className="block text-xs text-muted-foreground">
                            {WEBHOOK_CATEGORY_DESCRIPTION[category]()}
                          </span>
                        </span>
                      </label>
                    ))}
                  </div>
                  {(field.state.meta.isTouched || form.state.isSubmitted) &&
                    field.state.meta.errors.length > 0 && (
                      <p
                        className="text-sm text-destructive"
                        data-testid="webhook-categories-error"
                      >
                        {getFieldErrorMessage(field.state.meta)}
                      </p>
                    )}
                </div>
              )
            }}
          />

          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => goToList()}
              data-testid="webhook-cancel-button"
            >
              {m['common.cancel']()}
            </Button>
            <Button type="submit" disabled={isSubmitting} data-testid="webhook-save-button">
              {isSubmitting
                ? isCreate
                  ? m['webhooks.creating']()
                  : m['webhooks.saving']()
                : m['webhooks.save']()}
            </Button>
          </div>
        </form>
      </AppForm>
    </div>
  )
}
