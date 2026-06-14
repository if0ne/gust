// Scaffolding for the component/extension runtime; not all of this surface is
// wired into the executor yet.
#![allow(dead_code)]

use std::{collections::HashMap, fmt, sync::Arc};

use wasmtime::error::Context as _;

use super::{
    component::UnresolvedParts,
    ctx::Context,
    extension::Extension,
    types::{ComponentId, ExtensionId},
    wit::WitWorld,
};

/// A fully resolved WebAssembly component ready for instantiation.
///
/// Produced from an `UnresolvedComponent` by `super::binding`; holds the bound
/// extensions (`Arc<dyn Extension>`) but never references an `UnresolvedComponent`,
/// so this module depends on `extension` without re-introducing a cycle.
///
/// The linker is frozen and read-only. This type is cheaply cloneable via `Arc`.
#[derive(Clone)]
pub(super) struct ResolvedComponent {
    inner: Arc<ResolvedComponentInner>,
}

impl fmt::Debug for ResolvedComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedComponent")
            .field("id", &self.inner.id)
            .finish_non_exhaustive()
    }
}

impl ResolvedComponent {
    /// Freezes the parts of a bound component together with the extensions that
    /// were bound to it. Called by `super::binding` at the end of resolution.
    pub(super) fn new(
        parts: UnresolvedParts,
        extensions: HashMap<ExtensionId, Arc<dyn Extension>>,
    ) -> Self {
        Self {
            inner: Arc::new(ResolvedComponentInner {
                id: parts.id,
                component: parts.component,
                linker: parts.linker,
                name: parts.name,
                world: parts.world,
                extensions,
            }),
        }
    }

    pub(super) fn extensions(&self) -> &HashMap<ExtensionId, Arc<dyn Extension>> {
        &self.inner.extensions
    }

    /// Returns the component's unique identifier.
    pub(super) fn id(&self) -> &ComponentId {
        &self.inner.id
    }

    /// Returns the component's name.
    pub(super) fn name(&self) -> &str {
        &self.inner.name
    }

    /// Returns the world for this component.
    pub(super) fn world(&self) -> &WitWorld {
        &self.inner.world
    }

    /// Returns the wasmtime component for this component.
    pub(super) fn component(&self) -> &wasmtime::component::Component {
        &self.inner.component
    }

    /// Creates an `InstancePre` by pre-linking the component, ready for repeated instantiation.
    pub(super) fn instantiate_pre(
        &self,
    ) -> anyhow::Result<wasmtime::component::InstancePre<Context>> {
        let instance_pre = self
            .inner
            .linker
            .instantiate_pre(&self.inner.component)
            .context("failed to instantiate pre component")?;

        Ok(instance_pre)
    }
}

struct ResolvedComponentInner {
    id: ComponentId,
    component: wasmtime::component::Component,
    linker: wasmtime::component::Linker<Context>,
    name: String,
    world: WitWorld,
    extensions: HashMap<ExtensionId, Arc<dyn Extension>>,
}
