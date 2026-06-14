mod config;
mod handler;
mod infra;
mod job;
mod service;

use std::sync::Arc;

use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, fmt};

use crate::{
    handler::{router::base_router, state::AppState},
    infra::store::{Store, in_memory::MemoryStore, postgres::PostgresStore},
    job::executor::Executor,
    service::runtime::{
        resolver::{DefaultResolver, Resolver},
        runner::RunnerBuilder,
    },
};

#[tokio::main]
async fn main() -> Result<()> {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let cfg = config::Config::from_env()?;

    let cwd = std::env::current_dir().unwrap_or_default();
    info!(cwd = %cwd.display(), "Working directory");

    let store: Arc<dyn Store> = match &cfg.database_url {
        Some(url) => {
            info!("Connecting to database…");
            let pool = PgPoolOptions::new()
                .max_connections(10)
                .connect(url)
                .await?;
            sqlx::migrate!("./migrations").run(&pool).await?;
            info!("Migrations applied");
            Arc::new(PostgresStore::new(pool))
        }
        None => {
            warn!(
                "No DATABASE_URL set — using the in-memory store. All state is EPHEMERAL and lost on exit."
            );
            Arc::new(MemoryStore::new())
        }
    };

    let runner = RunnerBuilder::new().build()?;
    runner.start().await?;

    let runner = Arc::new(runner);
    info!(base_dir = %cfg.workflow_base_dir.display(), "Local component base directory");
    let resolver: Arc<dyn Resolver> = Arc::new(DefaultResolver::new(
        &cfg.component_cache_dir,
        &cfg.workflow_base_dir,
    ));

    let executor = Arc::new(Executor::new(
        Arc::clone(&store),
        Arc::clone(&runner),
        Arc::clone(&resolver),
        cfg.executor_max_concurrency,
    ));

    // Spawn the executor loop. Workflows run only when triggered via the API.
    {
        let exec = Arc::clone(&executor);
        tokio::spawn(async move { exec.run().await });
    }

    let app = base_router(AppState::new(store));
    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
    info!(addr = %cfg.bind_addr, "HTTP server listening");
    axum::serve(listener, app).await?;

    runner.stop().await?;

    Ok(())
}
