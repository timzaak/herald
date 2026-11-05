import OTPInput from 'react-otp-input'

interface OtpCodeInputProps {
  value: string
  onChange: (value: string) => void
  numInputs: number
  /** Testid base: container `<base>-input`, digits `<base>-digit-<i>`. */
  testIdBase: string
  /** Localized label text; each digit's aria-label is `<label> <n>`. */
  ariaLabel: string
}

/**
 * 6-digit one-time-code input shared by the OTP forms (login, phone
 * bind/rebind dialogs): a single row of digit inputs with uniform styling and
 * per-digit testids/aria-labels.
 */
export function OtpCodeInput({
  value,
  onChange,
  numInputs,
  testIdBase,
  ariaLabel,
}: OtpCodeInputProps) {
  return (
    <div className="mt-2 flex" data-testid={`${testIdBase}-input`}>
      <OTPInput
        value={value}
        onChange={onChange}
        numInputs={numInputs}
        inputType="number"
        shouldAutoFocus
        renderInput={(props, index) => (
          <input
            {...props}
            key={index}
            placeholder="•"
            className="otp-digit-input mx-1 h-12 w-10 rounded-md border border-input bg-background text-center text-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            data-testid={`${testIdBase}-digit-${index}`}
            aria-label={`${ariaLabel} ${index + 1}`}
          />
        )}
      />
    </div>
  )
}
