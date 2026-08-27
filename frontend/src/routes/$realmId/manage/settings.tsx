import { createFileRoute } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  listRealmConfigs,
  batchUpsertRealmConfigs,
  updateRealm,
  handleUpdateRealmPasskeyConfig,
  handleUpdateRealmEmailOtpConfig,
  handleSaveWhiteLabelDraft,
  handlePublishWhiteLabelConfig,
  handleDiscardWhiteLabelDraft,
  handleRestoreWhiteLabelConfig,
  handleUpdateCustomDomainConfig,
} from '@/lib/api-generated'
import type { UpsertRealmConfigRequest } from '@/lib/api-generated/types.gen'
import { TOTPConfigForm as TOTPConfigFormComponent } from '@/components/realm-config/totp-config-form'
import { PasskeyConfigForm as PasskeyConfigFormComponent } from '@/components/realm-config/passkey-config-form'
import { RegistrationConfigForm as RegistrationConfigFormComponent } from '@/components/realm-config/registration-config-form'
import { PlatformSignupConfigForm as PlatformSignupConfigFormComponent } from '@/components/realm-config/platform-signup-config-form'
import { EmailConfigForm as EmailConfigFormComponent } from '@/components/realm-config/email-config-form'
import { TurnstileConfigForm as TurnstileConfigFormComponent } from '@/components/realm-config/turnstile-config-form'
import { LdapConfigForm as LdapConfigFormComponent } from '@/components/realm-config/ldap-config-form'
import { WhiteLabelConfigForm as WhiteLabelConfigFormComponent } from '@/components/realm-config/white-label-config-form'
import { CustomDomainConfigForm as CustomDomainConfigFormComponent } from '@/components/realm-config/custom-domain-config-form'
import { ProviderConfigPage } from '@/components/oauth-config/provider-config-page'
import { LegalAgreementTab } from '@/components/settings/LegalAgreementTab'
import { useAuth } from '@/hooks/use-auth'
import { ADMIN_REALM_ID, PERMISSION } from '@/lib/constants/auth-constants'
import { toast } from 'sonner'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import type {
  TOTPConfigForm,
  RegistrationConfigForm,
  PlatformSignupConfigForm,
  EmailConfigForm,
  TurnstileConfigForm,
  PasskeyConfigForm,
  EmailOtpConfigForm,
  LdapConfigForm,
  WhiteLabelConfigForm as WhiteLabelConfigFormValues,
  CustomDomainConfigForm as CustomDomainConfigFormValues,
} from '@/lib/schemas/realm-config'
import {
  parseTOTPConfig,
  parseRegistrationConfig,
  parsePlatformSignupConfig,
  parseEmailConfig,
  parseTurnstileConfig,
  buildTOTPConfigRequest,
  buildRegistrationConfigRequest,
  buildPlatformSignupConfigRequest,
  buildEmailConfigRequest,
  buildTurnstileConfigRequest,
  parseLdapConfig,
  buildLdapConfigRequest,
  normalizeWhiteLabelConfig,
  toUpdateWhiteLabelConfigRequest,
  normalizeCustomDomainConfig,
  toUpdateCustomDomainConfigRequest,
} from '@/lib/realm-config-utils'
import { useState, useEffect } from 'react'
import { PageHeader, AccessDenied } from '@/components/shared'
import {
  queryKeys,
  realmQueryOptions,
  emailStatusQueryOptions,
  passkeyRealmConfigQueryOptions,
  emailOtpRealmConfigQueryOptions,
  ldapRealmConfigQueryOptions,
  whiteLabelRealmConfigQueryOptions,
  customDomainRealmConfigQueryOptions,
} from '@/data/query-options'
import { useAppForm, AppForm } from '@/components/ui/tanstack-form'
import { updateRealmSchema, type UpdateRealmFormData } from '@/lib/schemas/realm'
import { useFormMutation } from '@/hooks/use-form-mutation'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { TextField } from '@/components/shared/form-fields/text-field'
import { TextareaField } from '@/components/shared/form-fields/textarea-field'
import { m } from '@/paraglide/messages'
import { getErrorMessage, resolveApiError } from '@/lib/error-utils'
import { useOptionalRouteParams, useResolvedRealmId } from '@/lib/realm-routing'

export const Route = createFileRoute('/$realmId/manage/settings')({
  component: SettingsPage,
})

function GeneralTab({ realmId }: { realmId: string }) {
  const { data: realm, isLoading } = useQuery(realmQueryOptions(realmId))
  const auth = useAuth()
  const canUpdate = auth.permissions?.includes(PERMISSION.SETTINGS_MANAGE) ?? false

  const { isSubmitting, mutate } = useFormMutation({
    mutationFn: (data: UpdateRealmFormData) => updateRealm({ path: { realmId }, body: data }),
    getSuccessMessage: () => m['settings.realm_updated_success'](),
    invalidateQueries: [queryKeys.realm(realmId)],
  })

  const form = useAppForm({
    schema: updateRealmSchema,
    defaultValues: {
      name: '',
      description: '',
    },
    onSubmit: async ({ value }) => {
      await mutate(value)
    },
  })

  useEffect(() => {
    if (realm?.name !== undefined) {
      form.setFieldValue('name', realm.name)
    }
    if (realm !== undefined) {
      form.setFieldValue('description', realm.description ?? '')
    }
  }, [realm, form])

  if (isLoading) return <div>{m['settings.general_loading']()}</div>

  return (
    <Card>
      <CardContent className="space-y-4 max-w-lg pt-6">
        <div className="space-y-2">
          <Label>{m['settings.general_realm_id_label']()}</Label>
          <Input value={realmId} disabled data-testid="general-realm-id" />
        </div>

        <AppForm>
          <form
            id="general-realm-form"
            onSubmit={async (e) => {
              e.preventDefault()
              await form.handleSubmit()
            }}
          >
            <TextField
              form={form}
              name="name"
              label={m['settings.general_realm_name_label']()}
              inputId="general-realm-name"
              dataTestId="general-realm-name-input"
              disabled={!canUpdate}
            />
            <div className="mt-4">
              <TextareaField
                form={form}
                name="description"
                label={m['settings.general_description_label']()}
                inputId="general-realm-description"
                dataTestId="general-realm-description-input"
                disabled={!canUpdate}
                rows={3}
              />
            </div>
            {canUpdate && (
              <div className="mt-4">
                <Button
                  type="submit"
                  form="general-realm-form"
                  disabled={isSubmitting}
                  data-testid="general-realm-save"
                >
                  {isSubmitting ? m['settings.general_saving']() : m['settings.general_save']()}
                </Button>
              </div>
            )}
          </form>
        </AppForm>
      </CardContent>
    </Card>
  )
}

/**
 * Maps a thrown white-label mutation error to a user-facing message. The
 * generated SDK rejects with an `ErrorResponse` ({ code, message }) on HTTP
 * failures; standard `Error` instances surface their message. Falls back to the
 * supplied default so a toast always shows something actionable.
 */
function resolveErrorMessage(error: unknown, fallback: string): string {
  return resolveApiError(error).message ? getErrorMessage(error) : fallback
}

/**
 * Maps a config-save mutation error to the toast message shared by every
 * settings-tab mutation: 401/403 get their dedicated permission copy, any
 * other failure falls back to the generic save-failed text (surfacing the
 * backend message when present).
 */
function resolveConfigSaveError(error: unknown): string {
  const status = resolveApiError(error).status
  return status === 401
    ? m['settings.config_save_unauthorized']()
    : status === 403
      ? m['settings.config_save_forbidden']()
      : resolveErrorMessage(error, m['settings.config_save_failed']())
}

export function SettingsPage() {
  const fallbackRealmId = useResolvedRealmId()
  const routeParams = useOptionalRouteParams<{ realmId?: string }>(Route)
  const realmId = routeParams.realmId ?? fallbackRealmId
  const queryClient = useQueryClient()
  const auth = useAuth()
  const [activeTab, setActiveTab] = useState('general')

  const canViewConfig = auth.permissions?.includes(PERMISSION.SETTINGS_VIEW) ?? false
  const canUpdateConfig = auth.permissions?.includes(PERMISSION.SETTINGS_MANAGE) ?? false

  const {
    data: configs = [],
    isLoading,
    error,
  } = useQuery({
    queryKey: queryKeys.realmConfigs(realmId),
    queryFn: async () => {
      const response = await listRealmConfigs({ path: { realmId } })
      if (response.error) {
        throw response.error
      }
      return response.data
    },
    enabled: !!realmId && canViewConfig,
  })

  const { data: emailStatusData, error: emailStatusQueryError } = useQuery({
    ...emailStatusQueryOptions(realmId),
    enabled: !!realmId && canViewConfig,
  })

  // Passkey Realm config via dedicated endpoint (GET /api/realms/{realmId}/config/passkey)
  const { data: passkeyConfigData } = useQuery({
    ...passkeyRealmConfigQueryOptions(realmId),
    enabled: !!realmId && canViewConfig,
  })

  // Email-OTP Realm config via dedicated endpoint
  // (GET /api/realms/{realmId}/config/email-otp). Requires `settings.view`;
  // consumed by the Settings → Security "Email code" tab (design §4.2, §5.5).
  const { data: emailOtpConfigData } = useQuery({
    ...emailOtpRealmConfigQueryOptions(realmId),
    enabled: !!realmId && canViewConfig,
  })

  // LDAP directory config rows via the generic configs by-type list
  // (GET /api/configs/{realmId}/ldap). Requires `settings.view`; consumed by
  // the Settings "Corporate directory (LDAP)" tab. Dedicated query (instead of
  // the page-wide list-all) so saving only invalidates the LDAP keys.
  const { data: ldapConfigData, isLoading: isLdapConfigLoading } = useQuery({
    ...ldapRealmConfigQueryOptions(realmId),
    enabled: !!realmId && canViewConfig,
  })

  // White-label management state (published / draft / hasPrevious) via
  // GET /api/realms/{realmId}/config/white-label. Requires `settings.view`.
  const { data: whiteLabelConfigData, isLoading: isWhiteLabelLoading } = useQuery({
    ...whiteLabelRealmConfigQueryOptions(realmId),
    enabled: !!realmId && canViewConfig,
  })

  // Custom-domain management state (published / status) via
  // GET /api/realms/{realmId}/config/custom-domain. Requires `settings.view`.
  const {
    data: customDomainConfigData,
    isLoading: isCustomDomainLoading,
    isFetching: isCustomDomainRefreshing,
    refetch: refetchCustomDomainStatus,
  } = useQuery({
    ...customDomainRealmConfigQueryOptions(realmId),
    enabled: !!realmId && canViewConfig,
  })

  const mutation = useMutation({
    mutationFn: (configs: UpsertRealmConfigRequest[]) =>
      batchUpsertRealmConfigs({
        path: { realmId },
        body: { configs },
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.realmConfigs(realmId) })
      toast.success(m['settings.config_saved_success']())
    },
    onError: (error: unknown) => {
      console.error('Failed to save config:', error)
      toast.error(resolveConfigSaveError(error))
    },
  })

  // Dedicated Passkey config mutation (PUT /api/realms/{realmId}/config/passkey).
  // Passkey uses its own camelCase endpoint (see passkeyConfigSchema), distinct
  // from the generic snake_case realm_configs store used by TOTP/Turnstile/etc.
  const passkeyMutation = useMutation({
    mutationFn: (config: PasskeyConfigForm) =>
      handleUpdateRealmPasskeyConfig({
        path: { realmId },
        body: {
          enabled: config.enabled,
          userVerification: config.userVerification,
          crossPlatformAuthenticator: config.crossPlatformAuthenticator,
        },
      }).then((response) => {
        if (response.error) throw response.error
        return response.data
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.passkeyRealmConfig(realmId) })
      toast.success(m['settings.config_saved_success']())
    },
    onError: (error: unknown) => {
      console.error('Failed to save passkey config:', error)
      toast.error(resolveConfigSaveError(error))
    },
  })

  // Dedicated Email-OTP config mutation (PUT /api/realms/{realmId}/config/email-otp).
  // Email-OTP uses its own camelCase endpoint (see emailOtpConfigSchema), distinct
  // from the generic snake_case realm_configs store used by TOTP/Turnstile/etc.
  const emailOtpMutation = useMutation({
    mutationFn: (config: EmailOtpConfigForm) =>
      handleUpdateRealmEmailOtpConfig({
        path: { realmId },
        body: {
          enabled: config.enabled,
          autoRegister: config.autoRegister,
        },
      }).then((response) => {
        if (response.error) throw response.error
        return response.data
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.emailOtpRealmConfig(realmId) })
      toast.success(m['settings.config_saved_success']())
    },
    onError: (error: unknown) => {
      console.error('Failed to save email-otp config:', error)
      toast.error(resolveConfigSaveError(error))
    },
  })

  // LDAP directory config mutation (generic configs batch upsert). Saving the
  // `settings` row can flip the login-page entry visibility, so it also
  // invalidates the public `ldapStatus` query (same rationale as white-label
  // publish invalidating `publicConfig`).
  const ldapMutation = useMutation({
    mutationFn: (config: LdapConfigForm) =>
      batchUpsertRealmConfigs({
        path: { realmId },
        body: { configs: buildLdapConfigRequest(config) },
      }).then((response) => {
        if (response.error) throw response.error
        return response.data
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.ldapRealmConfig(realmId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.ldapStatus(realmId) })
      toast.success(m['settings.config_saved_success']())
    },
    onError: (error: unknown) => {
      console.error('Failed to save LDAP config:', error)
      toast.error(resolveConfigSaveError(error))
    },
  })

  // --- White-label mutations ---------------------------------------------------
  // Invalidate boundary (design §4.4.2): save-draft / discard-draft touch only
  // the admin draft state, so they invalidate `whiteLabelRealmConfig` only.
  // publish / restore change the published config, so they additionally
  // invalidate `publicConfig(realmId)` — otherwise the terminal-user auth
  // pages keep serving the stale published branding.
  const saveWhiteLabelDraftMutation = useMutation({
    mutationFn: (config: WhiteLabelConfigFormValues) =>
      handleSaveWhiteLabelDraft({
        path: { realmId },
        body: toUpdateWhiteLabelConfigRequest(config),
      }).then((response) => {
        if (response.error) throw response.error
        return response.data
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.whiteLabelRealmConfig(realmId) })
      toast.success(m['settings.white_label.save_draft_success']())
    },
    onError: (error: unknown) => {
      console.error('Failed to save white-label draft:', error)
      toast.error(resolveErrorMessage(error, m['settings.white_label.save_failed']()))
    },
  })

  const publishWhiteLabelMutation = useMutation({
    mutationFn: (config: WhiteLabelConfigFormValues) =>
      handlePublishWhiteLabelConfig({
        path: { realmId },
        body: toUpdateWhiteLabelConfigRequest(config),
      }).then((response) => {
        if (response.error) throw response.error
        return response.data
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.whiteLabelRealmConfig(realmId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.publicConfig(realmId) })
      toast.success(m['settings.white_label.publish_success']())
    },
    onError: (error: unknown) => {
      console.error('Failed to publish white-label config:', error)
      toast.error(resolveErrorMessage(error, m['settings.white_label.publish_failed']()))
    },
  })

  const discardWhiteLabelDraftMutation = useMutation({
    mutationFn: () =>
      handleDiscardWhiteLabelDraft({ path: { realmId } }).then((response) => {
        if (response.error) throw response.error
        return response.data
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.whiteLabelRealmConfig(realmId) })
      toast.success(m['settings.white_label.discard_draft_success']())
    },
    onError: (error: unknown) => {
      console.error('Failed to discard white-label draft:', error)
      toast.error(resolveErrorMessage(error, m['settings.white_label.discard_failed']()))
    },
  })

  const restoreWhiteLabelMutation = useMutation({
    mutationFn: () =>
      handleRestoreWhiteLabelConfig({ path: { realmId } }).then((response) => {
        if (response.error) throw response.error
        return response.data
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.whiteLabelRealmConfig(realmId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.publicConfig(realmId) })
      toast.success(m['settings.white_label.restore_success']())
    },
    onError: (error: unknown) => {
      console.error('Failed to restore white-label config:', error)
      toast.error(resolveErrorMessage(error, m['settings.white_label.restore_failed']()))
    },
  })

  // --- Custom-domain mutations -------------------------------------------------
  // A single update writes the published config + host→realm mapping in one
  // step, so it always changes what terminal-user auth pages serve and must
  // invalidate `publicConfig(realmId)` as well as `customDomainRealmConfig`.
  // The rethrow carries the HTTP `status` onto the thrown body
  // (`{ ...body, status }`): `onError` needs it to route 409 to the dedicated
  // localized `domain_in_use` message. The raw `response.error` is the parsed
  // JSON body and does NOT carry `.status`, so `onError` cannot distinguish a
  // 409 from a 400/500 without it.
  const updateCustomDomainMutation = useMutation({
    mutationFn: (config: CustomDomainConfigFormValues) =>
      handleUpdateCustomDomainConfig({
        path: { realmId },
        body: toUpdateCustomDomainConfigRequest(config),
      }).then((response) => {
        if (response.error) {
          const status = response.response?.status
          throw status ? { ...response.error, status } : response.error
        }
        return response.data
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.customDomainRealmConfig(realmId) })
      queryClient.invalidateQueries({ queryKey: queryKeys.publicConfig(realmId) })
      toast.success(m['settings.custom_domain.save_success']())
    },
    onError: (error: unknown) => {
      console.error('Failed to update custom-domain config:', error)
      // 409 "hostname already in use" has a dedicated localized message; other
      // errors fall back to the generic save_failed string.
      const status = (error as { status?: number })?.status
      if (status === 409) {
        toast.error(m['settings.custom_domain.domain_in_use']())
      } else {
        toast.error(resolveErrorMessage(error, m['settings.custom_domain.save_failed']()))
      }
    },
  })

  if (!canViewConfig) {
    return <AccessDenied message={m['settings.config_access_denied']()} />
  }

  if (isLoading) {
    return <div>{m['settings.config_loading']()}</div>
  }

  if (error) {
    const errorMessage = getErrorMessage(error)
    toast.error(m['settings.config_failed_to_load']({ message: errorMessage }))
    return <div>{m['settings.config_error_loading']()}</div>
  }

  const totpConfig = parseTOTPConfig(configs || [])
  const turnstileConfig = parseTurnstileConfig(configs || [])
  const registrationConfig = parseRegistrationConfig(configs || [])
  const emailConfig = parseEmailConfig(configs || [])
  // Platform self-service signup is an admin-realm-only switch (DEC-001/009).
  // Parsed unconditionally; the tab/UI is gated on `realmId === 'admin'`.
  const platformSignupConfig = parsePlatformSignupConfig(configs || [])

  // Derive Passkey form values from the dedicated endpoint response.
  // The API returns `userVerification` as a generic string; narrow it to the
  // schema enum, falling back to 'preferred' for unexpected values.
  const passkeyInitialConfig: PasskeyConfigForm | undefined = passkeyConfigData
    ? {
        enabled: passkeyConfigData.enabled,
        userVerification:
          passkeyConfigData.userVerification === 'required' ? 'required' : 'preferred',
        crossPlatformAuthenticator: passkeyConfigData.crossPlatformAuthenticator,
      }
    : undefined

  async function saveTOTPConfig(config: TOTPConfigForm) {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    // The mutation's `onError` surfaces the failure toast and logs the error.
    // Swallow the rejected promise so it doesn't propagate as an unhandled
    // rejection (per vitest config, rejections are expected to be handled in
    // components). Sibling forms share this latent leak; see FE-T06 handoff.
    await mutation.mutateAsync([buildTOTPConfigRequest(config)]).catch(() => {})
  }

  async function saveTurnstileConfig(config: TurnstileConfigForm) {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await mutation.mutateAsync(buildTurnstileConfigRequest(config)).catch(() => {})
  }

  async function saveRegistrationConfig(config: RegistrationConfigForm) {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await mutation.mutateAsync(buildRegistrationConfigRequest(config)).catch(() => {})
  }

  async function savePlatformSignupConfig(config: PlatformSignupConfigForm) {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await mutation.mutateAsync(buildPlatformSignupConfigRequest(config)).catch(() => {})
  }

  async function saveEmailConfig(config: EmailConfigForm) {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await mutation.mutateAsync(buildEmailConfigRequest(config)).catch(() => {})
    queryClient.invalidateQueries({ queryKey: queryKeys.emailStatus(realmId) })
  }

  async function savePasskeyConfig(config: PasskeyConfigForm) {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await passkeyMutation.mutateAsync(config).catch(() => {})
  }

  async function saveEmailOtpConfig(config: EmailOtpConfigForm) {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await emailOtpMutation.mutateAsync(config).catch(() => {})
  }

  async function saveLdapConfig(config: LdapConfigForm) {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await ldapMutation.mutateAsync(config).catch(() => {})
  }

  // --- White-label action wrappers --------------------------------------------
  // Each wrapper guards `settings.manage` (matching sibling forms) and swallows
  // the rejected promise — the mutation's `onError` already surfaces the toast.
  async function saveWhiteLabelDraft(config: WhiteLabelConfigFormValues) {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await saveWhiteLabelDraftMutation.mutateAsync(config).catch(() => {})
  }

  async function publishWhiteLabel(config: WhiteLabelConfigFormValues) {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await publishWhiteLabelMutation.mutateAsync(config).catch(() => {})
  }

  async function discardWhiteLabelDraft() {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await discardWhiteLabelDraftMutation.mutateAsync().catch(() => {})
  }

  async function restoreWhiteLabel() {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await restoreWhiteLabelMutation.mutateAsync().catch(() => {})
  }

  // --- Custom-domain action wrappers ------------------------------------------
  // Guards `settings.manage` and swallows the rejected promise — the mutation's
  // `onError` already surfaces the toast.
  async function updateCustomDomain(config: CustomDomainConfigFormValues) {
    if (!canUpdateConfig) {
      toast.error(m['settings.config_modify_denied']())
      return
    }

    await updateCustomDomainMutation.mutateAsync(config).catch(() => {})
  }

  return (
    <div className="space-y-6" data-testid="settings-page">
      <PageHeader title={m['settings.page_title']()} />

      <Tabs value={activeTab} onValueChange={setActiveTab}>
        <TabsList>
          <TabsTrigger value="general" data-testid="general-tab">
            {m['settings.tab_general']()}
          </TabsTrigger>
          <TabsTrigger value="totp" data-testid="totp-tab">
            {m['settings.tab_totp']()}
          </TabsTrigger>
          <TabsTrigger value="passkey" data-testid="passkey-tab">
            {m['settings.tab_passkey']()}
          </TabsTrigger>
          <TabsTrigger value="turnstile" data-testid="turnstile-tab">
            {m['settings.tab_turnstile']()}
          </TabsTrigger>
          <TabsTrigger value="registration" data-testid="registration-tab">
            {m['settings.tab_registration']()}
          </TabsTrigger>
          <TabsTrigger value="email" data-testid="email-tab">
            {m['settings.tab_email']()}
          </TabsTrigger>
          <TabsTrigger value="ldap" data-testid="ldap-tab">
            {m['settings.ldap.tab']()}
          </TabsTrigger>
          <TabsTrigger value="providers" data-testid="providers-tab">
            {m['settings.tab_providers']()}
          </TabsTrigger>
          <TabsTrigger value="legal" data-testid="legal-tab">
            {m['settings.legal.tab_legal']()}
          </TabsTrigger>
          <TabsTrigger value="white-label" data-testid="white-label-tab">
            {m['settings.white_label.tab_white_label']()}
          </TabsTrigger>
          <TabsTrigger value="custom-domain" data-testid="custom-domain-tab">
            {m['settings.custom_domain.tab_custom_domain']()}
          </TabsTrigger>
          {realmId === ADMIN_REALM_ID && (
            <TabsTrigger value="platform-signup" data-testid="platform-signup-tab">
              {m['settings.tab_platform_signup']()}
            </TabsTrigger>
          )}
        </TabsList>

        <TabsContent value="general">
          <GeneralTab realmId={realmId} />
        </TabsContent>

        <TabsContent value="totp">
          <TOTPConfigFormComponent
            initialConfig={totpConfig}
            onSave={saveTOTPConfig}
            isLoading={isLoading}
            disabled={!canUpdateConfig}
          />
        </TabsContent>

        <TabsContent value="passkey">
          <PasskeyConfigFormComponent
            initialConfig={passkeyInitialConfig}
            onSave={savePasskeyConfig}
            isLoading={isLoading}
            disabled={!canUpdateConfig}
          />
        </TabsContent>

        <TabsContent value="turnstile">
          <TurnstileConfigFormComponent
            initialConfig={turnstileConfig}
            onSave={saveTurnstileConfig}
            isLoading={isLoading}
            disabled={!canUpdateConfig}
          />
        </TabsContent>

        <TabsContent value="registration">
          <RegistrationConfigFormComponent
            initialConfig={registrationConfig}
            onSave={saveRegistrationConfig}
            isLoading={isLoading}
            disabled={!canUpdateConfig}
            emailConfigured={emailStatusData?.configured ?? false}
          />
        </TabsContent>

        <TabsContent value="email">
          <EmailConfigFormComponent
            realmId={realmId}
            initialConfig={emailConfig}
            onSave={saveEmailConfig}
            isLoading={isLoading}
            disabled={!canUpdateConfig}
            emailStatus={emailStatusData ?? null}
            emailStatusError={
              emailStatusQueryError instanceof Error ? emailStatusQueryError.message : null
            }
            emailOtpInitialConfig={emailOtpConfigData ?? undefined}
            onSaveEmailOtp={saveEmailOtpConfig}
          />
        </TabsContent>

        <TabsContent value="ldap">
          {(() => {
            // Fail-closed defaults while the query loads or on a malformed
            // stored row; once data arrives the query re-renders the form.
            const ldapState = parseLdapConfig(ldapConfigData ?? [])

            if (isLdapConfigLoading && !ldapConfigData) {
              return <div>{m['settings.config_loading']()}</div>
            }

            return (
              <LdapConfigFormComponent
                initialConfig={ldapState}
                hasBindPassword={ldapState.hasBindPassword}
                onSave={saveLdapConfig}
                isLoading={isLdapConfigLoading}
                disabled={!canUpdateConfig}
              />
            )
          })()}
        </TabsContent>

        <TabsContent value="providers">
          <ProviderConfigPage realmId={realmId} />
        </TabsContent>

        <TabsContent value="legal">
          <LegalAgreementTab realmId={realmId} canManage={canUpdateConfig} />
        </TabsContent>

        <TabsContent value="white-label">
          {(() => {
            // The form edits `draft ?? published`. When the query is still
            // loading (or returned nothing), fall back to an empty config so the
            // tab renders immediately; once data arrives the query re-renders.
            const wlState = whiteLabelConfigData
            const initialConfig = wlState
              ? normalizeWhiteLabelConfig(wlState.draft ?? wlState.published)
              : normalizeWhiteLabelConfig(null)
            const hasDraft = !!wlState?.draft
            const hasPrevious = !!wlState?.hasPrevious

            if (isWhiteLabelLoading && !wlState) {
              return <div>{m['settings.config_loading']()}</div>
            }

            return (
              <WhiteLabelConfigFormComponent
                initialConfig={initialConfig}
                hasDraft={hasDraft}
                hasPrevious={hasPrevious}
                disabled={!canUpdateConfig}
                onSaveDraft={saveWhiteLabelDraft}
                onPublish={publishWhiteLabel}
                onDiscardDraft={discardWhiteLabelDraft}
                onRestore={restoreWhiteLabel}
                isSavingDraft={saveWhiteLabelDraftMutation.isPending}
                isPublishing={publishWhiteLabelMutation.isPending}
                isDiscarding={discardWhiteLabelDraftMutation.isPending}
                isRestoring={restoreWhiteLabelMutation.isPending}
              />
            )
          })()}
        </TabsContent>

        <TabsContent value="custom-domain">
          {(() => {
            // The form edits the published config directly (single-state model).
            // When the query is still loading (or returned nothing), fall back to
            // an empty config so the tab renders immediately; once data arrives
            // the query re-renders.
            const cdState = customDomainConfigData
            const initialConfig = normalizeCustomDomainConfig(cdState?.published)
            const cnameTarget = cdState?.cnameTarget ?? ''
            const status = cdState?.status ?? null

            if (isCustomDomainLoading && !cdState) {
              return <div>{m['settings.config_loading']()}</div>
            }

            return (
              <CustomDomainConfigFormComponent
                initialConfig={initialConfig}
                disabled={!canUpdateConfig}
                cnameTarget={cnameTarget}
                status={status}
                onRefreshStatus={() => {
                  void refetchCustomDomainStatus()
                }}
                isRefreshing={isCustomDomainRefreshing}
                onSave={updateCustomDomain}
                isSaving={updateCustomDomainMutation.isPending}
              />
            )
          })()}
        </TabsContent>
        {realmId === ADMIN_REALM_ID && (
          <TabsContent value="platform-signup">
            <PlatformSignupConfigFormComponent
              initialConfig={platformSignupConfig}
              onSave={savePlatformSignupConfig}
              isLoading={isLoading}
              disabled={!canUpdateConfig}
            />
          </TabsContent>
        )}
      </Tabs>
    </div>
  )
}
