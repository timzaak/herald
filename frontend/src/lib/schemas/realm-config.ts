import { z } from 'zod'
import { m } from '@/paraglide/messages'

// TOTP 配置 Schema
// ✅ 前端 Schema 使用 camelCase（符合 JavaScript 约定）
export const totpConfigSchema = z.object({
  enabled: z.boolean(), // Realm 是否启用 TOTP
  forceEnabled: z.boolean(), // ✅ camelCase：是否强制所有用户启用 TOTP
})

// Passkey 配置 Schema
// ✅ camelCase：对齐后端 `GetRealmPasskeyConfigResponse` /
//    `UpdateRealmPasskeyConfigRequest`（均为 camelCase 线传输）。
//    实测字段：enabled（必填）+ P1 策略字段
//    userVerification / crossPlatformAuthenticator。
//    userVerification 在 wire 上为 string（后端枚举 "preferred"|"required"），
//    在此收窄为枚举并提供默认值，保证表单缺省时的可空安全。
export const passkeyConfigSchema = z.object({
  enabled: z.boolean(), // Realm 是否启用 Passkey
  forceEnabled: z.boolean().default(false), // 强制模式：引导未注册用户注册 Passkey（仅前端引导，不阻断登录）
  userVerification: z.enum(['preferred', 'required']).default('preferred'), // P1：用户验证要求
  crossPlatformAuthenticator: z.boolean().default(true), // P1：是否要求跨平台 authenticator
})

// Email-OTP 配置 Schema
// ✅ camelCase：对齐后端 `GetRealmEmailOtpConfigResponse` /
//    `UpdateRealmEmailOtpConfigRequest`（均为 camelCase 线传输）。
//    实测字段：enabled（是否启用邮箱验证码登录）/
//    autoRegister（未注册邮箱验证成功后是否自动注册并激活账户）。
export const emailOtpConfigSchema = z.object({
  enabled: z.boolean(), // Realm 是否启用邮箱验证码登录
  autoRegister: z.boolean(), // ✅ camelCase：未注册邮箱是否自动注册
})

// Registration 配置 Schema
export const registrationConfigSchema = z.object({
  enabled: z.boolean(), // 是否允许注册
  requireEmailVerification: z.boolean(), // ✅ camelCase：是否需要邮箱验证
})

// Platform self-service signup 配置 Schema
// ✅ admin realm 独有：控制公开自助开通 Realm 的总闸 (DEC-009/013)。
//    存于 realm_config(platform_signup, enabled) 单行，缺失 = false (fail-closed)。
export const platformSignupConfigSchema = z.object({
  enabled: z.boolean(), // 是否允许公开自助开通 Realm
})

// Turnstile 配置 Schema
export const turnstileConfigSchema = z.object({
  siteKey: z.string(),
  secretKey: z.string(),
})

// Email 配置 Schema
export const emailConfigSchema = z.object({
  provider: z.enum(['resend', 'smtp']),
  fromAddress: z.string().email().or(z.literal('')),
  resendApiKey: z.string().optional(),
  smtpHost: z.string().optional(),
  smtpPort: z.string().default('587'),
  smtpUsername: z.string().optional(),
  smtpPassword: z.string().optional(),
  smtpEncryption: z.enum(['starttls', 'ssl']).default('starttls'),
})

// White-label 背景配置 Schema
// ✅ camelCase：对齐后端 `WhiteLabelBackground` / `WhiteLabelBackgroundType`
//    （均为 camelCase 线传输）。`type` 对应 wire 上的 "image" | "gradient"。
export const whiteLabelBackgroundSchema = z.object({
  type: z.enum(['image', 'gradient']),
  value: z.string(),
})

// White-label 配置 Schema
// ✅ camelCase：对齐后端 `WhiteLabelConfig` / `UpdateWhiteLabelConfigRequest`
//    （均为 camelCase 线传输）。表单允许 `null` 或空字符串，保存时空字符串
//    normalize 为 `null`（见 realm-config-utils 的 toUpdateWhiteLabelConfigRequest）。
// 白标主色：空串 = 未设置（保存时 normalize 为 null），否则仅接受 CSS
// 十六进制色（#RGB / #RGBA / #RRGGBB / #RRGGBBAA），与后端
// validate_hex_color 一致（该值经 public-config 原文下发，须防注入）。
const HEX_COLOR_PATTERN = /^#(?:[0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/

export const whiteLabelConfigSchema = z.object({
  brandName: z.string().nullable(),
  logoUrl: z.string().nullable(),
  faviconUrl: z.string().nullable(),
  accentColor: z
    .string()
    .refine((value) => value === '' || HEX_COLOR_PATTERN.test(value), {
      error: () => m['settings.white_label.accent_color_invalid'](),
    })
    .nullable(),
  background: whiteLabelBackgroundSchema.nullable(),
  footerText: z.string().nullable(),
  loginTitle: z.string().nullable(),
  loginSubtitle: z.string().nullable(),
  registerTitle: z.string().nullable(),
  registerSubtitle: z.string().nullable(),
})

// LDAP 目录配置 Schema
// ✅ camelCase：对齐后端 `LdapDirectorySettings` 线传输（serde camelCase）。
// cross-field 规则镜像后端保存校验的最小集（不放宽，服务端 400 为权威兜底）：
// 凭据信道必须加密（ldap:// ⇒ starttls，ldaps:// ⇒ !starttls）、
// 过滤模板恰一个 {login} 占位符、括号配平。
const LDAP_URL_PATTERN = /^ldaps?:\/\/\S+$/

/**
 * `ldaps://` carries TLS in the scheme (StartTLS is redundant and rejected);
 * shared by the schema's cross-field encryption rule and the settings form's
 * StartTLS switch lock so the two can never disagree.
 */
export function isLdapsUrl(url: string): boolean {
  return url.startsWith('ldaps://')
}

export const ldapConfigSchema = z
  .object({
    enabled: z.boolean(), // Realm 是否启用企业账号登录（唯一判定源在后端 JSON）
    url: z
      .string()
      .min(1, { error: () => m['settings.ldap.url_required']() })
      .max(512, { error: () => m['settings.ldap.url_max_length']() })
      .refine((val) => LDAP_URL_PATTERN.test(val), {
        error: () => m['settings.ldap.url_invalid_scheme'](),
      }),
    starttls: z.boolean(), // ldap:// 时必须为 true；ldaps:// 时锁定为 false
    baseDn: z
      .string()
      .min(1, { error: () => m['settings.ldap.base_dn_required']() })
      .max(512, { error: () => m['settings.ldap.base_dn_max_length']() }),
    bindDn: z.string().max(512, { error: () => m['settings.ldap.bind_dn_max_length']() }), // 空串 = 匿名搜索
    bindPassword: z.string(), // 留空 = 保留已保存密码（掩码不可读）
    userFilter: z
      .string()
      .min(1, { error: () => m['settings.ldap.user_filter_required']() })
      .max(512, { error: () => m['settings.ldap.user_filter_max_length']() }),
    mailAttribute: z
      .string()
      .min(1, { error: () => m['settings.ldap.mail_attribute_required']() })
      .max(64, { error: () => m['settings.ldap.mail_attribute_max_length']() })
      .regex(/^[A-Za-z0-9-]+$/, { error: () => m['settings.ldap.mail_attribute_invalid']() }),
    // 可选显示名属性映射（如 displayName）；空串 = 不映射，保存时 normalize 为 null
    displayNameAttribute: z
      .string()
      .max(64, { error: () => m['settings.ldap.mail_attribute_max_length']() })
      .regex(/^[A-Za-z0-9-]*$/, { error: () => m['settings.ldap.mail_attribute_invalid']() })
      .default(''),
  })
  .superRefine((data, ctx) => {
    const isLdaps = isLdapsUrl(data.url)
    // 企业账号密码只允许在加密信道传输：明文 ldap://（未开 StartTLS）拒绝，
    // ldaps:// 已由 scheme 提供 TLS，StartTLS 必须关闭。
    if (isLdaps === data.starttls) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: m['settings.ldap.error_encryption_required'](),
        path: ['starttls'],
      })
    }
    const placeholderCount = data.userFilter.split('{login}').length - 1
    if (placeholderCount !== 1) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: m['settings.ldap.error_login_placeholder']({ token: '{login}' }),
        path: ['userFilter'],
      })
    }
    const open = data.userFilter.split('(').length - 1
    const close = data.userFilter.split(')').length - 1
    if (open !== close) {
      ctx.addIssue({
        code: z.ZodIssueCode.custom,
        message: m['settings.ldap.error_parens_unbalanced'](),
        path: ['userFilter'],
      })
    }
  })

// Custom-domain 配置 Schema
// ✅ camelCase：对齐后端 `CustomDomainConfig` / `UpdateCustomDomainConfigRequest`
//    （均为 camelCase 线传输）。`hostname` 为精确域名（如 `login.acme.com`），
//    表单允许 `null` 或空字符串，保存时空字符串 normalize 为 `null`
//    （见 realm-config-utils 的 toUpdateCustomDomainConfigRequest）。
//    刻意使用 z.string()（而非 .email()），格式校验留给后端，mapper 仅 trim。
export const customDomainConfigSchema = z.object({
  hostname: z.string().nullable(),
})

// 类型导出
export type TOTPConfigForm = z.infer<typeof totpConfigSchema>
export type PasskeyConfigForm = z.infer<typeof passkeyConfigSchema>
export type EmailOtpConfigForm = z.infer<typeof emailOtpConfigSchema>
export type RegistrationConfigForm = z.infer<typeof registrationConfigSchema>
export type PlatformSignupConfigForm = z.infer<typeof platformSignupConfigSchema>
export type TurnstileConfigForm = z.infer<typeof turnstileConfigSchema>
export type EmailConfigForm = z.infer<typeof emailConfigSchema>
export type WhiteLabelBackgroundForm = z.infer<typeof whiteLabelBackgroundSchema>
export type WhiteLabelConfigForm = z.infer<typeof whiteLabelConfigSchema>
export type CustomDomainConfigForm = z.infer<typeof customDomainConfigSchema>
export type LdapConfigForm = z.infer<typeof ldapConfigSchema>
/**
 * LDAP tab 状态 = 可编辑表单值 + 不可编辑的 `hasBindPassword`（由
 * `bind_password` 配置行的存在性推断——值恒掩码为 null，行存在性是
 * “已保存过服务账号密码”的唯一可用信号）。
 */
export type LdapConfigState = LdapConfigForm & { hasBindPassword: boolean }
