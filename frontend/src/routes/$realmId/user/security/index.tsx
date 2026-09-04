import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { ChangePasswordForm } from '@/components/profile/change-password-form'
import { TotpStatusCard } from '@/components/profile/totp/totp-status-card'
import { PasskeyList } from '@/components/profile/passkey/passkey-list'
import { PasskeyRegisterForm } from '@/components/profile/passkey/passkey-register-form'
import { DeleteAccountDialog } from '@/components/security/DeleteAccountDialog'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { TotpDisableForm } from '@/components/profile/totp/totp-disable-form'
import { TotpRegenerateForm } from '@/components/profile/totp/totp-regenerate-form'
import { PageHeader } from '@/components/shared'
import { Button } from '@/components/ui/button'
import { m } from '@/paraglide/messages'
import { realmPath, useResolvedRealmContext } from '@/lib/realm-routing'
import {
  passkeyListQueryOptions,
  passkeyStatusQueryOptions,
  userFeatureAvailabilityQueryOptions,
} from '@/data/query-options'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'

export const Route = createFileRoute('/$realmId/user/security/')({
  component: ProfileSecurity,
})

type TotpDialogType = 'disable' | 'regenerate' | null

export function ProfileSecurity() {
  const navigate = useNavigate()
  const realmContext = useResolvedRealmContext()
  const realmId = realmContext.realmId
  const [totpDialog, setTotpDialog] = useState<TotpDialogType>(null)
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  // Toggles the inline passkey registration form within the passkey tab.
  const [passkeyRegistering, setPasskeyRegistering] = useState(false)
  const [activeTab, setActiveTab] = useState('password')

  // Whether Passkey/TOTP are enabled for this realm. Each feature's tab is
  // hidden when the realm has it disabled, so users never see setup/management
  // affordances (or a misleading "X is not enabled for this realm" error after
  // entering a password) for a feature the realm turned off.
  const { data: userFeatureAvailability } = useQuery(userFeatureAvailabilityQueryOptions)
  const passkeyEnabled = userFeatureAvailability?.user.passkeyEnabled ?? false
  const totpEnabled = userFeatureAvailability?.user.totpEnabled ?? false

  // Force-mode guidance (passkey PRD §4.1): when the realm forces Passkey and
  // this user has none registered, surface a prompt pointing at the passkey
  // tab. Guidance only — nothing is blocked. The status query is gated on
  // passkeyEnabled because forceEnabled is only read when that is true, so
  // realms with Passkey off skip the extra status request.
  const { data: passkeyStatus } = useQuery({
    ...passkeyStatusQueryOptions(realmId),
    enabled: passkeyEnabled,
  })
  const { data: passkeyCredentials } = useQuery({
    ...passkeyListQueryOptions,
    enabled: passkeyEnabled,
    select: (data) => data.credentials,
  })
  const showPasskeyGuidance =
    passkeyEnabled &&
    (passkeyStatus?.forceEnabled ?? false) &&
    !passkeyRegistering &&
    (passkeyCredentials?.length ?? 0) === 0

  const handleDialogClose = () => setTotpDialog(null)

  return (
    <div className="space-y-6">
      <PageHeader title={m['profile.security_page_title']()} headingTestId="security-page-title" />

      {showPasskeyGuidance && (
        <Alert data-testid="passkey-force-guidance">
          <AlertTitle>{m['profile.passkey_force_banner_title']()}</AlertTitle>
          <AlertDescription className="flex items-center justify-between gap-4">
            <span>{m['profile.passkey_force_banner_description']()}</span>
            <Button
              size="sm"
              onClick={() => {
                setActiveTab('passkey')
                setPasskeyRegistering(true)
              }}
              data-testid="passkey-force-guidance-action"
            >
              {m['profile.passkey_force_banner_action']()}
            </Button>
          </AlertDescription>
        </Alert>
      )}

      <Tabs value={activeTab} onValueChange={setActiveTab}>
        <TabsList>
          <TabsTrigger value="password" data-testid="password-tab">
            {m['profile.password_tab']()}
          </TabsTrigger>
          {totpEnabled && (
            <TabsTrigger value="totp" data-testid="totp-tab">
              {m['profile.totp_tab']()}
            </TabsTrigger>
          )}
          {passkeyEnabled && (
            <TabsTrigger value="passkey" data-testid="passkey-tab">
              {m['profile.passkey_tab']()}
            </TabsTrigger>
          )}
        </TabsList>
        <TabsContent value="password" data-testid="password-section-title">
          <ChangePasswordForm />
        </TabsContent>
        {totpEnabled && (
          <TabsContent value="totp" data-testid="totp-section-title">
            <TotpStatusCard
              onEnable={() =>
                navigate({ to: realmPath(realmContext, '/user/security/totp-setup') })
              }
              onDisable={() => setTotpDialog('disable')}
              onRegenerate={() => setTotpDialog('regenerate')}
            />
          </TabsContent>
        )}
        {passkeyEnabled && (
          <TabsContent value="passkey" data-testid="passkey-section-title">
            {passkeyRegistering ? (
              <PasskeyRegisterForm
                onSuccess={() => setPasskeyRegistering(false)}
                onCancel={() => setPasskeyRegistering(false)}
              />
            ) : (
              <div className="space-y-4">
                <div>
                  <h2 className="text-base font-semibold">{m['profile.passkey_title']()}</h2>
                  <p className="text-sm text-muted-foreground">
                    {m['profile.passkey_description']()}
                  </p>
                </div>
                <PasskeyList onAdd={() => setPasskeyRegistering(true)} />
              </div>
            )}
          </TabsContent>
        )}
      </Tabs>

      <Dialog open={totpDialog === 'disable'} onOpenChange={(open) => !open && setTotpDialog(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{m['profile.totp_disable_title']()}</DialogTitle>
          </DialogHeader>
          <TotpDisableForm
            onSuccess={handleDialogClose}
            onCancel={handleDialogClose}
            isForceTotpEnabled={false}
          />
        </DialogContent>
      </Dialog>

      <Dialog
        open={totpDialog === 'regenerate'}
        onOpenChange={(open) => !open && setTotpDialog(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{m['profile.totp_regenerate_title']()}</DialogTitle>
          </DialogHeader>
          <TotpRegenerateForm onSuccess={handleDialogClose} onCancel={handleDialogClose} />
        </DialogContent>
      </Dialog>

      <section data-testid="danger-operations-section">
        <h2 className="text-base font-semibold">{m['security.delete_account.section_title']()}</h2>
        <p className="mt-0.5 text-sm text-muted-foreground">
          {m['security.delete_account.section_description']()}
        </p>
        <div className="mt-4 border-t border-border pt-4">
          <Button
            variant="destructive"
            onClick={() => setDeleteDialogOpen(true)}
            data-testid="delete-account-open-button"
          >
            {m['security.delete_account.button']()}
          </Button>
        </div>
      </section>

      <DeleteAccountDialog
        realmId={realmId}
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
      />
    </div>
  )
}
