use std::{fmt, time::Duration};

use moka::sync::Cache;

const DEFAULT_MAX_CAPACITY: u64 = 256;
const DEFAULT_TIME_TO_IDLE: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
pub(super) struct ComponentCache {
    inner: Cache<String, wasmtime::component::Component>,
}

impl fmt::Debug for ComponentCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentCache")
            .field("entry_count", &self.inner.entry_count())
            .finish()
    }
}

impl ComponentCache {
    pub(super) fn new() -> Self {
        Self {
            inner: Cache::builder()
                .max_capacity(DEFAULT_MAX_CAPACITY)
                .time_to_idle(DEFAULT_TIME_TO_IDLE)
                .build(),
        }
    }

    pub(super) fn get(&self, digest: &str) -> Option<wasmtime::component::Component> {
        self.inner.get(digest)
    }

    pub(super) fn insert(&self, digest: String, component: wasmtime::component::Component) {
        self.inner.insert(digest, component);
    }
}
