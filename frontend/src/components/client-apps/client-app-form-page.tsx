import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import {
  createClientAppSchema,
  updateClientAppSchema,
  DEFAULT_BROWSER_REFRESH_TTL_SECONDS,
  type CreateClientAppFormData,
  type UpdateClientAppFormData,
} from '@/lib/schemas/client-app-forms'
import { createClientApp, updateClientApp } from '@/lib/api-generated'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { useNavigate } from '@tanstack/react-router'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { RedirectUrisInput, type UriItem } from '@/components/client-apps/redirect-uris-input'
import { getFieldErrorMessage } from '@/lib/form-utils'
import { queryKeys } from '@/data/query-options'
import { toast } from 'sonner'
import { ArrowLeft } from 'lucide-react'
import type { ClientAppItem } from '@/lib/api-generated'
import { m } from '@/paraglide/messages'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'

function transformToUriItems(uris: string[]): UriItem[] {
  return uris.map((uri, index) => ({
    id: `init-${index}-${Date.now()}`,
    value: uri,
    isValid: true,
  }))
}

function transformFromUriItems(items: UriItem[]): string[] {
  return items.filter((item) => item.isValid).map((item) => item.value)
}

/** Presets for the browser refresh token family absolute TTL (design §4.3.2). */
const BROWSER_REFRESH_TTL_PRESETS = [
  { label: '1d', value: 86400 },
  { label: '7d', value: 604800 },
  { label: '14d', value: 1209600 },
  { label: '30d', value: 2592000 },
  { label: '60d', value: 5184000 },
  { label: '90d', value: 7776000 },
]

interface ClientAppFormPageProps {
  mode: 'create' | 'edit'
  realmId: string
  clientApp?: ClientAppItem
}

export function ClientAppFormPage({ mode, realmId, clientApp }: ClientAppFormPageProps) {
  const isCreate = mode === 'create'
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const clientAppsPath = realmPath({ ...realmContext, realmId }, '/manage/client-apps')

  const handleCancel = () => {
    navigate({ to: clientAppsPath })
  }

  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: (data: CreateClientAppFormData | UpdateClientAppFormData) => {
      if (isCreate) {
        const createData = data as CreateClientAppFormData
        // turnstileSecretKey is write-only. Omit it from the payload when empty
        // so create leaves the server-side secret unset (the generated client
        // drops undefined fields during serialization).
        const { turnstileSecretKey: _createSecret, ...createRest } = createData
        const createBody: CreateClientAppFormData = {
          ...createRest,
          ...(createData.turnstileSecretKey && createData.turnstileSecretKey.length > 0
            ? { turnstileSecretKey: createData.turnstileSecretKey }
            : {}),
        }
        return createClientApp({
          body: createBody,
        }).then((response) => {
          if (response.error) throw response.error
          return response.data
        })
      }
      const updateData = data as UpdateClientAppFormData
      // turnstileSecretKey is write-only and never echoed back. On update, an
      // empty value means "leave the stored secret untouched" — omit it from
      // the payload; a non-empty value replaces it.
      const { turnstileSecretKey: _updateSecret, ...updateRest } = updateData
      const updateBody: UpdateClientAppFormData = {
        ...updateRest,
        ...(updateData.turnstileSecretKey && updateData.turnstileSecretKey.length > 0
          ? { turnstileSecretKey: updateData.turnstileSecretKey }
          : {}),
      }
      return updateClientApp({
        path: { clientAppId: clientApp!.id },
        body: updateBody,
      }).then((response) => {
        if (response.error) throw response.error
        return response.data
      })
    },
    invalidateQueries: [queryKeys.clientAppsList(realmId)],
    onSuccess: (data) => {
      if (isCreate && data?.clientSecret) {
        toast.success(m['client_apps.created_with_secret'](), {
          description: data.clientSecret,
          duration: 15000,
        })
      } else {
        toast.success(
          isCreate ? m['client_apps.created_success']() : m['client_apps.updated_success']()
        )
      }
      navigate({ to: clientAppsPath })
    },
  })

  const form = useAppForm({
    schema: isCreate ? createClientAppSchema : updateClientAppSchema,
    defaultValues: isCreate
      ? ({
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
        } as CreateClientAppFormData)
      : ({
          name: clientApp?.name ?? '',
          description: clientApp?.description ?? '',
          redirectUris: clientApp?.redirectUris ?? [],
          iconUrl: clientApp?.iconUrl ?? '',
          enabled: clientApp?.enabled ?? true,
          browserRefreshAbsoluteTtlSeconds:
            clientApp?.browserRefreshAbsoluteTtlSeconds ?? DEFAULT_BROWSER_REFRESH_TTL_SECONDS,
          allowedOrigins: clientApp?.allowedOrigins ?? [],
          deviceCodeGrantEnabled: clientApp?.deviceCodeGrantEnabled ?? false,
          regenerateSecret: false,
          turnstileEnabled: clientApp?.turnstileEnabled ?? false,
          turnstileSiteKey: clientApp?.turnstileSiteKey ?? '',
          // NEVER pre-fill the secret: ClientAppItem intentionally omits
          // turnstileSecretKey (write-only). Empty means "leave stored secret
          // untouched" on update.
          turnstileSecretKey: '',
        } as UpdateClientAppFormData),
    onSubmit: async ({ value }) => {
      await mutate(value)
    },
  })

  return (
    <div className="space-y-6" data-testid="client-app-form-page">
      <div className="flex items-center gap-4">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={handleCancel}
          data-testid="client-app-form-back-button"
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div>
          <h1 className="text-2xl font-bold" data-testid="page-title">
            {isCreate ? m['client_apps.create_title']() : m['client_apps.edit_title']()}
          </h1>
          <p className="text-muted-foreground text-sm">
            {isCreate ? m['client_apps.create_description']() : m['client_apps.edit_description']()}
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
          className="max-w-4xl space-y-6"
        >
          <Tabs defaultValue="basic" className="w-full">
            <TabsList className="w-full">
              <TabsTrigger value="basic" data-testid="tab-basic">
                {m['client_apps.tab_basic']()}
              </TabsTrigger>
              <TabsTrigger value="redirect-uris" data-testid="tab-redirect-uris">
                {m['client_apps.tab_redirect_uris']()}
              </TabsTrigger>
              <TabsTrigger value="security" data-testid="tab-security">
                {m['client_apps.tab_security']()}
              </TabsTrigger>
              <TabsTrigger value="appearance" data-testid="tab-appearance">
                {m['client_apps.tab_appearance']()}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="basic" className="space-y-4 mt-4">
              {isCreate ? (
                <form.Field
                  name="clientId"
                  children={(field) => (
                    <div className="space-y-2">
                      <Label htmlFor="client-id">{m['client_apps.form_client_id_label']()}</Label>
                      <Input
                        id="client-id"
                        value={field.state.value ?? ''}
                        onChange={(e) => field.handleChange(e.target.value)}
                        placeholder={m['client_apps.form_client_id_placeholder']()}
                        data-testid="client-id-input"
                      />
                      {(field.state.meta.isTouched || form.state.isSubmitted) &&
                        field.state.meta.errors.length > 0 && (
                          <p className="text-sm text-destructive">
                            {getFieldErrorMessage(field.state.meta)}
                          </p>
                        )}
                    </div>
                  )}
                />
              ) : (
                <div className="space-y-2">
                  <Label>{m['client_apps.form_client_id_label']()}</Label>
                  <p
                    className="text-sm font-mono bg-muted px-3 py-2 rounded-md"
                    data-testid="client-id-display"
                  >
                    {clientApp?.clientId}
                  </p>
                </div>
              )}

              <form.Field
                name="name"
                children={(field) => (
                  <div className="space-y-2">
                    <Label htmlFor="app-name">{m['client_apps.form_name_label']()}</Label>
                    <Input
                      id="app-name"
                      value={field.state.value ?? ''}
                      onChange={(e) => field.handleChange(e.target.value)}
                      data-testid="client-app-name-input"
                    />
                    {(field.state.meta.isTouched || form.state.isSubmitted) &&
                      field.state.meta.errors.length > 0 && (
                        <p className="text-sm text-destructive">
                          {getFieldErrorMessage(field.state.meta)}
                        </p>
                      )}
                  </div>
                )}
              />

              <form.Field
                name="description"
                children={(field) => (
                  <div className="space-y-2">
                    <Label htmlFor="app-description">
                      {m['client_apps.form_description_label']()}
                    </Label>
                    <Textarea
                      id="app-description"
                      value={field.state.value ?? ''}
                      onChange={(e) => field.handleChange(e.target.value)}
                      rows={3}
                      placeholder={m['client_apps.form_description_placeholder']()}
                      data-testid="client-app-description-input"
                    />
                  </div>
                )}
              />
            </TabsContent>

            <TabsContent value="redirect-uris" className="mt-4">
              <form.Field
                name="redirectUris"
                children={(field) => (
                  <RedirectUrisInput
                    value={transformToUriItems(field.state.value ?? [])}
                    onChange={(items) => field.handleChange(transformFromUriItems(items))}
                    label={m['client_apps.form_redirect_uris_label']()}
                    required
                    helpText={m['client_apps.form_redirect_uris_help']()}
                    dataTestId="redirect-uris-input"
                  />
                )}
              />
            </TabsContent>

            <TabsContent value="security" className="space-y-4 mt-4">
              <form.Field
                name="enabled"
                children={(field) => (
                  <div className="flex items-center justify-between">
                    <Label htmlFor="app-enabled">{m['client_apps.form_enabled_label']()}</Label>
                    <Switch
                      id="app-enabled"
                      checked={field.state.value ?? true}
                      onCheckedChange={field.handleChange}
                      data-testid="client-app-enabled-switch"
                    />
                  </div>
                )}
              />

              <form.Field
                name="browserRefreshAbsoluteTtlSeconds"
                children={(field) => (
                  <div className="space-y-2">
                    <Label htmlFor="browser-refresh-ttl">
                      {m['client_apps.form_browser_refresh_ttl_label']()}
                    </Label>
                    <Input
                      id="browser-refresh-ttl"
                      type="number"
                      value={field.state.value ?? DEFAULT_BROWSER_REFRESH_TTL_SECONDS}
                      onChange={(e) => field.handleChange(Number(e.target.value))}
                      min={86400}
                      max={7776000}
                      placeholder={m['client_apps.form_browser_refresh_ttl_placeholder']()}
                      data-testid="browser-refresh-ttl-input"
                    />
                    <div className="flex flex-wrap gap-1">
                      {BROWSER_REFRESH_TTL_PRESETS.map((preset) => (
                        <Button
                          key={preset.value}
                          type="button"
                          variant="outline"
                          size="sm"
                          className="h-7 text-xs"
                          onClick={() => field.handleChange(preset.value)}
                          data-testid={`browser-refresh-ttl-preset-${preset.label}`}
                        >
                          {preset.label}
                        </Button>
                      ))}
                    </div>
                    <p className="text-xs text-muted-foreground">
                      {m['client_apps.form_browser_refresh_ttl_help']()}
                    </p>
                    {(field.state.meta.isTouched || form.state.isSubmitted) &&
                      field.state.meta.errors.length > 0 && (
                        <p className="text-sm text-destructive">
                          {getFieldErrorMessage(field.state.meta)}
                        </p>
                      )}
                  </div>
                )}
              />

              <form.Field
                name="allowedOrigins"
                children={(field) => (
                  <div className="space-y-2">
                    <RedirectUrisInput
                      value={transformToUriItems(field.state.value ?? [])}
                      onChange={(items) => field.handleChange(transformFromUriItems(items))}
                      label={m['client_apps.form_allowed_origins_label']()}
                      helpText={m['client_apps.form_allowed_origins_help']()}
                      placeholder="https://app.example.com"
                      dataTestId="allowed-origins-input"
                    />
                    {(field.state.meta.isTouched || form.state.isSubmitted) &&
                      field.state.meta.errors.length > 0 && (
                        <p className="text-sm text-destructive">
                          {getFieldErrorMessage(field.state.meta)}
                        </p>
                      )}
                  </div>
                )}
              />

              <form.Field
                name="deviceCodeGrantEnabled"
                children={(field) => (
                  <div className="flex items-center justify-between">
                    <Label htmlFor="device-code-grant">
                      {m['client_apps.form_device_code_grant_label']()}
                    </Label>
                    <Switch
                      id="device-code-grant"
                      checked={field.state.value ?? false}
                      onCheckedChange={field.handleChange}
                      data-testid="device-code-grant-switch"
                    />
                  </div>
                )}
              />

              <form.Field
                name="turnstileEnabled"
                children={(field) => (
                  <div className="flex items-center justify-between">
                    <div>
                      <Label htmlFor="app-turnstile-enabled">
                        {m['client_apps.form_turnstile_enabled_label']()}
                      </Label>
                      <p className="text-xs text-muted-foreground">
                        {m['client_apps.form_turnstile_enabled_hint']()}
                      </p>
                    </div>
                    <Switch
                      id="app-turnstile-enabled"
                      checked={field.state.value ?? false}
                      onCheckedChange={field.handleChange}
                      data-testid="client-app-turnstile-enabled-switch"
                    />
                  </div>
                )}
              />

              <form.Subscribe
                selector={(state) => state.values.turnstileEnabled}
                children={(turnstileEnabled) =>
                  turnstileEnabled ? (
                    <>
                      <form.Field
                        name="turnstileSiteKey"
                        children={(field) => (
                          <div className="space-y-2">
                            <Label htmlFor="app-turnstile-site-key">
                              {m['client_apps.form_turnstile_site_key_label']()}
                            </Label>
                            <Input
                              id="app-turnstile-site-key"
                              value={field.state.value ?? ''}
                              onChange={(e) => field.handleChange(e.target.value)}
                              placeholder={m['client_apps.form_turnstile_site_key_placeholder']()}
                              data-testid="client-app-turnstile-site-key-input"
                            />
                            <p className="text-xs text-muted-foreground">
                              {m['client_apps.form_turnstile_site_key_hint']()}
                            </p>
                            {(field.state.meta.isTouched || form.state.isSubmitted) &&
                              field.state.meta.errors.length > 0 && (
                                <p className="text-sm text-destructive">
                                  {getFieldErrorMessage(field.state.meta)}
                                </p>
                              )}
                          </div>
                        )}
                      />

                      <form.Field
                        name="turnstileSecretKey"
                        children={(field) => (
                          <div className="space-y-2">
                            <Label htmlFor="app-turnstile-secret-key">
                              {m['client_apps.form_turnstile_secret_key_label']()}
                            </Label>
                            <Input
                              id="app-turnstile-secret-key"
                              type="password"
                              value={field.state.value ?? ''}
                              onChange={(e) => field.handleChange(e.target.value)}
                              placeholder={
                                isCreate
                                  ? ''
                                  : m['client_apps.form_turnstile_secret_key_placeholder']()
                              }
                              data-testid="client-app-turnstile-secret-key-input"
                            />
                            <p className="text-xs text-muted-foreground">
                              {isCreate
                                ? m['client_apps.form_turnstile_secret_key_hint_create']()
                                : m['client_apps.form_turnstile_secret_key_hint_edit']()}
                            </p>
                            {(field.state.meta.isTouched || form.state.isSubmitted) &&
                              field.state.meta.errors.length > 0 && (
                                <p className="text-sm text-destructive">
                                  {getFieldErrorMessage(field.state.meta)}
                                </p>
                              )}
                          </div>
                        )}
                      />
                    </>
                  ) : null
                }
              />

              {!isCreate && (
                <form.Field
                  name="regenerateSecret"
                  children={(field) => (
                    <div className="flex items-center justify-between pt-4 border-t">
                      <div>
                        <Label htmlFor="regenerate-secret">
                          {m['client_apps.form_regenerate_secret_label']()}
                        </Label>
                        <p className="text-xs text-muted-foreground">
                          {m['client_apps.form_regenerate_secret_hint']()}
                        </p>
                      </div>
                      <Switch
                        id="regenerate-secret"
                        checked={field.state.value ?? false}
                        onCheckedChange={field.handleChange}
                        data-testid="regenerate-secret-switch"
                      />
                    </div>
                  )}
                />
              )}
            </TabsContent>

            <TabsContent value="appearance" className="space-y-4 mt-4">
              <form.Field
                name="iconUrl"
                children={(field) => (
                  <div className="space-y-2">
                    <Label htmlFor="icon-url">{m['client_apps.form_icon_url_label']()}</Label>
                    <Input
                      id="icon-url"
                      value={field.state.value ?? ''}
                      onChange={(e) => field.handleChange(e.target.value)}
                      placeholder={m['client_apps.form_icon_url_placeholder']()}
                      data-testid="icon-url-input"
                    />
                  </div>
                )}
              />
            </TabsContent>
          </Tabs>

          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={handleCancel}
              data-testid="cancel-button"
            >
              {m['client_apps.form_cancel']()}
            </Button>
            <Button type="submit" disabled={isSubmitting} data-testid="submit-button">
              {isSubmitting
                ? isCreate
                  ? m['client_apps.form_creating']()
                  : m['client_apps.form_saving']()
                : isCreate
                  ? m['client_apps.form_create']()
                  : m['client_apps.form_save_changes']()}
            </Button>
          </div>
        </form>
      </AppForm>
    </div>
  )
}
