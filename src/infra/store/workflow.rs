#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::service::workflow::spec::WorkflowSpec;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct WorkflowRow {
    pub workflow_id: String,
    pub yaml_source: String,
    pub spec: Value,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct WorkflowWithLastRun {
    pub workflow_id: String,
    pub is_active: bool,
    pub last_run_state: Option<String>,
    pub last_run_at: Option<DateTime<Utc>>,
}

#[async_trait::async_trait]
pub trait WorkflowStore: Send + Sync {
    /// Insert or update a workflow definition (keyed by `spec.id`).
    async fn upsert_workflow(&self, yaml: &str, spec: &WorkflowSpec) -> anyhow::Result<()>;
    /// List all workflows with their most recent run summary.
    async fn list_workflows(&self) -> anyhow::Result<Vec<WorkflowWithLastRun>>;
    async fn get_workflow(&self, workflow_id: &str) -> anyhow::Result<Option<WorkflowRow>>;
    /// Set the active flag; returns `false` if the workflow does not exist.
    async fn set_workflow_active(&self, workflow_id: &str, active: bool) -> anyhow::Result<bool>;
    async fn list_active_workflows(&self) -> anyhow::Result<Vec<WorkflowRow>>;
    async fn workflow_exists(&self, workflow_id: &str) -> anyhow::Result<bool>;
}
