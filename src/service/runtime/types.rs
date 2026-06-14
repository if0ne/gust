// Scaffolding for the component/extension runtime; not all of this surface is
// wired into the executor yet.
#![allow(dead_code)]

use std::{any::TypeId, collections::HashMap};

use uuid::Uuid;

use super::image::ImageRef;

/// Unique identifier for a component.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ComponentId(Uuid);

impl ComponentId {
    /// Creates a new identifier from an existing UUID.
    pub(crate) fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl std::fmt::Display for ComponentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for ComponentId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::parse_str(s)?;
        Ok(Self(uuid))
    }
}

/// Internal identifier backing both [`ExtensionId`] and [`TriggerId`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PluginId {
    Type(TypeId),
    Named(&'static str),
}

impl std::fmt::Display for PluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginId::Type(id) => write!(f, "{id:?}"),
            PluginId::Named(name) => f.write_str(name),
        }
    }
}

/// Unique identifier for an extension.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ExtensionId(PluginId);

impl ExtensionId {
    /// Creates an identifier from the Rust type of an extension.
    pub(crate) fn from_type<T: 'static + ?Sized>() -> Self {
        Self(PluginId::Type(TypeId::of::<T>()))
    }

    /// Creates an identifier from a static string name.
    pub(crate) fn from_name(name: &'static str) -> Self {
        Self(PluginId::Named(name))
    }
}

impl std::fmt::Display for ExtensionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Specification for deploying a single WebAssembly component with its trigger and extension bindings.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ComponentSpec {
    /// Component name
    pub name: String,
    /// Component image reference.
    pub image: ImageRef,
    /// Extension interface configurations keyed by WIT interface identifier.
    #[serde(default)]
    pub extensions: InterfaceConfigMap,
}

/// Post-resolution counterpart to [`ComponentSpec`]: the resolved component
/// bytes plus the extension config, ready to hand to `Runner::load_component`.
///
/// Where a [`ComponentSpec`] carries an [`ImageRef`], a `ComponentDesc` carries
/// the fetched `bytes` and their content `digest`.
#[derive(Debug)]
pub(crate) struct ComponentDesc {
    /// Component name
    pub name: String,
    /// Raw WebAssembly component bytes.
    pub bytes: Vec<u8>,
    /// Optional content digest used for caching compiled components.
    pub digest: Option<String>,
    /// Extension interface configurations keyed by WIT interface identifier.
    pub extensions: InterfaceConfigMap,
}

/// Map of WIT interface identifiers to their per-interface configuration.
#[derive(Debug, Clone, Default)]
pub(crate) struct InterfaceConfigMap(HashMap<InterfaceKey, InterfaceConfig>);

impl serde::Serialize for InterfaceConfigMap {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            let key_str: String = key.to_string();
            map.serialize_entry(&key_str, value)?;
        }
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for InterfaceConfigMap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let string_map: std::collections::HashMap<String, InterfaceConfig> =
            serde::Deserialize::deserialize(deserializer)?;
        let mut map = HashMap::default();
        for (key_str, value) in string_map {
            let key: InterfaceKey = key_str.parse().map_err(serde::de::Error::custom)?;
            map.insert(key, value);
        }
        Ok(InterfaceConfigMap(map))
    }
}

impl InterfaceConfigMap {
    /// Creates an empty interface configuration map.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Inserts a configuration entry for the given interface key.
    pub(crate) fn insert(&mut self, key: InterfaceKey, config: InterfaceConfig) {
        self.0.insert(key, config);
    }

    /// Returns true if the map contains an entry for the given interface identifier.
    pub(crate) fn contains(&self, interface: &InterfaceKey) -> bool {
        self.0.contains_key(interface)
    }

    /// Returns the configuration for the given interface identifier, if present.
    pub(crate) fn get(&self, interface: &InterfaceKey) -> Option<&InterfaceConfig> {
        self.0.get(interface)
    }

    /// Returns an iterator over all interface identifiers in the map.
    pub(crate) fn keys(&self) -> impl Iterator<Item = &InterfaceKey> {
        self.0.keys()
    }

    /// Returns an iterator over all interface identifier and configuration pairs.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&InterfaceKey, &InterfaceConfig)> {
        self.0.iter()
    }
}

impl IntoIterator for InterfaceConfigMap {
    type Item = (InterfaceKey, InterfaceConfig);
    type IntoIter = std::collections::hash_map::IntoIter<InterfaceKey, InterfaceConfig>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl FromIterator<(InterfaceKey, InterfaceConfig)> for InterfaceConfigMap {
    fn from_iter<T: IntoIterator<Item = (InterfaceKey, InterfaceConfig)>>(iter: T) -> Self {
        let mut this = Self::new();

        for (k, v) in iter {
            this.insert(k, v);
        }

        this
    }
}

/// Composite key identifying a WIT interface by namespace and package.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct InterfaceKey {
    pub namespace: String,
    pub package: String,
}

impl std::fmt::Display for InterfaceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.namespace, self.package)
    }
}

impl std::str::FromStr for InterfaceKey {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((namespace, package)) = s.split_once(':') {
            Ok(Self {
                namespace: namespace.to_string(),
                package: package.to_string(),
            })
        } else {
            Err(anyhow::anyhow!("invalid interface key format"))
        }
    }
}

/// Configuration for a single WIT interface, optionally scoped by backend.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct InterfaceConfig {
    /// Optional backend name to restrict which extension or trigger handles this interface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// Arbitrary properties passed to the handling extension or trigger.
    pub properties: serde_json::Value,
}

impl InterfaceConfig {
    /// Creates a new interface configuration from an optional backend and typed properties.
    pub(crate) fn new<T: serde::Serialize>(
        backend: Option<String>,
        properties: T,
    ) -> anyhow::Result<Self> {
        let properties = serde_json::to_value(&properties)
            .map_err(|e| anyhow::anyhow!("failed to serialize properties: {e}"))?;
        Ok(Self {
            backend,
            properties,
        })
    }
}
