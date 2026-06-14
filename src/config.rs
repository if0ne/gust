use std::{net::SocketAddr, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    /// Postgres connection string. `None` selects the ephemeral in-memory store
    /// (when `DATABASE_URL` is unset/empty or set to `memory`).
    pub database_url: Option<String>,
    /// Base directory used to resolve relative local component paths in workflow specs.
    pub workflow_base_dir: PathBuf,
    pub bind_addr: SocketAddr,
    pub executor_max_concurrency: usize,
    pub component_cache_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            // Unset/empty or "memory"/"mem" → in-memory store, so the app runs
            // with zero infrastructure. Any other value is a Postgres URL.
            database_url: match std::env::var("DATABASE_URL") {
                Ok(url)
                    if !url.is_empty()
                        && !url.eq_ignore_ascii_case("memory")
                        && !url.eq_ignore_ascii_case("mem") =>
                {
                    Some(url)
                }
                _ => None,
            },
            workflow_base_dir: std::env::var("WORKFLOW_BASE_DIR")
                .map(PathBuf::from)
                // In dev (cargo run) use the project root; in production use CWD.
                .unwrap_or_else(|_| {
                    std::env::var("CARGO_MANIFEST_DIR")
                        .map(PathBuf::from)
                        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default())
                }),
            bind_addr: std::env::var("BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".into())
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid BIND_ADDR: {e}"))?,
            executor_max_concurrency: std::env::var("EXECUTOR_MAX_CONCURRENCY")
                .unwrap_or_else(|_| "4".into())
                .parse()?,
            component_cache_dir: std::env::var("COMPONENT_CACHE_DIR")
                .unwrap_or_else(|_| "./.cache/components".into())
                .into(),
        })
    }
}
