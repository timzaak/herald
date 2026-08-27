// Centralized security and operational defaults for the Herald backend.
// All crates should import from here rather than defining local constants.

pub const DEFAULT_OAUTH_CODE_TTL_SECONDS: u64 = 600;
pub const DEFAULT_LOGIN_CHALLENGE_TTL_SECONDS: u64 = 300;

// --- TOTP ---
pub const TOTP_MAX_FAILURES: i64 = 5;
pub const TOTP_LOCKOUT_SECONDS: u64 = 900;

// --- Email OTP (design email-otp-login §4.5) ---
/// Maximum verification attempts before a code is invalidated and must be
/// re-sent. Matches `StoredOtp.max_attempts` written by the send handler.
pub const OTP_MAX_ATTEMPTS: i64 = 5;
/// TTL of a stored OTP code (seconds). Redis `EX` on `emailotp:{realm}:{email}`.
pub const OTP_CODE_TTL_SECONDS: u64 = 300;
// Rate limits: (max_requests, window_seconds) — matches `rate_limit_hit` params.
pub const OTP_SEND_IP_RATE_LIMIT: (i64, usize) = (5, 60);
pub const OTP_SEND_EMAIL_RATE_LIMIT: (i64, usize) = (2, 60);
pub const OTP_VERIFY_IP_RATE_LIMIT: (i64, usize) = (10, 60);
pub const OTP_VERIFY_EMAIL_RATE_LIMIT: (i64, usize) = (5, 60);

// --- Rate limits: (max_requests, window_seconds) ---
pub const LOGIN_IP_RATE_LIMIT: (i64, usize) = (10, 60);
pub const LOGIN_IDENTIFIER_RATE_LIMIT: (i64, usize) = (2, 60);

pub const REGISTER_IP_RATE_LIMIT: (i64, usize) = (5, 60);
pub const REGISTER_EMAIL_RATE_LIMIT: (i64, usize) = (5, 60);

pub const RESET_PASSWORD_REQUEST_IP_RATE_LIMIT: (i64, usize) = (5, 60);
pub const RESET_PASSWORD_REQUEST_EMAIL_RATE_LIMIT: (i64, usize) = (5, 60);
pub const RESET_PASSWORD_CONFIRM_IP_RATE_LIMIT: (i64, usize) = (5, 60);

pub const VERIFY_EMAIL_CONFIRM_IP_RATE_LIMIT: (i64, usize) = (5, 60);
pub const VERIFY_EMAIL_TRIGGER_IP_RATE_LIMIT: (i64, usize) = (5, 60);
pub const VERIFY_EMAIL_TRIGGER_EMAIL_RATE_LIMIT: (i64, usize) = (5, 60);
pub const CHANGE_EMAIL_REQUEST_IP_RATE_LIMIT: (i64, usize) = (1, 120);
pub const CHANGE_EMAIL_REQUEST_EMAIL_RATE_LIMIT: (i64, usize) = (1, 120);
pub const CHANGE_EMAIL_CONFIRM_IP_RATE_LIMIT: (i64, usize) = (5, 60);

// --- Self-service realm signup (design realm-create §4.1 / §5.1) ---
/// Same-IP 24h cap on self-service realm provisioning. The counter is
/// incremented after validation + human verification pass, before
/// `create_realm`, and is not rolled back on provisioning failure.
pub const SIGNUP_IP_RATE_LIMIT: (i64, usize) = (2, 86_400);

pub const TOTP_VERIFY_USER_RATE_LIMIT: (i64, usize) = (5, 60);
pub const TOTP_VERIFY_IP_RATE_LIMIT: (i64, usize) = (10, 60);

pub const REAUTH_VERIFY_USER_RATE_LIMIT: (i64, usize) = (5, 60);
pub const REAUTH_VERIFY_IP_RATE_LIMIT: (i64, usize) = (10, 60);

// --- Browser refresh token TTL ---
pub const BROWSER_REFRESH_ABSOLUTE_TTL_MIN_SECONDS: i32 = 86_400;
pub const BROWSER_REFRESH_ABSOLUTE_TTL_MAX_SECONDS: i32 = 7_776_000;

// --- Password ---
/// bcrypt cost factor. Kept in sync with `bcrypt::DEFAULT_COST` so the
/// single centralized constant is the source of truth for all hashing
/// call sites (do not call `bcrypt::DEFAULT_COST` directly).
pub const DEFAULT_BCRYPT_COST: u32 = 12;

/// Valid bcrypt hash (cost 12) of an unguessable marker string. Login burns a
/// verification against it whenever the submitted identifier does not resolve
/// to a stored password, so the unknown-identifier path pays the same bcrypt
/// cost as the known-identifier path. Without it, response latency acts as a
/// user-enumeration oracle even when the error messages are identical.
pub const DUMMY_BCRYPT_HASH: &str = "$2b$12$B2i4fbJ4ISySJJSPyi13iu4.LRUsShzTJ1o/EQfjfk8VAgFYtv99K";

// --- Email verification links (email_verification_code table) ---
/// TTL of emailed verify-email / reset-password / change-email codes
/// (seconds). Rows older than this are treated as invalid at lookup time —
/// an emailed link must not stay usable forever.
pub const EMAIL_VERIFICATION_CODE_TTL_SECONDS: u64 = 1800;

// --- LDAP directory login (design support-ldap §8, D2-7) ---
/// TCP/TLS connection establishment timeout for the LDAP directory adapter.
/// Bounded so an unreachable directory fails fast instead of hanging the
/// login request.
pub const LDAP_CONNECT_TIMEOUT_SECONDS: u64 = 5;
/// Hard wall-clock budget for the entire search-then-bind sequence
/// (connect + service bind + search + user bind). Exceeding it fails the
/// login as `directory_unavailable` (503) rather than pinning request
/// workers on a slow directory.
pub const LDAP_TIMEOUT_SECONDS: u64 = 10;

// --- HTTP ---
pub const DEFAULT_HTTP_CLIENT_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_HTTP_CLIENT_CONNECT_TIMEOUT_SECS: u64 = 10;

// --- OAuth ---
pub const OAUTH_STATE_TTL_SECONDS: u64 = 300;
pub const OAUTH_STATE_VALIDATION_TIMEOUT_SECONDS: i64 = 300;

/// Per-IP rate limits for unauthenticated OAuth endpoints that perform
/// upstream I/O (JWKS fetch / WeChat code2session) or unbounded state writes
/// per request. Without a cap, each request costs the server an outbound
/// HTTPS call or a Redis write (amplification / Redis-filler DoS).
pub const OAUTH_UPSTREAM_LOGIN_IP_RATE_LIMIT: (i64, usize) = (10, 60);
/// /authorize seeds OAuth state in Redis and does a client_app DB read per
/// request; allowed a higher ceiling since legitimate SPAs hit it per login.
pub const OAUTH_AUTHORIZE_IP_RATE_LIMIT: (i64, usize) = (30, 60);
/// /token does a Redis GETDEL plus client_app/user DB reads per request;
/// same ceiling as /authorize so an unauthenticated code flood cannot
/// hammer Redis/DB at network speed.
pub const OAUTH_TOKEN_IP_RATE_LIMIT: (i64, usize) = (30, 60);
pub const DEVICE_AUTHORIZE_IP_RATE_LIMIT: (i64, usize) = (10, 60);

// --- JWT ---
pub const DEFAULT_JWT_EXPIRATION_SECONDS: i64 = 7 * 24 * 60 * 60;

// --- Device Code ---
pub const DEVICE_CODE_TTL_SECONDS: u64 = 900;
pub const DEVICE_CODE_DEFAULT_INTERVAL_SECONDS: i64 = 5;
pub const DEVICE_CODE_SLOW_DOWN_INCREMENT_SECONDS: i64 = 5;
pub const DEVICE_CODE_USER_CODE_LENGTH: usize = 8;
pub const DEVICE_CODE_USER_CODE_ALPHABET: &str = "BCDFGHJKMNPQRSTVWXYZ";
