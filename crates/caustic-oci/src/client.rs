use std::path::Path;
use std::sync::Arc;

use futures_util::TryStreamExt;
use oci_client::client::{Client, ClientConfig};
use oci_client::manifest::{OciDescriptor, OciImageManifest, OciManifest};
use oci_client::secrets::RegistryAuth;
use oci_client::Reference;
use tokio::io::AsyncWriteExt;

use crate::Error;

fn parse_ref(registry: &str, tag: &str) -> Result<Reference, Error> {
    format!("{registry}:{tag}")
        .parse()
        .map_err(|e: oci_client::ParseError| Error::Parse(e.to_string()))
}

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
    let reference = parse_ref(registry, tag)?;
    let client = Client::new(ClientConfig::default());
    let auth = RegistryAuth::Anonymous;
    let (manifest, _) = client
        .pull_manifest(&reference, &auth)
        .await
        .map_err(|e| Error::Fetch(e.to_string()))?;

    match manifest {
        OciManifest::Image(m) => Ok(m),
        OciManifest::ImageIndex(_) => Err(Error::UnexpectedManifestType),
    }
}

pub fn find_layer_by_suffix<'a>(
    manifest: &'a OciImageManifest,
    suffix: &str,
) -> Option<&'a OciDescriptor> {
    manifest.layers.iter().find(|l| {
        l.annotations
            .as_ref()
            .and_then(|a| a.get("org.opencontainers.image.title"))
            .is_some_and(|t| t.ends_with(suffix))
    })
}

pub async fn pull_blob(
    registry: &str,
    tag: &str,
    layer: &OciDescriptor,
    dest: &Path,
) -> Result<(), Error> {
    let reference = parse_ref(registry, tag)?;
    let client = Client::new(ClientConfig::default());
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| Error::Io(e.to_string()))?;
    client
        .pull_blob(&reference, layer, &mut file)
        .await
        .map_err(|e| Error::Fetch(e.to_string()))?;
    file.flush()
        .await
        .map_err(|e| Error::Io(e.to_string()))?;
    Ok(())
}

pub async fn pull_blob_streaming(
    registry: &str,
    tag: &str,
    layer: &OciDescriptor,
    dest: &Path,
    progress: Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<(), Error> {
    let reference = parse_ref(registry, tag)?;
    let client = Client::new(ClientConfig::default());

    let stream = client
        .pull_blob_stream(&reference, layer)
        .await
        .map_err(|e| Error::Fetch(e.to_string()))?;

    let total = stream.content_length.unwrap_or(0);
    let mut file = tokio::fs::File::create(dest)
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

    file.flush()
        .await
        .map_err(|e| Error::Io(e.to_string()))?;
    Ok(())
}
