use std::{sync::Arc, time::Duration};

use tokio_util::sync::CancellationToken;
use wasmtime::error::Context as _;

const DEFAULT_EPOCH_DEADLINE_SECS: u64 = 30;
const DEFAULT_EPOCH_PRECISION_MS: u64 = 10;

/// Wrapper around [`wasmtime::Engine`] with project-specific defaults.
#[derive(Clone, Debug)]
pub struct Engine {
    inner: wasmtime::Engine,
    component_cache: super::cache::ComponentCache,
    deadline: Duration,
    precision: Duration,
    _epoch_ticker: Arc<EpochTicker>,
}

impl Engine {
    /// Creates a new engine with async support, epoch-based interruption, and optional pooling allocator.
    pub fn new() -> anyhow::Result<Self> {
        let mut config = wasmtime::Config::new();
        config.epoch_interruption(true);

        if let Ok(Some(true)) = use_pooling_allocator_by_default() {
            let pool_config = wasmtime::PoolingAllocationConfig::new();
            config.allocation_strategy(wasmtime::InstanceAllocationStrategy::Pooling(pool_config));
        }

        let deadline = Duration::from_secs(DEFAULT_EPOCH_DEADLINE_SECS);
        let precision = Duration::from_millis(DEFAULT_EPOCH_PRECISION_MS);

        let engine = wasmtime::Engine::new(&config)?;
        let epoch_ticker = EpochTicker::new(engine.clone(), precision);

        Ok(Self {
            inner: engine,
            component_cache: super::cache::ComponentCache::new(),
            deadline,
            precision,
            _epoch_ticker: Arc::new(epoch_ticker),
        })
    }

    pub(crate) fn wasmtime_engine(&self) -> &wasmtime::Engine {
        &self.inner
    }

    pub(crate) fn new_linker(&self) -> wasmtime::component::Linker<super::ctx::Context> {
        wasmtime::component::Linker::new(&self.inner)
    }

    pub(crate) fn new_component(
        &self,
        bytes: &[u8],
        digest: Option<String>,
    ) -> anyhow::Result<wasmtime::component::Component> {
        if let Some(ref digest) = digest
            && let Some(cached) = self.component_cache.get(digest)
        {
            return Ok(cached);
        }

        let component = wasmtime::component::Component::new(&self.inner, bytes)
            .context("failed to compile WebAssembly component")?;

        if let Some(digest) = digest {
            self.component_cache.insert(digest, component.clone());
        }

        Ok(component)
    }

    pub(crate) fn new_store(
        &self,
        data: super::ctx::Context,
    ) -> anyhow::Result<wasmtime::Store<super::ctx::Context>> {
        let mut store = wasmtime::Store::new(&self.inner, data);
        let ticks = self.deadline.as_millis() / self.precision.as_millis().max(1);
        store.set_epoch_deadline(ticks as u64);

        Ok(store)
    }
}

#[derive(Debug)]
struct EpochTicker {
    cancel: CancellationToken,
}

impl EpochTicker {
    fn new(engine: wasmtime::Engine, precision: Duration) -> Self {
        let cancel = CancellationToken::new();
        let token = cancel.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = tokio::time::sleep(precision) => {
                        engine.increment_epoch();
                    }
                }
            }
        });

        Self { cancel }
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

fn use_pooling_allocator_by_default() -> anyhow::Result<Option<bool>> {
    const BITS_TO_TEST: u32 = 42;
    let mut config = wasmtime::Config::new();
    config.wasm_memory64(true);
    config.memory_reservation(1 << BITS_TO_TEST);
    let engine = wasmtime::Engine::new(&config)?;
    let mut store = wasmtime::Store::new(&engine, ());
    // NB: the maximum size is in wasm pages to take out the 16-bits of wasm
    // page size here from the maximum size.
    let ty = wasmtime::MemoryType::new64(0, Some(1 << (BITS_TO_TEST - 16)));
    if wasmtime::Memory::new(&mut store, ty).is_ok() {
        Ok(Some(true))
    } else {
        Ok(None)
    }
}
