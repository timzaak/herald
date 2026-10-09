import { z } from 'zod'
import { m } from '@/paraglide/messages'

// Backend contract: the register endpoint validates nickname as
// length(min = 1, max = 50) — keep both forms on the same limit.
export const NICKNAME_MAX_LENGTH = 50

// Lazy message getters keep the locale live: the module is evaluated once at
// import time, so an eager m[...]() would freeze the first-loaded language.
export const nicknameFieldSchema = z
  .string()
  .min(1, { error: () => m['auth.register.nickname_required']() })
  .max(NICKNAME_MAX_LENGTH, { error: () => m['auth.nickname_max_length']() })
