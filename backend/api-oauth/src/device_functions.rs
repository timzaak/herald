// Single Redis Function library for the device flow (RFC 8628).
//
// One FUNCTION LOAD REPLACE registers every device-flow function atomically
// under the `herald_device` library:
//
// * device_verify_transition  -- atomic pending -> verified (device_verify.rs)
// * device_confirm_transition -- atomic verified -> authorized/denied
//   (device_confirm.rs)
// * device_token_poll         -- poll state machine + interval enforcement
//   (device_token.rs)
//
// Each function is the single validation authority for its transition; the
// handlers never pre-validate the device state on a plain read.

use herald_api_base::application::http::server::api_entities::ApiError;
use herald_api_base::application::http::state::AppState;
use herald_core::domain::security_constants::DEVICE_CODE_SLOW_DOWN_INCREMENT_SECONDS;

const DEVICE_FUNCTION_LIBRARY: &str = "herald_device";

/// Lua source of the `herald_device` library. The realm check always runs
/// FIRST, in every function: a wrong-realm caller never learns or advances
/// the state (an error-priority choice pinned across verify/confirm/token).
///
/// device_verify_transition -- atomic pending -> verified transition. The
/// user_code -> user binding must be written atomically: two concurrent verify
/// calls on the same pending code must not both succeed (the later writer
/// would silently steal the earlier caller's binding after that caller
/// already received a 200). A single FCALL closes the GET -> decide -> SET
/// race, mirroring the `device_token_poll` pattern.
///
/// Operation order:
/// 1. Key missing                  -> not_found
/// 2. realm mismatch               -> realm_mismatch (a wrong-realm caller
///    never learns or advances the state)
/// 3. denied/consumed/authorized   -> already_confirmed (terminal)
/// 4. verified + same user         -> ok, idempotent (no write)
/// 5. verified + different user    -> already_used
/// 6. pending                      -> write verified + user binding -> ok
///
/// device_confirm_transition -- atomic verified -> authorized/denied
/// transition. The terminal write is conditional: it only lands when the
/// state is still `verified` in the same realm for the same user. This closes
/// the gate -> write race where two concurrent confirms (approve/deny) could
/// overwrite each other's terminal state — the second FCALL observes the
/// first one's terminal status and returns already_used instead, keeping the
/// state machine irreversible. The login consent gate cannot live inside the
/// function (it needs DB access), so the handler gates on the session user
/// first and this function is the single validation authority at the write.
///
/// Operation order:
/// 1. Key missing            -> not_found
/// 2. realm mismatch         -> realm_mismatch
/// 3. status == pending      -> not_verified
/// 4. status != verified     -> already_used (terminal)
/// 5. user mismatch          -> already_used
/// 6. verified + same user   -> write authorized/denied -> ok
///
/// device_token_poll -- atomic device token polling state management: all
/// state transitions and interval enforcement in a single FCALL invocation,
/// eliminating race conditions between concurrent poll requests.
///
/// Operation order (realm check first, terminal states next):
/// 1. Key missing           -> expired_token
/// 2. realm mismatch        -> invalid_request (PRD device-code.md §4.2:
///    every endpoint answers a wrong-realm poll with invalid_request,
///    whatever the status — pending/verified included)
/// 3. status == consumed    -> invalid_request
/// 4. status == denied      -> access_denied
/// 5. status == authorized  -> consume + return user data
/// 6. interval too fast     -> slow_down (interval += slow-down increment)
/// 7. pending / verified    -> authorization_pending
const DEVICE_FUNCTION_CODE: &str = "#!lua name=herald_device\n\
\n\
local function device_verify_transition(keys, args)\n\
  local key = keys[1]\n\
  local expected_realm = args[1]\n\
  local user_id = args[2]\n\
\n\
  local data = redis.call('GET', key)\n\
  if not data then\n\
    return cjson.encode({ok=false, error='not_found'})\n\
  end\n\
\n\
  local state = cjson.decode(data)\n\
\n\
  if state.realm_id ~= expected_realm then\n\
    return cjson.encode({ok=false, error='realm_mismatch'})\n\
  end\n\
\n\
  local status = state.status\n\
  if status == 'denied' or status == 'consumed' or status == 'authorized' then\n\
    return cjson.encode({ok=false, error='already_confirmed'})\n\
  end\n\
\n\
  if status == 'verified' then\n\
    if state.user_id == user_id then\n\
      return cjson.encode({ok=true, client_id=state.client_id})\n\
    end\n\
    return cjson.encode({ok=false, error='already_used'})\n\
  end\n\
\n\
  if status == 'pending' then\n\
    state.status = 'verified'\n\
    state.user_id = user_id\n\
    redis.call('SET', key, cjson.encode(state), 'KEEPTTL')\n\
    return cjson.encode({ok=true, client_id=state.client_id})\n\
  end\n\
\n\
  return cjson.encode({ok=false, error='not_found'})\n\
end\n\
redis.register_function('device_verify_transition', device_verify_transition)\n\
\n\
local function device_confirm_transition(keys, args)\n\
  local key = keys[1]\n\
  local expected_realm = args[1]\n\
  local user_id = args[2]\n\
  local approved = args[3]\n\
\n\
  local data = redis.call('GET', key)\n\
  if not data then\n\
    return cjson.encode({ok=false, error='not_found'})\n\
  end\n\
\n\
  local state = cjson.decode(data)\n\
\n\
  if state.realm_id ~= expected_realm then\n\
    return cjson.encode({ok=false, error='realm_mismatch'})\n\
  end\n\
\n\
  if state.status == 'pending' then\n\
    return cjson.encode({ok=false, error='not_verified'})\n\
  end\n\
\n\
  if state.status ~= 'verified' then\n\
    return cjson.encode({ok=false, error='already_used'})\n\
  end\n\
\n\
  if state.user_id ~= user_id then\n\
    return cjson.encode({ok=false, error='already_used'})\n\
  end\n\
\n\
  local new_status\n\
  if approved == '1' then\n\
    new_status = 'authorized'\n\
  else\n\
    new_status = 'denied'\n\
  end\n\
  state.status = new_status\n\
  redis.call('SET', key, cjson.encode(state), 'KEEPTTL')\n\
  return cjson.encode({ok=true, status=new_status})\n\
end\n\
redis.register_function('device_confirm_transition', device_confirm_transition)\n\
\n\
local function device_token_poll(keys, args)\n\
  local key = keys[1]\n\
  local now = tonumber(args[1])\n\
  local expected_realm = args[2]\n\
\n\
  local data = redis.call('GET', key)\n\
  if not data then\n\
    return cjson.encode({ok=false, error='expired_token'})\n\
  end\n\
\n\
  local state = cjson.decode(data)\n\
\n\
  -- Realm check BEFORE any state handling: a wrong-realm poll never learns\n\
  -- the authorization state and never advances it (no consume, no interval\n\
  -- bump). Returning invalid_request for every status keeps the error-code\n\
  -- contract of PRD device-code.md 4.2.\n\
  if state.realm_id ~= expected_realm then\n\
    return cjson.encode({ok=false, error='invalid_request'})\n\
  end\n\
\n\
  -- Terminal states first\n\
  if state.status == 'consumed' then\n\
    return cjson.encode({ok=false, error='invalid_request'})\n\
  end\n\
\n\
  if state.status == 'denied' then\n\
    return cjson.encode({ok=false, error='access_denied'})\n\
  end\n\
\n\
  -- Authorized: consume and return the user data (realm already verified\n\
  -- above).\n\
  if state.status == 'authorized' then\n\
    state.status = 'consumed'\n\
    state.last_poll_at = now\n\
    redis.call('SET', key, cjson.encode(state), 'KEEPTTL')\n\
    return cjson.encode({\n\
      ok=true,\n\
      user_id=state.user_id,\n\
      realm_id=state.realm_id,\n\
      client_id=state.client_id\n\
    })\n\
  end\n\
\n\
  -- Check polling interval\n\
  if state.last_poll_at > 0 then\n\
    local elapsed = now - state.last_poll_at\n\
    if elapsed < state.interval then\n\
      state.interval = state.interval + {SLOW_DOWN_INCREMENT}\n\
      state.last_poll_at = now\n\
      redis.call('SET', key, cjson.encode(state), 'KEEPTTL')\n\
      return cjson.encode({ok=false, error='slow_down'})\n\
    end\n\
  end\n\
\n\
  -- Still pending or verified\n\
  state.last_poll_at = now\n\
  redis.call('SET', key, cjson.encode(state), 'KEEPTTL')\n\
  return cjson.encode({ok=false, error='authorization_pending'})\n\
end\n\
redis.register_function('device_token_poll', device_token_poll)\n\
";

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Load the device-flow Redis Function library (verify/confirm/token poll).
///
/// Idempotent -- safe to call multiple times (REPLACE semantics).
pub async fn init_device_functions(state: &AppState) -> Result<(), ApiError> {
    let mut conn = state
        .redis_manager
        .get()
        .await
        .map_err(|_| ApiError::internal("Internal server error"))?;

    redis::cmd("FUNCTION")
        .arg("LOAD")
        .arg("REPLACE")
        .arg(
            // {SLOW_DOWN_INCREMENT} is a placeholder so the increment stays
            // defined by DEVICE_CODE_SLOW_DOWN_INCREMENT_SECONDS rather than a
            // second, drifting copy inside the Lua source.
            DEVICE_FUNCTION_CODE.replace(
                "{SLOW_DOWN_INCREMENT}",
                &DEVICE_CODE_SLOW_DOWN_INCREMENT_SECONDS.to_string(),
            ),
        )
        .query_async::<String>(&mut conn)
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to load Redis Function library '{DEVICE_FUNCTION_LIBRARY}': {e}"
            );
            ApiError::internal("Internal server error")
        })?;

    tracing::info!("Redis Function library '{DEVICE_FUNCTION_LIBRARY}' loaded successfully");

    Ok(())
}
