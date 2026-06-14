use std::collections::HashMap;

use sqlx::PgPool;

use uuid::Uuid;

use crate::service::workflow::spec::WorkflowSpec;

use super::{
    task_instance::{TaskInstanceRow, TaskLogRow},
    workflow::{WorkflowRow, WorkflowWithLastRun},
    workflow_run::WorkflowRunRow,
};

const SELECT_COLS: &str = "id, workflow_run_id, task_id, state, try_number, max_retries, started_at, finished_at, exit_code, error, created_at";

/// Durable Postgres-backed store.
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl super::WorkflowStore for PostgresStore {
    async fn upsert_workflow(&self, yaml: &str, spec: &WorkflowSpec) -> anyhow::Result<()> {
        let spec_json = serde_json::to_value(spec)?;
        sqlx::query(
            r#"
            INSERT INTO workflow (workflow_id, yaml_source, spec, is_active, updated_at)
            VALUES ($1, $2, $3, true, NOW())
            ON CONFLICT (workflow_id) DO UPDATE
                SET yaml_source = EXCLUDED.yaml_source,
                    spec        = EXCLUDED.spec,
                    updated_at  = NOW()
            "#,
        )
        .bind(&spec.id)
        .bind(yaml)
        .bind(spec_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_workflows(&self) -> anyhow::Result<Vec<WorkflowWithLastRun>> {
        let rows = sqlx::query_as::<_, WorkflowWithLastRun>(
            r#"
            SELECT d.workflow_id, d.is_active,
                   r.state  AS last_run_state,
                   r.created_at AS last_run_at
            FROM workflow d
            LEFT JOIN LATERAL (
                SELECT state, created_at FROM workflow_run
                WHERE workflow_id = d.workflow_id
                ORDER BY created_at DESC
                LIMIT 1
            ) r ON true
            ORDER BY d.workflow_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_workflow(&self, workflow_id: &str) -> anyhow::Result<Option<WorkflowRow>> {
        let row = sqlx::query_as::<_, WorkflowRow>(
            "SELECT workflow_id, yaml_source, spec, is_active, created_at, updated_at FROM workflow WHERE workflow_id = $1",
        )
        .bind(workflow_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn set_workflow_active(&self, workflow_id: &str, active: bool) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE workflow SET is_active = $1, updated_at = NOW() WHERE workflow_id = $2",
        )
        .bind(active)
        .bind(workflow_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn list_active_workflows(&self) -> anyhow::Result<Vec<WorkflowRow>> {
        let rows = sqlx::query_as::<_, WorkflowRow>(
            "SELECT workflow_id, yaml_source, spec, is_active, created_at, updated_at FROM workflow WHERE is_active = true",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn workflow_exists(&self, workflow_id: &str) -> anyhow::Result<bool> {
        let row: (bool,) =
            sqlx::query_as("SELECT EXISTS(SELECT 1 FROM workflow WHERE workflow_id = $1)")
                .bind(workflow_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0)
    }
}

#[async_trait::async_trait]
impl super::WorkflowRunStore for PostgresStore {
    async fn create_run(
        &self,
        workflow_id: &str,
        logical_date: chrono::DateTime<chrono::Utc>,
        run_type: &str,
    ) -> anyhow::Result<Option<Uuid>> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            r#"
            INSERT INTO workflow_run (workflow_id, logical_date, state, run_type)
            VALUES ($1, $2, 'queued', $3)
            ON CONFLICT (workflow_id, logical_date) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(workflow_id)
        .bind(logical_date)
        .bind(run_type)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id,)| id))
    }

    async fn get_run(&self, run_id: Uuid) -> anyhow::Result<Option<WorkflowRunRow>> {
        let row = sqlx::query_as::<_, WorkflowRunRow>(
            "SELECT id, workflow_id, logical_date, state, run_type, started_at, finished_at, created_at FROM workflow_run WHERE id = $1",
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn list_runs_for_workflow(
        &self,
        workflow_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<WorkflowRunRow>> {
        let rows = sqlx::query_as::<_, WorkflowRunRow>(
            "SELECT id, workflow_id, logical_date, state, run_type, started_at, finished_at, created_at FROM workflow_run WHERE workflow_id = $1 ORDER BY logical_date DESC LIMIT $2",
        )
        .bind(workflow_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn max_run_logical_date(
        &self,
        workflow_id: &str,
    ) -> anyhow::Result<Option<chrono::DateTime<chrono::Utc>>> {
        let row: Option<(Option<chrono::DateTime<chrono::Utc>>,)> =
            sqlx::query_as("SELECT MAX(logical_date) FROM workflow_run WHERE workflow_id = $1")
                .bind(workflow_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.and_then(|(d,)| d))
    }

    async fn set_run_state(&self, run_id: Uuid, state: &str) -> anyhow::Result<()> {
        let finished = matches!(state, "success" | "failed");
        if finished {
            sqlx::query("UPDATE workflow_run SET state = $1, finished_at = NOW() WHERE id = $2")
                .bind(state)
                .bind(run_id)
                .execute(&self.pool)
                .await?;
        } else {
            sqlx::query("UPDATE workflow_run SET state = $1 WHERE id = $2")
                .bind(state)
                .bind(run_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn mark_run_running(&self, run_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE workflow_run SET state = 'running', started_at = NOW() WHERE id = $1 AND state = 'queued'",
        )
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl super::TaskStore for PostgresStore {
    async fn create_task_batch(
        &self,
        run_id: Uuid,
        tasks: &[(&str, &str, i32)],
    ) -> anyhow::Result<()> {
        for (task_id, state, max_retries) in tasks {
            sqlx::query(
                r#"
                INSERT INTO task_instance (workflow_run_id, task_id, state, max_retries)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (workflow_run_id, task_id) DO NOTHING
                "#,
            )
            .bind(run_id)
            .bind(task_id)
            .bind(state)
            .bind(max_retries)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn list_tasks_for_run(&self, run_id: Uuid) -> anyhow::Result<Vec<TaskInstanceRow>> {
        let rows = sqlx::query_as::<_, TaskInstanceRow>(&format!(
            "SELECT {SELECT_COLS} FROM task_instance WHERE workflow_run_id = $1 ORDER BY created_at"
        ))
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_task(&self, id: Uuid) -> anyhow::Result<Option<TaskInstanceRow>> {
        let row = sqlx::query_as::<_, TaskInstanceRow>(&format!(
            "SELECT {SELECT_COLS} FROM task_instance WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn claim_and_mark_running(&self, limit: i64) -> anyhow::Result<Vec<TaskInstanceRow>> {
        let mut tx = self.pool.begin().await?;
        let mut tasks = sqlx::query_as::<_, TaskInstanceRow>(&format!(
            r#"
            SELECT {SELECT_COLS}
            FROM task_instance
            WHERE state = 'queued'
            ORDER BY created_at
            LIMIT $1
            FOR UPDATE SKIP LOCKED
            "#
        ))
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;

        for t in &mut tasks {
            let row = sqlx::query_as::<_, (chrono::DateTime<chrono::Utc> ,)>(
                "UPDATE task_instance SET state = 'running', started_at = NOW() WHERE id = $1 RETURNING started_at",
            )
            .bind(t.id)
            .fetch_one(&mut *tx)
            .await?;
            t.state = "running".to_owned();
            t.started_at = Some(row.0);
        }
        tx.commit().await?;
        Ok(tasks)
    }

    async fn mark_task_running(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE task_instance SET state = 'running', started_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn mark_task_success(&self, id: Uuid, exit_code: i32) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE task_instance SET state = 'success', exit_code = $1, finished_at = NOW() WHERE id = $2",
        )
        .bind(exit_code)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_task_failed(
        &self,
        id: Uuid,
        exit_code: Option<i32>,
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE task_instance SET state = 'failed', exit_code = $1, error = $2, finished_at = NOW() WHERE id = $3",
        )
        .bind(exit_code)
        .bind(error)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn requeue_task(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE task_instance SET state = 'queued', try_number = try_number + 1, started_at = NULL, finished_at = NULL, exit_code = NULL, error = NULL WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_task_state(&self, id: Uuid, state: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE task_instance SET state = $1 WHERE id = $2")
            .bind(state)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_task_state_for(
        &self,
        run_id: Uuid,
        task_id: &str,
        state: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE task_instance SET state = $1 WHERE workflow_run_id = $2 AND task_id = $3",
        )
        .bind(state)
        .bind(run_id)
        .bind(task_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn task_state_map(&self, run_id: Uuid) -> anyhow::Result<HashMap<String, String>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT task_id, state FROM task_instance WHERE workflow_run_id = $1")
                .bind(run_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().collect())
    }

    async fn insert_task_log(
        &self,
        task_instance_id: Uuid,
        try_number: i32,
        stream: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO task_log (task_instance_id, try_number, stream, content) VALUES ($1, $2, $3, $4)",
        )
        .bind(task_instance_id)
        .bind(try_number)
        .bind(stream)
        .bind(content)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_task_logs(&self, task_instance_id: Uuid) -> anyhow::Result<Vec<TaskLogRow>> {
        let rows = sqlx::query_as::<_, TaskLogRow>(
            "SELECT id, task_instance_id, try_number, stream, content, created_at FROM task_log WHERE task_instance_id = $1 ORDER BY id",
        )
        .bind(task_instance_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

#[async_trait::async_trait]
impl super::Store for PostgresStore {
    async fn ping(&self) -> anyhow::Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }
}
