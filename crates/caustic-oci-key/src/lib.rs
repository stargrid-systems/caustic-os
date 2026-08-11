//! Embedded cosign public key.
//!
//! Injected at build time via `CAUSTIC_COSIGN_PUB` (path to a PEM file). When
//! unset the key is empty and verification is disabled at runtime.

include!(concat!(env!("OUT_DIR"), "/cosign_pub.rs"));

pub static COSIGN_PUB: &[u8] = COSIGN_KEY_BYTES;
