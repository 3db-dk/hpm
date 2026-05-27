//! Package archiving, checksumming, and signing.
//!
//! Produces a `{name}-{version}.zip` from a package directory, with SHA-256
//! checksum and optional Ed25519 signature.

use crate::path_util::relative_path_to_forward_slash;
use base64::Engine;
pub use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::DecodePrivateKey;
use ed25519_dalek::{Signer, VerifyingKey};
use glob::Pattern;
use hpm_package::manifest::StageConfig;
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

/// A single `from -> to` placement rule compiled from `[stage.platform.*]`.
struct CompiledPlaceRule {
    from: Pattern,
    /// Archive-path prefix or full path, depending on whether `to` ends
    /// with `/` in the manifest. See [`StageFilter::archive_path_for`].
    to: String,
    /// Whether `to` was authored as a directory (ends with `/`).
    to_is_dir: bool,
}

/// Derived from `[stage]` at pack time. Combines:
///   - workspace include/exclude globs from `[stage]`
///   - per-platform `place` rules (with `from`/`to` paths) from
///     `[stage.platform.*]`, filtered to the target platform plus exclusion
///     of files matched only by other platforms' rules.
pub struct StageFilter {
    include: Vec<Pattern>,
    exclude: Vec<Pattern>,
    /// `from` patterns for the target platform. A file matched by one of
    /// these is included and its archive path is rewritten via the
    /// matching rule's `to`.
    target_rules: Vec<CompiledPlaceRule>,
    /// `from` patterns claimed only by non-target platforms. A file matched
    /// only by these is excluded.
    other_platform_patterns: Vec<Pattern>,
}

impl StageFilter {
    /// Build a filter from `[stage]` for the given target platform.
    /// Pass `target = None` to pack without per-platform placement (used
    /// when the package declares no `[compat].platforms`).
    pub fn new(stage: &StageConfig, target: Option<&Platform>) -> Result<Self, PackError> {
        let include = compile_patterns(&stage.include)?;
        let exclude = compile_patterns(&stage.exclude)?;

        let mut target_rules = Vec::new();
        let mut other_platform_patterns = Vec::new();

        if let Some(target) = target {
            let target_str = target.as_str();
            for (platform_str, rules) in &stage.platform.entries {
                for rule in &rules.place {
                    let from = Pattern::new(&rule.from)
                        .map_err(|e| PackError::GlobPattern(e.to_string()))?;
                    if platform_str == target_str {
                        let trimmed = rule.to.trim();
                        let to_is_dir = trimmed.ends_with('/') || trimmed == ".";
                        let to = if trimmed == "." || trimmed == "./" {
                            String::new()
                        } else if to_is_dir {
                            trimmed.trim_end_matches('/').to_string()
                        } else {
                            trimmed.to_string()
                        };
                        target_rules.push(CompiledPlaceRule {
                            from,
                            to,
                            to_is_dir,
                        });
                    } else {
                        other_platform_patterns.push(from);
                    }
                }
            }
        }

        Ok(Self {
            include,
            exclude,
            target_rules,
            other_platform_patterns,
        })
    }

    /// Returns the archive-relative path for `rel_path`, or `None` if the
    /// file should be excluded from this platform's archive.
    pub fn archive_path_for(&self, rel_path: &str) -> Option<String> {
        // Explicit excludes always win.
        if self.exclude.iter().any(|p| p.matches(rel_path)) {
            return None;
        }
        // Explicit includes ("only ship these as common content") narrow
        // the set when present, but never override a target-platform
        // `from` match.
        let target_match = self
            .target_rules
            .iter()
            .find(|rule| rule.from.matches(rel_path));
        if let Some(rule) = target_match {
            return Some(rewrite_archive_path(rel_path, rule));
        }
        let other_match = self
            .other_platform_patterns
            .iter()
            .any(|p| p.matches(rel_path));
        if other_match {
            return None;
        }
        if !self.include.is_empty() && !self.include.iter().any(|p| p.matches(rel_path)) {
            return None;
        }
        Some(rel_path.to_string())
    }
}

fn compile_patterns(globs: &[String]) -> Result<Vec<Pattern>, PackError> {
    globs
        .iter()
        .map(|g| Pattern::new(g).map_err(|e| PackError::GlobPattern(e.to_string())))
        .collect()
}

fn rewrite_archive_path(rel_path: &str, rule: &CompiledPlaceRule) -> String {
    if rule.to_is_dir {
        // Take the basename of `rel_path` and append it under `to/`. This
        // matches the common case of `from = "build/Release/*.dylib"`,
        // `to = "dso/macos-aarch64/"`.
        let basename = rel_path.rsplit_once('/').map_or(rel_path, |(_, name)| name);
        if rule.to.is_empty() {
            basename.to_string()
        } else {
            format!("{}/{}", rule.to, basename)
        }
    } else {
        // `to` is a literal full archive path; use it verbatim. Useful when
        // exactly one file is being relocated under a renamed name.
        rule.to.clone()
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

/// Create a zip archive of the package directory, filtering via ignore
/// rules and `[stage]`.
///
/// Files are added in sorted order for deterministic output. When a `[stage]`
/// filter is supplied, each file's archive path is rewritten via the
/// matching `place` rule; unmatched files ship at their workspace-relative
/// path.
pub fn create_archive(
    package_dir: &Path,
    name: &str,
    version: &str,
    output_dir: &Path,
    ignore: &Gitignore,
    platform: Option<&Platform>,
    stage_filter: Option<&StageFilter>,
    inject_files: &[(String, Vec<u8>)],
) -> Result<PathBuf, PackError> {
    // Replace `/` with `-` in package name for flat archive filenames
    let safe_name = name.replace('/', "-");
    let archive_name = match platform {
        Some(p) => format!("{}-{}-{}.zip", safe_name, version, p),
        None => format!("{}-{}.zip", safe_name, version),
    };
    let archive_path = output_dir.join(&archive_name);

    // Collect (source path, archive path) pairs, sorted for determinism.
    let mut entries: Vec<(PathBuf, String)> = Vec::new();
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

        if !entry.file_type().is_file() {
            continue;
        }

        // Manifest globs (e.g. `build/Release/*.dylib`) use forward slashes,
        // so the input must too — `to_string_lossy()` would emit backslashes
        // on Windows and silently fail to match.
        let rel_str = relative_path_to_forward_slash(relative);
        let archive_path = match stage_filter {
            Some(filter) => match filter.archive_path_for(&rel_str) {
                Some(p) => p,
                None => continue,
            },
            None => rel_str,
        };
        entries.push((path.to_path_buf(), archive_path));
    }

    // Create zip
    let file = fs::File::create(&archive_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for (source, archive_name) in &entries {
        zip.start_file(archive_name.as_str(), options)?;
        let mut f = fs::File::open(source)?;
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
    let bytes = fs::read(path)?;
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

/// Pack a package directory into a signed, checksummed archive.
pub fn pack(
    package_dir: &Path,
    name: &str,
    version: &str,
    output_dir: &Path,
    signing_key: Option<&SigningKey>,
    platform: Option<&Platform>,
    stage_config: Option<&StageConfig>,
    inject_files: &[(String, Vec<u8>)],
) -> Result<PackResult, PackError> {
    let ignore = build_ignore_rules(package_dir)?;

    let stage_filter = match stage_config {
        Some(stage) => Some(StageFilter::new(stage, platform)?),
        None => None,
    };

    let archive_path = create_archive(
        package_dir,
        name,
        version,
        output_dir,
        &ignore,
        platform,
        stage_filter.as_ref(),
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
        fs::create_dir_all(dir.join("lib/macos-aarch64")).unwrap();
        fs::write(dir.join("lib/macos-aarch64/libfoo.dylib"), b"macho-binary").unwrap();
        fs::create_dir_all(dir.join("lib/windows-x86_64")).unwrap();
        fs::write(dir.join("lib/windows-x86_64/foo.dll"), b"pe-binary").unwrap();
    }

    fn test_stage_config() -> hpm_package::manifest::StageConfig {
        use hpm_package::manifest::{PlaceRule, PlatformStaging, StageConfig, StagePlatformRules};
        let mut entries = indexmap::IndexMap::new();
        for plat in ["linux-x86_64", "macos-aarch64", "windows-x86_64"] {
            entries.insert(
                plat.to_string(),
                StagePlatformRules {
                    place: vec![PlaceRule {
                        from: format!("lib/{}/*", plat),
                        to: format!("lib/{}/", plat),
                    }],
                },
            );
        }
        StageConfig {
            platform: PlatformStaging { entries },
            ..Default::default()
        }
    }

    #[test]
    fn platform_archive_name_includes_platform() {
        let dir = TempDir::new().unwrap();
        create_native_test_package(dir.path());

        let output_dir = TempDir::new().unwrap();
        let stage_config = test_stage_config();
        let platform = hpm_package::platform::Platform::LinuxX86_64;

        let result = pack(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            None,
            Some(&platform),
            Some(&stage_config),
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
        let stage_config = test_stage_config();
        let platform = hpm_package::platform::Platform::LinuxX86_64;

        let result = pack(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            None,
            Some(&platform),
            Some(&stage_config),
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
        assert!(!names.iter().any(|n| n.contains("macos-aarch64")));
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

        // The same `from` glob listed under every platform declares
        // common content with a shared install path — the place rule's
        // `to = "resolver/"` plus the basename-only rewrite keeps the
        // file at its original layout in every per-platform archive.
        let mut entries = indexmap::IndexMap::new();
        for plat in ["linux-x86_64", "macos-aarch64", "windows-x86_64"] {
            entries.insert(
                plat.to_string(),
                hpm_package::manifest::StagePlatformRules {
                    place: vec![hpm_package::manifest::PlaceRule {
                        from: "resolver/houdini*/**/*".to_string(),
                        to: "resolver/houdini21/".to_string(),
                    }],
                },
            );
        }
        let stage_config = hpm_package::manifest::StageConfig {
            platform: hpm_package::manifest::PlatformStaging { entries },
            ..Default::default()
        };

        for platform in [
            hpm_package::platform::Platform::LinuxX86_64,
            hpm_package::platform::Platform::MacosAarch64,
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
                Some(&stage_config),
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

        // Linux claims `shared/*`; macOS claims `shared/*` AND
        // `lib/macos-aarch64/*`. When packing for Linux, `shared/binary.so`
        // matches the target's rule (kept), and we also confirm the file
        // isn't dropped just because macOS also lists `shared/*`.
        let mut entries = indexmap::IndexMap::new();
        entries.insert(
            "linux-x86_64".to_string(),
            hpm_package::manifest::StagePlatformRules {
                place: vec![hpm_package::manifest::PlaceRule {
                    from: "shared/*".to_string(),
                    to: "shared/".to_string(),
                }],
            },
        );
        entries.insert(
            "macos-aarch64".to_string(),
            hpm_package::manifest::StagePlatformRules {
                place: vec![
                    hpm_package::manifest::PlaceRule {
                        from: "shared/*".to_string(),
                        to: "shared/".to_string(),
                    },
                    hpm_package::manifest::PlaceRule {
                        from: "lib/macos-aarch64/*".to_string(),
                        to: "lib/macos-aarch64/".to_string(),
                    },
                ],
            },
        );
        let stage_config = hpm_package::manifest::StageConfig {
            platform: hpm_package::manifest::PlatformStaging { entries },
            ..Default::default()
        };

        let output_dir = TempDir::new().unwrap();
        let result = pack(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            None,
            Some(&hpm_package::platform::Platform::LinuxX86_64),
            Some(&stage_config),
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
