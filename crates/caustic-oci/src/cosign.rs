//! Offline, key-based cosign signature verification.
//!
//! Only the registry fetch of the signature object needs network access. The
//! sigstore TUF trust root and Rekor transparency log are not consulted, so a
//! cached image can be re-verified offline.

use sigstore::cosign::verification_constraint::{PublicKeyVerifier, VerificationConstraint};
use sigstore::cosign::{ClientBuilder, CosignCapabilities, SignatureLayer, verify_constraints};
use sigstore::registry::{Auth, OciReference};

use crate::Error;

/// Build an [`OciReference`], using the `@digest` form for `sha256:` digests.
fn build_reference(registry: &str, reference_or_digest: &str) -> Result<OciReference, Error> {
    let full = if reference_or_digest.starts_with("sha256:") {
        format!("{registry}@{reference_or_digest}")
    } else {
        format!("{registry}:{reference_or_digest}")
    };
    full.parse::<OciReference>()
        .map_err(|e| Error::Parse(e.to_string()))
}

/// Verify signature layers against a PEM public key. No network access.
///
/// Passes when at least one layer was signed by the key.
fn verify_layers_with_pubkey(
    layers: &[SignatureLayer],
    public_key_pem: &[u8],
) -> Result<(), Error> {
    let verifier =
        PublicKeyVerifier::try_from(public_key_pem).map_err(|e| Error::Verify(e.to_string()))?;
    let constraints: [Box<dyn VerificationConstraint>; 1] = [Box::new(verifier)];
    verify_constraints(layers, constraints.iter()).map_err(|e| Error::Verify(e.to_string()))
}

/// Verify the cosign signature of a downloaded artifact against a public key.
///
/// `reference_or_digest` is a tag or a `sha256:` digest; a digest pins the
/// image immutably. Only the signature object is fetched from the registry.
/// `token` is accepted for symmetry with the rest of the crate but unused
/// (anonymous access only).
///
/// # Errors
///
/// Returns [`Error::Parse`] if the reference cannot be parsed.
/// Returns [`Error::Fetch`] if the registry cannot be reached.
/// Returns [`Error::Verify`] if the signature does not validate against the
/// key.
#[allow(unused_variables)]
pub async fn verify_artifact(
    registry: &str,
    reference_or_digest: &str,
    public_key_pem: &[u8],
    token: Option<&str>,
) -> Result<(), Error> {
    let image = build_reference(registry, reference_or_digest)?;
    let auth = Auth::Anonymous;

    let mut client = ClientBuilder::default()
        .build()
        .map_err(|e| Error::Fetch(e.to_string()))?;

    let layers = client
        .trusted_signature_layers(&auth, &image)
        .await
        .map_err(|e| Error::Fetch(e.to_string()))?;

    verify_layers_with_pubkey(&layers, public_key_pem)
}

#[cfg(test)]
mod tests {
    use sigstore::cosign::constraint::{Constraint, PrivateKeySigner};
    use sigstore::crypto::SigningScheme;

    use super::*;

    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// Mint a signature layer signed with a fresh ECDSA P-256 key.
    fn mint_signed_layer() -> (SignatureLayer, Vec<u8>) {
        let image_ref: OciReference = "ghcr.io/test/repo".parse().expect("valid reference");
        let mut layer =
            SignatureLayer::new_unsigned(&image_ref, DIGEST).expect("create unsigned layer");
        let signer = SigningScheme::ECDSA_P256_SHA256_ASN1
            .create_signer()
            .expect("create signer");
        let public_key_pem = signer
            .to_sigstore_keypair()
            .expect("keypair")
            .public_key_to_pem()
            .expect("public key pem")
            .into_bytes();
        let signer = PrivateKeySigner::new_with_signer(signer);
        assert!(
            signer.add_constraint(&mut layer).expect("sign layer"),
            "constraint applied"
        );
        (layer, public_key_pem)
    }

    #[test]
    fn build_reference_uses_tag_form_for_tag() {
        let reference = build_reference("ghcr.io/acme/img", "1.2.3").expect("valid reference");
        assert_eq!(reference.registry(), "ghcr.io");
        assert_eq!(reference.repository(), "acme/img");
        assert_eq!(reference.tag().unwrap(), "1.2.3");
        assert!(reference.digest().is_none());
    }

    #[test]
    fn build_reference_uses_digest_form_for_digest() {
        let digest = format!("sha256:{}", "0".repeat(64));
        let reference = build_reference("ghcr.io/acme/img", &digest).expect("valid reference");
        assert_eq!(reference.repository(), "acme/img");
        assert_eq!(reference.digest().unwrap(), &digest);
        assert!(reference.tag().is_none());
    }

    #[test]
    fn verify_accepts_matching_key() {
        let (layer, public_key_pem) = mint_signed_layer();
        verify_layers_with_pubkey(&[layer], &public_key_pem).expect("verification passes");
    }

    #[test]
    fn verify_rejects_wrong_key() {
        let (layer, _) = mint_signed_layer();
        let other = SigningScheme::ECDSA_P256_SHA256_ASN1
            .create_signer()
            .expect("create signer");
        let wrong_pem = other
            .to_sigstore_keypair()
            .expect("keypair")
            .public_key_to_pem()
            .expect("public key pem")
            .into_bytes();
        let err = verify_layers_with_pubkey(&[layer], &wrong_pem).unwrap_err();
        assert!(matches!(err, Error::Verify(_)));
    }

    #[test]
    fn verify_rejects_tampered_payload() {
        let (mut layer, public_key_pem) = mint_signed_layer();
        layer.raw_data = b"tampered payload".to_vec();
        let err = verify_layers_with_pubkey(&[layer], &public_key_pem).unwrap_err();
        assert!(matches!(err, Error::Verify(_)));
    }

    #[test]
    fn verify_rejects_empty_layer_list() {
        let (_, public_key_pem) = mint_signed_layer();
        let err = verify_layers_with_pubkey(&[], &public_key_pem).unwrap_err();
        assert!(matches!(err, Error::Verify(_)));
    }

    #[test]
    fn verify_rejects_invalid_public_key() {
        let (layer, _) = mint_signed_layer();
        let err = verify_layers_with_pubkey(&[layer], b"not a key").unwrap_err();
        assert!(matches!(err, Error::Verify(_)));
    }
}
