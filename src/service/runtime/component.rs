// Scaffolding for the component/extension runtime; not all of this surface is
// wired into the executor yet.
#![allow(dead_code)]

use std::{collections::HashSet, fmt};

use anyhow::Context as _;
use wasmtime::error::Context as _;

use super::{
    ctx::Context,
    engine::Engine,
    types::{ComponentDesc, ComponentId, InterfaceConfigMap},
    wit::{WitInterface, WitWorld},
};

/// A WebAssembly component that has not yet been fully resolved against its dependencies.
///
/// This type knows only how to compile a component and describe its WIT world; it
/// is deliberately unaware of [`super::extension::Extension`]. Binding extensions
/// to it — and producing a [`super::binding::ResolvedComponent`] — is the job of
/// [`super::binding`], which keeps `component` and `extension` free of a cycle.
pub(crate) struct UnresolvedComponent {
    id: ComponentId,
    component: wasmtime::component::Component,
    linker: wasmtime::component::Linker<Context>,
    world: WitWorld,
    name: String,
    extensions_config: InterfaceConfigMap,
}

impl fmt::Debug for UnresolvedComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnresolvedComponent")
            .field("id", &self.id)
            .field("world", &self.world)
            .finish_non_exhaustive()
    }
}

impl UnresolvedComponent {
    /// Creates a new unresolved component by compiling the provided description and extracting its WIT world.
    pub(super) fn new(
        id: ComponentId,
        engine: Engine,
        component_desc: ComponentDesc,
        world_extractor: impl FnOnce(
            &wasmtime::component::Component,
            &Engine,
        ) -> anyhow::Result<WitWorld>,
    ) -> anyhow::Result<Self> {
        let mut linker = engine.new_linker();
        let component = engine
            .new_component(&component_desc.bytes, component_desc.digest)
            .context("failed to compile component")?;

        wasmtime_wasi::p2::add_to_linker_async(&mut linker)
            .context("failed to link WASI to component")?;

        let world = world_extractor(&component, &engine).context("failed to extract WIT world")?;

        Ok(Self {
            id,
            component,
            linker,
            world,
            name: component_desc.name,
            extensions_config: component_desc.extensions,
        })
    }

    pub(super) fn extract_world(
        component: &wasmtime::component::Component,
        engine: &Engine,
    ) -> anyhow::Result<WitWorld> {
        let ty = component.component_type();
        let import_iter = ty.imports(engine.wasmtime_engine());
        let mut imports = HashSet::default();
        for (name, item) in import_iter {
            if matches!(
                item,
                wasmtime::component::types::ComponentItem::ComponentInstance(_)
            ) {
                imports.insert(name.parse::<WitInterface>().expect("infallible"));
            }
        }

        let export_iter = ty.exports(engine.wasmtime_engine());
        let mut exports = HashSet::default();
        for (name, item) in export_iter {
            if matches!(
                item,
                wasmtime::component::types::ComponentItem::ComponentInstance(_)
            ) {
                exports.insert(name.parse::<WitInterface>().expect("infallible"));
            }
        }

        Ok(WitWorld { imports, exports })
    }

    /// Returns the component's unique identifier.
    pub(crate) fn id(&self) -> &ComponentId {
        &self.id
    }

    /// Returns a reference to the component
    pub(crate) fn component(&self) -> &wasmtime::component::Component {
        &self.component
    }

    /// Returns a reference to the component's linker.
    pub(crate) fn linker(&self) -> &wasmtime::component::Linker<Context> {
        &self.linker
    }

    /// Returns a mutable reference to the component's linker.
    pub(crate) fn linker_mut(&mut self) -> &mut wasmtime::component::Linker<Context> {
        &mut self.linker
    }

    /// Returns the component's extracted WIT world describing its imports and exports.
    pub(crate) fn world(&self) -> &WitWorld {
        &self.world
    }

    /// Returns the extension interface configuration map for this component.
    pub(crate) fn extensions_config(&self) -> &InterfaceConfigMap {
        &self.extensions_config
    }

    /// Consumes the component, yielding the owned pieces [`super::binding`] needs
    /// to assemble a `ResolvedComponent` once extensions have been bound.
    pub(super) fn into_parts(self) -> UnresolvedParts {
        UnresolvedParts {
            id: self.id,
            component: self.component,
            linker: self.linker,
            name: self.name,
            world: self.world,
        }
    }
}

/// The owned innards of an [`UnresolvedComponent`], handed to [`super::binding`]
/// at the end of resolution.
pub(super) struct UnresolvedParts {
    pub(super) id: ComponentId,
    pub(super) component: wasmtime::component::Component,
    pub(super) linker: wasmtime::component::Linker<Context>,
    pub(super) name: String,
    pub(super) world: WitWorld,
}
