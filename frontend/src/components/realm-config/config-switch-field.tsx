import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { getFieldErrorMessage } from '@/lib/form-utils'

interface ConfigSwitchFieldProps {
  field: {
    state: {
      value: boolean
      meta: {
        errors: unknown[]
        isTouched?: boolean
      }
    }
    handleChange: (value: boolean) => void
  }
  form?: {
    state: {
      isSubmitted: boolean
    }
  }
  id: string
  label: string
  description: string
  disabled?: boolean
  errorTestId?: string
  checked?: boolean
  /**
   * Reason the switch cannot be enabled, shown as a tooltip on the switch
   * (e.g. a missing prerequisite such as email configuration).
   */
  switchTooltip?: string
}

/**
 * Reusable switch field component for config forms.
 * Provides consistent layout and styling for boolean configuration options.
 */
export function ConfigSwitchField({
  field,
  form,
  id,
  label,
  description,
  disabled,
  errorTestId,
  checked,
  switchTooltip,
}: ConfigSwitchFieldProps) {
  const switchElement = (
    <Switch
      id={id}
      checked={checked ?? field.state.value}
      onCheckedChange={field.handleChange}
      disabled={disabled}
      data-testid={`${id}-switch`}
    />
  )

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <Label htmlFor={id}>{label}</Label>
          <p className="text-sm text-muted-foreground">{description}</p>
        </div>
        {switchTooltip ? (
          <TooltipProvider delayDuration={200}>
            <Tooltip>
              <TooltipTrigger asChild>
                {/* span wrapper so the disabled Switch still receives hover/focus events for the tooltip */}
                <span
                  tabIndex={0}
                  data-testid={`${id}-switch-tooltip-trigger`}
                  className="inline-flex"
                >
                  {switchElement}
                </span>
              </TooltipTrigger>
              <TooltipContent data-testid={`${id}-switch-tooltip`}>{switchTooltip}</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        ) : (
          switchElement
        )}
      </div>
      {/* Form validation error display */}
      {(field.state.meta.isTouched || form?.state.isSubmitted) &&
        field.state.meta.errors.length > 0 && (
          <p className="text-sm text-destructive" data-testid={errorTestId}>
            {getFieldErrorMessage(field.state.meta)}
          </p>
        )}
    </div>
  )
}
