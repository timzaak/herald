import { AgreementLinks } from '@/components/legal/AgreementLinks'
import { Button } from '@/components/ui/button'
import type { LegalAgreementSummary } from '@/lib/api-generated'
import { formatDate } from '@/lib/date-utils'
import { m } from '@/paraglide/messages'

interface ConsentAgreementsPanelProps {
  realmId: string
  agreements: LegalAgreementSummary[]
  isPending: boolean
  /** Rendered message strings — callers keep their per-feature i18n keys. */
  title: string
  description: string
  agreeLabel: string
  declineLabel: string
  onAgree: () => void
  onDecline: () => void
  /** Testid prefix, e.g. `device` renders `device-consent-panel`. */
  testIdPrefix: string
}

/**
 * The login consent gate panel: one card per agreement (links + version and
 * effective date) with an agree and a decline button. Shared by every
 * entrance the gate can block; labels and testids stay caller-owned.
 */
export function ConsentAgreementsPanel({
  realmId,
  agreements,
  isPending,
  title,
  description,
  agreeLabel,
  declineLabel,
  onAgree,
  onDecline,
  testIdPrefix,
}: ConsentAgreementsPanelProps) {
  return (
    <div className="space-y-4" data-testid={`${testIdPrefix}-consent-panel`}>
      <h3 className="font-semibold">{title}</h3>
      <p className="text-sm text-muted-foreground">{description}</p>
      {agreements.map((agreement) => (
        <div
          key={agreement.version_id}
          className="rounded border p-3"
          data-testid={`${testIdPrefix}-agreement-${agreement.agreement_type}`}
        >
          <div className="font-medium">
            <AgreementLinks
              realmId={realmId}
              agreements={[agreement]}
              agreementType={agreement.agreement_type as 'terms_of_service' | 'privacy_policy'}
            />
          </div>
          <div className="text-sm text-muted-foreground">
            {m['legal.version_label']()}: {agreement.version_no} •{' '}
            {m['legal.effective_date_label']()}: {formatDate(agreement.effective_at)}
          </div>
        </div>
      ))}
      <Button
        type="button"
        className="w-full"
        disabled={isPending}
        data-testid={`${testIdPrefix}-agree-and-continue-button`}
        onClick={onAgree}
      >
        {isPending ? m['common.loading']() : agreeLabel}
      </Button>
      <Button
        type="button"
        variant="outline"
        className="w-full"
        data-testid={`${testIdPrefix}-consent-cancel-button`}
        onClick={onDecline}
      >
        {declineLabel}
      </Button>
    </div>
  )
}
