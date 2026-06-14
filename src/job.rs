pub mod executor;

use anyhow::Result;
use uuid::Uuid;

use crate::{infra::store::Store, service::workflow::spec::WorkflowSpec};

/// Create the `task_instance` rows for a workflow run from its stored spec.
/// Root tasks (no `depends_on`) start `queued`; the rest start `pending`.
pub async fn materialize_tasks(store: &dyn Store, run_id: Uuid, workflow_id: &str) -> Result<()> {
    let workflow_row = store
        .get_workflow(workflow_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("workflow {workflow_id} not found"))?;
    let spec: WorkflowSpec = serde_json::from_value(workflow_row.spec)?;

    let tasks: Vec<(&str, &str, i32)> = spec
        .tasks
        .iter()
        .map(|t| {
            let state = if t.depends_on.is_empty() {
                "queued"
            } else {
                "pending"
            };
            (t.id.as_str(), state, 0i32)
        })
        .collect();

    store.create_task_batch(run_id, &tasks).await?;
    Ok(())
}
