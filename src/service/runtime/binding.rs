// Scaffolding for the component/extension runtime; not all of this surface is
// wired into the executor yet.
#![allow(dead_code)]

//! Binds [`Extension`]s to a compiled [`UnresolvedComponent`], producing a
//! [`ResolvedComponent`].
//!
//! This is the only place that depends on *both* `component` and `extension`,
//! which keeps those two modules free of a dependency cycle: `component` knows
//! nothing about extensions, `extension` only references `component`, and this
//! module sits above both.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Context as _;

use super::{
    component::UnresolvedComponent, extension::Extension, resolved::ResolvedComponent,
    types::ExtensionId, wit::WitInterface,
};

impl UnresolvedComponent {
    /// Matches `extensions` against the component's imports, runs each one's bind
    /// hook, and freezes the result into a cloneable [`ResolvedComponent`].
    ///
    /// On a bind failure, already-bound extensions are unbound (best effort)
    /// before the error is returned.
    pub(super) async fn resolve(
        mut self,
        extensions: &HashMap<ExtensionId, Arc<dyn Extension>>,
    ) -> anyhow::Result<ResolvedComponent> {
        let mut extension_matches = Vec::new();

        {
            let component_world = self.world();
            let config_map = self.extensions_config();

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
                                    let merged =
                                        existing.merge(&intersection).with_context(|| {
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

        let extensions = bound_extensions
            .into_iter()
            .map(|e| (e.extension.id(), e.extension.clone()))
            .collect();

        Ok(ResolvedComponent::new(self.into_parts(), extensions))
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

struct ExtensionMatch {
    extension: Arc<dyn Extension>,
    interfaces: HashSet<WitInterface>,
    config: Option<serde_json::Value>,
}
