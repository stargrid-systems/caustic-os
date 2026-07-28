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
/// Returns [`Error::MissingAnnotation`] if the manifest has no version
/// annotation.
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
/// # Errors
///
/// Returns [`Error::MissingSha256Sums`] if the SHA256SUMS file is absent.
/// Returns [`Error::Io`] if a listed file cannot be read.
/// Returns [`Error::Other`] if the SHA256SUMS file is malformed.
/// Returns [`Error::ChecksumMismatch`] if a checksum does not match.
pub fn verify_sha256sums(dir: &Path) -> Result<(), Error> {
    let sums_path = dir.join("SHA256SUMS");
    let content = match std::fs::read_to_string(&sums_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Error::MissingSha256Sums),
        Err(e) => return Err(Error::Io(format!("{}: {e}", sums_path.display()))),
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("caustic-oci-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let hash = Sha256::digest(bytes);
        let mut out = String::with_capacity(hash.len() * 2);
        for byte in &hash {
            write!(out, "{byte:02x}").expect("formatting hex into a String never fails");
        }
        out
    }

    fn write_file(dir: &Path, name: &str, contents: &[u8]) {
        std::fs::write(dir.join(name), contents).expect("write file");
    }

    fn write_sums(dir: &Path, entries: &[(&str, &[u8])]) {
        let mut content = String::new();
        for &(name, data) in entries {
            write_file(dir, name, data);
            writeln!(content, "{}  {name}", sha256_hex(data)).expect("format into String");
        }
        std::fs::write(dir.join("SHA256SUMS"), content).expect("write SHA256SUMS");
    }

    #[test]
    fn extract_version_returns_annotation() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "org.opencontainers.image.version".to_string(),
            "1.2.3".to_string(),
        );
        let manifest = OciImageManifest {
            annotations: Some(annotations),
            ..Default::default()
        };
        assert_eq!(extract_version(&manifest).unwrap(), "1.2.3");
    }

    #[test]
    fn extract_version_missing_when_annotations_none() {
        let manifest = OciImageManifest::default();
        assert!(matches!(
            extract_version(&manifest),
            Err(Error::MissingAnnotation(_))
        ));
    }

    #[test]
    fn extract_version_missing_when_key_absent() {
        let manifest = OciImageManifest {
            annotations: Some(BTreeMap::new()),
            ..Default::default()
        };
        assert!(matches!(
            extract_version(&manifest),
            Err(Error::MissingAnnotation(_))
        ));
    }

    #[test]
    fn verify_sha256sums_fails_when_sums_missing() {
        let dir = unique_dir();
        assert!(matches!(
            verify_sha256sums(&dir),
            Err(Error::MissingSha256Sums)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn verify_sha256sums_ok_with_matching_entry() {
        let dir = unique_dir();
        write_sums(&dir, &[("root.img", b"hello world")]);
        assert!(verify_sha256sums(&dir).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn verify_sha256sums_ok_with_multiple_entries() {
        let dir = unique_dir();
        write_sums(
            &dir,
            &[("a.img", b"aaaa"), ("b.img", b"bbbb"), ("c.img", b"cccc")],
        );
        assert!(verify_sha256sums(&dir).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn verify_sha256sums_rejects_wrong_checksum() {
        let dir = unique_dir();
        write_file(&dir, "root.img", b"hello world");
        std::fs::write(dir.join("SHA256SUMS"), "deadbeef  root.img\n").expect("write SHA256SUMS");
        assert!(matches!(
            verify_sha256sums(&dir),
            Err(Error::ChecksumMismatch(name)) if name == "root.img"
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn verify_sha256sums_rejects_malformed_line() {
        let dir = unique_dir();
        std::fs::write(dir.join("SHA256SUMS"), "not-a-valid-line\n").expect("write SHA256SUMS");
        assert!(matches!(verify_sha256sums(&dir), Err(Error::Other(_))));
        let _ = std::fs::remove_dir_all(dir);
    }
}
