// Scaffolding for the component/extension runtime; not all of this surface is
// wired into the executor yet.
#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};

use anyhow::Context as _;
use wasmtime::error::Context as _;

/// Description of a WebAssembly component including its raw bytes and optional content digest.
#[derive(Debug)]
pub struct ComponentDesc {
    /// Component name
    pub name: String,
    /// Raw WebAssembly component bytes.
    pub bytes: Vec<u8>,
    /// Optional content digest used for caching compiled components.
    pub digest: Option<String>,
    /// Extension interface configurations keyed by WIT interface identifier.
    pub extensions: InterfaceConfigMap,
}

use super::{
    common::{ComponentId, ExtensionId, InterfaceConfigMap},
    ctx::{Context, ContextBuilder},
    engine::Engine,
    extension::Extension,
    wit::{WitInterface, WitWorld},
};

/// A WebAssembly component that has not yet been fully resolved against its dependencies.
pub struct UnresolvedComponent {
    id: ComponentId,
    engine: Engine,
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
    pub fn new(
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
            engine,
            component,
            linker,
            world,
            name: component_desc.name,
            extensions_config: component_desc.extensions,
        })
    }

    pub fn extract_world(
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
    pub fn id(&self) -> &ComponentId {
        &self.id
    }

    /// Returns a reference to the component
    pub fn component(&self) -> &wasmtime::component::Component {
        &self.component
    }

    /// Returns a reference to the component's linker.
    pub fn linker(&self) -> &wasmtime::component::Linker<Context> {
        &self.linker
    }

    /// Returns a mutable reference to the component's linker.
    pub fn linker_mut(&mut self) -> &mut wasmtime::component::Linker<Context> {
        &mut self.linker
    }

    /// Returns the component's extracted WIT world describing its imports and exports.
    pub fn world(&self) -> &WitWorld {
        &self.world
    }

    /// Returns the extension interface configuration map for this component.
    pub fn extensions_config(&self) -> &InterfaceConfigMap {
        &self.extensions_config
    }

    /// Consumes this unresolved component and produces a bound component with extensions and triggers attached.
    pub async fn resolve(
        mut self,
        extensions: &HashMap<ExtensionId, Arc<dyn Extension>>,
    ) -> anyhow::Result<ResolvedComponent> {
        let component_world = self.world();
        let config_map = self.extensions_config();

        let mut extension_matches = Vec::new();

        for ext in extensions.values() {
            let ext_imports = ext.imports();
            let mut matched_interfaces: HashSet<WitInterface> = HashSet::default();
            let mut merged_props = serde_json::Map::new();

            for comp_imp in &component_world.imports {
                for ext_imp in &ext_imports {
                    if let Some(intersection) = comp_imp.intersect(ext_imp) {
                        let id = intersection.id();

                        if let Some(interface_cfg) = config_map.get(&id)
                            && let Some(obj) = interface_cfg.properties.as_object()
                        {
                            for (k, v) in obj {
                                merged_props.insert(k.clone(), v.clone());
                            }
                        }

                        match matched_interfaces.take(&intersection) {
                            Some(existing) => {
                                let merged = existing.merge(&intersection).with_context(|| {
                                    format!(
                                        "failed to merge WIT interfaces for extension '{}'",
                                        ext.name()
                                    )
                                })?;
                                matched_interfaces.insert(merged);
                            }
                            None => {
                                matched_interfaces.insert(intersection);
                            }
                        }
                    }
                }
            }

            if !matched_interfaces.is_empty() {
                extension_matches.push(ExtensionMatch {
                    extension: ext.clone(),
                    interfaces: matched_interfaces,
                    config: (!merged_props.is_empty())
                        .then_some(serde_json::Value::Object(merged_props)),
                });
            }
        }

        let mut bound_extensions: Vec<&ExtensionMatch> = Vec::new();

        for extension_match in &extension_matches {
            self.validate_extension_backend_compatibility(extension_match)?;

            if let Err(err) = extension_match
                .extension
                .on_component_bind(
                    &mut self,
                    &extension_match.interfaces,
                    extension_match.config.as_ref(),
                )
                .await
            {
                for already_bound in &bound_extensions {
                    if let Err(unbind_err) =
                        already_bound.extension.on_component_unbind(self.id()).await
                    {
                        tracing::error!(
                            "Failed to unbind extension '{}' during rollback: {unbind_err:#}",
                            already_bound.extension.name()
                        );
                    }
                }

                return Err(err.context(format!(
                    "failed to bind extension '{}'",
                    extension_match.extension.name()
                )));
            }

            bound_extensions.push(extension_match);
        }

        Ok(ResolvedComponent {
            inner: Arc::new(ResolvedComponentInner {
                id: self.id,
                engine: self.engine,
                component: self.component,
                linker: self.linker,
                name: self.name,
                world: self.world,
                extensions: bound_extensions
                    .into_iter()
                    .map(|e| (e.extension.id(), e.extension.clone()))
                    .collect(),
            }),
        })
    }

    fn validate_extension_backend_compatibility(
        &self,
        extension_match: &ExtensionMatch,
    ) -> anyhow::Result<()> {
        let extension_backend = extension_match.extension.backend();

        for interface in &extension_match.interfaces {
            let interface_id = interface.id();
            if let Some(interface_config) = self.extensions_config().get(&interface_id)
                && let Some(config_backend) = &interface_config.backend
            {
                match extension_backend {
                    Some(ext_backend) if ext_backend != config_backend => {
                        anyhow::bail!(
                            "backend mismatch: extension backend \"{}\" does not match config backend \"{}\" for interface \"{}\"",
                            ext_backend,
                            config_backend,
                            interface_id
                        );
                    }
                    None => {
                        anyhow::bail!(
                            "backend mismatch: extension backend \"none\" does not match config backend \"{}\" for interface \"{}\"",
                            config_backend,
                            interface_id
                        );
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }
}

/// A fully resolved WebAssembly component ready for instantiation.
///
/// The linker is frozen and read-only. This type is cheaply cloneable via `Arc`.
#[derive(Clone)]
pub struct ResolvedComponent {
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
    pub(crate) fn extensions(&self) -> &HashMap<ExtensionId, Arc<dyn Extension>> {
        &self.inner.extensions
    }

    /// Returns the component's unique identifier.
    pub fn id(&self) -> &ComponentId {
        &self.inner.id
    }

    /// Returns the component's name.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Returns the world for this component.
    pub fn world(&self) -> &WitWorld {
        &self.inner.world
    }

    /// Returns the wasmtime component for this component.
    pub(crate) fn component(&self) -> &wasmtime::component::Component {
        &self.inner.component
    }

    /// Creates an `InstancePre` by pre-linking the component, ready for repeated instantiation.
    pub fn instantiate_pre(&self) -> anyhow::Result<wasmtime::component::InstancePre<Context>> {
        let instance_pre = self
            .inner
            .linker
            .instantiate_pre(&self.inner.component)
            .context("failed to instantiate pre component")?;

        Ok(instance_pre)
    }

    /// Creates a new wasmtime store with a fresh context for this component.
    ///
    /// Builds the context by letting each bound extension configure it via
    /// [`Extension::configure_ctx`], then applies the outgoing request policy.
    pub fn new_context(&self) -> anyhow::Result<Context> {
        let mut ctx_builder = ContextBuilder::new(self.inner.id.clone());

        for extension in self.inner.extensions.values() {
            extension.configure_ctx(&mut ctx_builder).with_context(|| {
                format!(
                    "extension '{}' failed to configure context",
                    extension.name()
                )
            })?;
        }

        Ok(ctx_builder.build())
    }
}

struct ResolvedComponentInner {
    id: ComponentId,
    engine: Engine,
    component: wasmtime::component::Component,
    linker: wasmtime::component::Linker<Context>,
    name: String,
    world: WitWorld,
    extensions: HashMap<ExtensionId, Arc<dyn Extension>>,
}

struct ExtensionMatch {
    extension: Arc<dyn Extension>,
    interfaces: HashSet<WitInterface>,
    config: Option<serde_json::Value>,
}
