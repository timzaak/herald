//! Internal API key middleware.
//!
//! Guards demo/test-only "internal" HTTP endpoints that bypass normal user
//! authentication. Access is gated solely by a shared secret (`X-Internal-API-Key`
//! header) read from the `INTERNAL_API_KEY` environment variable. When that env
//! var is unset or empty, every request is rejected (401), so in a production
//! build without the env var the routes are effectively inert while still
//! compiled in — matching the behavior of the existing internal fulfill route.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Constant-time string comparison.
///
/// Compares two ASCII strings byte-by-byte without short-circuiting, so timing
/// does not leak the position of the first mismatched byte. Returns early (non-
/// constant time) only when the lengths differ, which does not reveal secret
/// material. Mirrors `herald_infra_shopify::constant_time_compare` to avoid
/// pulling that crate into `api-base`.
pub fn constant_time_compare(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (byte_a, byte_b) in a.bytes().zip(b.bytes()) {
        result |= byte_a ^ byte_b;
    }

    result == 0
}

// Failure throttle for shared-secret gates. The endpoints behind such gates
// (internal billing/points routes, the Caddy on-demand-TLS ask route) mutate
// state or authorize issuance and have no other authentication, so the secret
// must not be brute-forceable at network speed: after MAX_FAILURES_PER_WINDOW
// failed comparisons inside a sliding FAILURE_WINDOW, every further request is
// rejected until the window rolls — turning an online guessing attack into
// ≤ MAX failures/minute. In-process state is sufficient: the API server is a
// single process and each secret is global (not per-instance). Each gate keeps
// its own throttle instance so hammering one secret does not lock out the
// other.
const MAX_FAILURES_PER_WINDOW: usize = 10;
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

/// Sliding-window failure budget for one shared-secret gate. See the module
/// comment for the threat model; use one instance per secret.
pub struct FailureThrottle {
    window_start_ms: AtomicU64,
    failure_count: AtomicUsize,
}

impl FailureThrottle {
    pub const fn new() -> Self {
        Self {
            window_start_ms: AtomicU64::new(0),
            failure_count: AtomicUsize::new(0),
        }
    }

    /// Whether the failure budget still allows another key comparison. Opens a
    /// fresh window when the previous one has expired; returns false when the
    /// throttle is tripped and the attempt must be rejected without comparing.
    pub fn allows_attempt(&self) -> bool {
        let now = now_epoch_ms();
        let window_ms = FAILURE_WINDOW.as_millis() as u64;
        let start = self.window_start_ms.load(Ordering::Relaxed);
        if now.saturating_sub(start) >= window_ms {
            // Window expired (or never opened): reset. A racing writer can only
            // reset to the same "fresh window" state, so no lock is needed.
            self.window_start_ms.store(now, Ordering::Relaxed);
            self.failure_count.store(0, Ordering::Relaxed);
            return true;
        }
        self.failure_count.load(Ordering::Relaxed) < MAX_FAILURES_PER_WINDOW
    }

    pub fn record_failure(&self) {
        self.failure_count.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for FailureThrottle {
    fn default() -> Self {
        Self::new()
    }
}

static INTERNAL_KEY_THROTTLE: FailureThrottle = FailureThrottle::new();

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Middleware that validates the `X-Internal-API-Key` header against the
/// `INTERNAL_API_KEY` environment variable.
///
/// Rejects with 401 UNAUTHORIZED when the header is missing, the env var is
/// unset/empty, or the values differ. Rejects with 429 TOO MANY REQUESTS when
/// the failure throttle is tripped. On success it forwards to the next layer
/// unchanged (no identity is injected — callers must not rely on one).
pub async fn internal_api_key_middleware(req: Request, next: Next) -> Response {
    let provided_key = req
        .headers()
        .get("X-Internal-API-Key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim);

    let expected_key = std::env::var("INTERNAL_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty());

    // Only key COMPARISONS are throttled. A missing header or unset env var
    // reveals no secret material and cannot be an online guess, so those
    // stay plain 401s and never trip the lockout.
    let attempt_is_guess = provided_key.is_some() && expected_key.is_some();

    if attempt_is_guess && !INTERNAL_KEY_THROTTLE.allows_attempt() {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }

    let authorized = matches!(
        (provided_key, &expected_key),
        (Some(provided), Some(expected)) if constant_time_compare(provided, expected)
    );

    if authorized {
        next.run(req).await
    } else {
        if attempt_is_guess {
            INTERNAL_KEY_THROTTLE.record_failure();
        }
        StatusCode::UNAUTHORIZED.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset_throttle(throttle: &FailureThrottle) {
        throttle.failure_count.store(0, Ordering::SeqCst);
        throttle
            .window_start_ms
            .store(now_epoch_ms(), Ordering::SeqCst);
    }

    #[test]
    fn throttle_trips_after_max_failures_and_recovers_after_window() {
        // WHY this matters: the internal endpoints have no other auth, so
        // the gate must cap online key-guessing at MAX_FAILURES_PER_WINDOW
        // per window — otherwise the shared secret is brute-forceable at
        // network speed.
        let throttle = FailureThrottle::new();
        reset_throttle(&throttle);
        for _ in 0..MAX_FAILURES_PER_WINDOW {
            assert!(throttle.allows_attempt());
            throttle.record_failure();
        }
        assert!(
            !throttle.allows_attempt(),
            "throttle must trip after the failure budget is spent"
        );
        assert!(!throttle.allows_attempt());

        // Window expiry reopens the budget (a locked-out operator must not
        // be denied forever).
        throttle.window_start_ms.store(
            now_epoch_ms().saturating_sub(FAILURE_WINDOW.as_millis() as u64 + 1),
            Ordering::SeqCst,
        );
        assert!(throttle.allows_attempt());
    }

    #[test]
    fn independent_throttles_do_not_share_budget() {
        // WHY this matters: the Caddy ask gate and the internal API key gate
        // protect different secrets; exhausting one budget must not lock out
        // the other route.
        let a = FailureThrottle::new();
        let b = FailureThrottle::new();
        reset_throttle(&a);
        reset_throttle(&b);
        for _ in 0..MAX_FAILURES_PER_WINDOW {
            a.record_failure();
        }
        assert!(!a.allows_attempt());
        assert!(b.allows_attempt());
    }

    #[test]
    fn constant_time_compare_rejects_mismatched_pairs() {
        assert!(constant_time_compare("secret", "secret"));
        assert!(!constant_time_compare("secret", "Secret"));
        assert!(!constant_time_compare("secret", "secrets"));
        assert!(!constant_time_compare("", "s"));
    }
}
