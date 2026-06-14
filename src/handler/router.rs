use axum::{
    Router,
    routing::{get, post},
};

use super::state::AppState;

/// Build the HTTP router: REST API, health probes, and the embedded-UI fallback.
pub fn base_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(super::status::healthz))
        .route("/readyz", get(super::status::readyz))
        .route(
            "/api/workflows",
            get(super::workflow::list_workflows).post(super::workflow::create_workflow),
        )
        .route(
            "/api/workflows/{workflow_id}",
            get(super::workflow::get_workflow),
        )
        .route(
            "/api/workflows/{workflow_id}/pause",
            post(super::workflow::pause_workflow),
        )
        .route(
            "/api/workflows/{workflow_id}/unpause",
            post(super::workflow::unpause_workflow),
        )
        .route(
            "/api/workflows/{workflow_id}/trigger",
            post(super::workflow::trigger_workflow),
        )
        .route(
            "/api/workflows/{workflow_id}/runs",
            get(super::workflow::list_runs),
        )
        .route("/api/runs/{run_id}", get(super::workflow::get_run))
        .route(
            "/api/runs/{run_id}/tasks",
            get(super::workflow::list_run_tasks),
        )
        .route(
            "/api/tasks/{task_instance_id}/logs",
            get(super::workflow::get_task_logs),
        )
        // SPA fallback
        .fallback(super::workflow::serve_ui)
        .with_state(state)
}
