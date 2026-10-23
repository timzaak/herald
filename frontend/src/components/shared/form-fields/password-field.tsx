import React, { type ReactNode } from 'react'
import { PasswordInput } from '@/components/ui/password-input'
import { Label } from '@/components/ui/label'
import { getFieldErrorMessage } from '@/lib/form-utils'

interface PasswordFieldProps {
  form: any // eslint-disable-line @typescript-eslint/no-explicit-any -- shared field wrapper accepts the repository's typed form instance
  name: string
  label: ReactNode
  inputId?: string
  dataTestId?: string
  placeholder?: string
  disabled?: boolean
  helpText?: ReactNode
  required?: boolean
}

export function PasswordField({
  form,
  name,
  label,
  inputId,
  dataTestId,
  placeholder,
  disabled = false,
  helpText,
  required = false,
}: PasswordFieldProps) {
  return (
    <form.Field
      name={name}
      children={(
        field: any // eslint-disable-line @typescript-eslint/no-explicit-any -- generic field component
      ) => {
        const inputProps = {
          id: inputId ?? field.name,
          'data-testid': dataTestId,
          value: field.state.value ?? '',
          onBlur: field.handleBlur,
          onChange: (e: React.ChangeEvent<HTMLInputElement>) => field.handleChange(e.target.value),
          placeholder,
          disabled,
        }
        return (
          <div className="space-y-2">
            <Label htmlFor={inputId ?? field.name}>
              {label}
              {required ? ' *' : null}
            </Label>
            <PasswordInput {...inputProps} />
            {helpText ? <p className="text-xs text-muted-foreground">{helpText}</p> : null}
            {(field.state.meta.isTouched || form.state.isSubmitted) &&
              field.state.meta.errors.length > 0 && (
                <p className="text-sm text-destructive" role="alert">
                  {getFieldErrorMessage(field.state.meta)}
                </p>
              )}
          </div>
        )
      }}
    />
  )
}
