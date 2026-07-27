use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

use oci_client::manifest::OciImageManifest;
use sha2::{Digest, Sha256};

use crate::Error;

const VERSION_ANNOTATION: &str = "org.opencontainers.image.version";

/// Extract the version annotation from a manifest.
///
/// # Errors
///
/// Returns [`Error::MissingAnnotation`] if the manifest has no version annotation.
pub fn extract_version(manifest: &OciImageManifest) -> Result<String, Error> {
    manifest
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get(VERSION_ANNOTATION))
        .cloned()
        .ok_or_else(|| Error::MissingAnnotation(VERSION_ANNOTATION.to_string()))
}

/// Verify SHA256 checksums listed in the `SHA256SUMS` file inside `dir`.
///
/// Returns `Ok(())` if the file does not exist.
///
/// # Errors
///
/// Returns [`Error::Io`] if a listed file cannot be read.
/// Returns [`Error::Other`] if the SHA256SUMS file is malformed.
/// Returns [`Error::ChecksumMismatch`] if a checksum does not match.
pub fn verify_sha256sums(dir: &Path) -> Result<(), Error> {
    let sums_path = dir.join("SHA256SUMS");
    let content = match std::fs::read_to_string(&sums_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::Io(e.to_string())),
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
        let mut hasher = Sha256::new();
        let mut file = std::fs::File::open(&path)
            .map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }

        let hash = hasher.finalize();
        let mut actual = String::with_capacity(hash.len() * 2);
        for byte in &hash {
            write!(actual, "{byte:02x}").expect("formatting hex into a String never fails");
        }

        if actual != expected {
            return Err(Error::ChecksumMismatch(name.to_string()));
        }
    }

    Ok(())
}
