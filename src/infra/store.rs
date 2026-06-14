pub mod in_memory;
pub mod postgres;
pub mod task_instance;
pub mod workflow;
pub mod workflow_run;

use anyhow::Result;

pub use task_instance::TaskStore;
pub use workflow::WorkflowStore;
pub use workflow_run::WorkflowRunStore;

/// Backend-agnostic state store — the persistence interface the rest of the app
/// depends on (used behind `Arc<dyn Store>`). The per-entity operations live on
/// the [`WorkflowStore`], [`WorkflowRunStore`], and [`TaskStore`] supertraits.
///
/// Implemented by [`PostgresStore`] (durable, used in production) and
/// [`MemoryStore`] (in-process, ephemeral — lets the app run with no external
/// infrastructure, handy for local dev with no `DATABASE_URL`).
#[async_trait::async_trait]
pub trait Store: WorkflowStore + WorkflowRunStore + TaskStore + Send + Sync {
    /// Readiness probe: verify the backend is reachable. The in-memory backend
    /// is always ready; Postgres is pinged with a trivial query.
    async fn ping(&self) -> Result<()>;
}
