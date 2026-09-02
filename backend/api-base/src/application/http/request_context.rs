use axum::{extract::Request, middleware::Next, response::Response};

tokio::task_local! {
    pub(crate) static REQUEST_ID: String;
}

/// Makes the request correlation id available while handlers build API errors.
/// `SetRequestIdLayer` must run before this middleware.
pub async fn bind_request_id(request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .unwrap_or("-")
        .to_owned();

    REQUEST_ID.scope(request_id, next.run(request)).await
}

pub fn current_request_id() -> Option<String> {
    REQUEST_ID
        .try_with(|request_id| request_id.clone())
        .ok()
        .filter(|request_id| request_id != "-")
}
