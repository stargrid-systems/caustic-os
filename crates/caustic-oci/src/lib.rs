//! Shared OCI registry operations for Caustic OS.

pub use oci_client::manifest::{OciDescriptor, OciImageManifest};

pub use self::client::{
    fetch_manifest, find_layer_by_suffix, list_tags, pull_blob, pull_blob_streaming, resolve_digest,
};
pub use self::cosign::verify_artifact;
pub use self::verify::{extract_created, extract_version, verify_sha256sums};

mod client;
mod cosign;
mod verify;

#[derive(Debug)]
pub enum Error {
    Parse(String),
    Fetch(String),
    Io(String),
    NoImageLayer,
    UnexpectedManifestType,
    MissingAnnotation(String),
    ChecksumMismatch(String),
    MissingSha256Sums,
    Verify(String),
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(s) => write!(f, "Failed to parse reference: {s}"),
            Self::Fetch(s) => write!(f, "Registry error: {s}"),
            Self::Io(s) => write!(f, "I/O error: {s}"),
            Self::NoImageLayer => write!(f, "Manifest has no .img layer"),
            Self::UnexpectedManifestType => write!(f, "Expected image manifest, got image index"),
            Self::MissingAnnotation(s) => write!(f, "Manifest missing {s} annotation"),
            Self::ChecksumMismatch(s) => write!(f, "Checksum mismatch for {s}"),
            Self::MissingSha256Sums => write!(f, "SHA256SUMS file is missing"),
            Self::Verify(s) => write!(f, "Signature verification failed: {s}"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {}
