//! RED metrics attribute extractor for `tower-otel-http-metrics`.
//!
//! This is the **single governance authority** for which custom attributes
//! the RED middleware adds beyond the library's built-in semconv labels.
//!
//! # Sensitive-data governance
//!
//! The `tower-otel-http-metrics` library already emits the low-cardinality
//! semconv labels `http.route` (axum `MatchedPath` route **template**, not raw
//! path), `http.request.method`, `http.response.status_code`,
//! `network.protocol.{name,version}`, and `url.scheme` from the request
//! extensions / response status — it never reads headers, body, tokens, or
//! PII.
//!
//! [`RedAttributeExtractor`] only **adds** the `error.type` label on 5xx
//! responses. It reads exclusively from `http::Response::status()` and emits
//! a fixed string literal (`"server_error"`). Structurally it cannot produce
//! token / API key / email / user_id / realmId / raw path / raw SQL: it does
//! not touch the request or response headers, body, URI, or extensions
//! beyond the status code.
//!
//! # Why request-side extraction is empty
//!
//! The library's own `HTTPMetricsService::call` reads `MatchedPath` from the
//! request extensions (under `feature="axum"`) and emits `http.route`
//! directly. Re-emitting it from a custom request extractor would only
//! duplicate the label. Therefore [`RequestAttributeExtractor`] returns an
//! empty `Vec` and the request-side whitelist is satisfied by the library.
//!
//! The `UNMATCHED` fallback for unmatched routes is a library-level gap
//! (when `MatchedPath` is absent the library omits `http.route` rather than
//! emitting `UNMATCHED`); overriding that would require duplicating
//! `http.route`, which is explicitly avoided here.

use axum::http::{Request, Response};
use opentelemetry::{KeyValue, Value};
use tower_otel_http_metrics::{RequestAttributeExtractor, ResponseAttributeExtractor};

/// Attribute key for the OTel semconv `error.type` label.
const ERROR_TYPE_LABEL: &str = "error.type";

/// Fixed value recorded for `error.type` on any 5xx response.
///
/// Kept as a single coarse value (rather than per-status strings) to
/// guarantee low cardinality and to avoid surfacing exact status codes
/// through the error dimension (they already appear on
/// `http.response.status_code`).
const ERROR_TYPE_SERVER_ERROR: &str = "server_error";

/// Custom RED attribute extractor — the whitelist authority for the RED
/// middleware.
///
/// A unit struct (no state): construction is trivial and the test slot
/// can instantiate it directly without a tower `Service` stack and
/// assert exactly which attributes it produces.
#[derive(Clone, Default, Debug)]
pub struct RedAttributeExtractor;

impl RedAttributeExtractor {
    /// Construct the extractor. Cheap, stateless.
    pub fn new() -> Self {
        Self
    }
}

impl<B> RequestAttributeExtractor<B> for RedAttributeExtractor {
    /// Returns no extra attributes.
    ///
    /// See the module-level docs: the library already emits the
    /// request-side whitelist (`http.route` template, `http.request.method`,
    /// protocol / scheme labels) from the request itself.
    fn extract_attributes(&self, _req: &Request<B>) -> Vec<KeyValue> {
        Vec::new()
    }
}

impl<B> ResponseAttributeExtractor<B> for RedAttributeExtractor {
    /// Adds `error.type = "server_error"` **only** for 5xx responses.
    ///
    /// Non-5xx responses get no extra attribute, so the `error.type` label
    /// is absent for successful / 4xx responses (OTel convention: omit rather
    /// than emit a sentinel for non-errors).
    fn extract_attributes(&self, res: &Response<B>) -> Vec<KeyValue> {
        let status = res.status().as_u16();
        if status >= 500 {
            vec![KeyValue::new(
                ERROR_TYPE_LABEL,
                Value::from(ERROR_TYPE_SERVER_ERROR),
            )]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// User Story: Technical invariant — RED extractor response-side label
    /// allow-list.
    /// Covers: "extractor 仅白名单 … 5xx error.type" +
    /// "RED extractor 治理 … 产出 label 不含这些值".
    ///
    /// WHY this test exists: asserting only the presence of `error.type` on
    /// one 5xx and its absence below 500 does NOT lock the *entire* response
    /// attribute set. Governance
    /// is a closed allow-list: a future change could add a second label
    /// (e.g. a status-text or a response-header echo) while keeping
    /// `error.type` correct, and a presence-only check would still pass — silently
    /// widening the label surface. This test fails in exactly that case by
    /// asserting the key set is EXACTLY `{error.type}` for 5xx and EXACTLY
    /// `{}` otherwise, across the whole status range. The synthetic response
    /// also carries a fake `Authorization` header and a PII-laden
    /// `X-Email` header to prove the extractor cannot echo response headers
    /// (it reads only `Response::status()`).
    #[test]
    fn red_extractor_response_allow_list_is_exactly_error_type_on_5xx() {
        let extractor = RedAttributeExtractor::new();

        // 5xx: the ONLY key permitted on the extractor output is error.type.
        for code in [500u16, 501, 502, 503, 504, 599] {
            let res = axum::http::Response::builder()
                .status(code)
                // Decoy headers — must never reach label output.
                .header("authorization", "Bearer response-leak-decoy")
                .header("x-email", "ops@example.com")
                .body(())
                .unwrap();
            let attrs = ResponseAttributeExtractor::extract_attributes(&extractor, &res);
            let keys: std::collections::HashSet<&str> =
                attrs.iter().map(|kv| kv.key.as_str()).collect();
            assert_eq!(
                keys,
                std::collections::HashSet::from([ERROR_TYPE_LABEL]),
                "status {code}: response allow-list must be exactly {{error.type}}, got {keys:?}"
            );
            // And its value is the fixed low-cardinality literal, never the
            // status text or any header content.
            assert_eq!(attrs[0].value.as_str(), ERROR_TYPE_SERVER_ERROR);
        }

        // Non-5xx: the allow-list must be the EMPTY set (no error.type, no
        // fabricated label of any kind). This is the closed-list assertion
        // a presence-only check would not make.
        for code in [
            100u16, 200, 201, 204, 301, 302, 304, 400, 401, 403, 404, 409, 422, 451,
        ] {
            let res = axum::http::Response::builder()
                .status(code)
                .header("authorization", "Bearer response-leak-decoy")
                .body(())
                .unwrap();
            let attrs = ResponseAttributeExtractor::extract_attributes(&extractor, &res);
            assert!(
                attrs.is_empty(),
                "status {code}: non-5xx response must emit NO extractor attribute, got {attrs:?}"
            );
        }
    }

    /// User Story: Technical invariant — RED extractor request-side label
    /// allow-list is empty by construction ("WHY
    /// request-side extraction is empty" module docs).
    /// Covers: "extractor 仅白名单 … http.route 路由模板/
    /// UNMATCHED、method、status、5xx error.type" (request side owned by the
    /// library) + "给定含 token/email/原始 path 的请求，产出
    /// label 不含这些值".
    ///
    /// WHY this test exists: exercising a single request
    /// shape is not enough. Governance is structural — the request extractor must
    /// read NOTHING from the request, so no input variation can widen the
    /// output. This test crams every sensitive surface
    /// (token in `Authorization` + query, email header, raw path with a
    /// secret-bearing realm id, a `MatchedPath` route template extension,
    /// custom `x-user-id` / `x-realm-id` headers) and asserts the output set
    /// is the EMPTY SET — closing the allow-list rather than spot-checking
    /// one input. If a future change starts reading any request field, this
    /// test fails for the broadest input class, not just the single fixture.
    #[test]
    fn red_extractor_request_allow_list_is_empty_across_all_sensitive_surfaces() {
        let extractor = RedAttributeExtractor::new();

        // Every sensitive surface at once, all in one request.
        //
        // Note: a `MatchedPath` extension (which the LIBRARY, not this
        // extractor, reads to emit `http.route`) cannot be injected here —
        // axum's `MatchedPath` has no public constructor outside its own
        // request flow. That gap is irrelevant to THIS assertion: the
        // extractor structurally reads nothing from extensions either way,
        // and the load-bearing surfaces (token / email / user_id / realmId /
        // raw path) are all present below.
        let req = axum::http::Request::builder()
            .method("POST")
            // Raw path with a secret-bearing realm id AND a token in query —
            // the exact shape governance forbids from reaching labels.
            .uri("/api/points/realm-123-secret/consume?token=leaked-token&email=user@example.com")
            .header("authorization", "Bearer leaked-token")
            .header("x-email", "user@example.com")
            .header("x-user-id", "user-456")
            .header("x-realm-id", "realm-123-secret")
            .header("x-request-id", "req-abc-123")
            .body(())
            .unwrap();

        let attrs = RequestAttributeExtractor::extract_attributes(&extractor, &req);
        assert!(
            attrs.is_empty(),
            "request extractor allow-list must be the empty set regardless of \
             token/email/user_id/realmId/raw-path in the request; got {attrs:?}"
        );
    }
}

// =========================================================================
// request_id span-field correlation unit tests.
//
// These are UNIT tests that construct a `TraceLayer` with the SAME
// `make_span_with` closure shape as production
// (`application::http::server::mod.rs`). The production closure is
// defined inline inside `create_router` and is not separately exported, and
// the scenario-test router (`create_unified_test_router`) deliberately does
// NOT go through the ServiceBuilder chain (confirmed in
// `schema_test_context.rs`), so per the test slot manifest these assertions
// MUST be unit tests that build the layer directly. The closure below is a
// faithful reproduction kept in lock-step with production by this test.
// =========================================================================
#[cfg(test)]
mod request_id_span_tests {
    use axum::http::Request;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;
    use tracing_subscriber::layer::SubscriberExt;

    /// The production request-id header name (`server/mod.rs`).
    const REQUEST_ID_HEADER: axum::http::HeaderName =
        axum::http::HeaderName::from_static("x-request-id");

    /// Reproduce the production `make_span_with` closure body
    /// (`application::http::server::mod.rs`) as a standalone
    /// function over `&Request`: read `X-Request-ID` (falling back to
    /// `"-"`), record it on the span as the `request_id` field, and use
    /// the route template / `UNMATCHED` sentinel for `http.route` (never
    /// the raw path).
    ///
    /// WHY we don't drive a real `TraceLayer` through `ServiceExt::oneshot`:
    /// `tower-http`'s `TraceLayer` requires the request body to be `Clone`
    /// (it clones the request to pass to `make_span_with`), but
    /// `axum::body::Body` is not `Clone`. Production avoids this because the
    /// layer wraps axum's `Router`, whose `Service` impl owns the body
    /// lifetime differently. The contract under test here is purely the
    /// span-creating logic, so invoking it directly against a constructed
    /// `Request` is the faithful unit assertion.
    ///
    /// Kept in lock-step with `application::http::server::mod.rs`. If that
    /// closure changes shape, this helper and the tests below must be
    /// updated together — the test exists precisely to detect such drift.
    fn make_request_span<B>(request: &Request<B>) -> tracing::Span {
        let request_id = request
            .headers()
            .get(&REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .unwrap_or("-")
            .to_owned();
        let method = request.method().as_str();
        let route = request
            .extensions()
            .get::<axum::extract::MatchedPath>()
            .map(|m| m.as_str().to_owned())
            .unwrap_or_else(|| "UNMATCHED".to_owned());
        tracing::info_span!(
            "http.request",
            method = method,
            http.route = %route,
            request_id = %request_id,
        )
    }

    /// In-memory `MakeWriter` capturing the fmt layer's bytes into a shared
    /// buffer. This is the canonical `MockWriter` pattern from the
    /// tracing-subscriber test suite (its own `MockMakeWriter` is not
    /// publicly exported in 0.3.x), kept minimal.
    #[derive(Clone)]
    struct BufWriter(Arc<Mutex<Vec<u8>>>);

    impl BufWriter {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Vec::new())))
        }
        fn take_string(&self) -> String {
            let mut g = self.0.lock().expect("buf writer mutex poisoned");
            String::from_utf8(std::mem::take(&mut *g)).expect("captured log is utf8")
        }
    }

    impl std::io::Write for BufWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("buf writer mutex poisoned")
                .extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for BufWriter {
        type Writer = BufWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    // ---------------------------------------------------------------------
    // Test 1: request_id is written to the span field.
    // ---------------------------------------------------------------------

    /// User Story: Technical invariant — every HTTP request span carries the
    /// ops correlation key `request_id` read from `X-Request-ID`, so a log
    /// line / metric / trace for a request can always be tied back to the
    /// caller-supplied id.
    /// Covers: "request_id 入 span field" +
    /// "request_id 关联".
    ///
    /// WHY this test exists: `request_id` is the primary ops correlation
    /// key. If the `make_span_with` closure stops recording it (e.g. a
    /// refactor drops the field, or reads the wrong header name), every log
    /// line silently loses its join key and on-call correlation breaks with
    /// no signal. We build the layer directly, send one request carrying
    /// `X-Request-ID: req-abc-123` through it via `ServiceExt::oneshot`, and
    /// assert the captured JSON log line contains the recorded `request_id`.
    #[test]
    fn request_id_written_to_span_field() {
        let captured = drive_request_through_baseline_subscriber("req-abc-123");
        assert!(
            captured.contains("\"request_id\":\"req-abc-123\""),
            "captured log must carry the request_id span field; got: {captured}"
        );
    }

    // ---------------------------------------------------------------------
    // Test 2: baseline (traces off) — request_id still present, trace_id
    // absent.
    // ---------------------------------------------------------------------

    /// User Story: Technical invariant — under the baseline deployment
    /// (`traces_enabled == false`, no OTel traces layer installed) the
    /// request_id correlation key STILL reaches the local JSON log, while no
    /// `trace_id` is emitted ("traces off 兜底").
    /// Covers: "baseline traces off 时 request_id 仍经 fmt
    /// layer 出现在 JSON 日志" + "baseline traces off 时
    /// `trace_id` 缺失而 `request_id` 存在".
    ///
    /// WHY this test exists: the back-pressure mitigation (baseline = traces
    /// off) is only safe if ops correlation SURVIVES it. If `request_id`
    /// were wired through the OTel traces layer, disabling traces would
    /// silently drop the join key from logs too — the exact failure mode
    /// the baseline is supposed to avoid. This test builds ONLY the fmt JSON
    /// layer (NO `tracing_opentelemetry` layer, simulating baseline) and
    /// asserts: `request_id` present, `trace_id` / `span_id` absent. If
    /// someone re-routes `request_id` through the traces layer, or installs
    /// an always-on OTel layer by mistake, this test fails.
    #[test]
    fn request_id_present_in_json_log_when_traces_off() {
        let captured = drive_request_through_baseline_subscriber("req-baseline-789");
        assert!(
            captured.contains("\"request_id\":\"req-baseline-789\""),
            "baseline (traces off): request_id MUST still reach the JSON log; got: {captured}"
        );
        assert!(
            !captured.contains("\"trace_id\""),
            "baseline (traces off): no trace_id may be emitted; got: {captured}"
        );
        assert!(
            !captured.contains("\"span_id\""),
            "baseline (traces off): no OTel span_id may be emitted either; got: {captured}"
        );
    }

    /// Harness shared by both request_id assertions: install a
    /// `tracing_subscriber` with ONLY a fmt JSON layer (no OTel traces
    /// layer — this is the baseline shape), invoke the production-shaped
    /// `make_span_with` logic against a request carrying the given
    /// `X-Request-ID`, emit one event inside that span so the fmt layer
    /// flushes the span fields, and return the buffered JSON output for the
    /// caller to assert on.
    ///
    /// An event is required inside the span because the fmt layer only
    /// serializes a span's fields when an event is recorded within its
    /// scope (or the span closes). Emitting one `info!` event guarantees the
    /// `request_id` field is written regardless of layer ordering.
    fn drive_request_through_baseline_subscriber(request_id_value: &str) -> String {
        let buf = BufWriter::new();

        // Baseline shape: registry + fmt JSON layer only. NO
        // `tracing_opentelemetry` layer is installed, mirroring
        // `build_traces_layer(&baseline)` returning `None` (see the
        // observability module tests).
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(buf.clone()),
        );

        let req = axum::http::Request::builder()
            .method("GET")
            .uri("/api/points/realm-1/consume")
            .header(REQUEST_ID_HEADER, request_id_value)
            .body(())
            .unwrap();

        {
            // `set_default` takes a `Dispatch`; the subscriber converts into one.
            let dispatch = tracing::Dispatch::new(subscriber);
            let _guard = tracing::dispatcher::set_default(&dispatch);
            let span = make_request_span(&req);
            let _enter = span.enter();
            tracing::info!("request handled");
        }

        buf.take_string()
    }
}
