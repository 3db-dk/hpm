//! Checksumming and Ed25519 signing of packed archives.

use base64::Engine;
pub use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::DecodePrivateKey;
use ed25519_dalek::{Signer, VerifyingKey};
use hpm_package::IoOp;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

use super::PackError;

/// Compute SHA-256 checksum of a file, returning hex-encoded string.
pub fn compute_archive_checksum(path: &Path) -> Result<String, PackError> {
    let bytes = fs::read(path).map_err(|e| IoOp::wrap("read archive for checksum", path, e))?;
    Ok(compute_bytes_checksum(&bytes))
}

/// Compute SHA-256 checksum of an in-memory byte slice. Use when you already
/// hold the archive bytes (e.g. after re-shaping for a third-party hosting
/// target) and don't want a disk round-trip.
pub fn compute_bytes_checksum(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    hex::encode(hash)
}

/// Hex encoding without external dep (sha2 re-exports what we need, but let's
/// just do it inline).
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}

/// Load an Ed25519 signing key from a PKCS#8 PEM file.
///
/// The file is expected to contain a `-----BEGIN PRIVATE KEY-----` block as
/// produced by `openssl genpkey -algorithm ed25519`.
pub fn load_signing_key(path: &Path) -> Result<SigningKey, PackError> {
    let pem = fs::read_to_string(path).map_err(|e| {
        PackError::SigningKey(format!("failed to read key file {}: {}", path.display(), e))
    })?;
    load_signing_key_from_pem(&pem)
}

/// Parse an Ed25519 signing key from inline PKCS#8 PEM content.
pub fn load_signing_key_from_pem(pem: &str) -> Result<SigningKey, PackError> {
    SigningKey::from_pkcs8_pem(pem.trim())
        .map_err(|e| PackError::SigningKey(format!("failed to parse PKCS#8 PEM: {e}")))
}

/// Sign an archive file, returning (base64 signature, hex key_id).
pub fn sign_archive(path: &Path, signing_key: &SigningKey) -> Result<(String, String), PackError> {
    let bytes = fs::read(path).map_err(|e| IoOp::wrap("read archive for signing", path, e))?;
    Ok(sign_bytes(&bytes, signing_key))
}

/// Sign an in-memory byte slice with the given Ed25519 key, returning
/// (base64 signature, hex key_id). Use when re-signing after the archive
/// has been mutated post-pack (e.g. third-party hosting layout reshapes).
pub fn sign_bytes(bytes: &[u8], signing_key: &SigningKey) -> (String, String) {
    let signature = signing_key.sign(bytes);
    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());

    let verifying_key: VerifyingKey = signing_key.verifying_key();
    let pub_bytes = verifying_key.to_bytes();
    let key_id = hex::encode(&pub_bytes[..8]);

    (sig_b64, key_id)
}
