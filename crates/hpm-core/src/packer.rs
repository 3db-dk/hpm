//! Package archiving, checksumming, and signing.
//!
//! Produces a `{name}-{version}.zip` from a package directory, with SHA-256
//! checksum and optional Ed25519 signature.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

/// Result of a successful pack operation.
#[derive(Debug)]
pub struct PackResult {
    /// Path to the created archive.
    pub archive_path: PathBuf,
    /// Hex-encoded SHA-256 checksum of the archive.
    pub checksum: String,
    /// Base64-encoded Ed25519 signature (if signing key provided).
    pub signature: Option<String>,
    /// Hex-encoded first 8 bytes of the public key (if signing key provided).
    pub key_id: Option<String>,
}

/// Errors from packing operations.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Invalid signing key: {0}")]
    SigningKey(String),

    #[error("Ignore pattern error: {0}")]
    IgnorePattern(#[from] ignore::Error),
}

/// Build gitignore-style rules for filtering archive contents.
///
/// Always excludes `.git/` and `.hpm/`. Additionally loads `.gitignore` and
/// `.hpmignore` if they exist in the package directory.
pub fn build_ignore_rules(dir: &Path) -> Result<Gitignore, PackError> {
    let mut builder = GitignoreBuilder::new(dir);

    // Always exclude .git/ and .hpm/
    builder.add_line(None, ".git/")?;
    builder.add_line(None, ".hpm/")?;

    // Load .gitignore if present
    let gitignore = dir.join(".gitignore");
    if gitignore.exists() {
        builder.add(gitignore);
    }

    // Load .hpmignore if present
    let hpmignore = dir.join(".hpmignore");
    if hpmignore.exists() {
        builder.add(hpmignore);
    }

    Ok(builder.build()?)
}

/// Create a zip archive of the package directory, filtering via ignore rules.
///
/// Files are added in sorted order for deterministic output.
pub fn create_archive(
    package_dir: &Path,
    name: &str,
    version: &str,
    output_dir: &Path,
    ignore: &Gitignore,
) -> Result<PathBuf, PackError> {
    let archive_name = format!("{}-{}.zip", name, version);
    let archive_path = output_dir.join(&archive_name);

    // Collect files, sorted for determinism
    let mut entries: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(package_dir).sort_by_file_name() {
        let entry = entry.map_err(|e| PackError::Io(std::io::Error::other(e)))?;

        let path = entry.path();
        let relative = path.strip_prefix(package_dir).unwrap_or(path);

        // Skip the root directory itself
        if relative == Path::new("") {
            continue;
        }

        // Check ignore rules
        let is_dir = entry.file_type().is_dir();
        if ignore
            .matched_path_or_any_parents(relative, is_dir)
            .is_ignore()
        {
            continue;
        }

        if entry.file_type().is_file() {
            entries.push(path.to_path_buf());
        }
    }

    // Create zip
    let file = fs::File::create(&archive_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for path in &entries {
        let relative = path.strip_prefix(package_dir).unwrap_or(path);
        let name = relative.to_string_lossy();

        zip.start_file(name.as_ref(), options)?;
        let mut f = fs::File::open(path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        zip.write_all(&buf)?;
    }

    zip.finish()?;
    Ok(archive_path)
}

/// Compute SHA-256 checksum of a file, returning hex-encoded string.
pub fn compute_archive_checksum(path: &Path) -> Result<String, PackError> {
    let bytes = fs::read(path)?;
    let hash = Sha256::digest(&bytes);
    Ok(hex::encode(hash))
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

/// Load an Ed25519 signing key from a file containing a 32-byte raw seed.
pub fn load_signing_key(path: &Path) -> Result<SigningKey, PackError> {
    let bytes = fs::read(path).map_err(|e| {
        PackError::SigningKey(format!("failed to read key file {}: {}", path.display(), e))
    })?;

    if bytes.len() != 32 {
        return Err(PackError::SigningKey(format!(
            "key file must be exactly 32 bytes, got {}",
            bytes.len()
        )));
    }

    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    Ok(SigningKey::from_bytes(&seed))
}

/// Sign an archive file, returning (base64 signature, hex key_id).
pub fn sign_archive(path: &Path, signing_key: &SigningKey) -> Result<(String, String), PackError> {
    let bytes = fs::read(path)?;
    let signature = signing_key.sign(&bytes);

    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());

    let verifying_key: VerifyingKey = signing_key.verifying_key();
    let pub_bytes = verifying_key.to_bytes();
    let key_id = hex::encode(&pub_bytes[..8]);

    Ok((sig_b64, key_id))
}

/// Pack a package directory into a signed, checksummed archive.
pub fn pack(
    package_dir: &Path,
    name: &str,
    version: &str,
    output_dir: &Path,
    signing_key: Option<&SigningKey>,
) -> Result<PackResult, PackError> {
    let ignore = build_ignore_rules(package_dir)?;
    let archive_path = create_archive(package_dir, name, version, output_dir, &ignore)?;
    let checksum = compute_archive_checksum(&archive_path)?;

    let (signature, key_id) = match signing_key {
        Some(key) => {
            let (sig, kid) = sign_archive(&archive_path, key)?;
            (Some(sig), Some(kid))
        }
        None => (None, None),
    };

    Ok(PackResult {
        archive_path,
        checksum,
        signature,
        key_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_package(dir: &Path) {
        fs::write(
            dir.join("hpm.toml"),
            r#"[package]
name = "test-pkg"
version = "1.0.0"
"#,
        )
        .unwrap();
        fs::write(dir.join("README.md"), "# Test").unwrap();
        fs::create_dir_all(dir.join("otls")).unwrap();
        fs::write(dir.join("otls/tool.hda"), b"hda-content").unwrap();
    }

    #[test]
    fn ignore_rules_exclude_git_and_hpm() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join(".hpm")).unwrap();

        let rules = build_ignore_rules(dir.path()).unwrap();

        assert!(rules
            .matched_path_or_any_parents(Path::new(".git/config"), false)
            .is_ignore());
        assert!(rules
            .matched_path_or_any_parents(Path::new(".hpm/config.toml"), false)
            .is_ignore());
        assert!(!rules
            .matched_path_or_any_parents(Path::new("hpm.toml"), false)
            .is_ignore());
    }

    #[test]
    fn ignore_rules_load_gitignore_and_hpmignore() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
        fs::write(dir.path().join(".hpmignore"), "build/\n").unwrap();

        let rules = build_ignore_rules(dir.path()).unwrap();

        assert!(rules
            .matched_path_or_any_parents(Path::new("debug.log"), false)
            .is_ignore());
        assert!(rules
            .matched_path_or_any_parents(Path::new("build/out.o"), false)
            .is_ignore());
        assert!(!rules
            .matched_path_or_any_parents(Path::new("src/main.rs"), false)
            .is_ignore());
    }

    #[test]
    fn archive_contains_expected_files() {
        let dir = TempDir::new().unwrap();
        create_test_package(dir.path());
        // Add dirs that should be excluded
        fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
        fs::write(dir.path().join(".git/config"), "gitconfig").unwrap();
        fs::create_dir_all(dir.path().join(".hpm")).unwrap();
        fs::write(dir.path().join(".hpm/config.toml"), "").unwrap();

        let output_dir = TempDir::new().unwrap();
        let ignore = build_ignore_rules(dir.path()).unwrap();
        let archive_path =
            create_archive(dir.path(), "test-pkg", "1.0.0", output_dir.path(), &ignore).unwrap();

        assert!(archive_path.exists());
        assert_eq!(archive_path.file_name().unwrap(), "test-pkg-1.0.0.zip");

        // Verify contents
        let file = fs::File::open(&archive_path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();

        assert!(names.contains(&"hpm.toml".to_string()));
        assert!(names.contains(&"README.md".to_string()));
        assert!(names.contains(&"otls/tool.hda".to_string()));
        assert!(!names.iter().any(|n| n.starts_with(".git")));
        assert!(!names.iter().any(|n| n.starts_with(".hpm")));
    }

    #[test]
    fn checksum_is_deterministic() {
        let dir = TempDir::new().unwrap();
        create_test_package(dir.path());

        let out1 = TempDir::new().unwrap();
        let out2 = TempDir::new().unwrap();
        let ignore = build_ignore_rules(dir.path()).unwrap();

        let path1 = create_archive(dir.path(), "test-pkg", "1.0.0", out1.path(), &ignore).unwrap();
        let path2 = create_archive(dir.path(), "test-pkg", "1.0.0", out2.path(), &ignore).unwrap();

        let cksum1 = compute_archive_checksum(&path1).unwrap();
        let cksum2 = compute_archive_checksum(&path2).unwrap();

        assert_eq!(cksum1, cksum2);
        assert_eq!(cksum1.len(), 64); // SHA-256 hex is 64 chars
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let dir = TempDir::new().unwrap();
        create_test_package(dir.path());

        let output_dir = TempDir::new().unwrap();
        let ignore = build_ignore_rules(dir.path()).unwrap();
        let archive_path =
            create_archive(dir.path(), "test-pkg", "1.0.0", output_dir.path(), &ignore).unwrap();

        // Generate a keypair for testing
        let secret = [42u8; 32];
        let signing_key = SigningKey::from_bytes(&secret);

        let (sig_b64, key_id) = sign_archive(&archive_path, &signing_key).unwrap();

        // Verify signature
        let sig_bytes = base64::engine::general_purpose::STANDARD
            .decode(&sig_b64)
            .unwrap();
        let signature =
            ed25519_dalek::Signature::from_bytes(sig_bytes.as_slice().try_into().unwrap());

        let archive_bytes = fs::read(&archive_path).unwrap();
        let verifying_key = signing_key.verifying_key();
        assert!(
            ed25519_dalek::Verifier::verify(&verifying_key, &archive_bytes, &signature).is_ok()
        );

        // key_id is first 8 bytes of public key in hex = 16 chars
        assert_eq!(key_id.len(), 16);
    }

    #[test]
    fn invalid_key_file_wrong_size() {
        let dir = TempDir::new().unwrap();
        let key_path = dir.path().join("bad.key");
        fs::write(&key_path, b"too-short").unwrap();

        let result = load_signing_key(&key_path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PackError::SigningKey(_)));
    }

    #[test]
    fn invalid_key_file_not_found() {
        let result = load_signing_key(Path::new("/nonexistent/key.bin"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PackError::SigningKey(_)));
    }

    #[test]
    fn pack_without_signing() {
        let dir = TempDir::new().unwrap();
        create_test_package(dir.path());

        let output_dir = TempDir::new().unwrap();
        let result = pack(dir.path(), "test-pkg", "1.0.0", output_dir.path(), None).unwrap();

        assert!(result.archive_path.exists());
        assert!(!result.checksum.is_empty());
        assert!(result.signature.is_none());
        assert!(result.key_id.is_none());
    }

    #[test]
    fn pack_with_signing() {
        let dir = TempDir::new().unwrap();
        create_test_package(dir.path());

        let secret = [7u8; 32];
        let signing_key = SigningKey::from_bytes(&secret);

        let output_dir = TempDir::new().unwrap();
        let result = pack(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            Some(&signing_key),
        )
        .unwrap();

        assert!(result.archive_path.exists());
        assert!(!result.checksum.is_empty());
        assert!(result.signature.is_some());
        assert!(result.key_id.is_some());
    }
}
