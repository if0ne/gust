use std::{collections::HashMap, sync::Arc};

use anyhow::Context as _;
use bytes::Bytes;
use parking_lot::RwLock;
use wasmtime_wasi::p2::{bindings::CommandPre, pipe::MemoryOutputPipe};

use super::{
    component::UnresolvedComponent,
    ctx::ContextBuilder,
    engine::Engine,
    extension::Extension,
    resolved::ResolvedComponent,
    types::{ComponentDesc, ComponentId, ExtensionId},
};

/// Outcome of running a WASM component.
pub(crate) struct RunOutcome {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub stdout: Bytes,
    pub stderr: Bytes,
}

// Max bytes captured per stream to prevent OOM from verbose components.
const MAX_LOG_BYTES: usize = 4 * 1024 * 1024; // 4 MiB

/// The resolved components of a single workflow, keyed by task id
/// (the component id within the workload).
#[derive(Default)]
struct Workload {
    components: HashMap<String, ResolvedComponent>,
}

/// Runs WebAssembly components against a single shared [`Engine`].
///
/// The engine — and its compiled-component cache — is shared across every
/// workflow. Each workload keeps its resolved components around so repeated
/// runs reuse the pre-linked component instead of recompiling.
pub(crate) struct Runner {
    engine: Engine,
    extensions: HashMap<ExtensionId, Arc<dyn Extension>>,
    workloads: RwLock<HashMap<String, Workload>>,
}

impl Runner {
    pub(crate) async fn start(&self) -> anyhow::Result<()> {
        let mut errors = Vec::new();

        for extension in self.extensions.values() {
            if let Err(err) = extension.start().await {
                errors.push(format!("extension '{}': {err:#}", extension.name()));
            }
        }

        if !errors.is_empty() {
            anyhow::bail!(
                "{} extensions and triggers failed to start:\n{}",
                errors.len(),
                errors.join("\n")
            );
        }

        log::info!("Runtime started");

        Ok(())
    }

    pub(crate) async fn stop(&self) -> anyhow::Result<()> {
        let mut errors = Vec::new();

        for extension in self.extensions.values() {
            if let Err(err) = extension.stop().await {
                errors.push(format!("extension '{}': {err:#}", extension.name()));
            }
        }

        if !errors.is_empty() {
            anyhow::bail!(
                "{} extension(s) failed to stop:\n{}",
                errors.len(),
                errors.join("\n")
            );
        }

        log::info!("Runtime stopped");

        Ok(())
    }

    /// Compiles and resolves `desc`, storing it under `workload_id` keyed by
    /// `component_id`. Idempotent — reloading overwrites the previous entry.
    ///
    /// Compilation itself is cached on the engine by content digest, so
    /// reloading an unchanged component is cheap.
    pub(crate) async fn load_component(
        &self,
        workload_id: &str,
        component_id: &str,
        desc: ComponentDesc,
    ) -> anyhow::Result<()> {
        let id = ComponentId::new(uuid::Uuid::new_v4());
        let resolved = UnresolvedComponent::new(
            id,
            self.engine.clone(),
            desc,
            UnresolvedComponent::extract_world,
        )
        .context("failed to load component")?
        .resolve(&self.extensions)
        .await?;

        self.workloads
            .write()
            .entry(workload_id.to_owned())
            .or_default()
            .components
            .insert(component_id.to_owned(), resolved);

        Ok(())
    }

    /// Returns true if `component_id` is loaded in `workload_id`.
    #[allow(dead_code)]
    pub(crate) fn has_component(&self, workload_id: &str, component_id: &str) -> bool {
        self.workloads
            .read()
            .get(workload_id)
            .is_some_and(|w| w.components.contains_key(component_id))
    }

    /// Drops a workload and every component it holds.
    #[allow(dead_code)]
    pub(crate) fn remove_workload(&self, workload_id: &str) {
        self.workloads.write().remove(workload_id);
    }

    /// Runs the `wasi:cli/run` export of a previously loaded component.
    pub(crate) async fn run(
        &self,
        workload_id: &str,
        component_id: &str,
    ) -> anyhow::Result<RunOutcome> {
        // `ResolvedComponent` is `Arc`-backed, so clone it out of the lock and
        // run without holding the registry locked for the task's lifetime.
        let component = self
            .workloads
            .read()
            .get(workload_id)
            .and_then(|w| w.components.get(component_id).cloned())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "component '{component_id}' is not loaded in workload '{workload_id}'"
                )
            })?;

        self.run_component(&component).await
    }

    async fn run_component(&self, component: &ResolvedComponent) -> anyhow::Result<RunOutcome> {
        let stdout_pipe = MemoryOutputPipe::new(MAX_LOG_BYTES);
        let stderr_pipe = MemoryOutputPipe::new(MAX_LOG_BYTES);

        let mut ctx_builder = ContextBuilder::new(component.id().clone());
        ctx_builder.wasi_ctx_builder().stdout(stdout_pipe.clone());
        ctx_builder.wasi_ctx_builder().stderr(stderr_pipe.clone());
        for extension in component.extensions().values() {
            extension.configure_ctx(&mut ctx_builder)?;
        }

        let mut store = self.engine.new_store(ctx_builder.build())?;

        let instance_pre = component.instantiate_pre()?;
        let command_pre = CommandPre::new(instance_pre)
            .map_err(|e| anyhow::anyhow!("component does not export wasi:cli/run: {e}"))?;
        let command = command_pre
            .instantiate_async(&mut store)
            .await
            .map_err(|e| anyhow::anyhow!("failed to instantiate WASM component: {e}"))?;

        let result = command.wasi_cli_run().call_run(&mut store).await;

        let stdout = stdout_pipe.contents();
        let stderr = stderr_pipe.contents();

        let outcome = match result {
            Ok(Ok(())) => RunOutcome {
                success: true,
                exit_code: Some(0),
                error: None,
                stdout,
                stderr,
            },
            Ok(Err(())) => RunOutcome {
                success: false,
                exit_code: Some(1),
                error: Some("Component exited with error".into()),
                stdout,
                stderr,
            },
            Err(e) => {
                let msg = e.to_string();
                let is_timeout = msg.contains("epoch") || msg.contains("interrupt");
                RunOutcome {
                    success: false,
                    exit_code: None,
                    error: Some(if is_timeout {
                        "Task exceeded timeout".into()
                    } else {
                        format!("Trap: {msg}")
                    }),
                    stdout,
                    stderr,
                }
            }
        };

        Ok(outcome)
    }
}

#[derive(Default)]
pub(crate) struct RunnerBuilder {
    extensions: HashMap<ExtensionId, Arc<dyn Extension>>,
}

impl RunnerBuilder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub(crate) fn extension(mut self, extension: Arc<dyn Extension>) -> Self {
        self.extensions.insert(extension.id(), extension);
        self
    }

    pub(crate) fn build(self) -> anyhow::Result<Runner> {
        Ok(Runner {
            engine: Engine::new()?,
            extensions: self.extensions,
            workloads: Default::default(),
        })
    }
}
