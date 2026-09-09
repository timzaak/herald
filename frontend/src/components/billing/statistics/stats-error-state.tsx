import { Button } from '@/components/ui/button'
import { getErrorMessage } from '@/lib/error-utils'
import { m } from '@/paraglide/messages'

interface StatsErrorStateProps {
  error: unknown
  onRetry: () => void
  testId: string
}

export function StatsErrorState({ error, onRetry, testId }: StatsErrorStateProps) {
  return (
    <div
      className="rounded-lg border border-destructive/50 bg-destructive/10 p-6 text-center"
      data-testid={testId}
    >
      <p className="text-destructive mb-3">
        {getErrorMessage(error) || m['billing.statistics_failed_to_load']()}
      </p>
      <Button onClick={onRetry} data-testid={`${testId}-retry-button`}>
        {m['common.retry']()}
      </Button>
    </div>
  )
}
