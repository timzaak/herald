import type { ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import { Plus } from 'lucide-react'
import { InfoTooltip } from './info-tooltip'

export interface PageHeaderProps {
  title: string
  /** Optional explanation shown in a tooltip beside the title (e.g. term definitions). */
  titleTooltip?: string
  subtitle?: string
  headingTestId?: string
  action?: {
    label: string
    onClick: () => void
    testId: string
    show?: boolean
    icon?: ReactNode
    disabled?: boolean
    disabledReason?: ReactNode
  }
  className?: string
}

export function PageHeader({
  title,
  titleTooltip,
  subtitle,
  headingTestId,
  action,
  className = '',
}: PageHeaderProps) {
  return (
    <div className={`flex flex-wrap items-center justify-between gap-2 ${className}`}>
      <div>
        <div className="flex items-center gap-1.5">
          <h1
            className="text-xl font-semibold tracking-tight"
            data-testid={headingTestId ?? `${title.toLowerCase()}-heading`}
          >
            {title}
          </h1>
          {titleTooltip && (
            <InfoTooltip
              label={titleTooltip}
              testId="page-header-title-tooltip"
              contentTestId="page-header-title-tooltip-content"
            >
              {titleTooltip}
            </InfoTooltip>
          )}
        </div>
        {subtitle && <p className="mt-0.5 text-sm text-muted-foreground">{subtitle}</p>}
      </div>
      {action && action.show !== false && (
        <div className="flex flex-col items-end gap-1">
          <Button onClick={action.onClick} data-testid={action.testId} disabled={action.disabled}>
            {action.icon ?? <Plus className="mr-2 h-4 w-4" />}
            {action.label}
          </Button>
          {action.disabledReason && (
            <span className="text-xs text-muted-foreground" data-testid={`${action.testId}-reason`}>
              {action.disabledReason}
            </span>
          )}
        </div>
      )}
    </div>
  )
}
