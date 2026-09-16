import { useQuery } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Clock, Key } from 'lucide-react'
import { formatDate } from '@/lib/totp-utils'
import { totpStatusQueryOptions } from '@/data/query-options'
import { m } from '@/paraglide/messages'

interface TotpStatusCardProps {
  onEnable: () => void
  onDisable: () => void
  onRegenerate: () => void
}

export function TotpStatusCard({ onEnable, onDisable, onRegenerate }: TotpStatusCardProps) {
  const { data, isLoading } = useQuery(totpStatusQueryOptions)

  if (isLoading) {
    return (
      <section data-testid="totp-status-card">
        <p className="py-6 text-sm text-muted-foreground">{m['profile.totp_loading']()}</p>
      </section>
    )
  }

  if (!data?.enabled) {
    return (
      <section data-testid="totp-status-card">
        <h2 className="text-base font-semibold">{m['profile.totp_title']()}</h2>
        <p className="mt-0.5 text-sm text-muted-foreground">{m['profile.totp_description']()}</p>
        <div className="mt-4 border-t border-border pt-4">
          <Button onClick={onEnable} data-testid="totp-enable-button">
            {m['profile.totp_enable_button']()}
          </Button>
        </div>
      </section>
    )
  }

  return (
    <section data-testid="totp-status-card-enabled">
      <div className="flex items-center justify-between">
        <h2 className="text-base font-semibold">{m['profile.totp_title']()}</h2>
        <Badge variant="default" data-testid="totp-status-badge">
          {m['profile.totp_enabled_badge']()}
        </Badge>
      </div>
      <p className="mt-0.5 text-sm text-muted-foreground">
        {m['profile.totp_protected_description']()}
      </p>
      <div className="mt-4 space-y-4 border-t border-border pt-4">
        <div className="flex items-center space-x-2 text-sm">
          <Clock className="h-4 w-4 text-muted-foreground" />
          <span className="text-muted-foreground">{m['profile.totp_enabled_at']()}</span>
          <span data-testid="totp-enabled-at">{formatDate(data.enabledAt)}</span>
        </div>

        {data.lastVerifiedAt && (
          <div className="flex items-center space-x-2 text-sm">
            <Clock className="h-4 w-4 text-muted-foreground" />
            <span className="text-muted-foreground">{m['profile.totp_last_verified']()}</span>
            <span data-testid="totp-last-verified-at">{formatDate(data.lastVerifiedAt)}</span>
          </div>
        )}

        <div className="flex items-center space-x-2 text-sm">
          <Key className="h-4 w-4 text-muted-foreground" />
          <span className="text-muted-foreground">
            {m['profile.totp_remaining_backup_codes']()}
          </span>
          <Badge variant="outline" data-testid="totp-remaining-backup-codes">
            {data.backupCodes.remaining}
          </Badge>
        </div>

        <div className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={onRegenerate} data-testid="totp-regenerate-button">
            {m['profile.totp_regenerate_codes_button']()}
          </Button>
          <Button variant="destructive" onClick={onDisable} data-testid="totp-disable-button">
            {m['profile.totp_disable_button']()}
          </Button>
        </div>
      </div>
    </section>
  )
}
