use std::path::Path;

use oci_client::manifest::OciImageManifest;
use sha2::{Digest, Sha256};

use crate::Error;

const VERSION_ANNOTATION: &str = "org.opencontainers.image.version";

pub fn extract_version(manifest: &OciImageManifest) -> Result<String, Error> {
    manifest
        .annotations
        .as_ref()
        .and_then(|a| a.get(VERSION_ANNOTATION))
        .cloned()
        .ok_or_else(|| Error::MissingAnnotation(VERSION_ANNOTATION.to_string()))
}

pub fn verify_sha256sums(dir: &Path) -> Result<(), Error> {
    let sums_path = dir.join("SHA256SUMS");
    let content = match std::fs::read_to_string(&sums_path) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    for line in content.lines() {
        let mut parts = line.splitn(2, "  ");
        let expected = parts
            .next()
            .ok_or_else(|| Error::Other("malformed SHA256SUMS line".to_string()))?;
        let name = parts
            .next()
            .ok_or_else(|| Error::Other("malformed SHA256SUMS line".to_string()))?;
        let path = dir.join(name);
        let bytes = std::fs::read(&path).map_err(|e| Error::Io(e.to_string()))?;

        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();

        if actual != expected {
            return Err(Error::ChecksumMismatch(name.to_string()));
        }
    }

    Ok(())
}
