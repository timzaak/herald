import { m } from '@/paraglide/messages'

/**
 * Maps a serialized audit action string (e.g. `"passkey_config.update"`) to its
 * localized friendly name. Used by the audit table, detail sheet, and filter bar
 * so all three surfaces render actions consistently.
 *
 * Keep this list in sync with the `AuditAction` enum variants in
 * `backend/domain/src/audit/event_types.rs` (the `#[serde(rename = "...")]`
 * values).
 *
 * `formatAuditAction` falls back to the raw string for any unmapped action, so
 * a future backend action will still display (raw) until a label is added here.
 */
export const AUDIT_ACTION_LABELS: Record<string, () => string> = {
  'user.create': () => m['audit.action_user_create'](),
  'user.update': () => m['audit.action_user_update'](),
  'user.delete': () => m['audit.action_user_delete'](),
  'role.create': () => m['audit.action_role_create'](),
  'role.update': () => m['audit.action_role_update'](),
  'role.delete': () => m['audit.action_role_delete'](),
  'permission.create': () => m['audit.action_permission_create'](),
  'permission.delete': () => m['audit.action_permission_delete'](),
  'role.assign': () => m['audit.action_role_assign'](),
  'role.unassign': () => m['audit.action_role_unassign'](),
  'permission.grant': () => m['audit.action_permission_grant'](),
  'permission.revoke': () => m['audit.action_permission_revoke'](),
  'rbac.permission_denied': () => m['audit.action_rbac_permission_denied'](),
  'realm.create': () => m['audit.action_realm_create'](),
  'realm.rbac_init': () => m['audit.action_realm_rbac_init'](),
  'auth.login': () => m['audit.action_auth_login'](),
  'auth.logout': () => m['audit.action_auth_logout'](),
  'auth.login_failed': () => m['audit.action_auth_login_failed'](),
  'product.create': () => m['audit.action_product_create'](),
  'product.update': () => m['audit.action_product_update'](),
  'product.delete': () => m['audit.action_product_delete'](),
  'oauth_config.create': () => m['audit.action_oauth_config_create'](),
  'oauth_config.update': () => m['audit.action_oauth_config_update'](),
  'oauth_config.delete': () => m['audit.action_oauth_config_delete'](),
  'passkey_config.update': () => m['audit.action_passkey_config_update'](),
  'passkey.register': () => m['audit.action_passkey_register'](),
  'passkey.delete': () => m['audit.action_passkey_delete'](),
  'agreement.consent': () => m['audit.action_agreement_consent'](),
  'agreement.published': () => m['audit.action_agreement_published'](),
  'agreement.reverted': () => m['audit.action_agreement_reverted'](),
}

/** Return the localized action name, falling back to the raw string if unmapped. */
export function formatAuditAction(action: string): string {
  return AUDIT_ACTION_LABELS[action]?.() ?? action
}
