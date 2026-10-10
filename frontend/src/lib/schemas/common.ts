import { z } from 'zod'
import { m } from '@/paraglide/messages'

export const emailSchema = z
  .string()
  .min(1, { error: () => m['auth.email_required']() })
  .email({ error: () => m['auth.email_invalid']() })
export const passwordSchema = z.string().min(8, { error: () => m['auth.password_min_length']() })
export const usernameSchema = z.string().min(3, { error: () => m['auth.username_min_length']() })

export const loginSchema = z.object({
  username: usernameSchema.or(emailSchema),
  password: passwordSchema,
})

// Directory credentials are owned by the enterprise directory, not Herald:
// only non-empty bounds apply (no local 8..36 password policy, wider username
// space than local usernames).
export const ldapLoginSchema = z.object({
  username: z
    .string()
    .min(1, { error: () => m['auth.ldap.username_required']() })
    .max(254, { error: () => m['auth.ldap.username_max_length']() }),
  password: z
    .string()
    .min(1, { error: () => m['auth.ldap.password_required']() })
    .max(512, { error: () => m['auth.ldap.password_max_length']() }),
})

export const createUserSchema = z.object({
  email: emailSchema,
  password: passwordSchema,
  nickname: z.string().min(2).max(50).optional(),
  status: z.number().int().min(0).max(2).optional(),
  roleIds: z.array(z.string()).min(1, { error: () => m['auth.role_required']() }),
})

export const updateUserSchema = z.object({
  email: emailSchema,
  nickname: z.string().min(2).max(50).optional(),
  status: z.number().int().min(0).max(2).optional(),
})

export const changePasswordSchema = z
  .object({
    oldPass: z.string().min(1, { error: () => m['auth.current_password_required']() }),
    newPass: passwordSchema,
    confirmPass: z.string().min(8, { error: () => m['auth.password_min_length']() }),
  })
  .superRefine((data, ctx) => {
    if (data.newPass !== data.confirmPass) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: m['profile.passwords_dont_match'](),
        path: ['confirmPass'],
      })
    }
  })

export type LoginFormData = z.infer<typeof loginSchema>
export type LdapLoginFormData = z.infer<typeof ldapLoginSchema>
export type CreateUserData = z.infer<typeof createUserSchema>
export type CreateUserFormData = z.infer<typeof createUserSchema>
export type UpdateUserFormData = z.infer<typeof updateUserSchema>
export type ChangePasswordFormData = z.infer<typeof changePasswordSchema>

// Permission schemas
export const permissionNameSchema = z
  .string()
  .min(1, { error: () => m['permissions.name_required']() })
  .max(100, { error: () => m['permissions.name_max_length']() })
  .regex(/^[a-z0-9_]+\.[a-z0-9_]+$/, { error: () => m['permissions.name_format']() })
  .refine((val) => !val.includes(' '), { error: () => m['permissions.name_no_spaces']() })
  .refine((val) => !val.includes('..'), { error: () => m['permissions.name_no_double_dots']() })

export const permissionDescriptionSchema = z
  .string()
  .max(500, { error: () => m['permissions.description_max_length']() })
  .optional()

export const createPermissionSchema = z.object({
  name: permissionNameSchema,
  description: permissionDescriptionSchema,
})

export const updatePermissionSchema = z.object({
  name: permissionNameSchema,
  description: permissionDescriptionSchema,
})

export type CreatePermissionFormData = z.infer<typeof createPermissionSchema>
export type UpdatePermissionFormData = z.infer<typeof updatePermissionSchema>

// Role schemas
export const roleNameSchema = z
  .string()
  .min(1, { error: () => m['roles.name_required']() })
  .max(100, { error: () => m['roles.name_max_length']() })
  .regex(/^[a-zA-Z0-9_-]+$/, { error: () => m['roles.name_format']() })
  .refine((val) => !val.includes(' '), { error: () => m['roles.name_no_spaces']() })
  .refine((val) => !val.includes('--'), { error: () => m['roles.name_no_consecutive_hyphens']() })

export const roleDescriptionSchema = z
  .string()
  .max(500, { error: () => m['roles.description_max_length']() })
  .optional()

export const createRoleSchema = z.object({
  name: roleNameSchema,
  description: roleDescriptionSchema,
})

export const updateRoleSchema = z.object({
  name: roleNameSchema,
  description: roleDescriptionSchema,
})

export type CreateRoleFormData = z.infer<typeof createRoleSchema>
export type UpdateRoleFormData = z.infer<typeof updateRoleSchema>
