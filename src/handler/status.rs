use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::state::AppState;

/// Liveness: the process is up and serving. Always 200.
pub(crate) async fn healthz() -> impl IntoResponse {
    StatusCode::OK
}

/// Readiness: the store backend is reachable. 503 if not.
pub(crate) async fn readyz(State(state): State<AppState>) -> Response {
    match state.store.ping().await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"status": "ready"}))).into_response(),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "unavailable", "error": e.to_string()})),
        )
            .into_response(),
    }
}
