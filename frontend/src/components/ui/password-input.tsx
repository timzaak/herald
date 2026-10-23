import * as React from 'react'
import { Eye, EyeOff } from 'lucide-react'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { m } from '@/paraglide/messages'

export interface PasswordInputProps extends Omit<
  React.InputHTMLAttributes<HTMLInputElement>,
  'ref' | 'type'
> {
  'data-testid'?: string
  ref?: React.Ref<HTMLInputElement>
}

export function PasswordInput({
  className,
  'data-testid': dataTestId,
  disabled,
  ref,
  ...props
}: PasswordInputProps) {
  const [isVisible, setIsVisible] = React.useState(false)

  return (
    <div className="relative w-full">
      <Input
        type={isVisible ? 'text' : 'password'}
        className={cn('pr-9', className)}
        data-testid={dataTestId}
        disabled={disabled}
        ref={ref}
        {...props}
      />
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        className="absolute right-1 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
        onClick={() => setIsVisible((prev) => !prev)}
        aria-label={isVisible ? m['common.hide_password']() : m['common.show_password']()}
        aria-pressed={isVisible}
        data-testid={dataTestId ? `${dataTestId}-visibility-toggle` : undefined}
      >
        {isVisible ? <EyeOff /> : <Eye />}
      </Button>
    </div>
  )
}

PasswordInput.displayName = 'PasswordInput'
