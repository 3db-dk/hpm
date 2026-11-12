//! One spec for every SHA-256 file hasher in hpm-core: for any contents
//! (empty, small, and over the 64 KiB read buffer), the streamed hash of a
//! file equals the lowercase-hex SHA-256 of its bytes. Each hasher's tests
//! run it, so merging them cannot change their output.

use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Reference model: SHA-256 of the bytes, two lowercase hex digits per byte.
pub(crate) fn reference_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn contents() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        Just(Vec::new()),
        prop::collection::vec(any::<u8>(), 1..256),
        // Straddle the 8 KiB and 64 KiB read buffers.
        (60_000usize..140_000).prop_map(|n| (0..n).map(|i| (i * 31 % 251) as u8).collect()),
    ]
}

/// Run the spec against `hash`, which hashes the file at a path.
pub(crate) fn check_file_hasher(hash: impl Fn(&Path) -> String) {
    let mut runner = TestRunner::new(Config {
        cases: 48,
        ..Config::default()
    });
    runner
        .run(&contents(), |bytes| {
            let dir = tempfile::TempDir::new().unwrap();
            let path = dir.path().join("f");
            std::fs::write(&path, &bytes).unwrap();
            prop_assert_eq!(hash(&path), reference_hex(&bytes));
            Ok(())
        })
        .unwrap();
}
