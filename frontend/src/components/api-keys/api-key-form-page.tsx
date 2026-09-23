import { useState } from 'react'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  createApiKeySchema,
  updateApiKeySchema,
  type CreateApiKeyFormData,
  type UpdateApiKeyFormData,
} from '@/lib/schemas/api-key-forms'
import { createApiKey, updateApiKey } from '@/lib/api-generated'
import type { CreateApiKeyResponse, ApiKeyListItem } from '@/lib/api-generated'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { usePermission } from '@/hooks/use-permission'
import { PERMISSION } from '@/lib/constants/auth-constants'
import { useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  clientAppsQueryOptions,
  queryKeys,
  adminRolesQueryOptions,
  updateApiKeyRolesMutation,
} from '@/data/query-options'
import { ArrowLeft } from 'lucide-react'
import { TextField, SwitchField } from '@/components/shared/form-fields'
import { RoleSelector } from '@/components/shared/role-selector'
import { ClientAppSelector } from '@/components/shared/client-app-selector'
import { m } from '@/paraglide/messages'
import { getErrorMessage } from '@/lib/error-utils'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'

type MutationResult = CreateApiKeyResponse | ApiKeyListItem

interface ApiKeyFormPageProps {
  mode: 'create' | 'edit'
  realmId: string
  apiKey?: ApiKeyListItem
}

export function ApiKeyFormPage({ mode, realmId, apiKey }: ApiKeyFormPageProps) {
  const isCreate = mode === 'create'
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const apiKeysPath = realmPath({ ...realmContext, realmId }, '/manage/api-keys')
  const { hasPermission } = usePermission()
  const canManageRoles = hasPermission(PERMISSION.ROLES_MANAGE)
  const [selectedRoleIds, setSelectedRoleIds] = useState<string[]>([])

  const { data: rolesData } = useQuery({
    ...adminRolesQueryOptions(realmId),
    enabled: isCreate && canManageRoles,
  })

  const { data: clientAppsData } = useQuery({
    ...clientAppsQueryOptions(realmId, { page: 0, pageSize: 100 }),
    enabled: isCreate,
  })

  const goToList = () => navigate({ to: apiKeysPath })

  const { isSubmitting, mutate } = useFormMutation<
    MutationResult,
    CreateApiKeyFormData | UpdateApiKeyFormData
  >({
    mutationFn: (data) => {
      if (isCreate) {
        return createApiKey({
          body: data as CreateApiKeyFormData,
        }).then((response) => {
          if (response.error) throw response.error
          return response.data as CreateApiKeyResponse
        })
      }
      return updateApiKey({
        path: { apiKeyId: apiKey!.id },
        body: data as UpdateApiKeyFormData,
      }).then((response) => {
        if (response.error) throw response.error
        return response.data as ApiKeyListItem
      })
    },
    invalidateQueries: [queryKeys.apiKeysList(realmId)],
    getSuccessMessage: () =>
      isCreate ? m['api_keys.created_success']() : m['api_keys.updated_success'](),
    onSuccess: async (data) => {
      if (isCreate) {
        // Roles are bound in a separate call after creation. Don't swallow its
        // failure: carry the real reason to the reveal page so it stays visible
        // (the plaintext key must still be revealed exactly once).
        let roleBindingError: string | undefined
        if (selectedRoleIds.length > 0 && canManageRoles) {
          try {
            await updateApiKeyRolesMutation(realmId, data.id, selectedRoleIds)
          } catch (error) {
            roleBindingError = getErrorMessage(error)
          }
        }
        void navigate({
          to: realmPath({ ...realmContext, realmId }, '/manage/api-keys/reveal'),
          state: {
            keyData: data,
            roleBindingError,
          } as unknown as Parameters<typeof navigate>[0]['state'],
        })
      } else {
        void goToList()
      }
    },
  })

  const form = useAppForm({
    schema: isCreate ? createApiKeySchema : updateApiKeySchema,
    defaultValues: isCreate
      ? ({
          name: '',
          expiresAt: '',
        } as CreateApiKeyFormData)
      : ({
          name: apiKey?.name ?? '',
          enabled: apiKey?.enabled ?? true,
          expiresAt: apiKey?.expiresAt ?? null,
        } as UpdateApiKeyFormData),
    onSubmit: async ({ value }) => {
      // Parse through schema to apply transforms (e.g. empty string -> undefined,
      // datetime-local -> RFC 3339)
      const schema = isCreate ? createApiKeySchema : updateApiKeySchema
      const parsed = schema.parse(value)
      await mutate(parsed)
    },
  })

  return (
    <div className="space-y-6" data-testid="api-key-form-page">
      <div className="flex items-center gap-4">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => goToList()}
          data-testid="api-key-form-back-button"
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div>
          <h1 className="text-2xl font-bold" data-testid="page-title">
            {isCreate ? m['api_keys.create_title']() : m['api_keys.edit_title']()}
          </h1>
          <p className="text-muted-foreground text-sm">
            {isCreate ? m['api_keys.create_description']() : m['api_keys.edit_description']()}
          </p>
        </div>
      </div>

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
            name="name"
            label={m['api_keys.form_name_label']()}
            inputId="api-key-name"
            dataTestId="api-key-name-input"
            placeholder={m['api_keys.form_name_placeholder']()}
            required
          />

          {isCreate && (
            <form.Field
              name="clientAppId"
              children={(field) => (
                <div className="space-y-2">
                  <Label>{m['api_keys.form_client_app_label']()}</Label>
                  <ClientAppSelector
                    clientApps={(clientAppsData?.items ?? []).map((app) => ({
                      id: app.id,
                      name: app.name,
                      clientId: app.clientId,
                    }))}
                    value={field.state.value}
                    onChange={field.handleChange}
                    disabled={isSubmitting}
                  />
                </div>
              )}
            />
          )}

          {!isCreate && (
            <SwitchField
              form={form}
              name="enabled"
              label={m['api_keys.form_enabled_label']()}
              inputId="api-key-enabled"
              dataTestId="api-key-enabled-switch"
            />
          )}

          <form.Field
            name="expiresAt"
            children={(field) => (
              <div className="space-y-2">
                <Label htmlFor="api-key-expires-at">{m['api_keys.form_expires_at_label']()}</Label>
                <div className="flex items-center gap-2">
                  <Input
                    id="api-key-expires-at"
                    type="datetime-local"
                    value={field.state.value ?? ''}
                    onChange={(e) =>
                      field.handleChange(
                        isCreate
                          ? (e.target.value as string | undefined)
                          : (e.target.value as string | null) || null
                      )
                    }
                    data-testid="api-key-expires-at-input"
                  />
                  {!isCreate && field.state.value && (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => field.handleChange(null)}
                      data-testid="api-key-expires-at-clear-button"
                    >
                      {m['api_keys.form_clear']()}
                    </Button>
                  )}
                </div>
              </div>
            )}
          />

          {isCreate && canManageRoles && (
            <div className="space-y-2">
              <Label>{m['api_keys.form_roles_label']()}</Label>
              <RoleSelector
                roles={(rolesData ?? [])
                  .filter((r) => !r.isBuiltin)
                  .map((r) => ({ id: r.id, name: r.name }))}
                selectedRoleIds={selectedRoleIds}
                onChange={setSelectedRoleIds}
                disabled={isSubmitting}
                placeholder={m['api_keys.form_roles_placeholder']()}
              />
              <p className="text-xs text-muted-foreground">{m['api_keys.form_roles_help']()}</p>
            </div>
          )}

          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => goToList()}
              data-testid="cancel-button"
            >
              {m['api_keys.form_cancel']()}
            </Button>
            <Button type="submit" disabled={isSubmitting} data-testid="submit-button">
              {isSubmitting
                ? isCreate
                  ? m['api_keys.form_creating']()
                  : m['api_keys.form_saving']()
                : isCreate
                  ? m['api_keys.form_create']()
                  : m['api_keys.form_save_changes']()}
            </Button>
          </div>
        </form>
      </AppForm>
    </div>
  )
}
