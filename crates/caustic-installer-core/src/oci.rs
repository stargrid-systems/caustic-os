use std::path::PathBuf;
use std::sync::Arc;

use futures_util::TryStreamExt;
use oci_client::client::{Client, ClientConfig};
use oci_client::manifest::{OciImageManifest, OciManifest};
use oci_client::secrets::RegistryAuth;
use oci_client::Reference;
use tokio::io::AsyncWriteExt;

pub async fn list_tags(registry: &str) -> Result<Vec<String>, Error> {
    let reference: Reference = registry
        .parse()
        .map_err(|e: oci_client::ParseError| Error::Parse(e.to_string()))?;
    let client = Client::new(ClientConfig::default());
    let auth = RegistryAuth::Anonymous;
    let response = client
        .list_tags(&reference, &auth, None, None)
        .await
        .map_err(|e| Error::Fetch(e.to_string()))?;
    Ok(response.tags)
}

pub async fn fetch_manifest(registry: &str, tag: &str) -> Result<OciImageManifest, Error> {
    let reference: Reference = format!("{registry}:{tag}")
        .parse()
        .map_err(|e: oci_client::ParseError| Error::Parse(e.to_string()))?;
    let client = Client::new(ClientConfig::default());
    let auth = RegistryAuth::Anonymous;
    let (manifest, _digest) = client
        .pull_manifest(&reference, &auth)
        .await
        .map_err(|e| Error::Fetch(e.to_string()))?;

    match manifest {
        OciManifest::Image(m) => Ok(m),
        OciManifest::ImageIndex(_) => Err(Error::UnexpectedManifestType),
    }
}

pub async fn pull_image(
    registry: String,
    tag: String,
    dest: PathBuf,
    progress: Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<(), Error> {
    let manifest = fetch_manifest(&registry, &tag).await?;

    let img_layer = manifest
        .layers
        .iter()
        .find(|l| {
            l.annotations
                .as_ref()
                .and_then(|a| a.get("org.opencontainers.image.title"))
                .is_some_and(|t| t.ends_with(".img"))
        })
        .ok_or(Error::NoImageLayer)?;

    let reference: Reference = format!("{registry}:{tag}")
        .parse()
        .map_err(|e: oci_client::ParseError| Error::Parse(e.to_string()))?;
    let client = Client::new(ClientConfig::default());

    let stream = client
        .pull_blob_stream(&reference, img_layer)
        .await
        .map_err(|e| Error::Fetch(e.to_string()))?;

    let total = stream.content_length.unwrap_or(0);
    let mut file = tokio::fs::File::create(&dest)
        .await
        .map_err(|e| Error::Io(e.to_string()))?;

    let mut downloaded: u64 = 0;
    let mut blob_stream = stream.stream;

    while let Some(chunk) = blob_stream
        .try_next()
        .await
        .map_err(|e| Error::Fetch(e.to_string()))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
        downloaded += chunk.len() as u64;
        progress(downloaded, total);
    }

    file.flush().await.map_err(|e| Error::Io(e.to_string()))?;
    Ok(())
}

#[derive(Debug)]
pub enum Error {
    Parse(String),
    Fetch(String),
    Io(String),
    NoImageLayer,
    UnexpectedManifestType,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(s) => write!(f, "Failed to parse reference: {s}"),
            Self::Fetch(s) => write!(f, "Registry error: {s}"),
            Self::Io(s) => write!(f, "I/O error: {s}"),
            Self::NoImageLayer => write!(f, "Manifest has no .img layer"),
            Self::UnexpectedManifestType => write!(f, "Expected image manifest, got image index"),
        }
    }
}

impl std::error::Error for Error {}
