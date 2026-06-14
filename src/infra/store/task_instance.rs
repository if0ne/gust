#![allow(dead_code)]
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct TaskInstanceRow {
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub task_id: String,
    pub state: String,
    pub try_number: i32,
    pub max_retries: i32,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct TaskLogRow {
    pub id: i64,
    pub task_instance_id: Uuid,
    pub try_number: i32,
    pub stream: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[async_trait::async_trait]
pub trait TaskStore: Send + Sync {
    /// Insert task rows for a run. Each tuple is `(task_id, state, max_retries)`.
    /// Existing `(run_id, task_id)` pairs are left untouched.
    async fn create_task_batch(
        &self,
        run_id: Uuid,
        tasks: &[(&str, &str, i32)],
    ) -> anyhow::Result<()>;
    async fn list_tasks_for_run(&self, run_id: Uuid) -> anyhow::Result<Vec<TaskInstanceRow>>;
    async fn get_task(&self, id: Uuid) -> anyhow::Result<Option<TaskInstanceRow>>;
    /// Atomically claim up to `limit` `queued` tasks and mark them `running`.
    /// On Postgres this uses `FOR UPDATE SKIP LOCKED` so multiple workers never
    /// claim the same task.
    async fn claim_and_mark_running(&self, limit: i64) -> anyhow::Result<Vec<TaskInstanceRow>>;
    async fn mark_task_running(&self, id: Uuid) -> anyhow::Result<()>;
    async fn mark_task_success(&self, id: Uuid, exit_code: i32) -> anyhow::Result<()>;
    async fn mark_task_failed(
        &self,
        id: Uuid,
        exit_code: Option<i32>,
        error: Option<&str>,
    ) -> anyhow::Result<()>;
    /// Retry: increment `try_number`, reset to `queued`.
    async fn requeue_task(&self, id: Uuid) -> anyhow::Result<()>;
    async fn set_task_state(&self, id: Uuid, state: &str) -> anyhow::Result<()>;
    /// Set the state of a task identified by `(workflow_run_id, task_id)`.
    async fn set_task_state_for(
        &self,
        run_id: Uuid,
        task_id: &str,
        state: &str,
    ) -> anyhow::Result<()>;
    async fn task_state_map(&self, run_id: Uuid) -> anyhow::Result<HashMap<String, String>>;
    async fn insert_task_log(
        &self,
        task_instance_id: Uuid,
        try_number: i32,
        stream: &str,
        content: &str,
    ) -> anyhow::Result<()>;
    async fn get_task_logs(&self, task_instance_id: Uuid) -> anyhow::Result<Vec<TaskLogRow>>;
}
