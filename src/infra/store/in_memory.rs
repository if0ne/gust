use std::collections::HashMap;

use parking_lot::Mutex;
use uuid::Uuid;

use crate::service::workflow::spec::WorkflowSpec;

use super::{
    Store,
    task_instance::{TaskInstanceRow, TaskLogRow},
    workflow::{WorkflowRow, WorkflowWithLastRun},
    workflow_run::WorkflowRunRow,
};

/// In-memory tables backing [`MemoryStore`].
#[derive(Default)]
struct MemData {
    workflows: HashMap<String, WorkflowRow>,
    workflow_runs: HashMap<Uuid, WorkflowRunRow>,
    task_instances: HashMap<Uuid, TaskInstanceRow>,
    task_logs: Vec<TaskLogRow>,
    log_seq: i64,
}

/// Ephemeral in-memory store. A single mutex covers all tables so cross-table
/// operations (e.g. claiming a task and marking it running) stay atomic.
pub struct MemoryStore {
    data: Mutex<MemData>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            data: Mutex::new(MemData::default()),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl super::WorkflowStore for MemoryStore {
    async fn upsert_workflow(&self, yaml: &str, spec: &WorkflowSpec) -> anyhow::Result<()> {
        let spec_json = serde_json::to_value(spec)?;
        let mut m = self.data.lock();
        let now = chrono::Utc::now();
        match m.workflows.get_mut(&spec.id) {
            // ON CONFLICT DO UPDATE: keep is_active and created_at.
            Some(existing) => {
                existing.yaml_source = yaml.to_owned();
                existing.spec = spec_json;
                existing.updated_at = now;
            }
            None => {
                m.workflows.insert(
                    spec.id.clone(),
                    WorkflowRow {
                        workflow_id: spec.id.clone(),
                        yaml_source: yaml.to_owned(),
                        spec: spec_json,
                        is_active: true,
                        created_at: now,
                        updated_at: now,
                    },
                );
            }
        }
        Ok(())
    }

    async fn list_workflows(&self) -> anyhow::Result<Vec<WorkflowWithLastRun>> {
        let m = self.data.lock();
        let mut workflows: Vec<&WorkflowRow> = m.workflows.values().collect();
        workflows.sort_by(|a, b| a.workflow_id.cmp(&b.workflow_id));
        let rows = workflows
            .into_iter()
            .map(|d| {
                let last = m
                    .workflow_runs
                    .values()
                    .filter(|r| r.workflow_id == d.workflow_id)
                    .max_by_key(|r| r.created_at);
                WorkflowWithLastRun {
                    workflow_id: d.workflow_id.clone(),
                    is_active: d.is_active,
                    last_run_state: last.map(|r| r.state.clone()),
                    last_run_at: last.map(|r| r.created_at),
                }
            })
            .collect();
        Ok(rows)
    }

    async fn get_workflow(&self, workflow_id: &str) -> anyhow::Result<Option<WorkflowRow>> {
        Ok(self.data.lock().workflows.get(workflow_id).cloned())
    }

    async fn set_workflow_active(&self, workflow_id: &str, active: bool) -> anyhow::Result<bool> {
        let mut m = self.data.lock();
        match m.workflows.get_mut(workflow_id) {
            Some(d) => {
                d.is_active = active;
                d.updated_at = chrono::Utc::now();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn list_active_workflows(&self) -> anyhow::Result<Vec<WorkflowRow>> {
        Ok(self
            .data
            .lock()
            .workflows
            .values()
            .filter(|d| d.is_active)
            .cloned()
            .collect())
    }

    async fn workflow_exists(&self, workflow_id: &str) -> anyhow::Result<bool> {
        Ok(self.data.lock().workflows.contains_key(workflow_id))
    }
}

#[async_trait::async_trait]
impl super::WorkflowRunStore for MemoryStore {
    async fn create_run(
        &self,
        workflow_id: &str,
        logical_date: chrono::DateTime<chrono::Utc>,
        run_type: &str,
    ) -> anyhow::Result<Option<Uuid>> {
        let mut m = self.data.lock();
        let exists = m
            .workflow_runs
            .values()
            .any(|r| r.workflow_id == workflow_id && r.logical_date == logical_date);
        if exists {
            return Ok(None);
        }
        let id = Uuid::new_v4();
        m.workflow_runs.insert(
            id,
            WorkflowRunRow {
                id,
                workflow_id: workflow_id.to_owned(),
                logical_date,
                state: "queued".to_owned(),
                run_type: run_type.to_owned(),
                started_at: None,
                finished_at: None,
                created_at: chrono::Utc::now(),
            },
        );
        Ok(Some(id))
    }

    async fn get_run(&self, run_id: Uuid) -> anyhow::Result<Option<WorkflowRunRow>> {
        Ok(self.data.lock().workflow_runs.get(&run_id).cloned())
    }

    async fn list_runs_for_workflow(
        &self,
        workflow_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<WorkflowRunRow>> {
        let m = self.data.lock();
        let mut rows: Vec<WorkflowRunRow> = m
            .workflow_runs
            .values()
            .filter(|r| r.workflow_id == workflow_id)
            .cloned()
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.logical_date));
        rows.truncate(limit.max(0) as usize);
        Ok(rows)
    }

    async fn max_run_logical_date(
        &self,
        workflow_id: &str,
    ) -> anyhow::Result<Option<chrono::DateTime<chrono::Utc>>> {
        Ok(self
            .data
            .lock()
            .workflow_runs
            .values()
            .filter(|r| r.workflow_id == workflow_id)
            .map(|r| r.logical_date)
            .max())
    }

    async fn set_run_state(&self, run_id: Uuid, state: &str) -> anyhow::Result<()> {
        let finished = matches!(state, "success" | "failed");
        let mut m = self.data.lock();
        if let Some(r) = m.workflow_runs.get_mut(&run_id) {
            r.state = state.to_owned();
            if finished {
                r.finished_at = Some(chrono::Utc::now());
            }
        }
        Ok(())
    }

    async fn mark_run_running(&self, run_id: Uuid) -> anyhow::Result<()> {
        let mut m = self.data.lock();
        if let Some(r) = m.workflow_runs.get_mut(&run_id)
            && r.state == "queued"
        {
            r.state = "running".to_owned();
            r.started_at = Some(chrono::Utc::now());
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl super::TaskStore for MemoryStore {
    async fn create_task_batch(
        &self,
        run_id: Uuid,
        tasks: &[(&str, &str, i32)],
    ) -> anyhow::Result<()> {
        let mut m = self.data.lock();
        let now = chrono::Utc::now();
        for (task_id, state, max_retries) in tasks {
            let exists = m
                .task_instances
                .values()
                .any(|t| t.workflow_run_id == run_id && t.task_id == *task_id);
            if exists {
                continue;
            }
            let id = Uuid::new_v4();
            m.task_instances.insert(
                id,
                TaskInstanceRow {
                    id,
                    workflow_run_id: run_id,
                    task_id: (*task_id).to_owned(),
                    state: (*state).to_owned(),
                    try_number: 0,
                    max_retries: *max_retries,
                    started_at: None,
                    finished_at: None,
                    exit_code: None,
                    error: None,
                    created_at: now,
                },
            );
        }
        Ok(())
    }

    async fn list_tasks_for_run(&self, run_id: Uuid) -> anyhow::Result<Vec<TaskInstanceRow>> {
        let m = self.data.lock();
        let mut rows: Vec<TaskInstanceRow> = m
            .task_instances
            .values()
            .filter(|t| t.workflow_run_id == run_id)
            .cloned()
            .collect();
        rows.sort_by_key(|t| t.created_at);
        Ok(rows)
    }

    async fn get_task(&self, id: Uuid) -> anyhow::Result<Option<TaskInstanceRow>> {
        Ok(self.data.lock().task_instances.get(&id).cloned())
    }

    async fn claim_and_mark_running(&self, limit: i64) -> anyhow::Result<Vec<TaskInstanceRow>> {
        let mut m = self.data.lock();
        let mut ids: Vec<(chrono::DateTime<chrono::Utc>, Uuid)> = m
            .task_instances
            .values()
            .filter(|t| t.state == "queued")
            .map(|t| (t.created_at, t.id))
            .collect();
        ids.sort_by_key(|(created, _)| *created);
        ids.truncate(limit.max(0) as usize);

        let now = chrono::Utc::now();
        let mut claimed = Vec::with_capacity(ids.len());
        for (_, id) in ids {
            if let Some(t) = m.task_instances.get_mut(&id) {
                t.state = "running".to_owned();
                t.started_at = Some(now);
                claimed.push(t.clone());
            }
        }
        Ok(claimed)
    }

    async fn mark_task_running(&self, id: Uuid) -> anyhow::Result<()> {
        if let Some(t) = self.data.lock().task_instances.get_mut(&id) {
            t.state = "running".to_owned();
            t.started_at = Some(chrono::Utc::now());
        }
        Ok(())
    }

    async fn mark_task_success(&self, id: Uuid, exit_code: i32) -> anyhow::Result<()> {
        if let Some(t) = self.data.lock().task_instances.get_mut(&id) {
            t.state = "success".to_owned();
            t.exit_code = Some(exit_code);
            t.finished_at = Some(chrono::Utc::now());
        }
        Ok(())
    }

    async fn mark_task_failed(
        &self,
        id: Uuid,
        exit_code: Option<i32>,
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(t) = self.data.lock().task_instances.get_mut(&id) {
            t.state = "failed".to_owned();
            t.exit_code = exit_code;
            t.error = error.map(str::to_owned);
            t.finished_at = Some(chrono::Utc::now());
        }
        Ok(())
    }

    async fn requeue_task(&self, id: Uuid) -> anyhow::Result<()> {
        if let Some(t) = self.data.lock().task_instances.get_mut(&id) {
            t.state = "queued".to_owned();
            t.try_number += 1;
            t.started_at = None;
            t.finished_at = None;
            t.exit_code = None;
            t.error = None;
        }
        Ok(())
    }

    async fn set_task_state(&self, id: Uuid, state: &str) -> anyhow::Result<()> {
        if let Some(t) = self.data.lock().task_instances.get_mut(&id) {
            t.state = state.to_owned();
        }
        Ok(())
    }

    async fn set_task_state_for(
        &self,
        run_id: Uuid,
        task_id: &str,
        state: &str,
    ) -> anyhow::Result<()> {
        if let Some(t) = self
            .data
            .lock()
            .task_instances
            .values_mut()
            .find(|t| t.workflow_run_id == run_id && t.task_id == task_id)
        {
            t.state = state.to_owned();
        }
        Ok(())
    }

    async fn task_state_map(&self, run_id: Uuid) -> anyhow::Result<HashMap<String, String>> {
        Ok(self
            .data
            .lock()
            .task_instances
            .values()
            .filter(|t| t.workflow_run_id == run_id)
            .map(|t| (t.task_id.clone(), t.state.clone()))
            .collect())
    }

    async fn insert_task_log(
        &self,
        task_instance_id: Uuid,
        try_number: i32,
        stream: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let mut m = self.data.lock();
        m.log_seq += 1;
        let id = m.log_seq;
        m.task_logs.push(TaskLogRow {
            id,
            task_instance_id,
            try_number,
            stream: stream.to_owned(),
            content: content.to_owned(),
            created_at: chrono::Utc::now(),
        });
        Ok(())
    }

    async fn get_task_logs(&self, task_instance_id: Uuid) -> anyhow::Result<Vec<TaskLogRow>> {
        let m = self.data.lock();
        let mut rows: Vec<TaskLogRow> = m
            .task_logs
            .iter()
            .filter(|l| l.task_instance_id == task_instance_id)
            .cloned()
            .collect();
        rows.sort_by_key(|l| l.id);
        Ok(rows)
    }
}

#[async_trait::async_trait]
impl Store for MemoryStore {
    async fn ping(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
