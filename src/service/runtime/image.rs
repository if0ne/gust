use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Context as _;
use base64::{Engine, prelude::BASE64_STANDARD};
use sha2::Digest;

/// Reference to a WebAssembly component image, supporting multiple source schemes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ImageRef {
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

/// Resolved WebAssembly component image containing raw bytes and a content digest.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ResolvedImage {
    /// Raw WASM component bytes.
    pub bytes: bytes::Bytes,
    /// Content digest identifying this image version.
    pub digest: String,
}

#[async_trait::async_trait]
pub(crate) trait Resolver: Send + Sync {
    async fn resolve(&self, image_ref: &super::image::ImageRef) -> anyhow::Result<ResolvedImage>;
}

#[allow(dead_code)]
pub(crate) struct DefaultResolver {
    cache_dir: PathBuf,
    base_dir: PathBuf,
    oci_client: oci_wasm::WasmClient,
}

impl DefaultResolver {
    /// Creates a new default image resolver.
    pub(crate) fn new(cache_dir: impl Into<PathBuf>, base_dir: impl Into<PathBuf>) -> Self {
        let config = oci_client::client::ClientConfig {
            protocol: oci_client::client::ClientProtocol::HttpsExcept(vec![
                "localhost".to_string(),
                "127.0.0.1".to_string(),
            ]),
            ..Default::default()
        };

        Self::with_oci_client(
            cache_dir,
            base_dir,
            oci_wasm::WasmClient::new(oci_client::Client::new(config)),
        )
    }

    /// Creates a new image resolver with a custom OCI client.
    pub(crate) fn with_oci_client(
        cache_dir: impl Into<PathBuf>,
        base_dir: impl Into<PathBuf>,
        oci_client: oci_wasm::WasmClient,
    ) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            base_dir: base_dir.into(),
            oci_client,
        }
    }
}

#[async_trait::async_trait]
impl Resolver for DefaultResolver {
    #[tracing::instrument(skip(self), fields(image_ref = %image_ref))]
    async fn resolve(&self, image_ref: &super::image::ImageRef) -> anyhow::Result<ResolvedImage> {
        match image_ref {
            super::image::ImageRef::RelativePath(path) => {
                resolve_relative_path(path, &self.base_dir).await
            }
            super::image::ImageRef::AbsolutePath(path) => resolve_file_source(path).await,
            super::image::ImageRef::Base64(data) => resolve_data_source(data).await,
            super::image::ImageRef::Oci(image_ref) => {
                resolve_oci_source(&self.oci_client, image_ref).await
            }
        }
    }
}

fn compute_digest(bytes: &[u8]) -> String {
    let hash = sha2::Sha256::digest(bytes);
    format!("sha256:{:x}", hash)
}

#[tracing::instrument(skip(base_path))]
async fn resolve_relative_path(path: &Path, base_path: &Path) -> anyhow::Result<ResolvedImage> {
    let path = PathBuf::from(path);

    let full_path = if path.is_absolute() {
        path
    } else {
        base_path.join(&path)
    };

    let canonical = full_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize path: {:?}", full_path))?;

    resolve_file_source(&canonical).await
}

#[tracing::instrument]
async fn resolve_file_source(path: &Path) -> anyhow::Result<ResolvedImage> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("failed to read file: {:?}", path))?;

    let digest = compute_digest(&bytes);
    tracing::debug!(path = ?path, bytes = bytes.len(), digest, "file source resolved");

    Ok(ResolvedImage {
        bytes: bytes.into(),
        digest,
    })
}

#[tracing::instrument(skip(data))]
async fn resolve_data_source(data: &str) -> anyhow::Result<ResolvedImage> {
    let bytes = BASE64_STANDARD
        .decode(data)
        .context("failed to decode base64 data")?;

    let digest = compute_digest(&bytes);
    tracing::debug!(bytes = bytes.len(), digest, "base64 data resolved");

    Ok(ResolvedImage {
        bytes: bytes.into(),
        digest,
    })
}

#[tracing::instrument(skip(client))]
async fn resolve_oci_source(
    client: &oci_wasm::WasmClient,
    url: &str,
) -> anyhow::Result<ResolvedImage> {
    let reference: oci_client::Reference = url
        .parse()
        .with_context(|| format!("failed to parse OCI reference: {url}"))?;

    let auth = oci_client::secrets::RegistryAuth::Anonymous;

    let mut image_data = client
        .pull(&reference, &auth)
        .await
        .context("failed to pull WASM image from OCI registry")?;

    if image_data.layers.is_empty() {
        anyhow::bail!("OCI image has no layers");
    }

    let layer = image_data.layers.swap_remove(0);
    let digest = image_data
        .digest
        .unwrap_or_else(|| compute_digest(&layer.data));

    tracing::debug!(
        url,
        layers = image_data.layers.len(),
        bytes = layer.data.len(),
        digest,
        "OCI image resolved"
    );

    Ok(ResolvedImage {
        bytes: layer.data,
        digest,
    })
}
