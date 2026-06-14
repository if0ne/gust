use std::sync::Arc;

use crate::infra::store::Store;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: Arc<dyn Store>,
}

impl AppState {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }
}
