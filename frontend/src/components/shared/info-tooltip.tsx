import type { ReactNode } from 'react'
import { Info } from 'lucide-react'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'

interface InfoTooltipProps {
  /** Full hint text; doubles as the trigger's aria-label so the tooltip content is reachable without hover. */
  label: string
  testId?: string
  contentTestId?: string
  contentClassName?: string
  iconClassName?: string
  children: ReactNode
}

/**
 * Shared "ⓘ" hint trigger: icon button, not a bare span — keyboard focus +
 * screen-reader access come from the button, with `aria-label` carrying the
 * full text so the tooltip is not the only way to reach it.
 */
export function InfoTooltip({
  label,
  testId,
  contentTestId,
  contentClassName,
  iconClassName = 'h-4 w-4',
  children,
}: InfoTooltipProps) {
  return (
    <TooltipProvider delayDuration={200}>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            aria-label={label}
            data-testid={testId}
            className="text-muted-foreground hover:text-foreground"
          >
            <Info className={iconClassName} aria-hidden />
          </button>
        </TooltipTrigger>
        <TooltipContent data-testid={contentTestId} className={contentClassName}>
          {children}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}
