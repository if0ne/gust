use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed_for_web::EmbedableFile;
use uuid::Uuid;

use super::{ApiError, state::AppState};

use crate::{job::materialize_tasks, service::workflow};

pub(crate) async fn list_workflows(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let workflows = state.store.list_workflows().await?;

    Ok(Json(workflows))
}

pub(crate) async fn create_workflow(
    State(state): State<AppState>,
    Json(spec): Json<workflow::spec::WorkflowSpec>,
) -> Result<impl IntoResponse, ApiError> {
    spec.validate()
        .map_err(|e| ApiError::bad_request(format!("workflow validation error: {e}")))?;

    // Persist a canonical YAML rendering of the validated spec for display.
    let yaml = serde_yml::to_string(&spec)
        .map_err(|e| ApiError::internal(format!("Failed to serialize workflow: {e}")))?;

    state.store.upsert_workflow(&yaml, &spec).await?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"workflow_id": spec.id})),
    ))
}

pub(crate) async fn get_workflow(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let row = state
        .store
        .get_workflow(&workflow_id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("workflow '{workflow_id}' not found")))?;
    Ok(Json(row))
}

pub(crate) async fn pause_workflow(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let found = state.store.set_workflow_active(&workflow_id, false).await?;
    if !found {
        return Err(ApiError::not_found(format!(
            "workflow '{workflow_id}' not found"
        )));
    }
    Ok(Json(
        serde_json::json!({"workflow_id": workflow_id, "is_active": false}),
    ))
}

pub(crate) async fn unpause_workflow(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let found = state.store.set_workflow_active(&workflow_id, true).await?;
    if !found {
        return Err(ApiError::not_found(format!(
            "workflow '{workflow_id}' not found"
        )));
    }
    Ok(Json(
        serde_json::json!({"workflow_id": workflow_id, "is_active": true}),
    ))
}

pub(crate) async fn trigger_workflow(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !state.store.workflow_exists(&workflow_id).await? {
        return Err(ApiError::not_found(format!(
            "workflow '{workflow_id}' not found"
        )));
    }

    let logical_date = chrono::Utc::now();
    let run_id = state
        .store
        .create_run(&workflow_id, logical_date, "manual")
        .await?
        .ok_or_else(|| {
            ApiError::bad_request("A run for this exact timestamp already exists".into())
        })?;

    materialize_tasks(&*state.store, run_id, &workflow_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"run_id": run_id})),
    ))
}

pub(crate) async fn list_runs(
    State(state): State<AppState>,
    Path(workflow_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let runs = state
        .store
        .list_runs_for_workflow(&workflow_id, 100)
        .await?;

    Ok(Json(runs))
}

pub(crate) async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let run = state
        .store
        .get_run(run_id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Run '{run_id}' not found")))?;

    Ok(Json(run))
}

pub(crate) async fn list_run_tasks(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let tasks = state.store.list_tasks_for_run(run_id).await?;

    Ok(Json(tasks))
}

pub(crate) async fn get_task_logs(
    State(state): State<AppState>,
    Path(task_instance_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let logs = state.store.get_task_logs(task_instance_id).await?;

    Ok(Json(logs))
}

#[derive(rust_embed_for_web::RustEmbed)]
#[folder = "web/dist"]
struct WebAssets;

pub(crate) async fn serve_ui(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match WebAssets::get(path) {
        Some(file) => serve_asset(file, &headers),
        // SPA fallback: serve index.html for client-side routes.
        None => match WebAssets::get("index.html") {
            Some(file) => serve_asset(file, &headers),
            None => (StatusCode::NOT_FOUND, "Not found").into_response(),
        },
    }
}

/// Serve an embedded asset, honoring ETag revalidation and gzip negotiation.
fn serve_asset(file: impl EmbedableFile, headers: &HeaderMap) -> Response {
    let etag = file.etag();

    // Conditional request: return 304 when the client's cached copy matches.
    if let Some(if_none_match) = headers.get(header::IF_NONE_MATCH)
        && if_none_match.as_bytes() == etag.as_ref().as_bytes()
    {
        return StatusCode::NOT_MODIFIED.into_response();
    }

    let mime = file.mime_type();
    let mime = mime
        .as_ref()
        .map_or("application/octet-stream", |m| m.as_ref());

    let builder = Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::ETAG, etag.as_ref());

    let accepts_gzip = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("gzip"));

    let response = if accepts_gzip && let Some(gzipped) = file.data_gzip() {
        builder
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(gzipped.as_ref().to_vec()))
    } else {
        builder.body(Body::from(file.data().as_ref().to_vec()))
    };

    response.expect("response builder with valid header values")
}
