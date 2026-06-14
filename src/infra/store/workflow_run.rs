#![allow(dead_code)]

use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct WorkflowRunRow {
    pub id: Uuid,
    pub workflow_id: String,
    pub logical_date: DateTime<Utc>,
    pub state: String,
    pub run_type: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[async_trait::async_trait]
pub trait WorkflowRunStore: Send + Sync {
    /// Insert a run if one doesn't already exist for `(workflow_id, logical_date)`.
    /// Returns the new run id, or `None` if one already existed.
    async fn create_run(
        &self,
        workflow_id: &str,
        logical_date: DateTime<Utc>,
        run_type: &str,
    ) -> anyhow::Result<Option<Uuid>>;
    async fn get_run(&self, run_id: Uuid) -> anyhow::Result<Option<WorkflowRunRow>>;
    async fn list_runs_for_workflow(
        &self,
        workflow_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<WorkflowRunRow>>;
    async fn max_run_logical_date(
        &self,
        workflow_id: &str,
    ) -> anyhow::Result<Option<DateTime<Utc>>>;
    async fn set_run_state(&self, run_id: Uuid, state: &str) -> anyhow::Result<()>;
    async fn mark_run_running(&self, run_id: Uuid) -> anyhow::Result<()>;
}
