use std::{sync::Arc, time::Duration};

use anyhow::Result;
use bytes::Bytes;
use wasmtime::{
    Config, Engine, Store, StoreLimitsBuilder,
    component::{Component, Linker, ResourceTable},
};
use wasmtime_wasi::{
    WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView,
    p2::{bindings::Command, pipe::MemoryOutputPipe},
};

/// Outcome of running a WASM component.
pub struct RunOutcome {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub stdout: Bytes,
    pub stderr: Bytes,
}

struct RunState {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: wasmtime::StoreLimits,
}

impl WasiView for RunState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

pub struct Runner {
    engine: Engine,
    linker: Arc<Linker<RunState>>,
}

// Max bytes captured per stream to prevent OOM from verbose components.
const MAX_LOG_BYTES: usize = 4 * 1024 * 1024; // 4 MiB

impl Runner {
    pub fn new() -> Result<Self> {
        let mut cfg = Config::new();
        cfg.wasm_component_model(true);
        cfg.epoch_interruption(true);

        let engine = Engine::new(&cfg)?;

        let mut linker: Linker<RunState> = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;

        // Background thread increments epoch every 100 ms.
        // set_epoch_deadline(N) = interrupt after N × 100 ms.
        let engine_tick = engine.clone();
        std::thread::Builder::new()
            .name("epoch-ticker".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(100));
                    engine_tick.increment_epoch();
                }
            })?;

        Ok(Self {
            engine,
            linker: Arc::new(linker),
        })
    }

    pub async fn run(
        &self,
        wasm: &[u8],
        timeout_seconds: u64,
        memory_mb: u64,
    ) -> Result<RunOutcome> {
        let stdout_pipe = MemoryOutputPipe::new(MAX_LOG_BYTES);
        let stderr_pipe = MemoryOutputPipe::new(MAX_LOG_BYTES);

        let mut builder = WasiCtxBuilder::new();
        builder.stdout(stdout_pipe.clone());
        builder.stderr(stderr_pipe.clone());
        let wasi = builder.build();

        let limits = StoreLimitsBuilder::new()
            .memory_size((memory_mb * 1024 * 1024) as usize)
            .build();

        let state = RunState {
            wasi,
            table: ResourceTable::new(),
            limits,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);

        // 10 ticks/sec; each tick = 100 ms.
        store.set_epoch_deadline(timeout_seconds * 10);

        let component = Component::from_binary(&self.engine, wasm)
            .map_err(|e| anyhow::anyhow!("Failed to compile WASM component: {e}"))?;

        let cmd = Command::instantiate_async(&mut store, &component, &self.linker)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to instantiate WASM component: {e}"))?;

        let result = cmd.wasi_cli_run().call_run(&mut store).await;

        let stdout = stdout_pipe.contents();
        let stderr = stderr_pipe.contents();

        match result {
            Ok(Ok(())) => Ok(RunOutcome {
                success: true,
                exit_code: Some(0),
                error: None,
                stdout,
                stderr,
            }),
            Ok(Err(())) => Ok(RunOutcome {
                success: false,
                exit_code: Some(1),
                error: Some("Component exited with error".into()),
                stdout,
                stderr,
            }),
            Err(e) => {
                let msg = e.to_string();
                let is_timeout = msg.contains("epoch") || msg.contains("interrupt");
                Ok(RunOutcome {
                    success: false,
                    exit_code: None,
                    error: Some(if is_timeout {
                        "Task exceeded timeout".into()
                    } else {
                        format!("Trap: {msg}")
                    }),
                    stdout,
                    stderr,
                })
            }
        }
    }
}
