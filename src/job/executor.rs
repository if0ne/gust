use std::{sync::Arc, time::Duration};

use anyhow::Result;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    infra::store::{Store, task_instance::TaskInstanceRow},
    service::{
        runtime::{image::Resolver, runner::Runner, types::ComponentDesc},
        workflow::{graph, spec::WorkflowSpec},
    },
};

pub struct Executor {
    store: Arc<dyn Store>,
    runner: Arc<Runner>,
    resolver: Arc<dyn Resolver>,
    semaphore: Arc<tokio::sync::Semaphore>,
}

impl Executor {
    pub fn new(
        store: Arc<dyn Store>,
        runner: Arc<Runner>,
        resolver: Arc<dyn Resolver>,
        max_concurrency: usize,
    ) -> Self {
        Self {
            store,
            runner,
            resolver,
            semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrency)),
        }
    }

    pub async fn run(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Err(e) = Arc::clone(&self).tick().await {
                error!("Executor tick error: {e:#}");
            }
        }
    }

    async fn tick(self: Arc<Self>) -> Result<()> {
        // Claim up to semaphore capacity worth of tasks.
        let available = self.semaphore.available_permits();
        if available == 0 {
            return Ok(());
        }

        let tasks = self.store.claim_and_mark_running(available as i64).await?;
        if tasks.is_empty() {
            return Ok(());
        }

        for task in tasks {
            let executor = Arc::clone(&self);
            let permit = Arc::clone(&self.semaphore).acquire_owned().await.unwrap();
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(e) = executor.run_task(task).await {
                    error!("Task execution error: {e:#}");
                }
            });
        }

        Ok(())
    }

    async fn run_task(&self, task: TaskInstanceRow) -> Result<()> {
        info!(task_id = %task.task_id, instance_id = %task.id, "Starting task");

        // Load workflow spec to find the task's component.
        let run = self
            .store
            .get_run(task.workflow_run_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("workflow_run {} not found", task.workflow_run_id))?;

        let workflow_row = self
            .store
            .get_workflow(&run.workflow_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("workflow {} not found", run.workflow_id))?;

        let spec: WorkflowSpec = serde_json::from_value(workflow_row.spec)?;
        let task_spec = spec
            .tasks
            .iter()
            .find(|t| t.id == task.task_id)
            .ok_or_else(|| anyhow::anyhow!("task {} not in workflow spec", task.task_id))?;

        // Resolve the component image and load it into the run's workload, so
        // the runner holds a pre-linked component keyed by task id.
        let load = async {
            let image = self.resolver.resolve(&task_spec.component.image).await?;
            let desc = ComponentDesc {
                name: task_spec.component.name.clone(),
                bytes: image.bytes.to_vec(),
                digest: Some(image.digest),
                extensions: task_spec.component.extensions.clone(),
            };
            self.runner
                .load_component(&run.workflow_id, &task.task_id, desc)
                .await
        };

        if let Err(e) = load.await {
            let msg = format!("Component load error: {e:#}");
            warn!(%msg);
            self.store
                .mark_task_failed(task.id, None, Some(&msg))
                .await?;
            self.advance_graph(&spec, task.workflow_run_id).await?;
            return Ok(());
        }

        // Run the WASM component.
        let outcome = self.runner.run(&run.workflow_id, &task.task_id).await?;

        // Persist logs.
        let stdout_str = String::from_utf8_lossy(&outcome.stdout);
        let stderr_str = String::from_utf8_lossy(&outcome.stderr);
        if !stdout_str.is_empty() {
            self.store
                .insert_task_log(task.id, task.try_number, "stdout", &stdout_str)
                .await?;
        }
        if !stderr_str.is_empty() {
            self.store
                .insert_task_log(task.id, task.try_number, "stderr", &stderr_str)
                .await?;
        }

        if outcome.success {
            info!(task_id = %task.task_id, "Task succeeded");
            self.store
                .mark_task_success(task.id, outcome.exit_code.unwrap_or(0))
                .await?;
        } else if (task.try_number as u32) < task.max_retries as u32 {
            info!(task_id = %task.task_id, try_number = task.try_number, "Retrying task");
            self.store.requeue_task(task.id).await?;
            return Ok(()); // Don't advance graph yet — task will run again.
        } else {
            warn!(task_id = %task.task_id, error = ?outcome.error, "Task failed");
            self.store
                .mark_task_failed(task.id, outcome.exit_code, outcome.error.as_deref())
                .await?;
        }

        self.advance_graph(&spec, task.workflow_run_id).await?;
        Ok(())
    }

    /// Recompute pending → queued / upstream_failed, and finalize workflow_run if done.
    async fn advance_graph(&self, spec: &WorkflowSpec, run_id: Uuid) -> Result<()> {
        let states = self.store.task_state_map(run_id).await?;
        let ready: Vec<String> = spec.ready_tasks(&states).map(str::to_owned).collect();
        for task_id in ready {
            self.store
                .set_task_state_for(run_id, &task_id, "queued")
                .await?;
        }

        let states = self.store.task_state_map(run_id).await?;
        let upstream_failed: Vec<String> = spec
            .upstream_failed_tasks(&states)
            .map(str::to_owned)
            .collect();
        for task_id in upstream_failed {
            self.store
                .set_task_state_for(run_id, &task_id, "upstream_failed")
                .await?;
        }

        // Re-read states (may have changed) and check if workflow_run is done.
        let states = self.store.task_state_map(run_id).await?;
        if graph::all_terminal(&states) {
            let run_state = if states.values().all(|s| s == "success") {
                "success"
            } else {
                "failed"
            };
            self.store.set_run_state(run_id, run_state).await?;
            info!(%run_id, state = run_state, "workflow run finished");
        } else {
            // Ensure run is marked running if it was still queued.
            self.store.mark_run_running(run_id).await?;
        }

        Ok(())
    }
}
