// Scaffolding for the component/extension runtime; not all of this surface is
// wired into the executor yet.
#![allow(dead_code)]

use std::{any::Any, collections::HashSet, sync::Arc};

/// Trait for extending the runtime with additional capabilities during component lifecycle.
#[async_trait::async_trait]
pub trait Extension: Any + Send + Sync + 'static {
    /// Returns the unique identifier for this extension.
    fn id(&self) -> super::common::ExtensionId {
        super::common::ExtensionId::from_type::<Self>()
    }

    /// Returns the human-readable name of this extension.
    fn name(&self) -> &'static str;

    /// Returns the optional backend name for this extension.
    fn backend(&self) -> Option<&'static str> {
        None
    }

    /// Returns the set of WIT interfaces this extension imports.
    fn imports(&self) -> HashSet<super::wit::WitInterface> {
        Default::default()
    }

    /// Called when the extension is started.
    async fn start(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Configures the store context builder before a component store is created.
    fn configure_ctx(&self, _ctx_builder: &mut super::ctx::ContextBuilder) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called when a component is being bound to the runtime before resolution.
    async fn on_component_bind(
        &self,
        _component: &mut super::component::UnresolvedComponent,
        _interfaces: &HashSet<super::wit::WitInterface>,
        _config: Option<&'_ serde_json::Value>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called when a component is being unbound from the runtime.
    async fn on_component_unbind(&self, _id: &super::common::ComponentId) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called when the extension is stopped.
    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// A resolved pairing of an extension with the WIT interfaces it matched against a component.
pub struct ResolvedExtension {
    pub(crate) extension: Arc<dyn Extension>,
    pub(crate) interfaces: HashSet<super::wit::WitInterface>,
    pub(crate) config: Option<serde_json::Value>,
}

impl ResolvedExtension {
    /// Creates a new resolved extension with the given extension and matched interfaces.
    pub fn new(
        extension: Arc<dyn Extension>,
        interfaces: HashSet<super::wit::WitInterface>,
        config: Option<serde_json::Value>,
    ) -> Self {
        Self {
            extension,
            interfaces,
            config,
        }
    }

    /// Returns a reference to the underlying extension.
    pub fn extension(&self) -> &dyn Extension {
        &*self.extension
    }

    /// Returns the set of WIT interfaces this extension was matched against.
    pub fn interfaces(&self) -> &HashSet<super::wit::WitInterface> {
        &self.interfaces
    }
}

impl std::fmt::Debug for ResolvedExtension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedExtension")
            .field("extension", &self.extension.name())
            .field("backend", &self.extension.backend())
            .field("interfaces", &self.interfaces)
            .field("config", &self.config)
            .finish()
    }
}
