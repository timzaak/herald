import { Badge } from '@/components/ui/badge'
import { CalendarClock, Wallet } from 'lucide-react'
import { formatPoolExpiry, type DerivedBucketCard } from './user-points-view'
import { m } from '@/paraglide/messages'

interface PointsBalanceCardProps {
  card: DerivedBucketCard
  loading?: boolean
}

// Pool-only types (FE-D04). Subscription and free-periodic balances are now
// surfaced via `PointsUsageDashboard` window rows; this card shows only the
// pool model (topup/registration/granted) that PRD §2.2 keeps zero-regression.
const BALANCES_BY_TYPE_KEYS = ['topup', 'registration', 'granted'] as const

export function PointsBalanceCard({ card, loading }: PointsBalanceCardProps) {
  if (loading) {
    return (
      <section data-testid="points-balance-card">
        <h2 className="text-base font-semibold">{m['points.balance_card_title']()}</h2>
        <div className="mt-4 border-t border-border pt-4">
          <div className="animate-pulse space-y-4">
            <div className="h-12 bg-muted rounded" />
          </div>
        </div>
      </section>
    )
  }

  const bucketTestId = card.bucketId
    ? `points-balance-card-${card.bucketId}`
    : 'points-balance-card'

  return (
    <section data-testid={bucketTestId}>
      <div className="flex items-center justify-between">
        <h2 className="text-base font-semibold">
          {card.name ?? m['points.bucket_card_unnamed']()}
        </h2>
        {card.enabled === false && (
          <Badge
            variant="secondary"
            data-testid={`points-balance-card-disabled-${card.bucketId ?? ''}`}
          >
            {m['points.bucket_card_disabled']()}
          </Badge>
        )}
      </div>
      <div className="mt-4 border-t border-border pt-4">
        <div className="flex items-center gap-2 text-muted-foreground text-sm mb-2">
          <Wallet className="h-4 w-4" />
          <span>{m['points.balance_pool']()}</span>
        </div>
        <div
          className="text-3xl font-bold"
          data-testid={`points-balance-total-${card.bucketId ?? ''}`}
        >
          {(card.spendableFromPool ?? 0).toLocaleString()}
        </div>
        <div
          className="mt-2 flex items-center gap-1.5 text-xs text-muted-foreground"
          data-testid={`points-balance-expiry-${card.bucketId ?? ''}`}
        >
          <CalendarClock className="h-3.5 w-3.5" />
          {formatPoolExpiry(card.expiresAt)}
        </div>
        <div className="mt-4 flex flex-wrap gap-2">
          {BALANCES_BY_TYPE_KEYS.map((typeKey) => {
            const value = card.balancesByType[typeKey]
            if (!value) {
              return null
            }
            return (
              <Badge
                key={typeKey}
                variant="outline"
                data-testid={`points-balance-type-${card.bucketId ?? ''}-${typeKey}`}
              >
                {m[`points.balance_type_${typeKey}`]({ count: value.toLocaleString() })}
              </Badge>
            )
          })}
        </div>
      </div>
    </section>
  )
}
