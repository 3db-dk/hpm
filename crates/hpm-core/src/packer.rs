//! Package archiving, checksumming, and signing.
//!
//! Produces a `{name}-{version}.zip` from a package directory, with SHA-256
//! checksum and optional Ed25519 signature.

use base64::Engine;
use ed25519_dalek::pkcs8::DecodePrivateKey;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use glob::Pattern;
use hpm_package::manifest::NativeConfig;
use hpm_package::platform::Platform;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

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
    /// Platform tag if this is a platform-specific archive.
    pub platform: Option<String>,
}

/// Filter that excludes files belonging to other platforms.
pub struct PlatformFilter {
    /// Glob patterns for files belonging to OTHER platforms (to exclude).
    pub exclude: Vec<Pattern>,
    /// Glob patterns for the target platform's own files. Acts as an
    /// override: a file matched by `exclude` is still kept when it is
    /// also claimed by `target`, so manifests where two platforms share
    /// a glob (intentional common content, e.g. a shared install path)
    /// don't end up cancelling the file out of every archive.
    pub target: Vec<Pattern>,
}

impl PlatformFilter {
    /// Build a filter from native config, excluding files for all platforms
    /// other than the target. A file claimed by the target platform is
    /// always kept, even if another platform also lists it.
    pub fn new(native_config: &NativeConfig, target: &Platform) -> Result<Self, PackError> {
        let target_str = target.as_str();
        let mut exclude = Vec::new();
        let mut target_patterns = Vec::new();
        for (platform_str, platform_files) in &native_config.platform_files {
            for pattern in &platform_files.files {
                let compiled =
                    Pattern::new(pattern).map_err(|e| PackError::GlobPattern(e.to_string()))?;
                if platform_str == target_str {
                    target_patterns.push(compiled);
                } else {
                    exclude.push(compiled);
                }
            }
        }
        Ok(Self {
            exclude,
            target: target_patterns,
        })
    }

    /// Returns true if the relative path should be filtered out.
    pub fn should_exclude(&self, rel_path: &str) -> bool {
        if !self.exclude.iter().any(|p| p.matches(rel_path)) {
            return false;
        }
        !self.target.iter().any(|p| p.matches(rel_path))
    }
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

    #[error("Invalid glob pattern: {0}")]
    GlobPattern(String),
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
    platform: Option<&Platform>,
    platform_filter: Option<&PlatformFilter>,
    inject_files: &[(String, Vec<u8>)],
) -> Result<PathBuf, PackError> {
    // Replace `/` with `-` in package name for flat archive filenames
    let safe_name = name.replace('/', "-");
    let archive_name = match platform {
        Some(p) => format!("{}-{}-{}.zip", safe_name, version, p),
        None => format!("{}-{}.zip", safe_name, version),
    };
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

        // Filter out files belonging to other platforms
        if let Some(filter) = platform_filter {
            let rel_str = relative.to_string_lossy();
            if filter.should_exclude(&rel_str) {
                continue;
            }
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

    // Write injected files
    for (name, content) in inject_files {
        zip.start_file(name.as_str(), options)?;
        zip.write_all(content)?;
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
    platform: Option<&Platform>,
    native_config: Option<&NativeConfig>,
    inject_files: &[(String, Vec<u8>)],
) -> Result<PackResult, PackError> {
    let ignore = build_ignore_rules(package_dir)?;

    let platform_filter = match (platform, native_config) {
        (Some(p), Some(nc)) => Some(PlatformFilter::new(nc, p)?),
        _ => None,
    };

    let archive_path = create_archive(
        package_dir,
        name,
        version,
        output_dir,
        &ignore,
        platform,
        platform_filter.as_ref(),
        inject_files,
    )?;
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
        platform: platform.map(|p| p.to_string()),
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
path = "studio/test-pkg"
name = "Test Package"
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

        assert!(
            rules
                .matched_path_or_any_parents(Path::new(".git/config"), false)
                .is_ignore()
        );
        assert!(
            rules
                .matched_path_or_any_parents(Path::new(".hpm/config.toml"), false)
                .is_ignore()
        );
        assert!(
            !rules
                .matched_path_or_any_parents(Path::new("hpm.toml"), false)
                .is_ignore()
        );
    }

    #[test]
    fn ignore_rules_load_gitignore_and_hpmignore() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
        fs::write(dir.path().join(".hpmignore"), "build/\n").unwrap();

        let rules = build_ignore_rules(dir.path()).unwrap();

        assert!(
            rules
                .matched_path_or_any_parents(Path::new("debug.log"), false)
                .is_ignore()
        );
        assert!(
            rules
                .matched_path_or_any_parents(Path::new("build/out.o"), false)
                .is_ignore()
        );
        assert!(
            !rules
                .matched_path_or_any_parents(Path::new("src/main.rs"), false)
                .is_ignore()
        );
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
        let archive_path = create_archive(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            &ignore,
            None,
            None,
            &[],
        )
        .unwrap();

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

        let path1 = create_archive(
            dir.path(),
            "test-pkg",
            "1.0.0",
            out1.path(),
            &ignore,
            None,
            None,
            &[],
        )
        .unwrap();
        let path2 = create_archive(
            dir.path(),
            "test-pkg",
            "1.0.0",
            out2.path(),
            &ignore,
            None,
            None,
            &[],
        )
        .unwrap();

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
        let archive_path = create_archive(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            &ignore,
            None,
            None,
            &[],
        )
        .unwrap();

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
    fn invalid_key_file_not_pem() {
        let dir = TempDir::new().unwrap();
        let key_path = dir.path().join("bad.pem");
        fs::write(&key_path, b"garbage").unwrap();

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
    fn load_signing_key_pem_roundtrip() {
        use ed25519_dalek::pkcs8::EncodePrivateKey;
        use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;

        let original = SigningKey::from_bytes(&[7u8; 32]);
        let pem = original.to_pkcs8_pem(LineEnding::LF).unwrap();

        let dir = TempDir::new().unwrap();
        let key_path = dir.path().join("signing.pem");
        fs::write(&key_path, pem.as_bytes()).unwrap();

        let loaded = load_signing_key(&key_path).unwrap();
        assert_eq!(
            loaded.verifying_key().to_bytes(),
            original.verifying_key().to_bytes()
        );
    }

    #[test]
    fn load_signing_key_from_pem_inline() {
        use ed25519_dalek::pkcs8::EncodePrivateKey;
        use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;

        let original = SigningKey::from_bytes(&[9u8; 32]);
        let pem = original.to_pkcs8_pem(LineEnding::LF).unwrap();

        let loaded = load_signing_key_from_pem(&pem).unwrap();
        assert_eq!(
            loaded.verifying_key().to_bytes(),
            original.verifying_key().to_bytes()
        );
    }

    #[test]
    fn pack_without_signing() {
        let dir = TempDir::new().unwrap();
        create_test_package(dir.path());

        let output_dir = TempDir::new().unwrap();
        let result = pack(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            None,
            None,
            None,
            &[],
        )
        .unwrap();

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
            None,
            None,
            &[],
        )
        .unwrap();

        assert!(result.archive_path.exists());
        assert!(!result.checksum.is_empty());
        assert!(result.signature.is_some());
        assert!(result.key_id.is_some());
    }

    fn create_native_test_package(dir: &Path) {
        create_test_package(dir);
        fs::create_dir_all(dir.join("lib/linux-x86_64")).unwrap();
        fs::write(dir.join("lib/linux-x86_64/libfoo.so"), b"elf-binary").unwrap();
        fs::create_dir_all(dir.join("lib/macos-universal")).unwrap();
        fs::write(
            dir.join("lib/macos-universal/libfoo.dylib"),
            b"macho-binary",
        )
        .unwrap();
        fs::create_dir_all(dir.join("lib/windows-x86_64")).unwrap();
        fs::write(dir.join("lib/windows-x86_64/foo.dll"), b"pe-binary").unwrap();
    }

    fn test_native_config() -> hpm_package::manifest::NativeConfig {
        let mut platform_files = indexmap::IndexMap::new();
        platform_files.insert(
            "linux-x86_64".to_string(),
            hpm_package::manifest::NativePlatformFiles {
                files: vec!["lib/linux-x86_64/*".to_string()],
            },
        );
        platform_files.insert(
            "macos-universal".to_string(),
            hpm_package::manifest::NativePlatformFiles {
                files: vec!["lib/macos-universal/*".to_string()],
            },
        );
        platform_files.insert(
            "windows-x86_64".to_string(),
            hpm_package::manifest::NativePlatformFiles {
                files: vec!["lib/windows-x86_64/*".to_string()],
            },
        );
        hpm_package::manifest::NativeConfig {
            platforms: vec![
                "linux-x86_64".to_string(),
                "macos-universal".to_string(),
                "windows-x86_64".to_string(),
            ],
            platform_files,
        }
    }

    #[test]
    fn platform_archive_name_includes_platform() {
        let dir = TempDir::new().unwrap();
        create_native_test_package(dir.path());

        let output_dir = TempDir::new().unwrap();
        let native_config = test_native_config();
        let platform = hpm_package::platform::Platform::LinuxX86_64;

        let result = pack(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            None,
            Some(&platform),
            Some(&native_config),
            &[],
        )
        .unwrap();

        assert_eq!(
            result.archive_path.file_name().unwrap(),
            "test-pkg-1.0.0-linux-x86_64.zip"
        );
        assert_eq!(result.platform.as_deref(), Some("linux-x86_64"));
    }

    #[test]
    fn platform_archive_excludes_other_platforms() {
        let dir = TempDir::new().unwrap();
        create_native_test_package(dir.path());

        let output_dir = TempDir::new().unwrap();
        let native_config = test_native_config();
        let platform = hpm_package::platform::Platform::LinuxX86_64;

        let result = pack(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            None,
            Some(&platform),
            Some(&native_config),
            &[],
        )
        .unwrap();

        let file = fs::File::open(&result.archive_path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();

        // Should contain linux files
        assert!(names.contains(&"lib/linux-x86_64/libfoo.so".to_string()));
        // Should NOT contain other platforms
        assert!(!names.iter().any(|n| n.contains("macos-universal")));
        assert!(!names.iter().any(|n| n.contains("windows-x86_64")));
        // Should still contain shared files
        assert!(names.contains(&"hpm.toml".to_string()));
        assert!(names.contains(&"README.md".to_string()));
    }

    #[test]
    fn shared_glob_across_platforms_rides_through_each_archive() {
        // A glob listed identically under every platform declares common
        // content with a shared install path; the matched files must
        // appear in every per-platform archive.
        let dir = TempDir::new().unwrap();
        create_test_package(dir.path());
        fs::create_dir_all(dir.path().join("resolver/houdini21")).unwrap();
        fs::write(
            dir.path().join("resolver/houdini21/foo.dll"),
            b"shared-binary",
        )
        .unwrap();

        let mut platform_files = indexmap::IndexMap::new();
        for plat in ["linux-x86_64", "macos-universal", "windows-x86_64"] {
            platform_files.insert(
                plat.to_string(),
                hpm_package::manifest::NativePlatformFiles {
                    files: vec!["resolver/houdini*/**/*".to_string()],
                },
            );
        }
        let native_config = hpm_package::manifest::NativeConfig {
            platforms: vec![
                "linux-x86_64".to_string(),
                "macos-universal".to_string(),
                "windows-x86_64".to_string(),
            ],
            platform_files,
        };

        for platform in [
            hpm_package::platform::Platform::LinuxX86_64,
            hpm_package::platform::Platform::MacosUniversal,
            hpm_package::platform::Platform::WindowsX86_64,
        ] {
            let output_dir = TempDir::new().unwrap();
            let result = pack(
                dir.path(),
                "test-pkg",
                "1.0.0",
                output_dir.path(),
                None,
                Some(&platform),
                Some(&native_config),
                &[],
            )
            .unwrap();

            let file = fs::File::open(&result.archive_path).unwrap();
            let mut zip = zip::ZipArchive::new(file).unwrap();
            let names: Vec<String> = (0..zip.len())
                .map(|i| zip.by_index(i).unwrap().name().to_string())
                .collect();

            assert!(
                names.contains(&"resolver/houdini21/foo.dll".to_string()),
                "shared resolver binary missing from {} archive: {:?}",
                platform,
                names
            );
        }
    }

    #[test]
    fn target_glob_overrides_other_platform_match() {
        // A path claimed by the target wins over an exclude from another
        // platform's glob.
        let dir = TempDir::new().unwrap();
        create_test_package(dir.path());
        fs::create_dir_all(dir.path().join("shared")).unwrap();
        fs::write(dir.path().join("shared/binary.so"), b"data").unwrap();

        let mut platform_files = indexmap::IndexMap::new();
        platform_files.insert(
            "linux-x86_64".to_string(),
            hpm_package::manifest::NativePlatformFiles {
                files: vec!["shared/*".to_string()],
            },
        );
        platform_files.insert(
            "macos-universal".to_string(),
            hpm_package::manifest::NativePlatformFiles {
                files: vec!["shared/*".to_string(), "lib/macos-universal/*".to_string()],
            },
        );
        let native_config = hpm_package::manifest::NativeConfig {
            platforms: vec!["linux-x86_64".to_string(), "macos-universal".to_string()],
            platform_files,
        };

        let output_dir = TempDir::new().unwrap();
        let result = pack(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            None,
            Some(&hpm_package::platform::Platform::LinuxX86_64),
            Some(&native_config),
            &[],
        )
        .unwrap();

        let file = fs::File::open(&result.archive_path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();

        assert!(names.contains(&"shared/binary.so".to_string()));
    }

    #[test]
    fn pack_without_platform_has_no_platform_tag() {
        let dir = TempDir::new().unwrap();
        create_native_test_package(dir.path());

        let output_dir = TempDir::new().unwrap();
        let result = pack(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            None,
            None,
            None,
            &[],
        )
        .unwrap();

        assert_eq!(
            result.archive_path.file_name().unwrap(),
            "test-pkg-1.0.0.zip"
        );
        assert!(result.platform.is_none());
    }

    #[test]
    fn inject_files_added_to_archive() {
        let dir = TempDir::new().unwrap();
        create_test_package(dir.path());

        let output_dir = TempDir::new().unwrap();
        let inject = vec![(
            "test-pkg.json".to_string(),
            b"{\"name\": \"test-pkg\"}".to_vec(),
        )];

        let result = pack(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            None,
            None,
            None,
            &inject,
        )
        .unwrap();

        let file = fs::File::open(&result.archive_path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();

        assert!(names.contains(&"test-pkg.json".to_string()));
        assert!(names.contains(&"hpm.toml".to_string()));

        // Verify injected file content
        let mut injected = zip.by_name("test-pkg.json").unwrap();
        let mut content = String::new();
        std::io::Read::read_to_string(&mut injected, &mut content).unwrap();
        assert_eq!(content, "{\"name\": \"test-pkg\"}");
    }
}
