import type { ReactNode } from 'react'
import { useCallback } from 'react'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { getFieldErrorMessage } from '@/lib/form-utils'

interface TextFieldProps {
  form: any // eslint-disable-line @typescript-eslint/no-explicit-any -- shared field wrapper accepts the repository's typed form instance
  name: string
  label: ReactNode
  inputId?: string
  dataTestId?: string
  type?: React.InputHTMLAttributes<HTMLInputElement>['type']
  placeholder?: string
  disabled?: boolean
  helpText?: ReactNode
  required?: boolean
  /** Testid for the error <p>; defaults to none (query via role="alert"). */
  errorTestId?: string
  transformValue?: (value: string) => unknown
  onSubmit?: () => void
}

export function TextField({
  form,
  name,
  label,
  inputId,
  dataTestId,
  type = 'text',
  placeholder,
  disabled = false,
  helpText,
  required = false,
  errorTestId,
  transformValue,
  onSubmit,
}: TextFieldProps) {
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Enter' && onSubmit) {
        e.preventDefault()
        onSubmit()
      }
    },
    [onSubmit]
  )

  return (
    <form.Field
      name={name}
      children={(
        field: any // eslint-disable-line @typescript-eslint/no-explicit-any -- generic field component
      ) => (
        <div className="space-y-2">
          <Label htmlFor={inputId ?? field.name}>
            {label}
            {required ? ' *' : null}
          </Label>
          <Input
            id={inputId ?? field.name}
            data-testid={dataTestId}
            type={type}
            value={field.state.value ?? ''}
            onBlur={field.handleBlur}
            onChange={(e) =>
              field.handleChange(transformValue ? transformValue(e.target.value) : e.target.value)
            }
            onKeyDown={handleKeyDown}
            placeholder={placeholder}
            disabled={disabled}
          />
          {helpText ? <p className="text-xs text-muted-foreground">{helpText}</p> : null}
          {(field.state.meta.isTouched || form.state.isSubmitted) &&
            field.state.meta.errors.length > 0 && (
              <p className="text-sm text-destructive" role="alert" data-testid={errorTestId}>
                {getFieldErrorMessage(field.state.meta)}
              </p>
            )}
        </div>
      )}
    />
  )
}
