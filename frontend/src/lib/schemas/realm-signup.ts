import { z } from 'zod'
import { m } from '@/paraglide/messages'
import { RESERVED_WORDS } from './realm'

// RealmName: 3-50 chars (mirrors backend SignupRequest.realmName / CreateRealmRequest.name).
const realmNameSchema = z
  .string()
  .min(3, { error: () => m['auth.signup.realm_name_min_length']() })
  .max(50, { error: () => m['auth.signup.realm_name_max_length']() })

// RealmSlug: optional custom realm id. 3-36 chars, alnum/hyphen/underscore,
// rejects reserved words (mirrors backend validate_realm_id). Empty/undefined
// lets the backend generate a UUID v7. The empty string is allowed (field left
// blank); the form maps empty → undefined when building the API body, matching
// the register-form convention.
const realmSlugSchema = z
  .string()
  .min(3, { error: () => m['auth.signup.realm_slug_format']() })
  .max(36, { error: () => m['auth.signup.realm_slug_format']() })
  .regex(/^[a-zA-Z0-9_-]+$/, { error: () => m['auth.signup.realm_slug_format']() })
  .refine((val) => !RESERVED_WORDS.some((word) => word === val.toLowerCase()), {
    error: () => m['auth.signup.realm_slug_reserved'](),
  })

export const signupEmailSchema = z.string().email({ error: () => m['auth.email_invalid']() })

// Self-service realm signup form schema. `turnstileToken` is required only when
// the admin realm's admin-web-console Client App has Turnstile enabled (DEC-008);
// the form passes it through as optional and the widget gates its own rendering.
// `emailVerificationCode` is required only when the admin realm's registration
// config turns on signup email verification (signup status
// `emailVerificationRequired`), mirroring the backend's conditional check.
export function createSignupSchema(emailVerificationRequired: boolean) {
  const baseSchema = z.object({
    realmName: realmNameSchema,
    realmSlug: realmSlugSchema.or(z.literal('')),
    email: signupEmailSchema,
    password: z.string().min(8, { error: () => m['auth.password_min_length']() }),
    turnstileToken: z.string().optional(),
    emailVerificationCode: z.string().optional(),
  })
  if (!emailVerificationRequired) return baseSchema
  return baseSchema.superRefine((data, ctx) => {
    if (!/^\d{6}$/.test(data.emailVerificationCode ?? '')) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: m['auth.signup.email_code_invalid'](),
        path: ['emailVerificationCode'],
      })
    }
  })
}

export type SignupFormValues = z.infer<ReturnType<typeof createSignupSchema>>
