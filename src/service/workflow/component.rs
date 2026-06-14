use std::{path::PathBuf, sync::Arc};

/// Reference to a WebAssembly component image, supporting multiple source schemes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageRef {
    RelativePath(PathBuf),
    AbsolutePath(PathBuf),
    Base64(Arc<str>),
    Oci(Arc<str>),
}

impl std::fmt::Display for ImageRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageRef::RelativePath(p) => write!(f, "{}", p.display()),
            ImageRef::AbsolutePath(p) => write!(f, "file://{}", p.display()),
            ImageRef::Base64(d) => write!(f, "data://{d}"),
            ImageRef::Oci(d) => write!(f, "https://{d}"),
        }
    }
}

impl serde::Serialize for ImageRef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for ImageRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for ImageRef {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(data) = s.strip_prefix("data://") {
            Ok(ImageRef::Base64(data.into()))
        } else if let Some(data) = s.strip_prefix("https://") {
            Ok(ImageRef::Oci(data.into()))
        } else if let Some(data) = s.strip_prefix("http://") {
            Ok(ImageRef::Oci(data.into()))
        } else if let Some(data) = s.strip_prefix("file://") {
            Ok(ImageRef::AbsolutePath(data.into()))
        } else {
            Ok(ImageRef::RelativePath(s.into()))
        }
    }
}

/// Specification for deploying a single WebAssembly component with its trigger and extension bindings.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ComponentSpec {
    /// Component name
    pub name: String,
    /// Component image reference.
    pub image: ImageRef,
}
