//! SHA-256 and lowercase hex: the one encoding hpm uses for hpm.lock
//! checksums, `_dev/<hash>` copy names, venv names, user-record keys,
//! archive checksums and signing key ids. Every digest goes through here so
//! the bytes on disk cannot drift between call sites.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// Two lowercase hex digits per byte.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0x0f)] as char);
    }
    out
}

/// Finish `hasher` and hex-encode the digest.
pub(crate) fn finish_hex(hasher: Sha256) -> String {
    to_hex(&hasher.finalize())
}

/// Hex SHA-256 of an in-memory byte slice.
pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

/// Size and hex SHA-256 of a file, streamed in 64 KiB reads.
pub(crate) fn sha256_file_sized(path: &Path) -> std::io::Result<(u64, String)> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buffer[..n]);
    }
    Ok((size, finish_hex(hasher)))
}

/// Hex SHA-256 of a file, streamed.
pub(crate) fn sha256_file(path: &Path) -> std::io::Result<String> {
    Ok(sha256_file_sized(path)?.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// `to_hex` is two lowercase hex digits per byte, equal to the
        /// `{:02x}` rendering.
        #[test]
        fn to_hex_matches_the_format_model(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
            let hex = to_hex(&bytes);
            prop_assert_eq!(hex.len(), bytes.len() * 2);
            prop_assert!(hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
            let model: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            prop_assert_eq!(hex, model);
        }

        /// The streamed file hash equals the in-memory hash of its bytes.
        #[test]
        fn file_hash_equals_bytes_hash(bytes in prop_oneof![
            Just(Vec::new()),
            prop::collection::vec(any::<u8>(), 1..512),
            (65_000usize..140_000).prop_map(|n| (0..n).map(|i| (i * 7 % 253) as u8).collect()),
        ]) {
            let dir = tempfile::TempDir::new().unwrap();
            let path = dir.path().join("f");
            std::fs::write(&path, &bytes).unwrap();
            prop_assert_eq!(sha256_file(&path).unwrap(), sha256_bytes(&bytes));
            prop_assert_eq!(sha256_file_sized(&path).unwrap().0, bytes.len() as u64);
        }
    }

    #[test]
    fn sha256_file_meets_the_digest_spec() {
        crate::digest_spec::check_file_hasher(|p| sha256_file(p).unwrap());
    }
}
