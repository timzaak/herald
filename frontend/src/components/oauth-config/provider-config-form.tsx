import { useMemo } from 'react'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Checkbox } from '@/components/ui/checkbox'
import type { OAuthConfigResponse } from '@/lib/api-generated'
import {
  PROVIDER_DISPLAY_NAMES,
  DEFAULT_SCOPES,
  type ProviderType,
} from '@/lib/oauth-provider-constants'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { getOAuthConfigSchema, type OAuthConfigFormData } from '@/lib/schemas/oauth-config'
import { m } from '@/paraglide/messages'

interface ProviderConfigFormProps {
  editingConfig?: OAuthConfigResponse | undefined
  onSubmit: (values: OAuthConfigFormData) => Promise<void>
  isPending: boolean
  onCancel: () => void
}

/**
 * Get default values for the form based on editing config
 */
function getFormDefaultValues(editingConfig?: OAuthConfigResponse) {
  if (editingConfig) {
    return {
      providerType: editingConfig.providerType as ProviderType,
      clientId: editingConfig.clientId,
      clientSecret: '', // Always clear secret when editing
      scopes: editingConfig.scopes || [],
      enabled: editingConfig.enabled,
    }
  }
  return {
    providerType: 'google' as const,
    clientId: '',
    clientSecret: '',
    scopes: [],
    enabled: true,
  }
}

export function ProviderConfigForm({
  editingConfig,
  onSubmit,
  isPending,
  onCancel,
}: ProviderConfigFormProps) {
  const isEditing = !!editingConfig

  // Use dynamic schema based on edit mode
  const schema = useMemo(() => getOAuthConfigSchema(isEditing), [isEditing])

  // Compute default values based on editing config
  // This ensures the form is initialized with correct values
  const defaultValues = useMemo(() => getFormDefaultValues(editingConfig), [editingConfig])

  const form = useAppForm({
    schema,
    defaultValues,
    onSubmit: async ({ value }) => {
      console.log('[ProviderConfigForm] Form onSubmit triggered', {
        isEditing,
        value,
        formState: {
          canSubmit: form.state.canSubmit,
          isSubmitting: form.state.isSubmitting,
          isValid: form.state.isValid,
          isDirty: form.state.isDirty,
        },
      })
      await onSubmit(value)
    },
  })

  // Use form.Subscribe to get the current provider type from form state
  // This ensures the provider type is always in sync with the form field

  return (
    <AppForm>
      <form
        key={editingConfig?.providerType ?? 'new'}
        onSubmit={(e) => {
          e.preventDefault()
          form.handleSubmit()
        }}
      >
        <form.Subscribe
          selector={(state) => ({
            providerType: state.values.providerType,
          })}
        >
          {(subscriptionState) => {
            const activeProviderType = isEditing
              ? ((editingConfig?.providerType as ProviderType | undefined) ??
                subscriptionState.providerType)
              : subscriptionState.providerType

            return (
              <div className="space-y-4">
                {/* Provider Type Field */}
                <form.Field name="providerType">
                  {(field) => (
                    <div className="space-y-2">
                      <Label htmlFor="providerType">{m['oauth.form_provider_type_label']()}</Label>
                      <Select
                        value={field.state.value}
                        onValueChange={(value: ProviderType) => {
                          field.handleChange(value)
                          // Auto-update scopes for new configs
                          if (!isEditing && value) {
                            const newScopes = DEFAULT_SCOPES[value] || []
                            form.setFieldValue('scopes', newScopes)
                          }
                        }}
                        disabled={isEditing}
                      >
                        <SelectTrigger data-testid="oauth-provider-type-select">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {Object.entries(PROVIDER_DISPLAY_NAMES).map(([key, name]) => (
                            <SelectItem key={key} value={key}>
                              {name}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                      {(field.state.meta.isTouched || form.state.isSubmitted) &&
                        field.state.meta.errors.length > 0 && (
                          <p className="text-sm text-destructive">
                            {getFieldErrorMessage(field.state.meta)}
                          </p>
                        )}
                    </div>
                  )}
                </form.Field>

                {/* clientId Field */}
                <form.Field name="clientId">
                  {(field) => (
                    <div className="space-y-2">
                      <Label htmlFor="clientId">{m['oauth.form_client_id_label']()}</Label>
                      <Input
                        id="clientId"
                        value={field.state.value ?? ''}
                        onChange={(e) => field.handleChange(e.target.value)}
                        placeholder={m['oauth.form_client_id_placeholder']()}
                        data-testid="oauth-client-id-input"
                      />
                      {(field.state.meta.isTouched || form.state.isSubmitted) &&
                        field.state.meta.errors.length > 0 && (
                          <p className="text-sm text-destructive">
                            {getFieldErrorMessage(field.state.meta)}
                          </p>
                        )}
                    </div>
                  )}
                </form.Field>

                {/* clientSecret Field */}
                <form.Field name="clientSecret">
                  {(field) => (
                    <div className="space-y-2">
                      <Label htmlFor="clientSecret">
                        {m['oauth.form_client_secret_label']()}{' '}
                        {isEditing && (
                          <span className="text-xs text-muted-foreground">
                            {m['oauth.form_client_secret_keep_hint']()}
                          </span>
                        )}
                      </Label>
                      <PasswordInput
                        id="clientSecret"
                        value={field.state.value ?? ''}
                        onChange={(e) => field.handleChange(e.target.value)}
                        placeholder={
                          isEditing
                            ? m['oauth.form_client_secret_edit_placeholder']()
                            : m['oauth.form_client_secret_create_placeholder']()
                        }
                        data-testid="oauth-client-secret-input"
                      />
                      {!isEditing && field.state.meta.errors.length > 0 && (
                        <p className="text-sm text-destructive">
                          {getFieldErrorMessage(field.state.meta)}
                        </p>
                      )}
                    </div>
                  )}
                </form.Field>

                {/* Scopes Field - Conditionally render based on provider type */}
                {activeProviderType !== 'wechat_miniprogram' && (
                  <form.Field name="scopes">
                    {(field) => (
                      <div className="space-y-2">
                        <Label htmlFor="scopes">
                          {m['oauth.form_scopes_label']()}
                          {activeProviderType === 'wechat' && (
                            <span className="text-xs text-muted-foreground">
                              {' '}
                              {m['oauth.form_scopes_wechat_hint']()}
                            </span>
                          )}
                        </Label>
                        {activeProviderType === 'wechat' ? (
                          <Input
                            id="scopes"
                            value={field.state.value?.join(', ') ?? ''}
                            disabled
                            data-testid="oauth-scopes-input"
                          />
                        ) : (
                          <Input
                            id="scopes"
                            value={field.state.value?.join(', ') ?? ''}
                            onChange={(e) =>
                              field.handleChange(e.target.value.split(',').map((s) => s.trim()))
                            }
                            placeholder={m['oauth.form_scopes_placeholder']()}
                            data-testid="oauth-scopes-input"
                          />
                        )}
                        {(field.state.meta.isTouched || form.state.isSubmitted) &&
                          field.state.meta.errors.length > 0 && (
                            <p className="text-sm text-destructive">
                              {getFieldErrorMessage(field.state.meta)}
                            </p>
                          )}
                      </div>
                    )}
                  </form.Field>
                )}

                {/* Enabled Field */}
                <form.Field name="enabled">
                  {(field) => (
                    <div className="flex items-center space-x-2">
                      <Checkbox
                        id="enabled"
                        checked={field.state.value ?? false}
                        onCheckedChange={(checked) => field.handleChange(checked === true)}
                        data-testid="oauth-enabled-checkbox"
                      />
                      <Label htmlFor="enabled">{m['oauth.form_enable_label']()}</Label>
                    </div>
                  )}
                </form.Field>

                {/* Actions */}
                <div className="flex justify-end space-x-2 pt-4">
                  <Button
                    type="button"
                    variant="outline"
                    onClick={onCancel}
                    data-testid="oauth-cancel-provider-button"
                  >
                    {m['oauth.form_cancel']()}
                  </Button>
                  <form.Subscribe
                    selector={(state) => ({
                      canSubmit: state.canSubmit,
                      isSubmitting: state.isSubmitting,
                    })}
                  >
                    {(subscribeState) => (
                      <Button
                        type="submit"
                        disabled={isPending || !subscribeState.canSubmit}
                        data-testid="oauth-save-provider-button"
                      >
                        {isPending
                          ? m['oauth.form_saving']()
                          : isEditing
                            ? m['oauth.form_save']()
                            : m['oauth.form_create']()}
                      </Button>
                    )}
                  </form.Subscribe>
                </div>
              </div>
            )
          }}
        </form.Subscribe>
      </form>
    </AppForm>
  )
}
